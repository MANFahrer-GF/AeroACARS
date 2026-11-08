// Bilder-Tab (N-13): Aufbau nach dem Entwurf „AeroACARS Bilder-Tab“. Alles hier ist
// Paketinhalt; das Programm liefert nur die Befehle der Faehigkeit `bilder-1`.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri, listen } from "../../lib/ipc";
import { Button, Modal, Notice } from "../ui";
import { DetailSpalte } from "./DetailSpalte";
import { EinschaltenKarte, WegeKarte } from "./Erststart";
import { GruppenAnsicht } from "./GruppenAnsicht";
import { QuellenPanel } from "./QuellenPanel";
import { SendenDialog } from "./SendenDialog";
import { Symbol } from "./Symbole";
import { bilderStore, useBilder } from "./bilderStore";
import { bildUrlVergessen } from "./useBildUrl";
import { bereinigen, offenNachHaken, serieUmschalten, umschalten } from "./logik/auswahl";
import { GRENZEN } from "./logik/grenzen";
import { KEIN_FILTER, anzeigbar, bytesText, filtern, flugOptionenBauen, flugSchluessel, gesendeteAelterAls, gruppieren, seitenweise, type Filter, type Gruppe } from "./logik/gruppen";
import { reichtKontingent } from "./logik/kontingent";
import { kurzZeit, simulatorName } from "./logik/metaFormat";
import type { FlugAuswahl } from "./logik/store";
import type { FlugEintrag } from "./logik/typen";
import { bildZeitIso, useKachelTexte } from "./Kacheln";
import { wiederholText, zusammenfassung } from "./logik/sendezustand";
import "./bilder.css";

export interface AktiverFlug {
  pirep_id: string;
  callsign: string;
  dpt: string;
  arr: string;
}

export interface BilderTabProps {
  pilotId: string | null;
  pilotName: string | null;
  aktiverFlug: AktiverFlug | null;
  /** Simulator, der gerade verbunden ist (z. B. „xplane12“, „msfs2024“); steuert den Aufnahmehinweis. */
  simKind: string | null;
  onKarteOeffnen: (fokus: { lat: number; lon: number; id: string; pirep: string | null }) => void;
}

const BILD_ENDUNGEN = /\.(jpe?g|png)$/i;
/** Ab dieser Breite steht das Detail als feste Spalte, darunter als Blatt ueber der Liste (bilder.css). */
const breiteSpalte = () => typeof window.matchMedia !== "function" || window.matchMedia("(min-width: 1181px)").matches;

export function BilderTab({ pilotId, pilotName, aktiverFlug, simKind, onKarteOeffnen }: BilderTabProps) {
  const simulator: "xplane" | "msfs" | null = simKind?.startsWith("xplane") ? "xplane" : simKind?.startsWith("msfs") ? "msfs" : null;
  const { t, i18n } = useTranslation();
  const { phase: phaseText } = useKachelTexte();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "de";
  const z = useBilder();
  const liste = z.liste;
  const [filter, setFilter] = useState<Filter>(KEIN_FILTER);
  const [seite, setSeite] = useState(0);
  const [auswahl, setAuswahl] = useState<ReadonlySet<string>>(new Set());
  const [offenId, setOffenId] = useState<string | null>(null);
  const [quellenOffen, setQuellenOffen] = useState(false);
  const [sendenOffen, setSendenOffen] = useState(false);
  const [einschaltenOffen, setEinschaltenOffen] = useState(false);
  const [spaeter, setSpaeter] = useState(false);
  const [zuordnen, setZuordnen] = useState<string[] | null>(null);
  const [zuordnenWahl, setZuordnenWahl] = useState("");
  const [kandidaten, setKandidaten] = useState<FlugEintrag[] | null>(null);
  const [kandidatenHinweis, setKandidatenHinweis] = useState<"gekuerzt" | "unvollstaendig" | null>(null);
  const kandidatenLauf = useRef(0);
  const [ziehen, setZiehen] = useState(false);

  useEffect(() => { void bilderStore.pilotSetzen(pilotId); }, [pilotId]);
  useEffect(() => {
    void bilderStore.laden().then(() => bilderStore.fortsetzen());
    void bilderStore.kontingentHolen();
  }, []);

  const alle = useMemo(() => (liste?.eintraege ?? []).filter(anzeigbar), [liste]);
  const gefiltert = useMemo(() => filtern(liste?.eintraege ?? [], filter), [liste, filter]);
  const gruppen = useMemo(() => gruppieren(gefiltert, GRENZEN), [gefiltert]);
  const alleGruppen = useMemo(() => gruppieren(alle, GRENZEN), [alle]);
  const sichtbar = useMemo(() => seitenweise(gruppen, seite, GRENZEN.seite), [gruppen, seite]);
  useEffect(() => { if (seite !== sichtbar.seite) setSeite(sichtbar.seite); }, [seite, sichtbar.seite]);
  useEffect(() => { setAuswahl((a) => bereinigen(a, alle.map((e) => e.id))); }, [alle]);
  useEffect(() => {
    if (z.wiederAuswaehlen.length === 0) return;
    setAuswahl((a) => new Set([...a, ...z.wiederAuswaehlen]));
    bilderStore.wiederAuswaehlenLoeschen();
  }, [z.wiederAuswaehlen]);
  // Kommt der Server zurueck, wird die Warnung ohne Zutun des Piloten wieder frei.
  const serverAus = z.serverErreichbar === false;
  useEffect(() => {
    if (!serverAus) return;
    const takt = setInterval(() => void bilderStore.kontingentHolen(), 15_000);
    return () => clearInterval(takt);
  }, [serverAus]);

  const [anfangGeoeffnet, setAnfangGeoeffnet] = useState(false);
  useEffect(() => {
    // Beim ersten Laden das neueste Bild des laufenden Flugs (sonst das neueste) zeigen: die Detailspalte ist sonst leer
    if (anfangGeoeffnet || alle.length === 0) return;
    setAnfangGeoeffnet(true);
    // In schmalen Fenstern liegt die Detailspalte als Blatt ueber der Liste: nicht ungefragt aufschlagen
    if (!breiteSpalte()) return;
    const neueste = [...alle].sort((a, b) => (bildZeitIso(b) ?? "").localeCompare(bildZeitIso(a) ?? ""));
    const imFlug = aktiverFlug ? neueste.find((e) => flugSchluessel(e) === aktiverFlug.pirep_id) : undefined;
    setOffenId((imFlug ?? neueste[0]).id);
  }, [alle, anfangGeoeffnet, aktiverFlug]);
  const offen = alle.find((e) => e.id === offenId) ?? null;
  useEffect(() => { if (offenId && !offen && liste) setOffenId(null); }, [offenId, offen, liste]);
  const flugBilder = useMemo(() => (offen ? alle.filter((e) => flugSchluessel(e) === flugSchluessel(offen) && flugSchluessel(e) !== "") : []), [alle, offen]);

  // Dateien per Ziehen (Tauri meldet Pfade, die Webview bekommt keine Dateien)
  useEffect(() => {
    let weg: (() => void) | null = null;
    let aus = false;
    void listen<{ paths?: string[]; type?: string }>("tauri://drag-drop", (ev) => {
      const pfade = (ev.payload?.paths ?? []).filter((p) => BILD_ENDUNGEN.test(p));
      setZiehen(false);
      if (pfade.length > 0) void bilderStore.hinzufuegen(pfade);
    }).then((u) => { if (aus) u(); else weg = u; }).catch(() => undefined);
    void listen("tauri://drag-enter", () => setZiehen(true)).catch(() => undefined);
    void listen("tauri://drag-leave", () => setZiehen(false)).catch(() => undefined);
    return () => { aus = true; weg?.(); };
  }, []);

  const flugOptionen = useMemo(
    () => flugOptionenBauen(aktiverFlug, kandidaten ?? [], alleGruppen, sprache, t("bilder.gruppe.laeuft")),
    [aktiverFlug, alleGruppen, kandidaten, sprache, t],
  );

  const aufraeumen = useMemo(() => gesendeteAelterAls(alle, GRENZEN.aufraeumen_tage, Date.now()), [alle]);
  const fluegeAnzahl = alleGruppen.filter((g) => g.schluessel !== "").length;
  const gewaehlteBilder = alle.filter((e) => auswahl.has(e.id));
  const zus = zusammenfassung(z.sendestand, auswahl);

  const toggle = useCallback((id: string) => {
    const neu = umschalten(auswahl, id);
    setAuswahl(neu);
    setOffenId((o) => offenNachHaken(o, id, neu, breiteSpalte()));
  }, [auswahl]);

  const serieWahl = useCallback((ids: string[]) => {
    const neu = serieUmschalten(auswahl, ids);
    setAuswahl(neu);
    setOffenId((o) => offenNachHaken(o, ids[0], neu, breiteSpalte()));
  }, [auswahl]);

  // „Senden“ in der Detailspalte waehlt das Bild nur fuer den Dialog: ein Abbruch laesst keinen Haken zurueck.
  const nurFuerDialog = useRef(false);
  function sendenAbbrechen() {
    setSendenOffen(false);
    setEinschaltenOffen(false);
    if (nurFuerDialog.current) setAuswahl(new Set());
    nurFuerDialog.current = false;
  }

  /** Hauptaktion der Detailspalte: das offene Bild. Mit Auswahl sendet nur die Leiste. */
  function sendenDetail() {
    if (!offenId) return;
    nurFuerDialog.current = true;
    sendenMit([offenId]);
  }

  function sendenMit(ziel: string[]) {
    if (ziel.length === 0) return;
    setAuswahl(new Set(ziel));
    if (z.eingeschaltet !== true) { setEinschaltenOffen(true); return; }
    void bilderStore.kontingentHolen();
    setSendenOffen(true);
  }

  function erneut(id: string) {
    const s = z.sendestand[id];
    const gleiche = s ? Object.entries(z.sendestand).filter(([, x]) => x.phase === "fehlgeschlagen" && x.sendungId === s.sendungId).map(([k]) => k) : [id];
    setAuswahl(new Set(gleiche.length ? gleiche : [id]));
    if (z.eingeschaltet !== true) { setEinschaltenOffen(true); return; }
    void bilderStore.kontingentHolen();
    setSendenOffen(true);
  }

  async function senden_ausfuehren(text: string, stufe: typeof z.geoStufe) {
    setSendenOffen(false);
    nurFuerDialog.current = false;
    // Dieselbe Reihenfolge wie in der Vorschau des Dialogs.
    const ids = [...gewaehlteBilder].sort((a, b) => (bildZeitIso(a) ?? "").localeCompare(bildZeitIso(b) ?? "")).map((e) => e.id);
    const rest = await bilderStore.senden(ids, text, stufe, phaseText);
    setAuswahl(rest);
  }

  async function loeschen(ids: string[]) {
    const ziel = alle.filter((e) => ids.includes(e.id));
    if (ziel.length === 0) return;
    if (!await bilderStore.loeschen(ziel, t("bilder.loeschen.bezeichnung", { zeit: kurzZeit(bildZeitIso(ziel[0])) }))) return;
    bildUrlVergessen(ziel.flatMap((e) => [e.pfad, e.vorschau_pfad]));
    setAuswahl((a) => { const n = new Set(a); for (const id of ids) n.delete(id); return n; });
    if (offenId && ids.includes(offenId)) setOffenId(null);
  }

  async function ordnerZeigen() {
    if (offen) await bilderStore.ordnerZeigen(offen);
  }

  function zuordnenOeffnen(ids: string[], wahl: string) {
    setZuordnenWahl(wahl);
    setZuordnen(ids);
    setKandidaten(null);
    setKandidatenHinweis(null);
    const lauf = ++kandidatenLauf.current;
    const bild = alle.find((e) => e.id === ids[0]);
    void bilderStore.flugKandidaten(bild ? bildZeitIso(bild) : null).then((k) => {
      if (lauf !== kandidatenLauf.current) return;
      setKandidaten(k.fluege);
      setKandidatenHinweis(k.unvollstaendig ? "unvollstaendig" : k.gekuerzt ? "gekuerzt" : null);
    });
  }

  function zuordnenSpeichern() {
    const ziel = flugOptionen.find((o) => (o.pirep_id ?? "") === zuordnenWahl);
    const f: FlugAuswahl = ziel ?? { pirep_id: null, rufzeichen: null, abflug: null, ziel: null, flugzeug_icao: null };
    const ids = zuordnen ?? [];
    setZuordnen(null);
    void (async () => {
      for (const id of ids) {
        const e = alle.find((x) => x.id === id);
        if (e) await bilderStore.flugSetzen(e, f);
      }
    })();
  }

  // Die Screenshot-Taste ist je Pilot anders: der Hinweis nennt nur, was das Programm meldet, nie eine feste Taste.
  const xplaneTaste = liste?.simulator_taste?.text ?? null;
  const kuerzelText = liste?.tastenkuerzel?.verfuegbar ? liste.tastenkuerzel.text : null;
  const aufnahmeText = (() => {
    if (simulator === "xplane") return xplaneTaste ? t("bilder.kopf.aufnahme_xplane", { taste: xplaneTaste }) : t("bilder.kopf.aufnahme_neutral");
    if (simulator === "msfs") return kuerzelText ? t("bilder.kopf.aufnahme", { kuerzel: kuerzelText, sim: "MSFS" }) : null;
    return null;
  })();

  // Sobald ein Flug laeuft, ist die Meldung von vorher falsch.
  const keinFlugVeraltet = !!aktiverFlug && z.meldung?.schluessel === "bilder.meldung.kein_flug";
  useEffect(() => { if (keinFlugVeraltet) bilderStore.meldungLoeschen(); }, [keinFlugVeraltet]);

  const erststart = alle.length === 0;
  const zahlen = {
    bilder: alle.length === 1 ? t("bilder.kopf.anzahl_bild_eins") : t("bilder.kopf.anzahl_bilder", { n: alle.length }),
    fluege: fluegeAnzahl === 1 ? t("bilder.kopf.anzahl_flug_eins") : t("bilder.kopf.anzahl_fluege", { n: fluegeAnzahl }),
    platz: bytesText(liste?.belegt_bytes ?? 0, sprache),
  };
  const altbestandAnzahl = isTauri ? Math.max(liste?.altbestand.length ?? 0, (liste?.quellen ?? []).reduce((sum, q) => sum + q.altbestand, 0)) : 0;
  const einschaltenKarte = z.eingeschaltet === false && !spaeter;
  const beobachtete = (liste?.quellen ?? []).filter((q) => q.aktiv && q.beobachtet);
  const meldung = z.meldung;
  // Aeltere Programme melden keinen Nachlauf; das Feld fehlt dann.
  const nachlauf = !aktiverFlug && liste?.nachlauf && liste.nachlauf.bis_ms > Date.now() ? liste.nachlauf : null;
  const neueImFlug = aktiverFlug ? alle.filter((e) => flugSchluessel(e) === aktiverFlug.pirep_id).length : 0;

  return (
    <div className="bt" style={{ ["--tile-w" as string]: `${GRENZEN.tile_breite_px}px` }}>
      <header className="bt-kopf">
        <h1>{t("bilder.titel")}</h1>
        <span className="bt-kopf__zahlen" title={alle.length > 0 ? t("bilder.kopf.zusammenfassung", zahlen) : undefined}>
          {alle.length > 0 ? t("bilder.kopf.zusammenfassung_kurz", zahlen) : t("bilder.kopf.keine")}
        </span>
        <div className="bt-kopf__rechts">
          {!erststart && <div className="bt-chips" role="group" aria-label={t("bilder.liste.filter.alle")}>
            {(["alle", "offen", "gesendet"] as const).map((k) => (
              <button key={k} type="button" className={`bt-chip${filter.gesendet === (k === "alle" ? "alle" : k) ? " bt-chip--an" : ""}`} aria-pressed={filter.gesendet === k} onClick={() => { setFilter({ ...filter, gesendet: k }); setSeite(0); }}>
                {t(`bilder.liste.filter.${k}`)}
              </button>
            ))}
            <button type="button" className={`bt-chip${filter.favoriten ? " bt-chip--an" : ""}`} aria-pressed={filter.favoriten} onClick={() => { setFilter({ ...filter, favoriten: !filter.favoriten }); setSeite(0); }}>
              {t("bilder.liste.filter.favoriten")}
            </button>
          </div>}
          {!erststart && <label className="bt-suche">
            {Symbol.suche}
            <input type="search" value={filter.suche} placeholder={t("bilder.kopf.suche_platzhalter")} aria-label={t("bilder.liste.suche_label")} onChange={(ev) => { setFilter({ ...filter, suche: ev.target.value }); setSeite(0); }} />
          </label>}
          {!erststart && aufnahmeText && <span className="bt-chip bt-chip--aufnahme" title={aufnahmeText}>{Symbol.kamera}{simulator === "xplane" && xplaneTaste ? t("bilder.kopf.aufnahme_xplane_kurz", { taste: xplaneTaste }) : aufnahmeText}</span>}
          {liste?.aufnahme_verfuegbar && simulator === "msfs" && (
            <Button size="sm" variant="primary" iconLeft={Symbol.kamera} onClick={() => void bilderStore.aufnehmen()}>{t("bilder.kopf.aufnehmen")}</Button>
          )}
          {isTauri && <Button size="sm" iconLeft={Symbol.plus} aria-label={t("bilder.kopf.hinzufuegen")} title={t("bilder.kopf.hinzufuegen")} onClick={() => void bilderStore.hinzufuegen()} />}
          {isTauri && <Button size="sm" iconLeft={Symbol.regler} aria-label={t("bilder.kopf.quellen")} title={t("bilder.kopf.quellen")} aria-pressed={quellenOffen} onClick={() => { setQuellenOffen((o) => !o); if (!quellenOffen) void bilderStore.quellenFinden(); }} />}
          {isTauri && !erststart && <Button size="sm" iconLeft={Symbol.ordner} aria-label={t("bilder.kopf.ordner_zeigen")} title={t("bilder.kopf.ordner_zeigen")} disabled={!offen} onClick={() => void ordnerZeigen()} />}
        </div>
      </header>

      {meldung && (
        <div className="bt-meldungen">
          <Notice tone={meldung.art === "ok" ? "success" : "error"} detail={t(meldung.schluessel, { ...meldung.werte, ...(meldung.werte?.grundText ? { grund: t(String(meldung.werte.grundText)) } : {}) })} actions={<Button size="sm" variant="quiet" onClick={() => bilderStore.meldungLoeschen()} aria-label={t("bilder.meldung.schliessen")}>{Symbol.zu}</Button>} />
        </div>
      )}
      {(z.fehler || z.uebernahme.fehler > 0 || z.uebernahme.verworfen > 0 || z.uebernahme.duplikate > 0 || z.uebernahme.veraltet > 0) && (
        <div className="bt-meldungen">
          {z.fehler && <Notice tone="error" detail={t("bilder.meldung.geladen_fehler", { fehler: z.fehler })} />}
          {z.uebernahme.fehler > 0 && <Notice tone="warn" detail={t("bilder.uebernahme.fehler", { fehler: z.uebernahme.letzterFehler ?? "" })} />}
          {z.uebernahme.verworfen > 0 && <Notice tone="info" detail={t("bilder.uebernahme.verworfen", { n: z.uebernahme.verworfen })} />}
          {z.uebernahme.duplikate > 0 && <Notice tone="info" detail={t("bilder.uebernahme.duplikate", { n: z.uebernahme.duplikate })} />}
          {z.uebernahme.veraltet > 0 && <Notice tone="info" detail={t("bilder.uebernahme.veraltet", { n: z.uebernahme.veraltet })} />}
        </div>
      )}

      {!aktiverFlug && erststart ? null : aktiverFlug ? (
        <div className="bt-band bt-band--aktiv">
          <span className="bt-band__punkt" aria-hidden="true" />
          <span className="bt-band__fett">{t("bilder.band.aktiv")}</span>
          <span className="bt-acars">{aktiverFlug.callsign}</span>
          <span>{aktiverFlug.dpt}{"–"}{aktiverFlug.arr}</span>
          <span className="bt-band__dim">
            {beobachtete.length > 0
              ? t("bilder.band.beobachtet", { sim: simKind ? simulatorName(simKind) : "", pfad: beobachtete.map((q) => q.pfad.split(/[\\/]/).slice(-2).join("/")).join(", ") }).replace(/^ · /, "")
              : t("bilder.band.keine_quelle")}
            {" · "}
            {simulator === "msfs" ? (kuerzelText ? t("bilder.band.taste_msfs", { kuerzel: kuerzelText }) : t("bilder.band.taste_msfs_neutral")) : xplaneTaste ? t("bilder.band.taste_xplane_bekannt", { taste: xplaneTaste }) : t("bilder.band.taste_xplane")}
          </span>
          <span className="bt-band__rechts">{neueImFlug === 1 ? t("bilder.band.neue_eins") : t("bilder.band.neue", { count: neueImFlug })}</span>
        </div>
      ) : nachlauf ? (
        <div className="bt-band bt-band--aktiv">
          <span className="bt-band__punkt" aria-hidden="true" />
          <span className="bt-band__fett">{t("bilder.band.nachlauf")}</span>
          {nachlauf.rufzeichen && <span className="bt-acars">{nachlauf.rufzeichen}</span>}
          <span>{nachlauf.abflug}{"–"}{nachlauf.ziel}</span>
          <span className="bt-band__dim">{t("bilder.band.nachlauf_text", { zeit: kurzZeit(new Date(nachlauf.bis_ms).toISOString()) })}</span>
        </div>
      ) : (
        <div className="bt-band bt-band--ruhe">
          <span className="bt-band__fett" style={{ color: "var(--text-dim)" }}>{t("bilder.band.kein_flug")}</span>
          <span>{t("bilder.band.kein_flug_text")}</span>
        </div>
      )}

      {quellenOffen && liste && <QuellenPanel liste={liste} vorschlaege={z.vorschlaege} store={bilderStore} simulator={simulator} yokeLernt={z.yokeLernt} onZu={() => setQuellenOffen(false)} />}

      {erststart ? (
        <div className="bt-erst">
          {einschaltenKarte && <EinschaltenKarte onEinschalten={() => void bilderStore.einschalten(true)} onSpaeter={() => setSpaeter(true)} />}
          <WegeKarte xplaneTaste={xplaneTaste} msfsTaste={kuerzelText} altbestand={altbestandAnzahl} arbeitet={z.arbeitet} onAlt={() => void bilderStore.altbestandAnzeigen().then(() => bilderStore.altbestandUebernehmen())} />
        </div>
      ) : (
        <div className="bt-haupt">
          <section className="bt-liste" data-ziehen={ziehen ? "an" : undefined} aria-label={t("bilder.titel")}>
            {gruppen.length === 0 && <div className="bt-leerhinweis"><strong>{t("bilder.leer.kein_treffer")}</strong></div>}
            {altbestandAnzahl > 0 && (
              <div className="bt-alt">
                <div className="bt-alt__text">
                  <span>{t("bilder.altbestand.titel", { n: altbestandAnzahl })}</span>
                  {z.altbestandErgebnis && <span>{t("bilder.altbestand.ergebnis", { mit: z.altbestandErgebnis.mit_daten, flug: z.altbestandErgebnis.nur_flug, ohne: z.altbestandErgebnis.ohne_daten })}</span>}
                </div>
                <Button disabled={z.arbeitet} onClick={() => void bilderStore.altbestandAnzeigen().then(() => bilderStore.altbestandUebernehmen())}>
                  {z.arbeitet ? t("bilder.altbestand.laeuft") : t("bilder.altbestand.uebernehmen")}
                </Button>
              </div>
            )}
            {aufraeumen.length > 0 && (
              <div className="bt-alt">
                <div className="bt-alt__text">
                  <span>{t("bilder.aufraeumen.titel", { n: aufraeumen.length, tage: GRENZEN.aufraeumen_tage })}</span>
                  <span>{t("bilder.aufraeumen.text", { platz: bytesText(aufraeumen.reduce((sum, e) => sum + e.belegt_bytes, 0), sprache) })}</span>
                </div>
                <Button onClick={() => { const neu = new Set(aufraeumen.map((e) => e.id)); setAuswahl(neu); setOffenId((o) => offenNachHaken(o, aufraeumen[aufraeumen.length - 1].id, neu, breiteSpalte())); }}>{t("bilder.aufraeumen.auswaehlen")}</Button>
              </div>
            )}
            <GruppenAnsicht
              gruppen={sichtbar.gruppen}
              sendestand={z.sendestand}
              auswahl={auswahl}
              offenId={offenId}
              aktivePirep={aktiverFlug?.pirep_id ?? null}
              jetzt={Date.now()}
              onWahl={toggle}
              onSerieWahl={serieWahl}
              onOeffnen={(e) => setOffenId((a) => (a === e.id ? null : e.id))}
              onErneut={erneut}
              onZuordnen={(g: Gruppe) => zuordnenOeffnen(g.kacheln.flatMap((k) => (k.art === "bild" ? [k.bild.id] : k.bilder.map((b) => b.id))), "")}
              herkunft={(g) => {
                const e = alle.find((x) => flugSchluessel(x) === g.schluessel && x.sidecar?.herkunft);
                return e?.sidecar?.herkunft ?? null;
              }}
            />
            {sichtbar.seiten > 1 && (
              <nav className="bt-seiten" aria-label={t("bilder.liste.seite", { seite: sichtbar.seite + 1, seiten: sichtbar.seiten })}>
                <Button size="sm" disabled={sichtbar.seite === 0} onClick={() => setSeite(sichtbar.seite - 1)}>{t("bilder.liste.zurueck")}</Button>
                <span>{t("bilder.liste.seite", { seite: sichtbar.seite + 1, seiten: sichtbar.seiten })}</span>
                <Button size="sm" disabled={sichtbar.seite >= sichtbar.seiten - 1} onClick={() => setSeite(sichtbar.seite + 1)}>{t("bilder.liste.vor")}</Button>
              </nav>
            )}
          </section>
          <DetailSpalte
            e={offen}
            gewaehlt={offen ? auswahl.has(offen.id) : false}
            flugBilder={flugBilder}
            aktivePirep={aktiverFlug?.pirep_id ?? null}
            einzelAktionen={auswahl.size === 0}
            onSenden={sendenDetail}
            mitOrdner={isTauri}
            onOrdner={() => void ordnerZeigen()}
            onLoeschen={() => offen && void loeschen([offen.id])}
            onText={(text) => offen && void bilderStore.textSetzen(offen, text)}
            onFavorit={(an) => offen && void bilderStore.favoritSetzen(offen, an)}
            onFlugAendern={() => { if (offen) zuordnenOeffnen([offen.id], flugSchluessel(offen)); }}
            onKarteOeffnen={onKarteOeffnen}
            onMarke={(id) => setOffenId(id)}
            onZu={() => setOffenId(null)}
          />
        </div>
      )}

      {auswahl.size > 0 && (
        <footer className="bt-leiste">
          <span className="bt-leiste__fett">{t("bilder.leiste.ausgewaehlt", { count: auswahl.size })}</span>
          <span className="bt-leiste__info">
            {t("bilder.leiste.info", { groesse: bytesText(gewaehlteBilder.reduce((s, e) => s + e.groesse, 0), sprache), max: GRENZEN.max_je_sendung })}
            {" · "}
            {z.kontingent ? (z.kontingent.rest > 0 ? t("bilder.kontingent.rest", { rest: z.kontingent.rest, grenze: z.kontingent.grenze }) : t("bilder.kontingent.leer", { minuten: Math.max(1, Math.ceil(z.kontingent.retry_after / 60)) })) : t("bilder.kontingent.unbekannt")}
          </span>
          {auswahl.size > GRENZEN.max_je_sendung && <span className="bt-leiste__warn">{t("bilder.senden.zu_viele", { max: GRENZEN.max_je_sendung })}</span>}
          {z.serverErreichbar === true && <span className="bt-leiste__server">{t("bilder.leiste.server_ok")}</span>}
          {serverAus && <span className="bt-leiste__server bt-leiste__server--aus">{t("bilder.leiste.nicht_erreichbar")}</span>}
          {zus.fehlgeschlagen > 0 && <span className="bt-leiste__warn">{t("bilder.leiste.fehlgeschlagen", { count: zus.fehlgeschlagen })}</span>}
          {zus.unklar && <span className="bt-leiste__warn">{t("bilder.leiste.unklar")}</span>}
          {zus.sendet > 0 && <span className="bt-leiste__info">{t("bilder.leiste.sendet", { count: zus.sendet })}</span>}
          <div className="bt-leiste__rechts">
            <Button variant="quiet" onClick={() => setAuswahl(new Set())}>{t("bilder.leiste.aufheben")}</Button>
            {isTauri && <Button iconLeft={Symbol.herunter} onClick={() => void bilderStore.exportieren([...auswahl])}>{t("bilder.leiste.exportieren")}</Button>}
            <Button variant="danger" iconLeft={Symbol.papierkorb} onClick={() => void loeschen([...auswahl])}>{t("bilder.leiste.papierkorb")}</Button>
            <Button variant="primary" iconLeft={Symbol.senden} disabled={auswahl.size > GRENZEN.max_je_sendung || !reichtKontingent(auswahl.size, z.kontingent)} onClick={() => sendenMit([...auswahl])}>
              {auswahl.size === 1 ? t("bilder.leiste.senden_eins") : t("bilder.leiste.senden_n", { count: auswahl.size })}
            </Button>
          </div>
        </footer>
      )}

      <SendenDialog
        offen={sendenOffen}
        bilder={gewaehlteBilder.filter((e) => e.sidecar?.meta).sort((a, b) => (bildZeitIso(a) ?? "").localeCompare(bildZeitIso(b) ?? ""))}
        pilotName={pilotName}
        startText={wiederholText(gewaehlteBilder, z.sendestand) ?? gewaehlteBilder.map((e) => e.sidecar?.text ?? "").filter(Boolean).join(" ").slice(0, GRENZEN.text_max_zeichen)}
        geoStufe={z.geoStufe}
        kontingent={z.kontingent}
        serverAus={serverAus}
        onGeo={(s) => void bilderStore.geoStufeSetzen(s)}
        onSenden={(text, stufe) => void senden_ausfuehren(text, stufe)}
        onZu={sendenAbbrechen}
      />

      <Modal open={einschaltenOffen} onClose={sendenAbbrechen} size="md" title={t("bilder.einschalten.titel")} closeLabel={t("bilder.senden.zu")}>
        <EinschaltenKarte onEinschalten={() => { void bilderStore.einschalten(true).then(() => { setEinschaltenOffen(false); setSendenOffen(true); void bilderStore.kontingentHolen(); }); }} onSpaeter={sendenAbbrechen} />
      </Modal>

      <Modal
        open={zuordnen !== null}
        onClose={() => setZuordnen(null)}
        size="sm"
        title={t("bilder.detail.flug.waehlen")}
        closeLabel={t("bilder.senden.zu")}
        footer={<><Button variant="quiet" onClick={() => setZuordnen(null)}>{t("bilder.senden.abbrechen")}</Button><Button variant="primary" onClick={zuordnenSpeichern}>{t("bilder.detail.flug.fertig")}</Button></>}
      >
        <label className="bt-label" htmlFor="bt-flugwahl">{t("bilder.detail.flug.label")}</label>
        <select id="bt-flugwahl" className="bt-wahl" style={{ width: "100%", maxWidth: "none", marginTop: 6 }} value={zuordnenWahl} onChange={(ev) => setZuordnenWahl(ev.target.value)}>
          <option value="">{t("bilder.detail.flug.ohne")}</option>
          {flugOptionen.map((o) => <option key={o.pirep_id ?? ""} value={o.pirep_id ?? ""}>{o.label}</option>)}
        </select>
        {kandidaten === null && <span className="bt-hinweis">{t("bilder.detail.flug.laedt")}</span>}
        {kandidaten !== null && kandidatenHinweis && <span className="bt-hinweis" data-warn={kandidatenHinweis === "unvollstaendig" ? "an" : undefined}>{t(`bilder.detail.flug.${kandidatenHinweis}`)}</span>}
      </Modal>
    </div>
  );
}
