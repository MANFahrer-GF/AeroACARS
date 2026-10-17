// Kabinenansagen (Beta): die Ansage-Seite. Oben der Ablauf (Freigabe →
// Texte → Ton → nächste Ansage), dann was gerade gesprochen wird (Sprecher,
// Text, Pegel live), darunter jede kommende und jede gelaufene Ansage als
// aufklappbare Zeile: Zeitpunkt, Stand, Text aller Sprachen, Anhören.
// Gleiche Seitenform wie das Bordbuch.
import { useEffect, useState, useSyncExternalStore } from "react";
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
  stoppen,
  ueberblick,
  type Ablauf,
  type Geplant,
  type Gespielt,
  type Profil,
  type Stand,
  type TeilStand,
  type Ueberblick,
} from "../../lib/kabinenansagen";
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

/** Ablaufzeile oben: Freigabe → Texte → Ton → nächste Ansage (Hausregel
 *  „Status als Tabelle oben“). */
function AblaufZeile({ ablauf, naechste, t }: { ablauf: Ablauf; naechste: Geplant | undefined; t: T }) {
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
            {t("kabine.ablauf.ankunft")}: {teilText(t, ablauf.ankunft)}
          </td>
          <td>{t("kabine.ablauf.ton_stand", { fertig: ablauf.ton_fertig, gesamt: ablauf.ton_gesamt })}</td>
          <td>
            {naechste
              ? `${t(`kabine.slot.${naechste.slot}`)}, ${wannText(t, naechste)}`
              : t("kabine.ablauf.keine")}
          </td>
        </tr>
      </tbody>
    </table>
  );
}

/** „Warum diese Ansagen“: Typ, Ablauf, Flugzeit, Sprachen. */
function ProfilZeile({ profil, sprachen, namen, t }: { profil: Profil; sprachen: string[]; namen: Intl.DisplayNames | null; t: T }) {
  const teile: string[] = [];
  if (profil.muster) teile.push(profil.muster);
  if (profil.flugart === "linie" || profil.flugart === "bizjet") teile.push(t(`kabine.profil.${profil.flugart}`));
  if (profil.flugzeit_min !== null) {
    teile.push(t("kabine.profil.flugzeit", { h: Math.floor(profil.flugzeit_min / 60), m: profil.flugzeit_min % 60 }));
  }
  if (!profil.flugzeit_bekannt) teile.push(t("kabine.profil.flugzeit_unbekannt"));
  else if (profil.kurzstrecke) teile.push(t("kabine.profil.kurzstrecke"));
  if (sprachen.length > 0) teile.push(sprachen.map((s) => sprachName(s, namen)).join(t("kabine.profil.und")));
  for (const s of profil.sprachen_ohne_stimme) teile.push(t("kabine.profil.ohne_stimme", { sprache: sprachName(s, namen) }));
  return (
    <p className="kabine-profil" data-testid="kabine-profil">
      {teile.join(" · ")}
    </p>
  );
}

function wannText(t: T, g: Geplant): string {
  if (g.faellig) return g.wartet ? t(`kabine.wartet.${g.wartet}`) : t("kabine.zeit.jetzt");
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

      {daten && (
        <>
          <AblaufZeile ablauf={daten.ablauf} naechste={naechste[0]} t={t} />
          <ProfilZeile profil={daten.profil} sprachen={daten.sprachen} namen={namen} t={t} />
        </>
      )}

      <div className={`kabine-live${a ? " ist-aktiv" : ""}`} aria-live="polite">
        <div className="kabine-live-kopf">
          <span className="kabine-sprecher">
            {a ? (a.vorschau ? t("kabine.seite.vorhoeren") : t(`kabine.spricht.${a.rolle}`)) : t("kabine.seite.ruhig")}
          </span>
          {a && <span className="kabine-slot">{t(`kabine.slot.${a.slot}`)}</span>}
          {a?.vorschau && (
            <button type="button" className="button button--ghost kabine-knopf" onClick={anhoerenStoppen}>
              {t("kabine.seite.anhoeren_stoppen")}
            </button>
          )}
        </div>
        {/* Ohne Ansage eine ruhige Linie statt der letzten Messwerte. */}
        <Pegel werte={a ? live.verlauf : []} aktiv={a !== null} />
        {a && a.rolle !== "chime" && (
          <ol className="kabine-teile">
            {a.teile.map((teil, i) => (
              <li key={teil.sprache} className={i === live.teil ? "ist-jetzt" : i < live.teil ? "ist-gesagt" : ""}>
                <span className="kabine-sprache">{sprachName(teil.sprache, namen)}</span>
                <p>{teil.text}</p>
              </li>
            ))}
          </ol>
        )}
        {!a && (
          <p className="kabine-ruhe-text">
            {naechste.length === 0
              ? t("kabine.seite.ruhe_hinweis")
              : t("kabine.seite.naechste_wann", { slot: t(`kabine.slot.${naechste[0].slot}`), wann: wannText(t, naechste[0]) })}
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
            <ul className="kabine-liste" data-testid="kabine-naechste">
              {naechste.map((g) => {
                const k = `n-${g.slot}`;
                const auf = offen === k;
                const aufgegeben = g.stand.art === "text_aufgegeben" || g.stand.art === "ton_aufgegeben";
                return (
                  <li key={k} className={`kabine-plan${auf ? " ist-offen" : ""}`}>
                    <button type="button" className="kabine-plan-kopf" aria-expanded={auf} onClick={() => umschalten(k)}>
                      <span className="kabine-plan-text">
                        <span className="kabine-plan-slot">{t(`kabine.slot.${g.slot}`)}</span>
                        <span className="kabine-plan-wann">{wannText(t, g)}</span>
                      </span>
                      <span className={`kabine-plan-status ${standKlasse(g.stand)}`}>{standText(t, g.stand)}</span>
                    </button>
                    {auf && (
                      <div className="kabine-plan-inhalt">
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
            <ul className="kabine-liste kabine-liste--verlauf" data-testid="kabine-bisher">
              {verlauf.map((g) => {
                const k = `b-${g.id}-${g.zeit}`;
                const auf = offen === k;
                const ausgang = g.ergebnis && g.ergebnis !== "gespielt" ? g.ergebnis : null;
                return (
                  <li key={k} className={auf ? "ist-offen" : ""}>
                    <button type="button" className="kabine-plan-kopf" aria-expanded={auf} onClick={() => umschalten(k)}>
                      <span className="kabine-verlauf-kopf">
                        <span className="kabine-zeit">{uhrzeit(g.zeit)}</span>
                        <strong>{t(`kabine.slot.${g.slot}`)}</strong>
                        <span className="kabine-rolle">{t(`kabine.rolle.${g.rolle}`)}</span>
                      </span>
                      {ausgang && <span className="kabine-plan-status ist-warnung">{t(`kabine.ergebnis.${ausgang}`)}</span>}
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
                        <div className="kabine-plan-knoepfe">
                          <button type="button" className="button button--ghost kabine-knopf" disabled={gesperrt} onClick={() => hoeren(g.slot, g.id)}>
                            {t("kabine.seite.nochmal")}
                          </button>
                        </div>
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
