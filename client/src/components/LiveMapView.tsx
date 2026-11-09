// v0.13.x — In-App Live-Map (Stratos-orientiert, AeroACARS-Identität).
//
// Ansichten:
//   • "own" — eigener aktiver Flug: geplante Route (gestrichelt + Wegpunkt-Dots
//     + TOC/TOD aus dem SimBrief-Navlog), geflogener Track (solide, app-weit ab
//     Flugstart akkumuliert), Flugzeug-Marker (heading-gedreht), Dep/Arr-Pins,
//     Stats-Leiste, Log-Panel, Follow-Zoom.
//   • "va"  — VA-Übersicht: alle aktiven Piloten (Proxy auf phpVMS /api/acars).
//
// Theme-aware: dunkle (dark-matter) bzw. helle (positron) CARTO-Basemap, die mit
// dem App-Theme (data-theme) umschaltet; Overlay-Farben aus CSS-Vars.
// Rein Anzeige — keine Wertung. Die Kartenebenen liegen als Bausteine in `livekarte/`.

import { BildGross } from "./bilder/BildGross";
import { useBildKarte, type BildFokus } from "./bilder/useBildKarte";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, Bid, SimSnapshot } from "../types";
import { setTrack } from "../lib/trackStore";
import { aircraftSvg } from "../lib/aircraftIcon";
import { phaseColor, phaseLabel as formatPhase } from "../lib/phaseColors";
import { displayCallsign } from "../lib/callsign";
import { simKindLabel } from "../lib/simKind";
import { useMapEvents, LiveMapEventList } from "./LiveMapEvents";
import { LiveMapEmptyState, nextBidInfo, type NextBidInfo } from "./LiveMapEmptyState";
import { LiveRecordingIndicator } from "./LiveRecordingIndicator";
import { kartenAnfrage, useKartengrundlage } from "./BasemapContext";
import { grundkarteBauen, grundkarteHolen, startStil, type Grundkartensteuerung } from "./livekarte/grundkarte";
import { useLiveZugangTakt } from "../lib/liveInhalt";
import { ohneDatumsgrenzenSprung } from "../lib/datumsgrenze";
import { overlayFarben } from "./livekarte/farben";
import { flugDatenSetzen, flugEbenenAnlegen, LYR_ROUTE_CASING, TRACK_LAYERS, type RouteFix } from "./livekarte/flugEbenen";
import { fremdeRoutenAnlegen } from "./livekarte/fremdeRouten";
import { netzEbenenAnlegen, useNetzKarte } from "./livekarte/netzKarte";
import { TAXI_LAYERS, taxiEbenenAnlegen, useTaxiKarte } from "./livekarte/taxiKarte";
import { useVaVerkehr } from "./livekarte/vaVerkehr";
import { distNm } from "./livekarte/werkzeug";

// Die Stil-Adressen kommen vom Server — siehe `BasemapContext`. CARTO
// verlangt seit dem 26.08.2026 einen Schlüssel, und der soll austauschbar
// sein, ohne dass jeder Pilot ein Update braucht. Aufbau: `livekarte/grundkarte.ts`.

interface Aircraft {
  lon: number;
  lat: number;
  hdg: number;
}

/** Die Symbole der Kartenleiste — Tabler Icons (MIT), eingebettet.
 *
 *  Zwei Fehlversuche vorher, beide von Thomas kassiert: erst Unicode-
 *  Zeichen (aus verschiedenen Schriftschnitten, unterschiedlich hoch, und
 *  ein Motiv stand doppelt), dann selbst gezeichnete Pfade. Beides war
 *  Ersatz fuer etwas, das laengst abgestimmt war — die Vorschau zeigte
 *  Tabler-Symbole, und die gibt es als offene Pfade.
 *
 *  Eingebettet statt nachgeladen: die Leiste laeuft im Fenster einer
 *  Anwendung, ein Netzzugriff fuer acht Glyphen waere eine Abhaengigkeit
 *  ohne Gegenwert. Quelle: github.com/tabler/tabler-icons, MIT.
 */
function Sym({ pfad }: { pfad: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="15"
      height="15"
      aria-hidden="true"
      focusable="false"
      style={{ display: "block" }}
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      dangerouslySetInnerHTML={{ __html: pfad }}
    />
  );
}

/** Tabler Icons (MIT). Jedes Motiv genau einmal — ausser dem Flugzeug,
 *  das sowohl "Kurs oben" als auch "VA-Verkehr" traegt; die beiden stehen
 *  in verschiedenen Gruppen und sind durch Beschriftung getrennt. */
const PFAD: Record<string, string> = {
  karte: "<path d=\"M3 7l6 -3l6 3l6 -3v13l-6 3l-6 -3l-6 3v-13\" /> <path d=\"M9 4v13\" /> <path d=\"M15 7v13\" />",
  satellit: "<path d=\"M3.707 6.293l2.586 -2.586a1 1 0 0 1 1.414 0l5.586 5.586a1 1 0 0 1 0 1.414l-2.586 2.586a1 1 0 0 1 -1.414 0l-5.586 -5.586a1 1 0 0 1 0 -1.414\" /> <path d=\"M6 10l-3 3l3 3l3 -3\" /> <path d=\"M10 6l3 -3l3 3l-3 3\" /> <path d=\"M12 12l1.5 1.5\" /> <path d=\"M14.5 17a2.5 2.5 0 0 0 2.5 -2.5\" /> <path d=\"M15 21a6 6 0 0 0 6 -6\" />",
  norden: "<path d=\"M12 5l0 14\" /> <path d=\"M18 11l-6 -6\" /> <path d=\"M6 11l6 -6\" />",
  kurs: "<path d=\"M16 10h4a2 2 0 0 1 0 4h-4l-4 7h-3l2 -7h-4l-2 2h-3l2 -4l-2 -4h3l2 2h4l-2 -7h3l4 7\" />",
  zentrieren: "<path d=\"M4 8v-2a2 2 0 0 1 2 -2h2\" /> <path d=\"M4 16v2a2 2 0 0 0 2 2h2\" /> <path d=\"M16 4h2a2 2 0 0 1 2 2v2\" /> <path d=\"M16 20h2a2 2 0 0 0 2 -2v-2\" /> <path d=\"M9 12l6 0\" /> <path d=\"M12 9l0 6\" />",
  track: "<path d=\"M3 19a2 2 0 1 0 4 0a2 2 0 0 0 -4 0\" /> <path d=\"M19 7a2 2 0 1 0 0 -4a2 2 0 0 0 0 4\" /> <path d=\"M11 19h5.5a3.5 3.5 0 0 0 0 -7h-8a3.5 3.5 0 0 1 0 -7h4.5\" />",
  // Rollweg: "traffic-cone". Vorher stand hier "building-airport" — das
  // heisst aber "Flughafen", nicht "Rollweg", und seine acht Striche liefen
  // bei 15 px zu einem Fleck zusammen. Ein Weg-Motiv verbietet sich: der
  // naechstbeste Kandidat ("arrow-guide") traegt Punkt und Knick wie das
  // Track-Symbol daneben. Der Hut meint den Bodenbereich, nicht den Weg —
  // vier Striche, die auch klein noch eine Silhouette haben.
  taxi: "<path d=\"M4 20l16 0\" /> <path d=\"M9.4 10l5.2 0\" /> <path d=\"M7.8 15l8.4 0\" /> <path d=\"M6 20l5 -15h2l5 15\" />",
  bilder: "<path d=\"M4 8h3l2-3h6l2 3h3v11H4z\" /> <circle cx=\"12\" cy=\"13\" r=\"3.5\" />",
  va: "<path d=\"M16 10h4a2 2 0 0 1 0 4h-4l-4 7h-3l2 -7h-4l-2 2h-3l2 -4l-2 -4h3l2 2h4l-2 -7h3l4 7\" />",
};

function readTheme(): "dark" | "light" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}
// Phasenabhängiger Folge-Zoom: am Boden nah dran, im Reiseflug weit.
// Abgestimmt auf die FlightPhase-Strings (snake_case) aus dem Backend.
// Gibt null zurück, wenn die Phase unbekannt ist → Höhen-Fallback.
function zoomForPhase(phase: string): number | null {
  const p = phase.toLowerCase();
  if (/preflight|boarding|board|pushback|blocks_on|arrived|pirep|gate|park|stand/.test(p)) return 14;
  if (/taxi/.test(p)) return 13;
  if (/takeoff|take-off|departure/.test(p)) return 11;
  if (/climb/.test(p)) return 7.5;
  if (/cruise/.test(p)) return 5;
  if (/descent|descend/.test(p)) return 7.5;
  if (/approach|final/.test(p)) return 9;
  if (/landing|flare|rollout|touch/.test(p)) return 12.5;
  // holding & alles andere → Höhen-Fallback (richtet sich nach der Flughöhe)
  return null;
}
// Folge-Zoom für echte Flüge: Phase zuerst, sonst nach Höhe (MSL ft).
function targetFollowZoom(phase: string, altMslFt?: number | null): number {
  const z = zoomForPhase(phase);
  if (z != null) return z;
  if (altMslFt == null || Number.isNaN(altMslFt)) return 6.5;
  if (altMslFt < 1500) return 12.5;
  if (altMslFt < 10000) return 9;
  if (altMslFt < 22000) return 7.5;
  return 5;
}

interface Props {
  activeFlight: ActiveFlightInfo | null;
  simSnapshot: SimSnapshot | null;
  /** For the System-Zeile's "Recorder aktiv · {Simulator} · …" (README §7) —
   *  the same value App.tsx's own sidebar status pill already shows. */
  simKind: string | undefined;
  onSwitchToBriefing: () => void;
  /** Liegt der Karten-Reiter gerade vorn?
   *
   *  Die Karte bleibt seit v1.6.11 ueber Reiterwechsel hinweg eingehaengt —
   *  sie neu aufzubauen kostete jedes Mal Stil, Schriften und Kacheln von
   *  CARTO/ArcGIS, und genau das war die gemeldete Traegheit. Verdeckt braucht
   *  sie aber nicht weiterzupollen: die Streckenaufzeichnung laeuft ohnehin im
   *  Rust-Teil (`record_track_point`, fenster-unabhaengig), die Karte LIEST sie
   *  nur. Pausieren verliert also keine Daten.
   *
   *  Optional mit Vorgabe `true`, damit Einbindungen ohne diese Angabe
   *  (Tests, Vorschau) sich unveraendert verhalten. */
  sichtbar?: boolean;
  /** Stelle, die im Bilder-Tab angeklickt wurde (P-11.36): die Karte fliegt dorthin. */
  bildFokus?: BildFokus | null;
}

export function LiveMapView({ activeFlight, simSnapshot, simKind, onSwitchToBriefing, sichtbar = true, bildFokus = null }: Props) {
  const grundlage = useKartengrundlage();
  const zugangTakt = useLiveZugangTakt();
  // Die Karte wird einmalig gebaut; der Schlüssel kommt erst danach vom
  // Server. Über diesen Verweis liest `kartenAnfrage` bei jeder Kachel
  // den aktuellen Stand — siehe dort.
  const grundlageRef = useRef(grundlage);
  grundlageRef.current = grundlage;
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const acMarkerRef = useRef<maplibregl.Marker | null>(null);
  const pinMarkersRef = useRef<maplibregl.Marker[]>([]);
  const acCatRef = useRef<string | null>(null); // zuletzt gerendertes Eigen-Flieger-Icon (ICAO)
  // v0.15.8: „beim bewussten Einschalten von Follow EINMAL den phasen-passenden
  // Zoom setzen" — danach bleibt der manuelle Zoom des Nutzers erhalten (laufendes
  // Folgen schwenkt nur noch). Startet true, damit die erste Aktivierung greift.
  const followEngageRef = useRef(true);
  // v0.15.6: verhindert, dass ein Nutzer-Pan (der Follow ausschaltet) die
  // „auf-die-ganze-Route-zoomen"-Logik auslöst. Nur BEWUSSTES Abhaken von Follow
  // soll auf die Route fitten — ein Pan lässt die Ansicht einfach stehen.
  const suppressRouteFitRef = useRef(false);
  const grundkarteRef = useRef<Grundkartensteuerung | null>(null);

  const [mapReady, setMapReady] = useState(false);
  const [follow, setFollow] = useState(true);
  // Basemap: "auto" = theme-gekoppelt (dark/light), "sat" = Esri-Satellit (manuell).
  const [basemap, setBasemap] = useState<"auto" | "sat">(
    () => (typeof localStorage !== "undefined" && localStorage.getItem("aaLivemapBasemap") === "sat" ? "sat" : "auto"),
  );

  // Karte in Flugrichtung drehen (Track-up). BEWUSST abschaltbar und
  // standardmäßig AUS: Eine sich mitdrehende Karte ist beim Fliegen
  // hilfreich, beim Planen und Nachschauen aber hinderlich — Norden oben
  // ist die Erwartung, solange man nicht ausdrücklich etwas anderes will.
  const [trackUp, setTrackUp] = useState<boolean>(
    () => typeof localStorage !== "undefined" && localStorage.getItem("aaLivemapTrackUp") === "1",
  );
  const simSnapshotRef = useRef<SimSnapshot | null>(null);
  useEffect(() => { simSnapshotRef.current = simSnapshot; });
  const { netz, setNetz, flModus, setFlModus, flWert, setFlWert, netzZeit } = useNetzKarte({ mapRef, mapReady, sichtbar, zugangTakt, simSnapshotRef });
  const groundLoaded = useTaxiKarte(mapRef, mapReady);
  const [theme, setTheme] = useState<"dark" | "light">(readTheme());
  // Wird der Reiter wieder vorgeholt, muss MapLibre seine Leinwand neu
  // vermessen: verdeckt steht der Behaelter auf `display: none` und damit auf
  // 0x0. Ohne dieses `resize` bliebe die Karte beim Zurueckkommen leer oder
  // stark verzerrt — der klassische Fallstrick beim Verstecken statt Ausbauen.
  //
  // Zwei Bilder Verzoegerung, damit der Browser das Layout schon gerechnet hat:
  // ein `resize` im selben Bild misst noch die alten 0x0.
  useEffect(() => {
    if (!sichtbar || !mapReady) return;
    let id2 = 0;
    const id1 = requestAnimationFrame(() => {
      id2 = requestAnimationFrame(() => mapRef.current?.resize());
    });
    return () => {
      cancelAnimationFrame(id1);
      if (id2) cancelAnimationFrame(id2);
    };
  }, [sichtbar, mapReady]);

  // README §6/§2: next booked flight, shown in the idle empty-state card AND
  // the header's idle context line — fetched once per active-flight-identity
  // so both read the same answer instead of polling `phpvms_get_bids` twice.
  // `undefined` = loading.
  //
  // v1.4.7 (Feld-Report Thomas, Screenshot): dep-array war `[]` — fetchte
  // GENAU EINMAL beim Mount und nie wieder. Ein Pilot der die Karte offen
  // hatte, während er BTI358 flog und den PIREP absendete, sah danach
  // weiterhin BTI358 als "nächster gebuchter Flug" — nicht weil phpVMS den
  // Bid nicht entfernt hätte, sondern weil diese Komponente serverseitige
  // Änderungen gar nicht erst nachfragte. Jetzt an `activeFlight?.pirep_id`
  // gekoppelt: refetcht bei jedem Flug-Start (nicht mehr "next" relevant)
  // UND bei jedem Flug-Ende (genau der Moment, in dem "next" veraltet sein
  // könnte).
  const [next, setNext] = useState<NextBidInfo | null | undefined>(undefined);
  useEffect(() => {
    let cancelled = false;
    setNext(undefined);
    void (async () => {
      try {
        const bids = await invoke<Bid[]>("phpvms_get_bids");
        const bid = bids[0] ?? null;
        const flight = bid?.flight;
        // Field bug (2026-08-03): phpVMS's own `dpt_time` schedule field can
        // be null on a real, existing booking (verified — not a "no time
        // set" placeholder, genuinely absent on this booking's API
        // response). BidsList.tsx already has a fallback for exactly this
        // (`f.dpt_time ?? sched_out_epoch via bid_simbrief_preview`) —
        // replicated here rather than showing "no flight booked" for a
        // flight that plainly IS booked.
        let std: string | null = flight?.dpt_time ?? null;
        if (!std && bid) {
          try {
            const preview = await invoke<{ sched_out_epoch: number | null }>("bid_simbrief_preview", {
              bidId: bid.id,
            });
            if (preview?.sched_out_epoch != null) {
              const d = new Date(preview.sched_out_epoch * 1000);
              std = `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
            }
          } catch {
            // no SimBrief-direct preview either — std stays null, the card
            // just won't show a departure time for this booking.
          }
        }
        if (!cancelled) setNext(nextBidInfo(bid, std));
      } catch {
        if (!cancelled) setNext(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight?.pirep_id]);
  // README §2: header UTC clock, ticking every second — same pattern as
  // PilotHeader.tsx's own zulu clock.
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    // QS-Befund 18.08.2026: seit die Karte ueber Reiterwechsel hinweg
    // eingehaengt bleibt, haette dieser Takt die (sehr grosse) Komponente
    // waehrend des GANZEN Fluges jede Sekunde neu gerendert — auch verdeckt.
    // Genau die Grundlast, die der Umbau vermeiden sollte. Beim Auftauchen
    // wird sofort einmal nachgestellt, damit die Uhr nicht nachgeht.
    if (!sichtbar) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [sichtbar]);
  const [routeFixes, setRouteFixes] = useState<RouteFix[]>([]);
  const [depArr, setDepArr] = useState<{ dep?: [number, number]; arr?: [number, number] }>({});
  // v0.15.x: geflogener Track. QUELLE ist jetzt das Backend — der Rust-Streamer
  // akkumuliert ihn bei voller Tick-Rate (fokus-/fenster-unabhängig → lückenlos
  // auch bei X-Plane-Vollbild). Früher kam der Track aus dem im Hintergrund
  // GEDROSSELTEN Snapshot-Stream (setInterval im Webview) → die Linie hatte
  // Lücken. Wir pollen `flight_get_track` und spiegeln ihn in lokalen State;
  // `track` steht in der Redraw-Dep-Liste, damit die Linie sicher neu zeichnet,
  // wenn Punkte ankommen (auch wenn simSnapshot kurz stehen bleibt).
  const [trackPoints, setTrackPoints] = useState<[number, number][]>([]);

  // v1.4.7 (Feld-Report Thomas, "bei Kurs oben zittert die Karte immer"):
  // rohe Telemetrie-Peilung ändert sich JEDEN 500ms-Poll um Bruchteile eines
  // Grades. Zwei Effekte (Orientierung unten + der Redraw-Effekt weiter unten
  // im File) lösen davon abhängig je einen eigenen 350–400ms `easeTo` auf die
  // Bahn aus — bei ~500ms Poll-Intervall wird das Ease nie fertig, bevor der
  // nächste Tick es mit einem minimal neuen Ziel unterbricht → sichtbares
  // Zittern der ganzen Karte (nur in Kurs-oben spürbar, weil Norden-oben die
  // Peilung dauerhaft auf 0 pinnt und diese Kette nie erneut anstößt).
  // Fix: auf ganze Grad runden, BEVOR der Wert als Dependency/Ziel verwendet
  // wird — Rauschen unter 0.5° löst dann gar keinen neuen Effect-Lauf mehr
  // aus (React vergleicht Deps per ===), echte Kursänderungen bleiben sofort
  // wirksam. Kein useMemo nötig: die Eingabewerte ändern sich praktisch bei
  // jedem Tick, die eigentliche Wirkung ist die STABILITÄT des gerundeten
  // Primitives für die Ziel-Effekte, nicht gesparte Rechenzeit.
  const rawHeading = simSnapshot?.heading_deg_true ?? simSnapshot?.heading_deg_magnetic;
  const trackUpHeadingDeg =
    typeof rawHeading === "number" && Number.isFinite(rawHeading) ? Math.round(rawHeading) : null;

  const pirepId = activeFlight?.pirep_id ?? null;
  // ---- effektive Daten (aktiver Flug) ----
  const effFixes = routeFixes;
  const effTrack: [number, number][] = trackPoints;
  const effDep = depArr.dep;
  const effArr = depArr.arr;
  const effAircraft: Aircraft | null =
    simSnapshot && typeof simSnapshot.lat === "number"
      ? {
          lon: simSnapshot.lon,
          lat: simSnapshot.lat,
          hdg: simSnapshot.heading_deg_true ?? simSnapshot.heading_deg_magnetic ?? 0,
        }
      : null;
  const effDepIcao = activeFlight?.dpt_airport;
  const effArrIcao = activeFlight?.arr_airport;

  // v0.15.13: die gerenderte Track-Linie immer bis zur LIVE-Position des
  // Flugzeugs ziehen. `effTrack` (aus dem trackStore) wird ausgedünnt, der
  // letzte aufgezeichnete Punkt liegt also bis zu ~220 m hinter dem Marker
  // → die weiße Linie „hinkte" sichtbar hinterher (Live-Report Thomas, BTX2222).
  // Wir hängen den aktuellen Sim-Punkt als zusätzliches Linienende an (nur
  // fürs Rendern; trackStore bleibt ausgedünnt persistiert).
  const effTrackLive: [number, number][] = effAircraft
    ? [...effTrack, [effAircraft.lon, effAircraft.lat]]
    : effTrack;

  const phaseLabel = formatPhase(activeFlight?.phase);

  // ---- Nächster Wegpunkt + ETA ----
  const nav = useMemo(() => {
    if (!effAircraft) return { nextIdent: undefined as string | undefined, nextLabel: "—", eta: "—" };
    const ac: [number, number] = [effAircraft.lon, effAircraft.lat];
    const acToArr = effArr ? distNm(ac, effArr) : Infinity;
    let next: RouteFix | null = null;
    let bestD = Infinity;
    for (const f of effFixes) {
      const fp: [number, number] = [f.lon, f.lat];
      // nur Fixe, die näher am Ziel sind als wir (= voraus), und nicht das Ziel selbst.
      if (effArr && distNm(fp, effArr) >= acToArr - 1) continue;
      const d = distNm(ac, fp);
      if (d < bestD) {
        bestD = d;
        next = f;
      }
    }
    const nextLabel = next ? `${next.ident} · ${Math.round(bestD)} nm` : "—";
    // v0.19.3: distance-to-GO, i.e. great-circle from the aircraft to the
    // destination — `acToArr`, computed above. This used to read
    // `activeFlight.distance_nm`, which is the accumulated distance already
    // FLOWN: the ETA therefore started near zero and grew for the whole flight,
    // telling a pilot on short final he had another hour to run.
    const dtgNm = Number.isFinite(acToArr) ? acToArr : null;
    const gs = simSnapshot?.groundspeed_kt;
    // README §4.1 wants an absolute UTC time ("15:50z"), not a countdown
    // duration — the Flugwerte card's ETA cell reads this directly.
    let eta = "—";
    if (dtgNm != null && gs != null && gs > 30) {
      const mins = Math.round((dtgNm / gs) * 60);
      const arrival = new Date(Date.now() + mins * 60_000);
      eta = `${arrival.toISOString().slice(11, 16)}z`;
    }
    return { nextIdent: next?.ident, nextLabel, eta, dtgNm };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effFixes, effAircraft, effArr, activeFlight?.distance_nm, simSnapshot?.groundspeed_kt]);

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: startStil(grundlage.glyphen),
      center: [6, 48],
      zoom: 4,
      attributionControl: { compact: true },
      // Der Schlüssel gehört an JEDE Anfrage, nicht nur an die
      // Stil-Adresse — die style.json verweist selbst weiter auf
      // TileJSON, Kacheln, Schriften und Sprites. Siehe `kartenAnfrage`.
      transformRequest: kartenAnfrage(() => grundlageRef.current.schluessel),
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
    // Field feedback (2026-08-03): the attribution's `compact: true` reads
    // as "collapsed by default, expands on click" — but MapLibre's own
    // AttributionControl (`_updateCompact` in its source) actually adds
    // BOTH `maplibregl-compact` AND the expanded `maplibregl-compact-show`
    // class together on its very first render, so it starts OPEN and only
    // collapses once the pilot clicks the (i) themselves. Force it closed
    // once after load so "compact" really means collapsed-by-default.
    map.once("load", () => {
      const attrib = containerRef.current?.querySelector(".maplibregl-ctrl-attrib");
      attrib?.classList.remove("maplibregl-compact-show");
      attrib?.removeAttribute("open");
    });
    mapRef.current = map;
    // Follow nicht „einsperren", aber ehrlich: zieht der Nutzer die Karte selbst
    // weg (echter Pan = originalEvent gesetzt; unser easeTo/jumpTo löst KEIN
    // „dragstart" aus), schalten wir Follow sichtbar AUS (Haken weg) statt es
    // heimlich zu pausieren. Zoomen/Scrollen/+- lässt Follow an — man darf immer
    // zoomen, ohne den Flieger zu verlieren.
    map.on("dragstart", (e: { originalEvent?: unknown }) => {
      if (e.originalEvent) {
        // Pan = bewusst woanders hinschauen → Follow sichtbar aus + NICHT auf die
        // ganze Route fitten.
        suppressRouteFitRef.current = true;
        setFollow(false);
      }
    });
    map.on("load", () => {
      // Die Bereitschaft MUSS gesetzt werden, auch wenn das Anlegen der
      // Ebenen scheitert. Vorher stand sie dahinter: ein einziger
      // Fehler in addOverlays — etwa ein Ausdruck an einer Eigenschaft,
      // die keine nimmt — verhinderte sie, und damit lief der ganze
      // Datenabruf nie an. Auf der Karte blieb nur der Verkehr übrig,
      // der aus einer anderen Quelle kommt.
      try {
        grundkarteRef.current = grundkarteBauen(map);
        // Reihenfolge = Stapelung: Die Bodendaten liegen unter Route, Spur und Flugzeug.
        netzEbenenAnlegen(map);
        fremdeRoutenAnlegen(map);
        taxiEbenenAnlegen(map);
        flugEbenenAnlegen(map);
      } catch (e) {
        console.error("[karte] Ebenen konnten nicht angelegt werden:", e);
      } finally {
        setMapReady(true);
      }
    });
    return () => {
      map.remove();
      mapRef.current = null;
      setMapReady(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Theme beobachten + Basemap umschalten ----
  useEffect(() => {
    const obs = new MutationObserver(() => {
      const next = readTheme();
      setTheme((prev) => (prev === next ? prev : next));
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => obs.disconnect();
  }, []);
  useEffect(() => {
    try {
      localStorage.setItem("aaLivemapBasemap", basemap);
    } catch {
      /* persist best-effort */
    }
    const map = mapRef.current;
    if (!map || !mapReady) return;
    grundkarteRef.current?.zeigen(basemap === "sat" ? "satellit" : theme === "dark" ? "dunkel" : "hell");
    overlayFarben(map, basemap === "sat");
  }, [theme, basemap, mapReady]);

  // Vektorstile holen, und neu, sobald der Server andere Adressen nennt.
  useEffect(() => {
    if (!mapReady || !grundkarteRef.current) return;
    return grundkarteHolen(
      grundkarteRef.current,
      { dunkel: grundlage.dunkel, hell: grundlage.hell },
      kartenAnfrage(() => grundlageRef.current.schluessel),
    );
  }, [mapReady, grundlage.dunkel, grundlage.hell]);

  // ---- Routen-Fixes laden ----
  // v0.15.6: Wegpunkte können VERSPÄTET reinkommen — z.B. wenn die SimBrief-OFP
  // erst nach dem Flugstart per „OFP aktualisieren" geladen wird. Darum POLLEN
  // wir statt one-shot und setzen nur bei echter Änderung (kein Re-Render-Spam).
  // Fetch-Fehler lassen die zuletzt geladene Route stehen (nicht löschen).
  useEffect(() => {
    if (!pirepId) {
      setRouteFixes([]);
      return;
    }
    if (!sichtbar) return;
    let cancelled = false;
    const sig = (fx: RouteFix[]) => fx.map((f) => `${f.ident}@${f.lat.toFixed(3)},${f.lon.toFixed(3)}`).join("|");
    const load = () =>
      invoke<RouteFix[]>("flight_get_route_fixes")
        .then((fx) => {
          if (cancelled) return;
          const next = fx ?? [];
          setRouteFixes((prev) => (sig(prev) === sig(next) ? prev : next));
        })
        .catch(() => {
          /* transienter Fehler → letzte Route behalten */
        });
    load();
    const id = window.setInterval(load, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pirepId, sichtbar]);

  // ---- geflogenen Track laden (Backend ist die Quelle) ----
  // v0.15.x: Lücken-Fix. Der Rust-Streamer akkumuliert den Track lückenlos
  // (fokus-unabhängig); wir pollen ihn hierher.
  // v0.15.25: Der Backend-Track ist nach einem App-Neustart mitten im Flug
  // jetzt VOLLSTÄNDIG — `try_resume_flight` reseedet ihn aus phpVMS (dem
  // autoritativen Superset). Damit entfällt der fragile localStorage-Seed-Gate
  // (`next.length < seed.length`), der einen korrekten Backend-Track blockierte,
  // sobald ein veralteter localStorage-Seed länger war. Wir spiegeln den Track
  // weiterhin in den Store (setTrack), damit getTrack/localStorage konsistent
  // bleiben — das ist jetzt nur noch ein harmloser Cache, kein Gate mehr.
  useEffect(() => {
    if (!pirepId) {
      setTrackPoints([]);
      return;
    }
    if (!sichtbar) return;
    let cancelled = false;
    const load = () =>
      invoke<[number, number][]>("flight_get_track")
        .then((pts) => {
          if (cancelled) return;
          const next = pts ?? [];
          setTrackPoints((prev) => {
            // Transient leeren Backend-Track ignorieren (active_flight wird am
            // Flugende ge-take()t) — die letzte Linie behalten, nicht wegblitzen.
            if (next.length === 0 && prev.length > 0) return prev;
            // Dedup: gleiche Länge + gleicher letzter Punkt = keine neuen Punkte
            // → kein Redraw (spart das setData alle 2 s im geparkten Zustand).
            // Beim Cap-Crawl (>5000 Punkte) ändert sich der letzte Punkt → es
            // wird weitergezeichnet.
            if (
              next.length === prev.length &&
              (prev.length === 0 ||
                (next[next.length - 1][0] === prev[prev.length - 1][0] &&
                  next[next.length - 1][1] === prev[prev.length - 1][1]))
            ) {
              return prev;
            }
            return next;
          });
          // Store + localStorage konsistent halten (nur reale, nicht-leere Tracks).
          if (next.length > 0) {
            setTrack(pirepId, next);
          }
        })
        .catch(() => {
          /* transienter Fehler → zuletzt gehaltenen Track behalten */
        });
    load();
    const id = window.setInterval(load, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pirepId, sichtbar]);

  useEffect(() => {
    let cancelled = false;
    if (!activeFlight) {
      setDepArr({});
      return;
    }
    const lookup = async (icao: string): Promise<[number, number] | undefined> => {
      try {
        const a = await invoke<{ lat?: number | null; lon?: number | null }>("airport_get", { icao });
        if (a?.lat != null && a?.lon != null) return [a.lon, a.lat];
      } catch {
        /* ignore */
      }
      return undefined;
    };
    void (async () => {
      const dep = await lookup(activeFlight.dpt_airport);
      const arr = await lookup(activeFlight.arr_airport);
      if (!cancelled) setDepArr({ dep, arr });
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight?.dpt_airport, activeFlight?.arr_airport, activeFlight]);

  // Kartenausrichtung an den Steuerkurs koppeln.
  //
  // `easeTo` statt `setBearing`: Ein harter Sprung bei jedem Telemetriepaket
  // sieht aus wie Ruckeln. Beim Ausschalten zurück auf Norden, sonst bliebe
  // die Karte in der zuletzt geflogenen Richtung stehen — der Pilot hätte
  // eine schief stehende Karte ohne erkennbaren Grund.
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    try { localStorage.setItem("aaLivemapTrackUp", trackUp ? "1" : "0"); } catch { /* egal */ }
    // Merken ja, Kamera bewegen nein: `trackUpHeadingDeg` folgt der Telemetrie,
    // ohne diese Sperre liefe verdeckt bei jedem Paket ein 400-ms-Schwenk auf
    // einer 0x0-Leinwand (QS-Befund 18.08.2026).
    if (!sichtbar) return;
    if (!trackUp) {
      map.easeTo({ bearing: 0, duration: 400 });
      return;
    }
    if (trackUpHeadingDeg !== null) {
      map.easeTo({ bearing: trackUpHeadingDeg, duration: 400 });
    }
  }, [trackUp, trackUpHeadingDeg, sichtbar]);

  // ---- Redraw: eigener Flug (Quellen + Flugzeug-Marker + Pins) ----
  // Läuft immer; VA-Flieger liegen als zusätzliche Marker mit drauf (eigene Effekte).
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    // QS-Befund 18.08.2026, der schwerste: dieser Effekt haengt an
    // `simSnapshot` und lief als einziger grosser Effekt ungesperrt weiter.
    // Mit der dauerhaft eingehaengten Karte haette er waehrend des GANZEN
    // Fluges Marker verschoben und `easeTo`/`jumpTo` auf einer unsichtbaren
    // Leinwand gefahren — MapLibre haelt dafuer eine Animationsschleife am
    // Laufen. Beim Auftauchen laeuft er neu und zeichnet alles frisch.
    if (!sichtbar) return;
    flugDatenSetzen(map, { fixes: effFixes, track: effTrackLive, dep: effDep, arr: effArr, nextIdent: nav.nextIdent });

    // Flugzeug-Marker (kategorieabhängiges Icon wie VPS/Stratos)
    if (effAircraft) {
      const lngLat: [number, number] = [effAircraft.lon, effAircraft.lat];
      const icao = simSnapshot?.aircraft_icao ?? null;
      if (!acMarkerRef.current) {
        const el = document.createElement("div");
        el.className = "aa-ac-marker";
        el.innerHTML = aircraftSvg(icao);
        acCatRef.current = icao;
        acMarkerRef.current = new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat(lngLat).addTo(map);
      } else if (acCatRef.current !== icao) {
        // Muster wurde (nach-)geladen → Icon aktualisieren.
        acMarkerRef.current.getElement().innerHTML = aircraftSvg(icao);
        acCatRef.current = icao;
      }
      // Phasenabhängige Farbe (Fill + Glow + Pulse) wie auf dem VPS.
      acMarkerRef.current.getElement().style.setProperty("--ac-color", phaseColor(activeFlight?.phase));
      acMarkerRef.current.setLngLat(lngLat).setRotation(effAircraft.hdg);
      // was_just_resumed = der Resume-Gate wartet noch auf einen frischen
      // Sim-Snapshot. In dem Zustand kann X-Plane kurz eine Reload-/Lade-
      // position melden (z.B. Nordeuropa) → NICHT blind dorthin folgen.
      // Field feedback (2026-08-03, "Norden oben/Kurs oben gehen nur
      // sporadisch"): this effect's own easeTo/jumpTo calls (below) and the
      // separate orientation effect's `easeTo({bearing, duration:400})`
      // both fire off the SAME simSnapshot tick, independently, with no
      // coordination. MapLibre's easeTo isn't per-property-isolated — a
      // second call while the first is still mid-flight restarts the
      // animation from whatever the CURRENT interpolated values are, so a
      // center-only call here could freeze an in-flight bearing rotation
      // wherever it happened to be, and vice versa. Every camera call in
      // THIS effect now names its own target bearing explicitly (same
      // trackUp/heading source the orientation effect uses) so whichever
      // call "wins" the race, it's still driving toward the same target —
      // no more silent stalls.
      const targetBearing = trackUp ? (trackUpHeadingDeg ?? map.getBearing()) : 0;

      // Follow: bei gesetztem Haken IMMER auf den Flieger zentrieren — egal wie
      // weit man rausgezoomt hat. Gate NUR am `follow`-State (ein einziger Zustand,
      // nichts kann mehr desyncen). was_just_resumed unterdrückt das Folgen der
      // evtl. falschen Reload-Position direkt nach einem Resume.
      if (follow && !activeFlight?.was_just_resumed) {
        const c = map.getCenter();
        const far = Math.abs(c.lng - lngLat[0]) > 2 || Math.abs(c.lat - lngLat[1]) > 2;
        if (followEngageRef.current) {
          // Bewusstes (Wieder-)Einschalten von Follow (Erst-Mount oder der
          // "Auf Flug zentrieren"/"Folgen"-Knopf) → phasen-passenden Zoom
          // setzen, egal wie weit die Kamera gerade weg ist.
          if (far) {
            map.jumpTo({
              center: lngLat,
              zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
              bearing: targetBearing,
            });
          } else {
            map.easeTo({
              center: lngLat,
              zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
              bearing: targetBearing,
              duration: 350,
            });
          }
          followEngageRef.current = false;
        } else if (far) {
          // Field feedback (2026-08-03, "Map hängt, kann nicht rauszoomen,
          // zeigt nicht mehr alle Flüge"): laufendes Folgen, aber die Kamera
          // ist weit vom Flugzeug abgedriftet — das passierte auch schon von
          // einem simplen Rauszoomen per Mausrad: MapLibre zoomt um den
          // CURSOR, nicht um die Kartenmitte, also verschiebt Rauszoomen
          // abseits des Flugzeug-Icons `getCenter()` vom Flieger weg. Der
          // alte Code behandelte JEDE große Abweichung als "Spur verloren,
          // hart auf festen Phasen-Zoom zurücksetzen" — das hat ein
          // bewusstes Rauszoomen binnen einer Sekunde wieder rückgängig
          // gemacht, jedes Mal. Hier nur auf den Flieger zentrieren, OHNE
          // den Zoom anzufassen — der Phasen-Zoom-Override ist jetzt allein
          // dem bewussten (Wieder-)Einschalten oben vorbehalten.
          map.jumpTo({ center: lngLat, bearing: targetBearing });
        } else {
          // laufendes Folgen → NUR schwenken. Dein manueller Zoom bleibt erhalten
          // (kein Zurückziehen mehr auf den Phasen-Zoom — genau das war der Bug).
          map.easeTo({ center: lngLat, bearing: targetBearing, duration: 400 });
        }
      }
    } else {
      acMarkerRef.current?.remove();
      acMarkerRef.current = null;
    }

    // Dep/Arr-Pins
    pinMarkersRef.current.forEach((m) => m.remove());
    pinMarkersRef.current = [];
    const mk = (coord: [number, number], label: string, kind: "dep" | "arr") => {
      const el = document.createElement("div");
      el.className = `aa-pin aa-pin--${kind}`;
      el.textContent = label;
      pinMarkersRef.current.push(new maplibregl.Marker({ element: el, anchor: "bottom" }).setLngLat(coord).addTo(map));
    };
    if (effDep && effDepIcao) mk(effDep, effDepIcao, "dep");
    if (effArr && effArrIcao) mk(effArr, effArrIcao, "arr");
    // eslint-disable-next-line react-hooks/exhaustive-deps
    // was_just_resumed in den Deps: sobald die Resume-Warte-Phase endet, läuft
    // der Redraw erneut und zentriert (bei Follow) auf die frische Sim-Position.
    // v0.15.6: phase/dep/arr/nextIdent mit in die Deps — sonst frieren Phasen-
    // Zoom + Marker-Farbe + Pins/Highlight ein, falls simSnapshot mal stehen
    // bleibt (Sim-Pause/Resume), während sich die Phase noch ändert.
  }, [
    mapReady,
    sichtbar,
    follow,
    simSnapshot,
    routeFixes,
    // v0.15.x: trackPoints in den Deps, damit die geflogene Linie sicher neu
    // zeichnet, sobald der Backend-Poll neue Punkte liefert — auch wenn
    // simSnapshot im Hintergrund kurz steht.
    trackPoints,
    depArr.dep,
    depArr.arr,
    activeFlight?.was_just_resumed,
    activeFlight?.phase,
    activeFlight?.dpt_airport,
    activeFlight?.arr_airport,
    nav.nextIdent,
  ]);

  // einmal auf die eigene Route fitten, wenn Follow BEWUSST abgehakt wurde
  // (nicht, wenn ein Pan Follow ausgeschaltet hat — dann bleibt die Ansicht stehen).
  const fittedRef = useRef<string | null>(null);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || !activeFlight || follow) return;
    if (suppressRouteFitRef.current) return;
    // In Flugreihenfolge und ueber die Datumsgrenze weitergezaehlt — sonst
    // umfasst der Ausschnitt einer Pazifikroute die ganze Welt.
    const pts = ohneDatumsgrenzenSprung([
      ...(effDep ? [effDep] : []),
      ...effFixes.map((f) => [f.lon, f.lat] as [number, number]),
      ...(effArr ? [effArr] : []),
    ]);
    // pirepId mit im Key: ein neuer Flug derselben Strecke (gleiche Fix-Anzahl +
    // gleiche Dep/Arr) soll wieder gefittet werden, nicht als „schon erledigt" gelten.
    const key = `${pirepId}-${effFixes.length}-${effDepIcao}-${effArrIcao}`;
    if (pts.length >= 2 && fittedRef.current !== key) {
      fittedRef.current = key;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 80, duration: 600, maxZoom: 8 });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapReady, follow, effFixes, effDepIcao, effArrIcao, activeFlight]);

  // Steht vor dem VA-Effekt, der `bildKarte` beim Einpassen braucht; muss nach dem Anlegen der Karte bleiben.
  const bildKarte = useBildKarte(mapRef, bildFokus, activeFlight?.pirep_id ?? null, sichtbar);
  const { showVa, setShowVa, vaVisible, fremdeRouteHinweis, setFremdeRouteHinweis } = useVaVerkehr({
    mapRef, mapReady, sichtbar, activeFlight, bilderVorrang: bildKarte.bilderAn && bildKarte.marken > 0,
  });

  // README §3 EBENEN: Track / Taxiweg show/hide, two independent toggles
  // (field feedback, 2026-08-03 — split apart from the spec's original
  // combined "Track & Taxiweg" switch). Deliberately additive and separate
  // from `addOverlays` — the trackline/taxi-map DATA and how it's computed
  // stay exactly as they were (README §0), this only toggles the existing
  // layers' visibility. `setLayoutProperty` is a no-op-safe call guarded by
  // `getLayer`, so it can't fight the style-reload re-add logic above; it
  // just re-asserts the current toggle state whenever it changes.
  const [showTrack, setShowTrack] = useState(true);
  const [showTaxi, setShowTaxi] = useState(true);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    for (const id of TRACK_LAYERS) {
      // Die dunkle Unterlage braucht die Route nur auf dem bunten Satellitenbild.
      const an = showTrack && (id !== LYR_ROUTE_CASING || basemap === "sat");
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", an ? "visible" : "none");
    }
  }, [showTrack, basemap, mapReady]);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    const vis = showTaxi ? "visible" : "none";
    for (const id of TAXI_LAYERS) {
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", vis);
    }
  }, [showTaxi, mapReady]);

  // ---- Stats (Flugwerte card — README §4.1: HÖHE, GS, REST, ETA only) ----
  const stats = useMemo(() => {
    const fmt = (v: number | null | undefined, suffix: string) =>
      v == null || Number.isNaN(v) ? "—" : `${Math.round(v)}${suffix}`;
    const s = simSnapshot;
    const flLabel =
      s?.altitude_msl_ft != null
        ? s.altitude_msl_ft >= 18000
          ? `FL${Math.round(s.altitude_msl_ft / 100)}`
          : `${Math.round(s.altitude_msl_ft)} ft`
        : "—";
    return {
      alt: flLabel,
      gs: fmt(s?.groundspeed_kt, " kts"),
      // v0.19.3: distance to GO (aircraft → destination), not the distance
      // already flown. Same source as the ETA above — see `nav.dtgNm`.
      dtg: nav.dtgNm != null ? `${Math.round(nav.dtgNm)} nm` : "—",
    };
  }, [simSnapshot, nav]);

  const showOwnContent = !!activeFlight;
  const ident = activeFlight
    ? displayCallsign(activeFlight.airline_icao, activeFlight.flight_number, activeFlight.callsign)
    : null;
  const clock = new Date(nowMs).toISOString().slice(11, 19);
  // QS-Runde 4: `useMapEvents` haengt zwei Takte an — das Aktivitaetsprotokoll
  // (alle 2 s) und den Hoppie-Nachrichtenfaden. Beide hingen nur am aktiven
  // Flug, liefen also mit der dauerhaft eingehaengten Karte den GANZEN Flug
  // lang weiter, obwohl niemand hinsieht. Der Hoppie-Teil geht dabei sogar ins
  // Netz. Beim Auftauchen wird ohnehin sofort einmal frisch geholt.
  const { events } = useMapEvents(sichtbar && showOwnContent);
  const flyToPosition = (position: [number, number]) => {
    mapRef.current?.flyTo({ center: position, duration: 700 });
  };
  const recorderState: "ok" | "off" | "danger" = !activeFlight
    ? "off"
    : activeFlight.connection_state === "blocked" || activeFlight.connection_state === "failing"
      ? "danger"
      : "ok";

  // Das rechte Panel beginnt immer unter der Schalterleiste, auch wenn sie umbricht.
  const schalterleisteRef = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const leiste = schalterleisteRef.current;
    if (!leiste || typeof ResizeObserver === "undefined") return;
    const setzen = () =>
      leiste.parentElement?.style.setProperty("--aa-panel-oben", `${Math.max(96, leiste.offsetTop + leiste.offsetHeight + 12)}px`);
    setzen();
    const beobachter = new ResizeObserver(setzen);
    beobachter.observe(leiste);
    return () => beobachter.disconnect();
  }, []);
  return (
    <section className="aa-livemap">
      {/* README §2 — no buttons here at all; every control lives in the
          Kartenschalter-Block over the map (§3). This header carries
          identity/context and clock only. */}
      <header className="aa-livemap__header">
        <div className="aa-livemap__header-left">
          <h1 className="aa-livemap__header-title">{t("livemap.title")}</h1>
          <span className="aa-livemap__header-sep" aria-hidden="true" />
          <div className="aa-livemap__header-context">
            {activeFlight ? (
              <>
                <span className="aa-livemap__header-callsign">{ident}</span>
                <span className="aa-livemap__header-route">
                  {t("livemap.header_context_flight", {
                    dep: activeFlight.dpt_airport,
                    arr: activeFlight.arr_airport,
                    phase: phaseLabel,
                  })}
                </span>
                {activeFlight.was_just_resumed && (
                  <span className="aa-livemap__resume-hint" title={t("livemap.resume_hint")}>
                    {" "}
                    ⏳
                  </span>
                )}
              </>
            ) : (
              <span className="aa-livemap__header-route">
                {next
                  ? t("livemap.header_idle_next", { ident: next.ident, std: `${next.std}z` })
                  : t("livemap.header_idle_none")}
              </span>
            )}
          </div>
        </div>
        <div className="aa-livemap__header-right">
          <div className="bt-karte__umschalter bt-karte__umschalter--kopf" role="group" aria-label={t("bilder.karte.umschalter")}>
            <button type="button" aria-pressed={!bildKarte.kugel} aria-label={t("bilder.karte.flach")} onClick={() => bildKarte.setKugel(false)}>{t("bilder.karte.karte")}</button>
            <button type="button" aria-pressed={bildKarte.kugel} aria-label={t("bilder.karte.kugel_lang")} onClick={() => bildKarte.setKugel(true)}>{t("bilder.karte.kugel")}</button>
          </div>
          <span className="aa-livemap__header-vacount">{t("livemap.header_va_count", { count: vaVisible.length })}</span>
          <span className="aa-livemap__header-clock">
            {clock}
            <span className="aa-livemap__header-z">z</span>
          </span>
        </div>
      </header>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {/* Kurzer Hinweis, wenn zu einem angeklickten Kollegen keine
              Route vorliegt — sonst sähe der Klick aus, als täte er nichts. */}
          {fremdeRouteHinweis && (
            <div
              role="status"
              onClick={() => setFremdeRouteHinweis(null)}
              style={{
                position: "absolute",
                // UNTER die Kartenschalter-Leiste, und darueber in der
                // Stapelung. Bei `top: 12` + `zIndex: 5` lag der Hinweis
                // hinter der Leiste (die hat `z-index: 500`, App.css) —
                // fuer den Piloten sah es aus, als tue der Klick nichts,
                // waehrend der Grund verdeckt darunter stand (Thomas,
                // 20.09.2026). Die Leiste endet bei rund 76px.
                top: 92,
                left: "50%",
                transform: "translateX(-50%)",
                zIndex: 600,
                background: "rgba(20,26,33,0.92)",
                border: "1px solid var(--border, #2a3344)",
                borderRadius: 8,
                padding: "6px 12px",
                fontSize: "0.8rem",
                color: "var(--text-muted, #9aa4b2)",
                cursor: "pointer",
              }}
            >
              {fremdeRouteHinweis}
            </div>
          )}
          {/* README §3 — Kartenschalter-Block, one overlay container, four
              named groups. Every switch shows its own state (bold/accent =
              active) — the whole point is that today's unlabeled icon
              buttons didn't. */}
          <div ref={schalterleisteRef} className="aa-livemap-controls aa-livemap-overlay">
            {/* Symbole statt Woerter: eine Ja/Nein-Entscheidung kostete
                hier zwei breite Wörter plus Überschrift. Vertretbar, weil
                bei einem Zweier-Umschalter immer eine Hälfte hell ist —
                der Zustand bleibt eindeutig, anders als bei unabhängigen
                Schaltern. Die Bedeutung trägt der Tooltip. */}
            <div className="aa-livemap-controls__group">
              <div className="aa-livemap-seg">
                <button
                  type="button"
                  className={`aa-livemap-seg__btn ${basemap !== "sat" ? "aa-livemap-seg__btn--active" : ""}`}
                  aria-pressed={basemap !== "sat"}
                  aria-label={t("livemap.view_map")}
                  title={t("livemap.view_map")}
                  onClick={() => setBasemap("auto")}
                >
                  <Sym pfad={PFAD.karte} />
                </button>
                <button
                  type="button"
                  className={`aa-livemap-seg__btn ${basemap === "sat" ? "aa-livemap-seg__btn--active" : ""}`}
                  aria-pressed={basemap === "sat"}
                  aria-label={t("livemap.view_sat")}
                  title={t("livemap.view_sat")}
                  onClick={() => setBasemap("sat")}
                >
                  <Sym pfad={PFAD.satellit} />
                </button>
              </div>
            </div>

            <div className="aa-livemap-controls__group">
              <div className="aa-livemap-seg">
                <button
                  type="button"
                  className={`aa-livemap-seg__btn ${!trackUp ? "aa-livemap-seg__btn--active" : ""}`}
                  aria-pressed={!trackUp}
                  aria-label={t("livemap.orient_north")}
                  title={t("livemap.orient_north")}
                  onClick={() => setTrackUp(false)}
                >
                  <Sym pfad={PFAD.norden} />
                </button>
                <button
                  type="button"
                  className={`aa-livemap-seg__btn ${trackUp ? "aa-livemap-seg__btn--active" : ""}`}
                  aria-pressed={trackUp}
                  onClick={() => setTrackUp(true)}
                  aria-label={t("livemap.orient_track")}
                  title={t("livemap.orient_track")}
                >
                  <Sym pfad={PFAD.kurs} />
                </button>
              </div>
            </div>

            <div className="aa-livemap-controls__group">
              <div className="aa-livemap-controls__row">
                {/* Symbol UND Wort immer, in beiden Zustaenden. Diese vier sind
                    unabhaengige Ein/Aus-Schalter; der Zustand haengt allein an
                    der Faerbung (--active) und aria-pressed. Das Wort nur im
                    An-Zustand zu zeigen liess die Leiste beim Umschalten in der
                    Breite springen ("werden immer groesser"), darum nicht. */}
                <button
                  type="button"
                  className={`aa-livemap-toggle ${showTrack ? "aa-livemap-toggle--active" : ""}`}
                  aria-pressed={showTrack}
                  aria-label={t("livemap.layer_track")}
                  title={t("livemap.layer_track")}
                  onClick={() => setShowTrack((v) => !v)}
                >
                  <Sym pfad={PFAD.track} />
                  <span className="aa-livemap-toggle__wort">{t("livemap.layer_track")}</span>
                </button>
                <button
                  type="button"
                  className={`aa-livemap-toggle ${showTaxi ? "aa-livemap-toggle--active" : ""}`}
                  aria-pressed={showTaxi}
                  onClick={() => setShowTaxi((v) => !v)}
                  title={
                    // v0.21: which airports actually have taxi-map coverage —
                    // used to be its own "TAXI" stat tile in the old 9-tile
                    // header strip (now removed per the new chrome's 4-value
                    // Flugwerte card); the underlying "pilot should know this
                    // exists" reason from that comment still applies, so it
                    // moved to this toggle's tooltip instead of disappearing.
                    groundLoaded.length > 0 ? t("livemap.taxi_loaded", { airports: groundLoaded.join(" · ") }) : undefined
                  }
                  aria-label={t("livemap.layer_taxi")}
                >
                  <Sym pfad={PFAD.taxi} />
                  <span className="aa-livemap-toggle__wort">{t("livemap.layer_taxi")}</span>
                </button>
                {bildKarte.marken > 0 && (
                  <button
                    type="button"
                    className={`aa-livemap-toggle ${bildKarte.bilderAn ? "aa-livemap-toggle--active" : ""}`}
                    aria-pressed={bildKarte.bilderAn}
                    aria-label={t("livemap.layer_bilder")}
                    title={t("livemap.layer_bilder")}
                    onClick={() => bildKarte.setBilderAn((v) => !v)}
                  >
                    <Sym pfad={PFAD.bilder} />
                    <span className="aa-livemap-toggle__wort">{t("livemap.layer_bilder")}</span>
                  </button>
                )}
                <button
                  type="button"
                  className={`aa-livemap-toggle ${showVa ? "aa-livemap-toggle--active" : ""}`}
                  aria-pressed={showVa}
                  onClick={() => setShowVa((v) => !v)}
                  aria-label={t("livemap.layer_va")}
                  title={t("livemap.layer_va")}
                >
                  <Sym pfad={PFAD.va} />
                  <span className="aa-livemap-toggle__wort">{t("livemap.layer_va")}</span>
                </button>
                {/* Netz-Umschalter statt zweier Schalter: so KANN nicht
                    beides zugleich an sein. Zwei Netze uebereinander
                    ergeben ein Bild, in dem niemand mehr erkennt, wer
                    welchen Luftraum betreut. */}
                <span className="aa-livemap-seg" role="group" aria-label={t("livemap.netz", "Netz")}>
                  {(["aus", "vatsim", "ivao"] as const).map((wert) => (
                    <button
                      key={wert}
                      type="button"
                      className={`aa-livemap-seg__btn ${netz === wert ? "aa-livemap-seg__btn--active" : ""}`}
                      aria-pressed={netz === wert}
                      onClick={() => setNetz(wert)}
                      title={
                        wert === "aus"
                          ? t("livemap.netz_aus_hint", "Kein Online-Netz anzeigen")
                          : wert === "vatsim"
                            ? t("livemap.layer_vatsim_hint", "Sektoren nach Höhe (VATGlasses, CC BY-NC-SA), Lotsen und fremde Piloten aus dem VATSIM-Netz")
                            : t("livemap.netz_ivao_hint", "Lotsen, ATIS und fremde Piloten aus dem IVAO-Netz. Flächen aus den FIR-Grenzen — IVAO gibt keine eigenen Sektoren heraus.")
                      }
                    >
                      {wert === "aus"
                        ? t("livemap.netz_aus", "Aus")
                        : wert === "vatsim"
                          ? "VATSIM"
                          : "IVAO"}
                    </button>
                  ))}
                </span>
                {/* Nur bei VATSIM: IVAO liefert keine Hoehenbaender, dort
                    gelten die Flaechen fuer den ganzen Luftraum — ein
                    Regler ohne Wirkung waere eine Luege. */}
                {netz === "vatsim" && (
                  <span className="aa-livemap-flwahl">
                    {/* Feldbefund: der Regler war hinter "eigene Hoehe"
                        versteckt — ein Bedienelement, das man suchen muss,
                        ist falsch gebaut. Jetzt IMMER sichtbar; "eigene
                        Hoehe" schaltet ihn nur still, solange der Flug die
                        Hoehe liefert. Ohne Sim-Verbindung gilt ohnehin der
                        Regler, egal was der Knopf sagt. */}
                    <button
                      type="button"
                      className={`aa-livemap-toggle ${flModus === "auto" ? "aa-livemap-toggle--active" : ""}`}
                      onClick={() => setFlModus((m) => (m === "auto" ? "fest" : "auto"))}
                      title={t("livemap.fl_auto_hint", "Sektoren auf der eigenen Flughöhe zeigen")}
                    >
                      {t("livemap.fl_auto", "eigene Höhe")}
                    </button>
                    <input
                      type="range"
                      min={0}
                      max={450}
                      step={10}
                      value={flWert}
                      disabled={flModus === "auto" && simSnapshot != null}
                      onChange={(e) => { setFlModus("fest"); setFlWert(Number(e.target.value)); }}
                      aria-label={t("livemap.fl_regler", "Flugfläche für die Sektoransicht")}
                    />
                    <span className="aa-livemap-flwahl__wert">
                      {flModus === "auto" && simSnapshot != null
                        ? `FL${String(Math.max(0, Math.round((simSnapshot.altitude_msl_ft ?? 0) / 100))).padStart(3, "0")}`
                        : `FL${String(flWert).padStart(3, "0")}`}
                    </span>
                  </span>
                )}
              </div>
            </div>

            <div className="aa-livemap-controls__group">
              <div className="aa-livemap-controls__row">
                {/* Nur bei laufendem Flug — ohne einen hat es keine Wirkung,
                    und ein abgeblendeter Knopf belegt trotzdem Platz. Als
                    Fadenkreuz statt als Satz: die Wirkung ist sofort
                    ersichtlich, sobald man ihn einmal benutzt hat. */}
                {activeFlight && effAircraft && (
                <button
                  type="button"
                  className="aa-livemap-toggle"
                  aria-label={t("livemap.center_on_flight")}
                  title={t("livemap.center_on_flight")}
                  onClick={() => {
                    const map = mapRef.current;
                    if (!map || !effAircraft) return;
                    followEngageRef.current = true;
                    suppressRouteFitRef.current = true;
                    setFollow(true);
                    const tz = targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft);
                    map.easeTo({ center: [effAircraft.lon, effAircraft.lat], zoom: tz, duration: 500 });
                  }}
                >
                  <Sym pfad={PFAD.zentrieren} />
                </button>
                )}
                {/* "Folgen" (continuous camera tracking) isn't in the
                    handoff's own control table — confirmed with the user
                    that it stays as its own switch, distinct from the
                    one-shot centering button above (see follow.test.tsx for
                    why: it auto-disables on a real user pan, which the
                    centering button doesn't need to know about). */}
                {activeFlight && (
                  <button
                    type="button"
                    className={`aa-livemap-toggle ${follow ? "aa-livemap-toggle--active" : ""}`}
                    aria-pressed={follow}
                    onClick={() => {
                      const next = !follow;
                      setFollow(next);
                      followEngageRef.current = next;
                      suppressRouteFitRef.current = next;
                    }}
                  >
                    {t("livemap.follow")}
                  </button>
                )}
              </div>
            </div>
          </div>

          {showOwnContent && (
            <div className="aa-livemap-panel">
              {/* README §4.1 — Flugwerte card. */}
              <div className="aa-livemap-flightcard aa-livemap-overlay">
                <div className="aa-livemap-flightcard__head">
                  <span className="aa-livemap-flightcard__rubric">{t("livemap.own_flight")}</span>
                  <span className="aa-livemap-flightcard__live">{t("livemap.live")}</span>
                </div>
                <div className="aa-livemap-flightcard__route">
                  <span className="aa-livemap-flightcard__airport">{activeFlight!.dpt_airport}</span>
                  <span className="aa-livemap-flightcard__arrow" aria-hidden="true">→</span>
                  <span className="aa-livemap-flightcard__airport">{activeFlight!.arr_airport}</span>
                </div>
                <div className="aa-livemap-flightcard__bar">
                  <div
                    className="aa-livemap-flightcard__bar-fill"
                    style={{
                      width: `${
                        activeFlight!.distance_nm && nav.dtgNm != null
                          ? Math.min(100, Math.max(0, ((activeFlight!.distance_nm - nav.dtgNm) / activeFlight!.distance_nm) * 100))
                          : 0
                      }%`,
                    }}
                  />
                </div>
                <div className="aa-livemap-flightcard__grid">
                  {/* Field feedback (2026-08-03): phase used to be tucked
                      into the footer as a small 12px dim caption next to
                      the aircraft type/registration — technically present
                      but easy to miss next to the bold 22px stat values
                      here, so it didn't read as "a data field" at all.
                      Promoted to its own full-width row, same label/value
                      styling as the other stats (colored via phaseColor()
                      like the aircraft marker itself, for a quick visual
                      match). */}
                  <div className="aa-livemap-flightcard__phase-row">
                    <span className="aa-livemap-flightcard__label">{t("livemap.field_phase")}</span>
                    <span
                      className="aa-livemap-flightcard__value"
                      style={{ color: phaseColor(activeFlight?.phase) }}
                    >
                      {phaseLabel}
                    </span>
                  </div>
                  <div>
                    <span className="aa-livemap-flightcard__label">{t("livemap.field_alt")}</span>
                    <span className="aa-livemap-flightcard__value">{stats.alt}</span>
                  </div>
                  <div>
                    <span className="aa-livemap-flightcard__label">{t("livemap.field_gs")}</span>
                    <span className="aa-livemap-flightcard__value">{stats.gs}</span>
                  </div>
                  <div>
                    <span className="aa-livemap-flightcard__label">{t("livemap.field_rest")}</span>
                    <span className="aa-livemap-flightcard__value">{stats.dtg}</span>
                  </div>
                  <div>
                    <span className="aa-livemap-flightcard__label">{t("livemap.field_eta")}</span>
                    <span className="aa-livemap-flightcard__value">{nav.eta}</span>
                  </div>
                </div>
                <div className="aa-livemap-flightcard__foot">
                  {/* README §4.1 wants "Muster + Registrierung" left. Uses
                      `activeFlight`'s own phpVMS-sourced `aircraft_name`/
                      `aircraft_icao` + `planned_registration` — not the live
                      SIM snapshot (whatever's loaded in the sim isn't
                      necessarily what phpVMS actually planned/booked). Phase
                      moved to its own row in the grid above (see comment
                      there) — this footer is single-purpose again. */}
                  <span>
                    {[activeFlight!.aircraft_name || activeFlight!.aircraft_icao, activeFlight!.planned_registration]
                      .filter(Boolean)
                      .join(" · ") || "—"}
                  </span>
                </div>
              </div>

              <LiveMapEventList
                events={events}
                callsign={ident}
                onJumpTo={flyToPosition}
              />
            </div>
          )}

          {/* Field feedback (2026-08-03): this card used to be shown whenever
              there was no own flight, VA traffic or not — it visibly blocked
              the map exactly where colleague aircraft render, which the
              pilot explicitly wants to see when they're not flying
              themselves. The OLD behaviour only showed the equivalent
              full-map sentence when there was NEITHER an own flight NOR any
              VA traffic — restoring that exact gate here. Bildpunkte zaehlen
              genauso: die Karte laege sonst ueber ihnen, sie waeren nicht klickbar. */}
          {!showOwnContent && vaVisible.length === 0 && !(bildKarte.bilderAn && bildKarte.marken > 0) && (
            <LiveMapEmptyState next={next} onGoToBriefing={onSwitchToBriefing} />
          )}

          {/* README §7 — System-Zeile, overlay bottom-left. Field feedback
              (2026-08-03): the "Systemmeldungen (n)" count-link was replaced
              by the actual recorder connection detail (last send, total
              sent) — the same LiveRecordingIndicator the sidebar already
              shows, so "what/when/how much are we sending" lives where the
              pilot is actually looking, not behind a bare counter. */}
          <div className="aa-livemap-sysline aa-livemap-overlay">
            {/* Wie lange der letzte Netz-Durchlauf gebraucht hat, getrennt
                nach Holen und Zeichnen. Beim ersten Zuschalten dauert es
                20-25 s, und aus der Ferne war nicht zu klaeren, woran —
                also misst es sich selbst. */}
            {netzZeit && (
              <span
                className="aa-livemap-sysline__text"
                style={{ opacity: 0.7, fontVariantNumeric: "tabular-nums" }}
                title={t("livemap.netz_zeit_hint", "Holen der Daten und Zeichnen der Flächen beim letzten Durchlauf")}
              >
                {netzZeit.erster ? "Netz (erstmals): " : "Netz: "}
                {(netzZeit.abruf / 1000).toFixed(1)} s holen ·{" "}
                {netzZeit.zeichnen === null
                  ? "zeichnet …"
                  : `${(netzZeit.zeichnen / 1000).toFixed(1)} s zeichnen`}
              </span>
            )}
            <span className={`aa-livemap-sysline__dot aa-livemap-sysline__dot--${recorderState}`} aria-hidden="true" />
            <span className="aa-livemap-sysline__text">
              {recorderState === "off"
                ? t("livemap.sys_inactive")
                : t("livemap.sys_active", {
                    sim: simKindLabel(simKind),
                    uplink: recorderState === "ok" ? t("livemap.sys_uplink_ok") : t("livemap.sys_uplink_lost"),
                  })}
            </span>
            {/* QS-Runde 5: `sichtbar` gehoert hier dazu, nicht nur `activeFlight`.
                LiveRecordingIndicator haelt einen eigenen Sekundentakt fuer sein
                "vor X Sekunden" — mit der dauerhaft eingehaengten Karte liefe
                der sonst den ganzen Flug lang in einem unsichtbaren Teilbaum.
                Verdeckt gar nicht erst einhaengen ist billiger als jede Sperre
                im Inneren, und beim Auftauchen steht sofort der aktuelle Wert. */}
            {activeFlight && sichtbar && (
              <>
                <span className="aa-livemap-sysline__sep" aria-hidden="true" />
                <LiveRecordingIndicator
                  lastPositionAt={activeFlight.last_position_at}
                  queuedCount={activeFlight.queued_position_count}
                  positionCount={activeFlight.position_count}
                  connectionState={
                    activeFlight.connection_state === "blocked"
                      ? "blocked"
                      : activeFlight.connection_state === "failing"
                        ? "failing"
                        : "live"
                  }
                />
              </>
            )}
          </div>
        </div>
      </div>
      {bildKarte.gross && <BildGross bilder={bildKarte.gross.bilder} startId={bildKarte.gross.id} onZu={bildKarte.grossZu} />}
    </section>
  );
}
