// Kabinenansagen (Beta): die Ansage-Seite. Oben der Ablauf (Freigabe →
// Texte → Ton → nächste Ansage), dann was gerade gesprochen wird (Sprecher,
// Text, Pegel live), darunter jede kommende und jede gelaufene Ansage als
// aufklappbare Zeile: Zeitpunkt, Stand, Text aller Sprachen, Anhören.
// Gleiche Seitenform wie das Bordbuch.
import { useEffect, useState, useSyncExternalStore } from "react";
import { Notice } from "../ui";
import { useTranslation } from "react-i18next";
import { formatIpcError } from "../../lib/ipc";
import {
  abonnieren,
  aktuell,
  anhoeren,
  anhoerenStoppen,
  einstellungenLesen,
  einstellungenSetzen,
  erneut,
  lautstaerkeSetzen,
  stoppen,
  stummSetzen,
  ueberblick,
  musikZustand,
  type Ablauf,
  type Geplant,
  type Gespielt,
  type Profil,
  type Stand,
  type TeilStand,
  type Teil,
  type Ueberblick,
  type MusikZustand,
} from "../../lib/kabinenansagen";
import { MusikKasten, MusikRegler } from "./BoardingMusik";
import "./kabine.css";

const BALKEN = 48;

function uhrzeit(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Pegelanzeige: die letzten Messwerte als senkrechte Balken. */
function Pegel({ werte, aktiv }: { werte: number[]; aktiv: boolean }) {
  const reihe = Array.from({ length: BALKEN }, (_, i) => werte[werte.length - BALKEN + i] ?? 0);
  return (
    <svg className={`kabine-pegel${aktiv ? " ist-aktiv" : ""}`} viewBox={`0 0 ${BALKEN * 6} 64`} preserveAspectRatio="none" aria-hidden="true">
      {reihe.map((w, i) => {
        const h = Math.max(2, Math.round(w * 60));
        return <rect key={i} x={i * 6 + 1} y={32 - h / 2} width={4} height={h} rx={2} />;
      })}
    </svg>
  );
}

/** „Britisches Englisch“, „Schweizer Hochdeutsch“ — und bei der
 *  Standardform nur „Deutsch“ statt „Deutsch (Deutschland)“. */
function sprachName(sprache: string, sprachen: Intl.DisplayNames | null): string {
  try {
    const name = sprachen?.of(sprache) ?? sprache;
    return name.includes("(") ? (sprachen?.of(sprache.split("-")[0]) ?? name) : name;
  } catch {
    return sprache;
  }
}

/** Fehlermeldung des Servers in eine verständliche Klasse (Punkt 21). */
export function fehlerKlasse(meldung: string): string {
  const m = meldung.toLowerCase();
  if (/\b(429|503)\b|ausgelastet|zu_viele/.test(m)) return "ausgelastet";
  if (/\b402\b|budget|kontingent/.test(m)) return "kontingent";
  if (/\b(401|403)\b/.test(m)) return "zugang";
  if (/timed out|timeout|zeitüberschreitung/.test(m)) return "zeit";
  if (/nicht erreichbar|error sending|connect|dns|offline/.test(m)) return "offline";
  return "sonst";
}

type T = ReturnType<typeof useTranslation>["t"];

function standText(t: T, s: Stand): string {
  switch (s.art) {
    case "text_ab":
      if (s.nach_abheben) return t("kabine.stand.text_ab_abheben");
      return s.um !== null ? t("kabine.stand.text_ab", { zeit: uhrzeit(s.um) }) : t("kabine.stand.text_ab_ohne");
    case "text_fehler":
    case "ton_wartet":
      return t(`kabine.stand.${s.art}`, { zeit: uhrzeit(s.um), grund: t(`kabine.fehler.${fehlerKlasse(s.meldung)}`) });
    case "text_aufgegeben":
    case "ton_aufgegeben":
      return t(`kabine.stand.${s.art}`, { grund: t(`kabine.fehler.${fehlerKlasse(s.meldung)}`) });
    case "ton_wird_erzeugt":
      return t("kabine.stand.ton_wird_erzeugt", { fertig: s.fertig, von: s.von });
    default:
      return t(`kabine.stand.${s.art}`);
  }
}

function standKlasse(s: Stand): string {
  if (s.art === "bereit") return "ist-bereit";
  if (s.art === "text_aufgegeben" || s.art === "ton_aufgegeben") return "ist-fehler";
  if (s.art === "text_fehler" || s.art === "ton_wartet") return "ist-warnung";
  return "ist-offen";
}

function teilText(t: T, s: TeilStand): string {
  switch (s.art) {
    case "offen":
      return s.um !== null ? t("kabine.ablauf.teil.offen_um", { zeit: uhrzeit(s.um) }) : t("kabine.ablauf.teil.offen");
    case "fehler":
      return t("kabine.ablauf.teil.fehler", { zeit: uhrzeit(s.um) });
    default:
      return t(`kabine.ablauf.teil.${s.art}`);
  }
}

/** Bezeichnung einer Ansage; „Reiseflug 80“ heißt je nach Airline-Service
 *  „Zweiter Service“ oder „Ende des Bordverkaufs“ (Server liefert `art`). */
function slotName(t: T, slot: string, art?: string | null): string {
  return slot === "reiseflug80" && art ? t(`kabine.slot.reiseflug80_${art}`) : t(`kabine.slot.${slot}`);
}

/** Text in Sätze teilen (an . ! ? mit folgendem Leerzeichen). */
export function saetzeVon(text: string): string[] {
  return text.split(/(?<=[.!?…])\s+/u).map((x) => x.trim()).filter(Boolean);
}

/** Die gesprochenen Stücke eines Teils: vom Ton-Server (Text und Startzeit
 *  genau), sonst Sätze mit geschätzter Startzeit nach ihrer Länge. */
export function stueckeVon(teil: Teil): { ab_s: number; text: string }[] {
  if (teil.stuecke && teil.stuecke.length > 0) return teil.stuecke;
  const saetze = saetzeVon(teil.text);
  const gesamt = saetze.reduce((n, x) => n + x.length, 0) || 1;
  let summe = 0;
  return saetze.map((text) => {
    const ab_s = (summe / gesamt) * teil.dauer_s;
    summe += text.length;
    return { ab_s, text };
  });
}

/** Welches Stück läuft nach `sek` Sekunden? */
export function satzIndex(teil: Teil, sek: number): number {
  const st = stueckeVon(teil);
  let i = 0;
  while (i + 1 < st.length && st[i + 1].ab_s <= sek) i++;
  return i;
}

/** Untertitel: groß der Satz, der gerade gesprochen wird, blass davor und
 *  danach — keine Textwand, auch nicht bei der langen Sicherheitsansage
 *  (Thomas 18.10.2026). Der volle Text bleibt unten aufklappbar. */
function Untertitel({ ansage, teil, teilStart, namen, t }: { ansage: Gespielt; teil: number; teilStart: number; namen: Intl.DisplayNames | null; t: T }) {
  const jetzt = ansage.teile[teil];
  if (!jetzt) return null;
  const saetze = stueckeVon(jetzt).map((x) => x.text);
  const i = satzIndex(jetzt, Math.max(0, (performance.now() - teilStart) / 1000));
  const danach = ansage.teile.slice(teil + 1).map((x) => sprachName(x.sprache, namen));
  const fertig = ansage.teile.slice(0, teil).map((x) => sprachName(x.sprache, namen));
  return (
    <div className="kabine-untertitel" data-testid="kabine-untertitel">
      <div className="kabine-untertitel-kopf">
        <span className="kabine-sprache">{sprachName(jetzt.sprache, namen)}</span>
        {saetze.length > 1 && <span className="kabine-untertitel-zaehler">{t("kabine.untertitel.satz", { n: i + 1, von: saetze.length })}</span>}
      </div>
      {i > 0 && <p className="kabine-untertitel-davor">{saetze[i - 1]}</p>}
      <p className="kabine-untertitel-jetzt">{saetze[i] ?? jetzt.text}</p>
      {i + 1 < saetze.length && <p className="kabine-untertitel-danach">{saetze[i + 1]}</p>}
      {(danach.length > 0 || fertig.length > 0) && (
        <p className="kabine-untertitel-sprachen">
          {fertig.length > 0 && t("kabine.untertitel.fertig", { sprachen: fertig.join(", ") })}
          {fertig.length > 0 && danach.length > 0 && " · "}
          {danach.length > 0 && t("kabine.untertitel.danach", { sprachen: danach.join(", ") })}
        </p>
      )}
    </div>
  );
}

/** Ablaufzeile oben: Freigabe → Texte → Ton → nächste Ansage (Hausregel
 *  „Status als Tabelle oben“). */
function AblaufZeile({ ablauf, naechste, stumm, t }: { ablauf: Ablauf; naechste: Geplant | undefined; stumm: boolean; t: T }) {
  const f = ablauf.freigabe;
  const freigabe = f.art === "nicht_erreichbar" ? t("kabine.ablauf.freigabe.nicht_erreichbar", { zeit: uhrzeit(f.um) }) : t(`kabine.ablauf.freigabe.${f.art}`);
  return (
    <table className="kabine-ablauf" data-testid="kabine-ablauf">
      <thead>
        <tr>
          <th>{t("kabine.ablauf.freigabe.titel")}</th>
          <th>{t("kabine.ablauf.texte")}</th>
          <th>{t("kabine.ablauf.ton")}</th>
          <th>{t("kabine.ablauf.naechste")}</th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <td>{freigabe}</td>
          <td>
            {t("kabine.ablauf.abflug")}: {teilText(t, ablauf.abflug)}
            <br />
            {ablauf.halbzeit && ablauf.halbzeit.art !== "entfaellt" && (
              <>
                {t("kabine.ablauf.halbzeit")}: {teilText(t, ablauf.halbzeit)}
                <br />
              </>
            )}
            {t("kabine.ablauf.ankunft")}: {teilText(t, ablauf.ankunft)}
          </td>
          <td>{t("kabine.ablauf.ton_stand", { fertig: ablauf.ton_fertig, gesamt: ablauf.ton_gesamt })}</td>
          <td>
            {naechste
              ? `${slotName(t, naechste.slot, naechste.art)}, ${wannText(t, naechste)}`
              : t("kabine.ablauf.keine")}
            {stumm && naechste && <span className="kabine-ablauf-stumm"> · {t("kabine.ablauf.stumm")}</span>}
          </td>
        </tr>
      </tbody>
    </table>
  );
}

/** „Warum diese Ansagen“: Typ, Ablauf, Flugzeit, Sprachen. */
function ProfilZeile({ profil, sprachen, namen, kapitaen, t }: { profil: Profil; sprachen: string[]; namen: Intl.DisplayNames | null; kapitaen?: string | null; t: T }) {
  const teile: string[] = [];
  if (profil.muster) teile.push(profil.muster);
  if (kapitaen) teile.push(t("kabine.profil.kapitaen", { name: kapitaen }));
  if (profil.flugart === "linie" || profil.flugart === "bizjet") teile.push(t(`kabine.profil.${profil.flugart}`));
  if (profil.flugzeit_min !== null) {
    teile.push(t("kabine.profil.flugzeit", { h: Math.floor(profil.flugzeit_min / 60), m: profil.flugzeit_min % 60 }));
  }
  if (!profil.flugzeit_bekannt) teile.push(t("kabine.profil.flugzeit_unbekannt"));
  else if (profil.kurzstrecke) teile.push(t("kabine.profil.kurzstrecke"));
  if (sprachen.length > 0) teile.push(sprachen.map((s) => sprachName(s, namen)).join(t("kabine.profil.und")));
  if (sprachen.length > 1) teile.push(t("kabine.profil.stimmen"));
  for (const s of profil.sprachen_ohne_stimme) teile.push(t("kabine.profil.ohne_stimme", { sprache: sprachName(s, namen) }));
  return (
    <p className="kabine-profil" data-testid="kabine-profil">
      {teile.join(" · ")}
    </p>
  );
}

/** Ton oben auf der Seite: Stumm (nur bis zum Neustart) und Lautstärke,
 *  die sofort wirkt — ohne Umweg über die Einstellungen. Darunter der
 *  eigene Regler der Boarding-Musik (2.1.1, Design 19.10.2026). */
function TonLeiste({ stumm, lautstaerke, musik, t, onStumm }: { stumm: boolean; lautstaerke: number; musik: number | null; t: T; onStumm: (s: boolean) => void }) {
  const [wert, setWert] = useState(Math.round(lautstaerke * 100));
  const [zieht, setZieht] = useState(false);
  useEffect(() => {
    if (!zieht) setWert(Math.round(lautstaerke * 100));
  }, [lautstaerke, zieht]);
  const loslassen = () => {
    setZieht(false);
    void lautstaerkeSetzen(wert / 100, true);
  };
  return (
    <div className={`kabine-ton${stumm ? " ist-stumm" : ""}`} role="group" aria-label={t("kabine.ton.titel")} data-testid="kabine-ton">
      <div className="kabine-ton-zeile">
        <button
          type="button"
          className="kabine-stumm"
          aria-pressed={stumm}
          title={stumm ? t("kabine.ton.aufheben_titel") : t("kabine.ton.stumm_titel")}
          onClick={() => onStumm(!stumm)}
        >
          <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M11 5 6 9H3v6h3l5 4z" fill="currentColor" stroke="none" />
            {stumm ? (
              <path d="m16 9 5 6M21 9l-5 6" />
            ) : (
              <path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13" />
            )}
          </svg>
          <span>{stumm ? t("kabine.ton.stumm") : t("kabine.ton.ansagen")}</span>
        </button>
        <input
          type="range"
          min={0}
          max={100}
          step={5}
          value={wert}
          aria-label={t("kabine.ton.lautstaerke")}
          onChange={(ev) => {
            const v = Number(ev.target.value);
            setZieht(true);
            setWert(v);
            void lautstaerkeSetzen(v / 100, false);
          }}
          onMouseUp={loslassen}
          onTouchEnd={loslassen}
          onKeyUp={loslassen}
        />
        <span className="kabine-ton-wert">{wert} %</span>
      </div>
      {musik !== null && <MusikRegler lautstaerke={musik} t={t} />}
    </div>
  );
}

function wannText(t: T, g: Geplant): string {
  if (g.faellig) return g.wartet ? t(`kabine.wartet.${g.wartet}`) : t("kabine.zeit.jetzt");
  if (g.wartet === "anschnallzeichen") return t("kabine.wartet.anschnallzeichen");
  return g.wann !== null ? t("kabine.zeit.ca", { zeit: uhrzeit(g.wann) }) : t(`kabine.wann.${g.slot}`);
}

export function KabinenSeite({
  onOpenSettings,
  onAus,
}: {
  onOpenSettings: () => void;
  /** Nach „Ansagen ausschalten“: Reiter ausblenden. */
  onAus?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const live = useSyncExternalStore(abonnieren, aktuell);
  const [daten, setDaten] = useState<Ueberblick | null | undefined>(undefined);
  const [offen, setOffen] = useState<string | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [neu, setNeu] = useState(0);
  // Sofort umschalten, nicht erst beim nächsten Nachlesen aus Rust.
  const [stummJetzt, setStummJetzt] = useState<boolean | null>(null);
  const [musik, setMusik] = useState<MusikZustand | null>(null);
  const [musikNeu, setMusikNeu] = useState(0);
  useEffect(() => {
    let weg = false;
    const holen = () => void musikZustand().then((m) => !weg && setMusik(m)).catch(() => undefined);
    holen();
    const iv = setInterval(holen, 2000);
    return () => {
      weg = true;
      clearInterval(iv);
    };
  }, [musikNeu]);

  // Verlauf und Stand kommen aus Rust; alle drei Sekunden und nach jeder
  // neuen oder beendeten Ansage nachlesen.
  const ansageId = live.ansage?.id;
  useEffect(() => {
    let weg = false;
    const holen = () => void ueberblick().then((d) => !weg && setDaten(d)).catch(() => !weg && setDaten(null));
    holen();
    const iv = setInterval(holen, 3000);
    return () => {
      weg = true;
      clearInterval(iv);
    };
  }, [ansageId, neu]);

  let namen: Intl.DisplayNames | null = null;
  try {
    namen = new Intl.DisplayNames([i18n.language], { type: "language", languageDisplay: "dialect" });
  } catch {
    namen = null;
  }

  const a: Gespielt | null = live.ansage;
  const verlauf = [...(daten?.verlauf ?? [])].reverse();
  const naechste: Geplant[] = daten?.geplant ?? [];

  const gesperrt = a !== null || daten?.belegt === true;
  const stumm = stummJetzt ?? daten?.stumm ?? false;
  const stummUmschalten = (s: boolean) => {
    setStummJetzt(s);
    // Erst den neuen Stand aus Rust übernehmen, dann die sofortige Anzeige
    // lösen — sonst spränge der Knopf kurz auf den alten Stand zurück.
    void stummSetzen(s)
      .then(() => ueberblick())
      .then((d) => setDaten(d))
      .catch(() => undefined)
      .finally(() => setStummJetzt(null));
  };

  const hoeren = (slot: string, id?: number) => {
    setFehler(null);
    void anhoeren(slot, id).catch((e) => setFehler(formatIpcError(e)));
  };
  const nochmalVersuchen = () => {
    void erneut().then(() => setNeu((n) => n + 1));
  };
  const umschalten = (k: string) => setOffen((o) => (o === k ? null : k));

  return (
    <section className="kabine-seite">
      <header className="kabine-kopf">
        <div>
          <h2>{t("kabine.seite.titel")}</h2>
          <div className="kabine-unter">
            {daten === null || daten === undefined
              ? t("kabine.seite.kein_flug")
              : daten.grund
                ? t("kabine.seite.keine_ansagen")
                : daten.sprachen.length > 0
                  ? daten.sprachen.map((s) => sprachName(s, namen)).join(" · ")
                  : t("kabine.seite.warte_auf_texte")}
          </div>
        </div>
        <div className="kabine-kopf-knoepfe">
          {daten && <TonLeiste stumm={stumm} lautstaerke={daten.lautstaerke} musik={musik?.lautstaerke ?? null} t={t} onStumm={stummUmschalten} />}
          <button type="button" className="button button--ghost" onClick={onOpenSettings}>
            {t("kabine.seite.einstellungen")}
          </button>
          <button
            type="button"
            className="button button--ghost"
            onClick={() => {
              // Sofort still, dann dauerhaft aus (bis zum Wiedereinschalten
              // in den Einstellungen).
              stoppen();
              void einstellungenLesen()
                .then((e) => einstellungenSetzen({ ...e, an: false }))
                .then(() => onAus?.());
            }}
          >
            {t("kabine.seite.ausschalten")}
          </button>
        </div>
      </header>

      {/* Warum nichts kommt: oben und ausgeschrieben, nicht als leere Seite. */}
      {live.fehler && !a && (
        <div className="kabine-grund" role="status" data-testid="kabine-wiedergabe-fehler">
          <strong>{t("kabine.grund.wiedergabe.titel")}</strong>
          <p>{t("kabine.grund.wiedergabe.text")}</p>
          <p className="kabine-grund-detail">{live.fehler}</p>
        </div>
      )}
      {daten?.grund && (
        <div className="kabine-grund" role="status" data-testid="kabine-grund">
          <strong>{t(`kabine.grund.${daten.grund.grund}.titel`)}</strong>
          <p>
            {daten.grund.grund === "beta_nicht_erreichbar"
              ? t("kabine.grund.beta_nicht_erreichbar.text", { zeit: uhrzeit(daten.grund.detail) })
              : t(`kabine.grund.${daten.grund.grund}.text`)}
          </p>
        </div>
      )}

      {daten?.hinweis === "anschnallzeichen" && (
        <Notice
          tone="warn"
          level={t("kabine.hinweis.anschnallzeichen.titel")}
          data-testid="kabine-hinweis"
          detail={<span>{t("kabine.hinweis.anschnallzeichen.text")}</span>}
        />
      )}
      {daten && (
        <>
          <AblaufZeile ablauf={daten.ablauf} naechste={naechste[0]} stumm={stumm} t={t} />
          <ProfilZeile profil={daten.profil} sprachen={daten.sprachen} namen={namen} kapitaen={daten.kapitaen} t={t} />
        </>
      )}
      {musik && <MusikKasten zustand={musik} t={t} onNeu={() => setMusikNeu((n) => n + 1)} />}

      <div className={`kabine-live${a ? " ist-aktiv" : ""}`} aria-live="polite">
        <div className="kabine-live-kopf">
          <span className="kabine-sprecher">
            {a ? (a.vorschau ? t("kabine.seite.vorhoeren") : t(`kabine.spricht.${a.rolle}`)) : t("kabine.seite.ruhig")}
          </span>
          {a && <span className="kabine-slot">{slotName(t, a.slot)}</span>}
          {a?.vorschau && (
            <button type="button" className="button button--ghost kabine-knopf" onClick={anhoerenStoppen}>
              {t("kabine.seite.anhoeren_stoppen")}
            </button>
          )}
        </div>
        {/* Ohne Ansage eine ruhige Linie statt der letzten Messwerte. */}
        <Pegel werte={a ? live.verlauf : []} aktiv={a !== null} />
        {a && a.rolle !== "chime" && <Untertitel ansage={a} teil={live.teil} teilStart={live.teilStart} namen={namen} t={t} />}
        {!a && stumm && (
          <p className="kabine-ruhe-text kabine-stumm-hinweis" data-testid="kabine-stumm-hinweis">
            {t("kabine.seite.stumm_hinweis")}
          </p>
        )}
        {!a && !stumm && (
          <p className="kabine-ruhe-text">
            {naechste.length === 0
              ? t("kabine.seite.ruhe_hinweis")
              : t("kabine.seite.naechste_wann", { slot: slotName(t, naechste[0].slot, naechste[0].art), wann: wannText(t, naechste[0]) })}
          </p>
        )}
      </div>
      {fehler && (
        <p className="cpdlc-panel__error" role="status">
          {fehler}
        </p>
      )}

      <div className="kabine-spalten">
        <div>
          <h3 className="kabine-h3">{t("kabine.seite.als_naechstes")}</h3>
          {naechste.length === 0 ? (
            <div className="kabine-leer">{t("kabine.seite.nichts_wartet")}</div>
          ) : (
            <ul className="kabine-liste kabine-liste--rollbar" data-testid="kabine-naechste">
              {naechste.map((g) => {
                const k = `n-${g.slot}`;
                const auf = offen === k;
                const aufgegeben = g.stand.art === "text_aufgegeben" || g.stand.art === "ton_aufgegeben";
                return (
                  <li key={k} className={`kabine-plan${auf ? " ist-offen" : ""}`}>
                    <button type="button" className="kabine-plan-kopf" aria-expanded={auf} onClick={() => umschalten(k)}>
                      <span className="kabine-plan-text">
                        <span className="kabine-plan-slot">{slotName(t, g.slot, g.art)}</span>
                        <span className="kabine-plan-wann">{g.faellig || g.wartet ? wannText(t, g) : t("kabine.zeit.spielt", { wann: wannText(t, g) })}</span>
                      </span>
                      <span className={`kabine-plan-status ${standKlasse(g.stand)}`}>{g.rolle === "chime" && g.stand.art === "bereit" ? t("kabine.stand.bereit_gong") : standText(t, g.stand)}</span>
                    </button>
                    {auf && (
                      <div className="kabine-plan-inhalt">
                        <p className="kabine-ausloeser">
                          <span className="kabine-sprache">{t("kabine.ausloeser_titel")}</span> {t(`kabine.ausloeser.${g.slot}`)}
                        </p>
                        {g.rolle === "chime" ? (
                          <p className="kabine-verlauf-text">{t("kabine.seite.gong_text")}</p>
                        ) : g.teile.every((teil) => teil.text === null) ? (
                          <p className="kabine-verlauf-text">{t("kabine.seite.noch_kein_text")}</p>
                        ) : (
                          g.teile.map((teil) => (
                            <div key={teil.sprache} className="kabine-teil">
                              <span className="kabine-sprache">
                                {sprachName(teil.sprache, namen)}
                                {teil.keine_stimme && ` · ${t("kabine.seite.keine_stimme")}`}
                              </span>
                              <p className="kabine-verlauf-text">{teil.text ?? t("kabine.seite.noch_kein_text")}</p>
                            </div>
                          ))
                        )}
                        {"meldung" in g.stand && <p className="kabine-grund-detail">{g.stand.meldung}</p>}
                        <div className="kabine-plan-knoepfe">
                          {g.stand.art === "bereit" && (
                            <button type="button" className="button button--ghost kabine-knopf" disabled={gesperrt} onClick={() => hoeren(g.slot)}>
                              {t("kabine.seite.anhoeren")}
                            </button>
                          )}
                          {aufgegeben && (
                            <button type="button" className="button button--ghost kabine-knopf" onClick={nochmalVersuchen}>
                              {t("kabine.seite.erneut")}
                            </button>
                          )}
                        </div>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        <div>
          <h3 className="kabine-h3">{t("kabine.seite.bisher")}</h3>
          {verlauf.length === 0 ? (
            <div className="kabine-leer">{t("kabine.seite.noch_nichts")}</div>
          ) : (
            <ul className="kabine-liste kabine-liste--verlauf kabine-liste--rollbar" data-testid="kabine-bisher">
              {verlauf.map((g) => {
                const k = `b-${g.id}-${g.zeit}`;
                const auf = offen === k;
                // Ohne Ausgang: läuft (nach der Startmeldung) oder wird gerade gestartet.
                const ausgang = g.ergebnis === null ? (g.gestartet ? "laeuft" : "startet") : g.ergebnis !== "gespielt" ? g.ergebnis : null;
                const ausgangText =
                  ausgang === "entfallen"
                    ? t("kabine.ergebnis.entfallen_weil", { grund: t(`kabine.entfall.${g.grund ?? "sonst"}`) })
                    : ausgang && t(`kabine.ergebnis.${ausgang}`);
                return (
                  <li key={k} className={auf ? "ist-offen" : ""}>
                    <button type="button" className="kabine-plan-kopf" aria-expanded={auf} onClick={() => umschalten(k)}>
                      <span className="kabine-verlauf-kopf">
                        <span className="kabine-zeit">{uhrzeit(g.zeit)}</span>
                        <strong>{t(`kabine.slot.${g.slot}`)}</strong>
                        <span className="kabine-rolle">{t(`kabine.rolle.${g.rolle}`)}</span>
                      </span>
                      {ausgang && (
                        <span className={`kabine-plan-status ${ausgang === "laeuft" || ausgang === "startet" ? "ist-bereit" : "ist-warnung"}`}>{ausgangText}</span>
                      )}
                    </button>
                    {!auf && g.teile[0] && <p className="kabine-verlauf-text kabine-verlauf-text--kurz">{g.teile[0].text}</p>}
                    {auf && (
                      <div className="kabine-plan-inhalt">
                        {g.teile.map((teil) => (
                          <div key={teil.sprache} className="kabine-teil">
                            <span className="kabine-sprache">{sprachName(teil.sprache, namen)}</span>
                            <p className="kabine-verlauf-text">{teil.text}</p>
                          </div>
                        ))}
                        {/* Entfallene ohne Ton: nichts zum Anhören. */}
                        {g.teile.length > 0 && (
                          <div className="kabine-plan-knoepfe">
                            <button type="button" className="button button--ghost kabine-knopf" disabled={gesperrt} onClick={() => hoeren(g.slot, g.id)}>
                              {t("kabine.seite.nochmal")}
                            </button>
                          </div>
                        )}
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}
