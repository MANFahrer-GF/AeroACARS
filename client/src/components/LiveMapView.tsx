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
// Rein Anzeige — keine Wertung.

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  buildPilotFeatures, emptyFC, fetchVatsimData, esc as vatsimEsc,
} from "../lib/vatsimKarte";
import { ladeSektoren, type Netz } from "../lib/vatglassesKarte";
import maplibregl, { type StyleSpecification } from "maplibre-gl";
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

// Die Stil-Adressen kommen vom Server — siehe `BasemapContext`. CARTO
// verlangt seit dem 26.08.2026 einen Schlüssel, und der soll austauschbar
// sein, ohne dass jeder Pilot ein Update braucht.
//
// Satellit (Esri World Imagery + Namens-Overlay, kein API-Key). Manuell wählbar
// über den Karten-Toggle. glyphs auf die CARTO-Fonts (führen "Noto Sans Regular"
// wie dark/light — demotiles hat den Font NICHT, sonst fehlten Waypoint-Namen).
//
// ⚠ Auch die SCHRIFTEN kommen von CARTO. Der Satellitenstil braucht den
// Schlüssel deshalb genauso, obwohl seine Kacheln von Esri stammen —
// sonst fehlten dort eines Tages alle Beschriftungen.
const satStil = (glyphen: string): StyleSpecification => ({
  version: 8,
  glyphs: glyphen,
  sources: {
    "esri-imagery": {
      type: "raster",
      tiles: ["https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}"],
      tileSize: 256,
      maxzoom: 19,
      attribution: "Imagery © Esri, Maxar, Earthstar Geographics, USDA, USGS, AeroGRID, IGN, GIS User Community",
    },
    "esri-reference": {
      type: "raster",
      tiles: ["https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}"],
      tileSize: 256,
      maxzoom: 19,
    },
  },
  layers: [
    { id: "esri-imagery", type: "raster", source: "esri-imagery" },
    { id: "esri-reference", type: "raster", source: "esri-reference" },
  ],
});

interface RouteFix {
  ident: string;
  lat: number;
  lon: number;
  kind: string;
}
interface VaFlight {
  id?: number | string;
  ident?: string;
  flight_number?: string;
  status_text?: string;
  phase?: number | string;
  airline?: { icao?: string; iata?: string } | null;
  aircraft?: { icao?: string; registration?: string; name?: string } | null;
  dpt_airport_id?: string;
  arr_airport_id?: string;
  user?: { name?: string; ident?: string; pilot_id?: string } | null;
  position?: {
    lat?: number;
    lon?: number;
    heading?: number;
    altitude?: number;
    altitude_msl?: number;
    altitude_agl?: number;
    gs?: number;
    ias?: number;
    vs?: number;
  } | null;
}
interface Aircraft {
  lon: number;
  lat: number;
  hdg: number;
}

/** Nur die Sektoren zeigen, die bei dieser Flugflaeche zustaendig sind.
 *
 *  Der Server liefert mit `fl=alle` jedes Hoehenband auf einmal; gefiltert
 *  wird hier. Dadurch wirkt der Regler augenblicklich statt erst beim
 *  naechsten Abruf — und die Karte zeigt dasselbe wie die Live-Karte auf
 *  live.kant.ovh, die es genauso macht. Flaechen ohne Hoehenangabe und die
 *  Nahverkehrsbereiche gelten immer: sie haben kein Band, das man filtern
 *  koennte.
 */
function hoehenfilterSetzen(m: maplibregl.Map, fl: number): void {
  const filter: maplibregl.FilterSpecification = ["any",
    ["has", "ohne_hoehe"],
    ["has", "tracon"],
    ["all",
      ["<=", ["coalesce", ["get", "fl_von"], 0], fl],
      [">=", ["coalesce", ["get", "fl_bis"], 999], fl],
    ],
  ];
  for (const id of ["vatsim-sectors-fill", "vatsim-sectors-line", "vatsim-sector-labels-symbol"]) {
    if (m.getLayer(id)) {
      try { m.setFilter(id, filter); } catch { /* Ebene fehlt → nichts zu filtern */ }
    }
  }
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
  va: "<path d=\"M16 10h4a2 2 0 0 1 0 4h-4l-4 7h-3l2 -7h-4l-2 2h-3l2 -4l-2 -4h3l2 2h4l-2 -7h3l4 7\" />",
};

/** Eine Kartenebene anlegen, ohne die uebrigen zu gefaehrden.
 *
 *  MapLibre wirft bei einem ungueltigen Paint- oder Layout-Ausdruck. Da
 *  die Ebenen nacheinander angelegt werden, riss ein einziger Fehler
 *  alle nachfolgenden mit — zuletzt ein `case` bei `line-dasharray`
 *  (nimmt keine Ausdruecke), wodurch Lotsen, Tuerme und Sektoren
 *  verschwanden und nur der Verkehr blieb. Eine kaputte Ebene darf
 *  fehlen; sie darf nicht die Karte leeren.
 */
function ebeneAnlegen(
  m: maplibregl.Map,
  spec: maplibregl.LayerSpecification,
  davor?: string,
): void {
  if (m.getLayer(spec.id)) return;
  try {
    m.addLayer(spec, davor);
  } catch (e) {
    console.warn(`[karte] Ebene "${spec.id}" nicht angelegt:`, e);
  }
}

function readTheme(): "dark" | "light" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}
/** Great-circle-Distanz in nautischen Meilen zwischen [lon,lat]-Paaren. */
function distNm(a: [number, number], b: [number, number]): number {
  const R = 3440.065; // Erdradius in nm
  const toRad = (d: number) => (d * Math.PI) / 180;
  const dLat = toRad(b[1] - a[1]);
  const dLon = toRad(b[0] - a[0]);
  const la1 = toRad(a[1]);
  const la2 = toRad(b[1]);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(la1) * Math.cos(la2) * Math.sin(dLon / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
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

function escHtml(s: unknown): string {
  return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string);
}
function flLabel(msl?: number | null): string {
  if (msl == null || Number.isNaN(msl)) return "—";
  return msl >= 18000 ? `FL${Math.round(msl / 100)}` : `${Math.round(msl)} ft`;
}
/** README §5 — Kollegen-Karteichen: header (callsign+pilot), route row
 *  (DEP→ARR + phase), 3-col grid (HÖHE/GS/ETA), footer (Muster). `eta` is
 *  computed outside this function (needs an async airport-coordinate
 *  lookup — see `vaEtaFor` at the call site) and passed in, defaulting to
 *  "—" until resolved so opening the popup never blocks on it. */
function vaPopupHtml(f: VaFlight, eta = "—"): string {
  const cs = f.ident || [f.airline?.icao, f.flight_number].filter(Boolean).join("") || f.flight_number || "—";
  const ac = [f.aircraft?.icao, f.aircraft?.registration].filter(Boolean).join(" · ");
  const pilot = f.user?.name ?? "";
  const phaseTxt = f.status_text ?? "";
  // Field feedback (2026-08-03): the colored phase pill from the old popup
  // ("fand ich richtig mega") comes back exactly as it was — same
  // phaseColor() source as the marker itself, not the plain dim text this
  // rewrite briefly had instead. Text + color together (not color alone)
  // already satisfies README §9's "Zustand nie nur über Farbe".
  const pcol = phaseColor(f.status_text ?? (f.phase != null ? String(f.phase) : null));
  const pos = f.position ?? {};
  const gs = pos.gs != null ? `${Math.round(pos.gs)}` : "—";
  const cell = (k: string, v: string, u = "") =>
    `<div class="aa-vapop__cell"><span class="aa-vapop__k">${k}</span><span class="aa-vapop__v">${escHtml(v)}${u ? `<i>${u}</i>` : ""}</span></div>`;
  return (
    `<div class="aa-vapop__head">` +
    `<span class="aa-vapop__cs">${escHtml(cs)}</span>` +
    (pilot ? `<span class="aa-vapop__pilot">${escHtml(pilot)}</span>` : "") +
    `</div>` +
    `<div class="aa-vapop__route">` +
    `<span>${escHtml(f.dpt_airport_id ?? "—")}<span class="aa-vapop__arrow">→</span>${escHtml(f.arr_airport_id ?? "—")}</span>` +
    (phaseTxt ? `<span class="aa-vapop__phase" style="--p:${pcol}">${escHtml(phaseTxt)}</span>` : "") +
    `</div>` +
    `<div class="aa-vapop__grid">` +
    cell("HÖHE", flLabel(pos.altitude_msl ?? pos.altitude)) +
    cell("GS", gs, " kt") +
    cell("ETA", eta) +
    `</div>` +
    (ac ? `<div class="aa-vapop__sub">${escHtml(ac)}</div>` : "")
  );
}

const SRC_ROUTE = "aa-planned-route";
const SRC_WPTS = "aa-planned-wpts";
const SRC_TRACK = "aa-flown-track";
const SRC_TRACK_DOTS = "aa-flown-track-dots";
const LYR_ROUTE = "aa-planned-route-line";
const LYR_WPTS = "aa-planned-wpts-circles";
const LYR_WPT_LABELS = "aa-planned-wpts-labels";
const LYR_TRACK = "aa-flown-track-line";
const LYR_TRACK_DOTS = "aa-flown-track-dots-circles";
const LYR_ROUTE_CASING = "aa-planned-route-casing"; // dunkle Unterlage nur auf Satellit

// ─── v0.21: Taxi-Karte (Flughafen-Bodendaten) ────────────────────────────
//
// Rollwege, Haltepunkte und Standplaetze aus OpenStreetMap (ODbL), auf dem VPS
// gespiegelt. Der Layer liegt UNTER Route und Track — die Taxi-Karte ist der
// Untergrund, auf dem man rollt, nicht die Hauptsache.
//
// Sichtbar erst ab Zoom 12: darueber ist man im Anflug oder Reiseflug, da
// stoert das Rollweg-Gewirr nur. Beim Rollen zoomt man ohnehin nah heran.
const SRC_GROUND = "aa-ground";
const LYR_GROUND_APRON = "aa-ground-apron";
const LYR_GROUND_TAXI = "aa-ground-taxi";
const LYR_GROUND_TAXI_LABELS = "aa-ground-taxi-labels";
const LYR_GROUND_RWY = "aa-ground-runway";
const LYR_GROUND_STANDS = "aa-ground-stands";
const LYR_GROUND_STAND_LANES = "aa-ground-stand-lanes";
const LYR_GROUND_STAND_LABELS = "aa-ground-stand-labels";
const LYR_GROUND_TERMINAL = "aa-ground-terminal";
const LYR_GROUND_HOLD = "aa-ground-holding";
const LYR_GROUND_RWY_LABELS = "aa-ground-runway-labels";
const GROUND_MIN_ZOOM = 12;

// README §3 EBENEN — originally spec'd as one combined "Track & Taxiweg"
// switch; field feedback (2026-08-03) asked for them separable ("Track
// ein-/ausschalten, Taxiwege ein-/ausschalten... das wäre gut, wenn man das
// separat machen könnte"), so these are two independent layer groups now,
// each with its own toggle.
const TRACK_LAYERS = [LYR_ROUTE, LYR_ROUTE_CASING, LYR_WPTS, LYR_WPT_LABELS, LYR_TRACK, LYR_TRACK_DOTS];
const TAXI_LAYERS = [
  LYR_GROUND_APRON,
  LYR_GROUND_TAXI,
  LYR_GROUND_TAXI_LABELS,
  LYR_GROUND_RWY,
  LYR_GROUND_STANDS,
  LYR_GROUND_STAND_LANES,
  LYR_GROUND_STAND_LABELS,
  LYR_GROUND_TERMINAL,
  LYR_GROUND_HOLD,
  LYR_GROUND_RWY_LABELS,
];

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
}

export function LiveMapView({ activeFlight, simSnapshot, simKind, onSwitchToBriefing, sichtbar = true }: Props) {
  const grundlage = useKartengrundlage();
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  // v0.21: Bodendaten der Taxi-Karte. Ref (nicht State), weil `addOverlays`
  // sie beim Basemap-Wechsel braucht — und das laeuft ausserhalb des Renders.
  const groundRef = useRef<GeoJSON.FeatureCollection | null>(null);
  const [groundLoaded, setGroundLoaded] = useState<string[]>([]);
  const acMarkerRef = useRef<maplibregl.Marker | null>(null);
  const pinMarkersRef = useRef<maplibregl.Marker[]>([]);
  const vaMarkersRef = useRef<maplibregl.Marker[]>([]);
  const vaPopupRef = useRef<maplibregl.Popup | null>(null);
  const vaPopupIdRef = useRef<string | null>(null); // welcher VA-Flug das offene Popup zeigt
  // README §5 — colleague popup ETA needs the arrival airport's coordinate,
  // which /api/acars doesn't send (only the ICAO). Cached per-ICAO so
  // reopening/refreshing a popup for the same destination doesn't re-fetch.
  const airportCoordCacheRef = useRef(new Map<string, [number, number] | null>());
  const vaFittedRef = useRef(false);
  // Dead-Reckoning: VA-Marker zwischen den 12-s-Polls flüssig weiterrechnen.
  const vaDrRef = useRef<{ marker: maplibregl.Marker; lat: number; lon: number; hdg: number; gs: number }[]>([]);
  const vaDrT0Ref = useRef(0);
  const acCatRef = useRef<string | null>(null); // zuletzt gerendertes Eigen-Flieger-Icon (ICAO)
  // v0.15.8: „beim bewussten Einschalten von Follow EINMAL den phasen-passenden
  // Zoom setzen" — danach bleibt der manuelle Zoom des Nutzers erhalten (laufendes
  // Folgen schwenkt nur noch). Startet true, damit die erste Aktivierung greift.
  const followEngageRef = useRef(true);
  // v0.15.6: verhindert, dass ein Nutzer-Pan (der Follow ausschaltet) die
  // „auf-die-ganze-Route-zoomen"-Logik auslöst. Nur BEWUSSTES Abhaken von Follow
  // soll auf die Route fitten — ein Pan lässt die Ansicht einfach stehen.
  const suppressRouteFitRef = useRef(false);
  const dataRef = useRef<{
    fixes: RouteFix[];
    track: [number, number][];
    dep?: [number, number];
    arr?: [number, number];
    nextIdent?: string;
  }>({ fixes: [], track: [] });

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
  const basemapRef = useRef<"auto" | "sat">(basemap);
  const [showVa, setShowVa] = useState(true); // VA-Verkehr ein-/ausblenden
  // VATSIM-Overlay (Sektoren + Lotsen + fremde Piloten). Standard AUS und
  // gemerkt: das ist eine bewusste Zuschaltung, kein Dauerzustand — die
  // Karte gehoert zuerst dem eigenen Flug.
  // Welches Netz die Karte zeigt. Nie beide zugleich: zwei Netze
  // uebereinander ergeben ein Bild, in dem niemand mehr erkennt, wer
  // welchen Luftraum betreut. Die Ausschliesslichkeit steckt deshalb in
  // der Bauform — ein Umschalter statt zweier Schalter, es GEHT nicht
  // anders. Siehe docs/spec/livemap-netze-und-leiste.md.
  /** Wie lange der letzte Netz-Durchlauf gedauert hat, in Millisekunden.
   *
   *  Eingebaut, weil das Zuschalten beim ersten Mal 20-25 s dauert und ich
   *  es aus der Ferne nicht eingrenzen konnte: Datenkette gemessen 471 ms,
   *  Server hoechstens 1,2 s ueber 27 Abrufe, Grafikkarte arbeitet (die
   *  Software-Rendering-These ist damit tot). Bleibt das Zeichnen — und das
   *  misst sich nur dort, wo es passiert. `zeichnen` laeuft bis die Karte
   *  nach dem Setzen der Daten zur Ruhe kommt, erfasst also auch das
   *  Zerlegen der Geometrie im Hintergrund. */
  const [netzZeit, setNetzZeit] = useState<
    { abruf: number; zeichnen: number | null; erster: boolean } | null
  >(null);
  const netzErsterRef = useRef(true);

  // Beim Start IMMER aus. Nicht gemerkt, bewusst.
  //
  // Ein Netz ist eine Zuschaltung fuer den Moment, kein Dauerzustand: die
  // Karte gehoert zuerst dem eigenen Flug. Wer sie oeffnet, soll sehen,
  // was er fliegt — und selbst entscheiden, ob fremder Verkehr dazukommt
  // (Thomas, 16.08.2026). Nebenbei faellt damit die teuerste Arbeit weg,
  // solange niemand sie angefordert hat.
  const [netz, setNetz] = useState<Netz>("aus");
  const showVatsim = netz !== "aus";
  const vatsimPilotsRef = useRef<GeoJSON.FeatureCollection>(emptyFC());
  const vatsimAtcRef = useRef<GeoJSON.FeatureCollection>(emptyFC());
  const vatsimSectorsRef = useRef<GeoJSON.FeatureCollection>(emptyFC());
  const vatsimSectorLabelsRef = useRef<GeoJSON.FeatureCollection>(emptyFC());
  const vatsimPopupRef = useRef<maplibregl.Popup | null>(null);
  // Welche Flugflaeche die Sektoren zeigen. "auto" folgt dem eigenen Flug —
  // die Karte beantwortet dann durchgehend "wer ist fuer MICH zustaendig".
  // Ohne Flug gilt der Regler (Start FL200: dort spielt der Streckenflug).
  const [flModus, setFlModus] = useState<"auto" | "fest">("auto");
  const [flWert, setFlWert] = useState(200);
  const flModusRef = useRef(flModus);
  const flWertRef = useRef(flWert);
  useEffect(() => { flModusRef.current = flModus; flWertRef.current = flWert; }, [flModus, flWert]);
  const simSnapshotRef = useRef<SimSnapshot | null>(null);
  useEffect(() => { simSnapshotRef.current = simSnapshot; });
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

  // ── VATSIM-Overlay: Daten holen, solange es eingeschaltet ist ─────────
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    // Verdeckt kein 1,8-MB-Datafeed alle 30 s. Beim Wiederauftauchen laeuft der
    // Effekt neu und holt sofort einmal; ist die Antwort noch frisch, kommt sie
    // aus dem Zwischenspeicher in `fetchVatsimData` statt aus dem Netz.
    if (!sichtbar) return;

    const setzen = (
      pilots: GeoJSON.FeatureCollection, atc: GeoJSON.FeatureCollection,
      sectors: GeoJSON.FeatureCollection, labels: GeoJSON.FeatureCollection,
    ) => {
      vatsimPilotsRef.current = pilots;
      vatsimAtcRef.current = atc;
      vatsimSectorsRef.current = sectors;
      vatsimSectorLabelsRef.current = labels;
      (map.getSource("vatsim-pilots") as maplibregl.GeoJSONSource | undefined)?.setData(pilots);
      (map.getSource("vatsim-atc-airports") as maplibregl.GeoJSONSource | undefined)?.setData(atc);
      (map.getSource("vatsim-sectors") as maplibregl.GeoJSONSource | undefined)?.setData(sectors);
      (map.getSource("vatsim-sector-labels") as maplibregl.GeoJSONSource | undefined)?.setData(labels);
    };

    if (!showVatsim) {
      vatsimPopupRef.current?.remove();
      vatsimPopupRef.current = null;
      setzen(emptyFC(), emptyFC(), emptyFC(), emptyFC());
      return;
    }

    let beendet = false;
    let abbruch = new AbortController();
    const holen = async () => {
      abbruch.abort();
      abbruch = new AbortController();
      const tStart = performance.now();
      try {
        // VATSpy braucht der Client nicht mehr — der Server wertet es aus.
        const daten = await fetchVatsimData(abbruch.signal);
        if (beendet) return;
        // Sektoren nach HOEHE: im Flug die eigene Flugflaeche, sonst der
        // Regler. Genau dafuer sind die VATGlasses-Daten da — die flachen
        // FIR-Flaechen waren fuer Deutschland schlicht falsch.
        const eigeneFl = simSnapshotRef.current
          ? Math.max(0, Math.round((simSnapshotRef.current.altitude_msl_ft ?? 0) / 100))
          : null;
        const fl = flModusRef.current === "auto" && eigeneFl != null ? eigeneFl : flWertRef.current;
        // Die Sektoren rechnet der Live-Server — dieselbe Quelle wie die
        // Karte auf live.kant.ovh, damit beide dasselbe zeigen.
        // Alle Hoehenbaender auf einmal — gefiltert wird auf der Karte.
        const sektoren = await ladeSektoren(
          "alle", abbruch.signal, undefined, netz,
        );
        if (beendet) return;
        setzen(
          buildPilotFeatures(daten.pilots),
          // Platz-Marker kommen seit dem Umbau vom Server — dieselbe
          // Quelle wie die Live-Karte. Vorher rechnete der Client sie
          // selbst und lag daneben: Marker uebereinander, "LGT" statt
          // Balken, US- und australische Plaetze unsichtbar.
          sektoren.plaetze,
          // Die kleinen Nahverkehrsbereiche zuletzt, damit sie nicht
          // unter einem Hoehensektor verschwinden.
          // Reihenfolge wie auf der Live-Karte: gebuchte und grobe
          // Grenzen unten, Hoehensektoren darueber, Nahbereiche zuoberst.
          { type: "FeatureCollection",
            features: [
              ...sektoren.gebucht.features,
              // Die groben FIR-Flaechen tragen andere Feldnamen als die
              // Sektoren (`fir`, `fir_name`, `controllers`). Ohne diese
              // Uebersetzung zeigte das Klickfenster leere Striche —
              // "– –" und "FL–FL". Wortgleich von der Live-Karte.
              ...sektoren.firFlaechen.features.map((f) => ({
                ...f,
                properties: {
                  ...f.properties,
                  ruf: String(f.properties?.ruf ?? f.properties?.fir ?? ""),
                  gesprochen: String(f.properties?.fir_name || f.properties?.fir || ""),
                  frequenz: String(f.properties?.frequenz ?? ""),
                  farbe: "#4ade80",
                  ohne_hoehe: 1,
                },
              })),
              ...sektoren.flaechen.features,
              ...sektoren.nahbereich.features,
            ] },
          { type: "FeatureCollection",
            features: [
              ...sektoren.marken.features,
              ...sektoren.firMarken.features.map((f) => ({
                ...f,
                properties: { ...f.properties, farbe: "#4ade80" },
              })),
              ...sektoren.nahbereichMarken.features,
            ] },
        );
        // Der Server hat jedes Hoehenband geliefert — hier faellt die
        // Entscheidung, welches davon zu sehen ist.
        hoehenfilterSetzen(map, fl);

        // Bis die Karte zur Ruhe kommt: das ist der Teil, der beim ersten
        // Zuschalten so lange braucht.
        const tGesetzt = performance.now();
        const erster = netzErsterRef.current;
        netzErsterRef.current = false;
        const abruf = Math.round(tGesetzt - tStart);
        // SOFORT anzeigen, mit noch offenem Zeichnen-Wert.
        //
        // Vorher hing die ganze Anzeige an `idle`. Kommt die Karte nie zur
        // Ruhe — weil dauernd Kacheln nachladen —, erschien gar nichts, und
        // die Messung schwieg genau dann, wenn es interessant wurde
        // (Feldbefund Thomas: "habe ich nicht gesehen").
        setNetzZeit({ abruf, zeichnen: null, erster });
        let fertig = false;
        const abschliessen = () => {
          if (fertig || beendet) return;
          fertig = true;
          setNetzZeit({ abruf, zeichnen: Math.round(performance.now() - tGesetzt), erster });
        };
        map.once("idle", abschliessen);
        // Sicherheitsnetz: kommt die Ruhe nicht, gilt der Wert nach 60 s
        // als das, was er ist — mindestens so lange.
        window.setTimeout(abschliessen, 60_000);
      } catch (e) {
        if (!beendet && (e as Error)?.name !== "AbortError") {
          console.warn("[vatsim] Abruf fehlgeschlagen:", e);
        }
      }
    };
    void holen();
    const takt = setInterval(() => { void holen(); }, 30_000);
    return () => { beendet = true; abbruch.abort(); clearInterval(takt); };
    // Der Regler loest KEINEN neuen Abruf mehr aus: die Daten enthalten
    // ohnehin jedes Band, gefiltert wird im Effekt darunter.
    //
    // Abhaengig von `netz`, NICHT von `showVatsim`. Die abgeleitete
    // Ja/Nein-Groesse bleibt beim Wechsel VATSIM -> IVAO unveraendert wahr;
    // der Effekt lief dann nicht neu, und man musste erst auf "Aus" und
    // zurueck (Feldbefund Thomas, 16.08.2026). Genau die Falle, die eine
    // abgeleitete Groesse in einer Abhaengigkeitsliste aufmacht.
  }, [netz, mapReady, sichtbar]);

  // Hoehenregler: wirkt augenblicklich auf die schon geladenen Sektoren.
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || !showVatsim) return;
    const eigeneFl = simSnapshotRef.current
      ? Math.max(0, Math.round((simSnapshotRef.current.altitude_msl_ft ?? 0) / 100))
      : null;
    hoehenfilterSetzen(
      map,
      flModus === "auto" && eigeneFl != null ? eigeneFl : flWert,
    );
  }, [mapReady, showVatsim, flModus, flWert]);

  // Klickfenster fuer das VATSIM-Overlay — Klartext statt Rohdaten
  // (dieselbe Bauform wie auf der Live-Karte der Webapp).
  //
  // Feldbefund (Screenshot, 12.08.): zwei Fenster uebereinander. Zwei
  // Ursachen: jeder Klick baute ein NEUES Fenster, ohne das alte zu
  // schliessen — und ein Klick, der Platz UND Sektor trifft, feuerte
  // BEIDE Behandler. Deshalb: EIN gemeinsames Fenster (Ref), und eine
  // Vorfahrtsregel ueber ein Merkmal am Ursprungs-Ereignis.
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;

    const fenster = vatsimPopupRef;
    const oeffnen = (lngLat: maplibregl.LngLatLike, html: string, breite: string) => {
      fenster.current?.remove();
      fenster.current = new maplibregl.Popup({ closeButton: true, className: "aa-vapop", maxWidth: breite })
        .setLngLat(lngLat)
        .setHTML(html)
        .addTo(map);
    };
    const schonBehandelt = (e: maplibregl.MapMouseEvent): boolean => {
      const ev = e.originalEvent as MouseEvent & { _vatsimKlick?: boolean };
      if (ev._vatsimKlick) return true;
      ev._vatsimKlick = true;
      return false;
    };

    const aufPilot = (e: maplibregl.MapMouseEvent & { features?: maplibregl.MapGeoJSONFeature[] }) => {
      const f = e.features?.[0]; if (!f || schonBehandelt(e)) return;
      const p = f.properties ?? {};
      const alt = Number(p.alt ?? 0), gs = Number(p.gs ?? 0);
      oeffnen(e.lngLat, 
          `<div class="vatsim-pop">` +
          `<div class="vatsim-pop-title">${vatsimEsc(p.callsign)}</div>` +
          `<div class="vatsim-pop-row">${vatsimEsc(p.dep ?? "—")} → ${vatsimEsc(p.arr ?? "—")}</div>` +
          `<div class="vatsim-pop-row vatsim-pop-dim">${vatsimEsc(p.actype ?? "—")} · FL${String(Math.round(alt / 100)).padStart(3, "0")} · ${gs} kt</div>` +
          `</div>`, "260px");
    };
    const aufAtc = (e: maplibregl.MapMouseEvent & { features?: maplibregl.MapGeoJSONFeature[] }) => {
      const f = e.features?.[0]; if (!f || schonBehandelt(e)) return;
      const p = f.properties ?? {};
      let liste: { tag: string; callsign: string; frequency: string; code?: string }[] = [];
      try { liste = JSON.parse(String(p.positions ?? "[]")); } catch { liste = []; }
      const reihenfolge = ["ATIS", "DEL", "GND", "TWR", "APP"];
      liste.sort((a, b) => reihenfolge.indexOf(a.tag) - reihenfolge.indexOf(b.tag));
      const zeilen = liste.map((x) =>
        `<div class="vatsim-pop-row"><span class="vatsim-pop-chip vatsim-pop-chip--${vatsimEsc(x.tag.toLowerCase())}">${vatsimEsc(x.tag)}</span>` +
        `<b>${vatsimEsc(x.callsign)}</b>` +
        `<span class="vatsim-pop-freq">${vatsimEsc(x.frequency)}</span>` +
        (x.tag === "ATIS" && x.code ? `<span class="vatsim-pop-info">INFO: ${vatsimEsc(x.code)}</span>` : "") +
        `</div>`).join("");
      oeffnen(e.lngLat, `<div class="vatsim-pop"><div class="vatsim-pop-title">${vatsimEsc(p.icao)} — ATC</div>${zeilen}</div>`, "300px");
    };
    const aufSektor = (e: maplibregl.MapMouseEvent & { features?: maplibregl.MapGeoJSONFeature[] }) => {
      const f = e.features?.[0]; if (!f || schonBehandelt(e)) return;
      const p = f.properties ?? {};
      oeffnen(e.lngLat,
          `<div class="vatsim-pop">` +
          `<div class="vatsim-pop-title">${vatsimEsc(p.ruf)}</div>` +
          `<div class="vatsim-pop-row"><b>${vatsimEsc(p.gesprochen || "—")}</b>` +
          `<span class="vatsim-pop-freq">${vatsimEsc(p.frequenz || "—")}</span></div>` +
          // Die Bezeichnung fehlt, wenn mehrere unterschiedlich benannte
          // Bloecke zu einer Flaeche verschmolzen wurden — dann steht
          // hier nur das Hoehenband statt eines fuehrenden Trennpunkts.
          (p.block
            ? `<div class="vatsim-pop-row vatsim-pop-dim">${vatsimEsc(p.block)} · FL${vatsimEsc(p.fl_von)}–FL${vatsimEsc(p.fl_bis)}</div>`
            : `<div class="vatsim-pop-row vatsim-pop-dim">FL${vatsimEsc(p.fl_von)}–FL${vatsimEsc(p.fl_bis)}</div>`) +
          `</div>`, "300px");
    };

    const zeiger = () => { map.getCanvas().style.cursor = "pointer"; };
    const zeigerWeg = () => { map.getCanvas().style.cursor = ""; };
    map.on("click", "vatsim-pilots-symbol", aufPilot);
    map.on("click", "vatsim-atc-airports-circle", aufAtc);
    map.on("click", "vatsim-sectors-fill", aufSektor);
    for (const id of ["vatsim-pilots-symbol", "vatsim-atc-airports-circle", "vatsim-sectors-fill"]) {
      map.on("mouseenter", id, zeiger);
      map.on("mouseleave", id, zeigerWeg);
    }
    return () => {
      map.off("click", "vatsim-pilots-symbol", aufPilot);
      map.off("click", "vatsim-atc-airports-circle", aufAtc);
      map.off("click", "vatsim-sectors-fill", aufSektor);
      for (const id of ["vatsim-pilots-symbol", "vatsim-atc-airports-circle", "vatsim-sectors-fill"]) {
        map.off("mouseenter", id, zeiger);
        map.off("mouseleave", id, zeigerWeg);
      }
    };
  }, [mapReady]);

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
  const [vaFlights, setVaFlights] = useState<VaFlight[]>([]);
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
  // VA-Flieger ohne den eigenen Flug (steckt auch in /api/acars).
  const vaVisible = useMemo(
    () => (showVa ? vaFlights.filter((f) => !activeFlight || String(f.id ?? "") !== activeFlight.pirep_id) : []),
    [showVa, vaFlights, activeFlight],
  );

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

  // dataRef für die styledata-Re-Adds aktuell halten
  dataRef.current = { fixes: effFixes, track: effTrackLive, dep: effDep, arr: effArr, nextIdent: nav.nextIdent };

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style:
        basemapRef.current === "sat"
          ? satStil(grundlage.glyphen)
          : readTheme() === "dark"
            ? grundlage.dunkel
            : grundlage.hell,
      center: [6, 48],
      zoom: 4,
      attributionControl: { compact: true },
      // Der Schlüssel gehört an JEDE Anfrage, nicht nur an die
      // Stil-Adresse — die style.json verweist selbst weiter auf
      // TileJSON, Kacheln, Schriften und Sprites. Siehe `kartenAnfrage`.
      transformRequest: kartenAnfrage(grundlage.schluessel),
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
        addOverlays(map);
      } catch (e) {
        console.error("[karte] Ebenen konnten nicht angelegt werden:", e);
      } finally {
        setMapReady(true);
      }
    });
    map.on("styledata", () => {
      // Auch hier: ein Fehler darf den Stilwechsel nicht abbrechen.
      if (map.isStyleLoaded()) {
        try { addOverlays(map); }
        catch (e) { console.error("[karte] Ebenen nach Stilwechsel:", e); }
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
    basemapRef.current = basemap;
    try {
      localStorage.setItem("aaLivemapBasemap", basemap);
    } catch {
      /* persist best-effort */
    }
    // setStyle() wirft ALLE Quellen+Layer weg — die Overlays (Route/Track/WPTs)
    // müssen danach neu angelegt werden. Tücke beim MEHRFACHEN Umschalten: ein
    // EINMALIGER Re-Add (sobald isStyleLoaded()) kann mit dem Stil-Abbau RACEN —
    // feuert er, während der alte Overlay-Layer noch existiert, überspringt
    // addOverlays' „nur-wenn-fehlt"-Guard das Neu-Anlegen, danach wischt setStyle
    // den Layer weg → Route weg (genau ab dem 2. Umschalten; beim 1. gibt's noch
    // keinen Alt-Layer, der den Skip auslöst). Darum NICHT einmalig, sondern
    // SELBSTKORRIGIEREND: ein paar Sekunden lang die Overlays IMMER WIEDER
    // anlegen, sobald sie fehlen (kein Einzelschuss, der verlieren kann;
    // konvergiert, sobald der Stil sich beruhigt). isStyleLoaded() statt `idle`
    // (das im Follow-Modus durch easeTo ausgehungert wird) → kamera-/kachel-
    // unabhängig. Unmount-/Re-Run-sicher (cancelled + mapRef-Check).
    const map = mapRef.current;
    if (!map) return;
    map.setStyle(
      basemap === "sat"
        ? satStil(grundlage.glyphen)
        : theme === "dark"
          ? grundlage.dunkel
          : grundlage.hell,
    );
    let cancelled = false;
    let elapsed = 0;
    const ensureOverlays = () => {
      if (cancelled || mapRef.current !== map) return; // unmounted / Karte ersetzt
      // Sentinel LYR_ROUTE: fehlt der Route-Layer, legt addOverlays ALLE Overlays
      // (Route/Casing/Track/WPTs/Labels) wieder an — sie werden gemeinsam erzeugt.
      if (map.isStyleLoaded() && !map.getLayer(LYR_ROUTE)) addOverlays(map);
      elapsed += 120;
      if (elapsed < 4000) setTimeout(ensureOverlays, 120);
    };
    ensureOverlays();
    return () => {
      cancelled = true;
    };
  }, [theme, basemap]);

  // ---- Overlays anlegen (idempotent) + aus dataRef füllen ----
  function addOverlays(map: maplibregl.Map) {
    const sat = basemapRef.current === "sat";
    const accent = cssVar("--accent", "#0a84ff");
    // Auf Satellit (immer dunkel-buntes Bild) die „dunkle-Karte"-Behandlung
    // ERZWINGEN — heller Track/Text + dunkle Halos, unabhängig vom App-Theme;
    // sonst verschwänden Track/Route/Labels auf dem Bild. So sieht alles auf
    // Sat aus wie auf der dunklen Karte.
    const trackColor = sat ? "#f2f5fa" : cssVar("--map-track", "#e8e8e8");
    const haloColor = sat ? "#0b0f16" : cssVar("--surface", "#ffffff");
    const textColor = sat ? "#f4f7fc" : cssVar("--text", "#e8edf2");
    const warning = cssVar("--warning", "#ff9f0a");
    const empty: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };

    // ── Taxi-Karte ────────────────────────────────────────────────────────
    //
    // ZUERST hinzufuegen: MapLibre stapelt in Reihenfolge, und die Bodendaten
    // sind der Untergrund. Route, Track und das eigene Flugzeug gehoeren
    // darueber.
    //
    // Farben: auf Satellit dieselbe Behandlung wie auf der dunklen Karte (siehe
    // `sat` oben) — helle Linien, dunkle Halos. Das Rollweg-Gruen ist bewusst
    // kraeftig: es muss sowohl auf grauem Beton (Satellit) als auch auf der
    // dunklen Karte stehen.
    // ── VATSIM-Overlay — dieselbe ruhige Bauform wie auf der Live-Karte
    //    der Webapp (12.08.2026): Flaeche kaum getoент, duenne Kante,
    //    Kennung + Frequenz IN der Flaeche. Quellen immer anlegen; ob
    //    Daten drinstehen, entscheidet der Poll (showVatsim).
    if (!map.getSource("vatsim-sectors")) {
      map.addSource("vatsim-sectors", { type: "geojson", data: vatsimSectorsRef.current });
    }
    {
      ebeneAnlegen(map, {
        id: "vatsim-sectors-fill", type: "fill", source: "vatsim-sectors",
        // Farbe je BESITZENDER STATION aus den VATGlasses-Daten — dieselben
        // Farben, die Lotsen und VATGlasses-Nutzer kennen.
        paint: {
          "fill-color": ["get", "farbe"],
          // Wie auf der Live-Karte: gebuchte Positionen sehr schwach
          // ("gleich", nicht "jetzt"), grobe Ersatzgrenzen etwas
          // schwaecher als ein Hoehensektor. `has` statt Wertvergleich —
          // die Sektoren fuehren diese Felder gar nicht, und ein
          // Null-Vergleich laesst MapLibre werfen.
          "fill-opacity": ["case",
            ["has", "gebucht"], 0.05,
            ["has", "ohne_hoehe"], 0.13,
            0.16],
        },
      });
    }
    {
      ebeneAnlegen(map, {
        id: "vatsim-sectors-line", type: "line", source: "vatsim-sectors",
        paint: {
          "line-color": ["get", "farbe"],
          "line-width": ["case", ["has", "ohne_hoehe"], 1.4, 1],
          // KEIN Ausdruck bei line-dasharray: das ist in MapLibre eine
          // CrossFadedProperty und nimmt keine Ausdruecke — ein `case`
          // darin laesst das Anlegen der Ebene WERFEN, und da die Ebenen
          // hier nacheinander entstehen, fehlten ab da ALLE folgenden:
          // keine Lotsen, keine Tuerme, nur noch Verkehr.
          "line-opacity": ["case", ["has", "gebucht"], 0.4, 0.6],
        },
      });
    }
    if (!map.getSource("vatsim-sector-labels")) {
      map.addSource("vatsim-sector-labels", { type: "geojson", data: vatsimSectorLabelsRef.current });
    }
    if (!map.getLayer("vatsim-sector-labels-symbol")) {
      {
        // Kein try/catch mehr: `ebeneAnlegen` faengt selbst ab und wirft
        // nicht weiter — der aeussere Block fing also nichts und behauptete
        // trotzdem, fehlende Glyphen zu behandeln. Das tut er nicht: eine
        // fehlende Schrift wirft NICHT beim Anlegen, sie faellt erst beim
        // Zeichnen auf und steht nur in der Browser-Konsole. Genau diese
        // Sorte Schein-Absicherung hat auf der Live-Karte eine ganze Runde
        // gekostet (16.08.2026, 404 auf den kombinierten Schriftsatz).
        ebeneAnlegen(map, {
          id: "vatsim-sector-labels-symbol", type: "symbol", source: "vatsim-sector-labels",
          layout: {
            "text-field": ["format",
              ["get", "ruf"], {},
              "\n", {},
              ["get", "frequenz"], { "font-scale": 0.8 },
            ],
            // CARTO-Glyphs: "Noto Sans Regular" gibt es auf dark, light UND
            // Satellit — "Noto Sans" NICHT (siehe BASEMAP_SAT-Kommentar).
            "text-font": ["Noto Sans Regular"],
            "text-size": 13,
            "text-line-height": 1.3,
            "text-padding": 6,
          },
          paint: {
            "text-color": ["get", "farbe"],
            "text-opacity": 0.95,
            "text-halo-color": "#07090e",
            "text-halo-width": 2,
          },
        });
      }
    }
    if (!map.getSource("vatsim-atc-airports")) {
      map.addSource("vatsim-atc-airports", { type: "geojson", data: vatsimAtcRef.current });
    }
    {
      ebeneAnlegen(map, {
        id: "vatsim-atc-app-ring", type: "circle", source: "vatsim-atc-airports",
        filter: ["==", ["get", "hat_app"], 1],
        paint: {
          // Der Ring IST der Anflug (Radar-Bauform) — die Sektorflaeche
          // der APP-Station wird bewusst nicht gefuellt, sie laege genau
          // ueber dem Platz und allem, was dort beschriftet ist.
          "circle-radius": ["interpolate", ["linear"], ["zoom"], 4, 9, 8, 20],
          "circle-color": "rgba(0,0,0,0)",
          "circle-stroke-width": 1.6,
          "circle-stroke-color": "#fbbf24",
          "circle-stroke-opacity": 0.75,
        },
      });
    }
    {
      ebeneAnlegen(map, {
        id: "vatsim-atc-airports-circle", type: "circle", source: "vatsim-atc-airports",
        paint: {
          "circle-radius": ["case", [">", ["get", "count"], 1], 7, 5.5],
          // Farbe nach Stationsart, wie auf der Live-Karte: ein Turm ist
          // blau, die Rollkontrolle violett. Der Client malte stattdessen
          // ueberall denselben dunklen Punkt mit tuerkisem Rand — dieselbe
          // Lage sah auf beiden Karten verschieden aus.
          "circle-color": ["match", ["get", "tone"],
            "twr", "#38bdf8",
            "gnd", "#818cf8",
            "#38bdf8",
          ],
          "circle-stroke-width": 1.5,
          "circle-stroke-color": "#07090e",
          "circle-opacity": 0.95,
        },
      });
    }
    if (!map.getLayer("vatsim-atc-airports-label")) {
      try {
        ebeneAnlegen(map, {
          id: "vatsim-atc-airports-label", type: "symbol", source: "vatsim-atc-airports",
          layout: {
            // EINE Zeile (Radar-Vorbild): ICAO, daneben die Stationsarten
            // als feste Farb-Slots — A(TIS) gelb, D blau, G gruen, T rot.
            // Approach ist KEIN Buchstabe, sondern der Ring um den Platz.
            // Ein leerer Slot verschwindet einfach.
            "text-field": ["format",
              ["get", "icao"], {},
              "  ", {},
              ["get", "t_atis"], { "text-color": "#eab308" },
              ["get", "t_del"], { "text-color": "#60a5fa" },
              ["get", "t_gnd"], { "text-color": "#4ade80" },
              ["get", "t_twr"], { "text-color": "#f87171" },
            ],
            "text-font": ["Noto Sans Regular"],
            "text-size": 10,
            "text-offset": [0, 1.4],
            "text-anchor": "top",
            "text-padding": 4,
          },
          paint: {
            "text-color": "#bfeef7",
            "text-halo-color": "#07090e",
            "text-halo-width": 2.4,
          },
        });
      } catch { /* dito */ }
    }
    if (!map.getSource("vatsim-pilots")) {
      map.addSource("vatsim-pilots", { type: "geojson", data: vatsimPilotsRef.current });
    }
    if (!map.hasImage("vatsim-plane")) {
      const img = makeVatsimPlaneImage();
      if (img) map.addImage("vatsim-plane", img, { pixelRatio: 2 });
    }
    {
      ebeneAnlegen(map, {
        id: "vatsim-pilots-symbol", type: "symbol", source: "vatsim-pilots",
        // Befund: in der Uebersicht lagen hunderte Pfeile uebereinander.
        // Unter Zoom 4.5 traegt kein einzelner Flieger Information — dort
        // sind Sektoren und Stationen die Karte, nicht der Verkehr.
        minzoom: 4.5,
        layout: {
          "icon-image": "vatsim-plane",
          "icon-rotate": ["get", "hdg"],
          "icon-rotation-alignment": "map",
          "icon-allow-overlap": true,
          "icon-size": ["interpolate", ["linear"], ["zoom"], 4.5, 0.38, 8, 0.7],
        },
        paint: { "icon-opacity": 0.88 },
      });
    }

    if (!map.getSource(SRC_GROUND)) {
      map.addSource(SRC_GROUND, {
        type: "geojson",
        data: groundRef.current ?? empty,
        // ODbL verlangt Namensnennung. An der Quelle statt irgendwo im UI:
        // so kann sie nicht vergessen werden, wenn jemand die Karte umbaut.
        attribution: "Bodendaten © OpenStreetMap contributors (ODbL)",
      });
    }
    if (!map.getLayer(LYR_GROUND_APRON)) {
      map.addLayer({
        id: LYR_GROUND_APRON,
        type: "line",
        source: SRC_GROUND,
        minzoom: GROUND_MIN_ZOOM,
        filter: ["==", ["get", "k"], "apron"],
        paint: {
          "line-color": sat ? "#8aa0b4" : "#3a4a5c",
          "line-width": 1,
          "line-opacity": sat ? 0.5 : 0.7,
        },
      });
    }
    // Zwei Kategorien, nach FUNKTION getrennt — nicht nach OSM-Tag.
    //
    // Die Farbe nach dem Tag zu richten war doppelt falsch.
    // Erst waren Berlin (taxilane) gruen und Hamburg (parking_position) blau —
    // dieselbe Sache, zwei Farben. Dann habe ich ALLES gruen gemacht, und schon
    // war der Stand nicht mehr vom Rollweg zu unterscheiden.
    //
    // Richtig: Es gibt genau ZWEI Dinge, und die Funktion entscheidet die Farbe.
    //   * HAUPTROLLWEG (`taxiway`) — worauf ATC dich schickt, "taxi via A, T".
    //     Gruen, kraeftig.
    //   * STAND-BEREICH — die Vorfeld-Zuwege UND die Standplaetze. Blau.
    //     Dazu zaehlen `taxilane` (so nennt Berlin es), `parking_position` als
    //     Linie (so nennt Hamburg es) und die Stand-Punkte.
    //
    // Weil beide Tags gleich (blau) behandelt werden, sehen Berlin und Hamburg
    // endlich GLEICH aus — und der Stand hebt sich klar vom Rollweg ab.
    const IS_STAND_LANE: maplibregl.FilterSpecification = [
      "any",
      ["==", ["get", "k"], "taxilane"],
      [
        "all",
        ["==", ["get", "k"], "parking_position"],
        ["==", ["geometry-type"], "LineString"],
      ],
    ];
    // Stand-Bereich ZUERST (liegt unter dem Hauptrollweg).
    if (!map.getLayer(LYR_GROUND_STAND_LANES)) {
      map.addLayer({
        id: LYR_GROUND_STAND_LANES,
        type: "line",
        source: SRC_GROUND,
        minzoom: 13,
        filter: IS_STAND_LANE,
        layout: { "line-cap": "round" },
        paint: {
          "line-color": "#60a5fa",
          "line-width": ["interpolate", ["linear"], ["zoom"], 13, 0.5, 16, 1.6, 18, 3],
          "line-opacity": 0.7,
        },
      });
    }
    // Hauptrollwege — gruen. Farbcodierte Rollwege (Muenchen) in ihrer
    // BODENFARBE: kraeftiges Blau bzw. Orange, damit man der Linie folgen kann
    // wie am echten Platz ("folge der blauen Linie zum Gate"). Nur das Label zu
    // faerben war zu unauffaellig — der Pilot orientiert sich an der Linie.
    if (!map.getLayer(LYR_GROUND_TAXI)) {
      map.addLayer({
        id: LYR_GROUND_TAXI,
        type: "line",
        source: SRC_GROUND,
        minzoom: GROUND_MIN_ZOOM,
        filter: ["==", ["get", "k"], "taxiway"],
        layout: { "line-cap": "round", "line-join": "round" },
        paint: {
          "line-color": [
            "case",
            ["==", ["get", "c"], "blue"],
            "#3b82f6",
            ["==", ["get", "c"], "orange"],
            "#f97316",
            "#4ade80",
          ],
          "line-width": ["interpolate", ["linear"], ["zoom"], 12, 1.2, 16, 3.5, 18, 6],
          "line-opacity": 0.95,
        },
      });
    }
    // Stand-Nummern — die stehen im `r`-Feld der parking_position-Linien und
    // gate-Punkte. Erst ab Zoom 15, sonst Zahlensalat auf dem Vorfeld.
    if (!map.getLayer(LYR_GROUND_STAND_LABELS)) {
      map.addLayer({
        id: LYR_GROUND_STAND_LABELS,
        type: "symbol",
        source: SRC_GROUND,
        minzoom: 15,
        filter: [
          "all",
          ["in", ["get", "k"], ["literal", ["parking_position", "gate"]]],
          ["has", "r"],
        ],
        layout: {
          "symbol-placement": "point",
          "text-field": ["get", "r"],
          "text-font": ["Noto Sans Regular", "Arial Unicode MS Regular"],
          "text-size": ["interpolate", ["linear"], ["zoom"], 15, 8, 18, 12],
          "text-allow-overlap": false,
          "text-padding": 2,
        },
        paint: {
          "text-color": "#dbeafe",
          "text-halo-color": "#1e3a5f",
          "text-halo-width": 1.4,
        },
      });
    }
    if (!map.getLayer(LYR_GROUND_RWY)) {
      map.addLayer({
        id: LYR_GROUND_RWY,
        type: "line",
        source: SRC_GROUND,
        minzoom: GROUND_MIN_ZOOM,
        filter: ["==", ["get", "k"], "runway"],
        layout: { "line-cap": "butt" },
        paint: {
          "line-color": sat ? "#e8edf2" : "#c9ccd1",
          "line-width": ["interpolate", ["linear"], ["zoom"], 12, 3, 16, 12, 18, 24],
          "line-opacity": sat ? 0.75 : 0.9,
        },
      });
    }
    // Bahn-Bezeichnung ("05/23") entlang der Bahn. OSM hat sie im `r`-Feld — wir
    // luden sie ohnehin schon mit, zeigten sie nur nicht. Weiss auf schwarzem
    // Halo, damit sie auf dem hellen Bahn-Beton (Satellit) UND auf der dunklen
    // Karte lesbar bleibt.
    if (!map.getLayer(LYR_GROUND_RWY_LABELS)) {
      map.addLayer({
        id: LYR_GROUND_RWY_LABELS,
        type: "symbol",
        source: SRC_GROUND,
        minzoom: GROUND_MIN_ZOOM,
        filter: ["all", ["==", ["get", "k"], "runway"], ["has", "r"]],
        layout: {
          "symbol-placement": "line",
          "text-field": ["get", "r"],
          "text-font": ["Noto Sans Regular", "Arial Unicode MS Regular"],
          "text-size": ["interpolate", ["linear"], ["zoom"], 12, 11, 16, 16, 18, 20],
          "text-letter-spacing": 0.1,
          "text-allow-overlap": false,
          "symbol-spacing": 400,
          "text-keep-upright": true,
        },
        paint: {
          "text-color": "#ffffff",
          "text-halo-color": "#0b0f16",
          "text-halo-width": 2,
        },
      });
    }
    // Terminals: beim Rollen die wichtigste Orientierung ueberhaupt ("ich muss
    // zu Terminal 2"). Sie liegen in OSM als Flaeche vor, der Import speichert
    // sie als Umriss-Linie — als dezente Kontur reicht das voellig.
    if (!map.getLayer(LYR_GROUND_TERMINAL)) {
      map.addLayer({
        id: LYR_GROUND_TERMINAL,
        type: "line",
        source: SRC_GROUND,
        minzoom: 13,
        filter: ["==", ["get", "k"], "terminal"],
        paint: {
          "line-color": sat ? "#c4b5fd" : "#8b7fd4",
          "line-width": 1.4,
          "line-opacity": 0.85,
        },
      });
    }
    // Der fruehere separate blaue Stand-LINIEN-Layer ist weg. Die
    // parking_position-Linien sind die Zuwegung zum Stand — worauf man rollt.
    // Sie werden jetzt vom blauen Stand-Bereich-Layer gezeichnet (IS_STAND_LANE
    // oben), zusammen mit den taxilanes. So bleibt der Stand-Bereich klar vom
    // gruenen Hauptrollweg getrennt und keine Linie wird doppelt gezeichnet.
    if (!map.getLayer(LYR_GROUND_STANDS)) {
      map.addLayer({
        id: LYR_GROUND_STANDS,
        type: "circle",
        source: SRC_GROUND,
        minzoom: 14,
        filter: [
          "all",
          ["in", ["get", "k"], ["literal", ["gate", "parking_position"]]],
          ["==", ["geometry-type"], "Point"],
        ],
        paint: {
          "circle-radius": ["interpolate", ["linear"], ["zoom"], 14, 1.5, 18, 4],
          "circle-color": "#60a5fa",
          "circle-opacity": 0.8,
        },
      });
    }
    // (Stand-Nummern werden oben zusammen mit den Stand-Bereichen gezeichnet.)
    // Haltepunkte vor der Bahn — beim Rollen das Wichtigste ueberhaupt.
    // Deshalb gelb und mit Rand, damit sie auf jedem Untergrund herausstechen.
    if (!map.getLayer(LYR_GROUND_HOLD)) {
      map.addLayer({
        id: LYR_GROUND_HOLD,
        type: "circle",
        source: SRC_GROUND,
        minzoom: 13,
        filter: ["==", ["get", "k"], "holding_position"],
        paint: {
          "circle-radius": ["interpolate", ["linear"], ["zoom"], 13, 2, 18, 6],
          "circle-color": "#facc15",
          "circle-stroke-color": "#0b0f16",
          "circle-stroke-width": 1,
          "circle-opacity": 0.95,
        },
      });
    }
    if (!map.getLayer(LYR_GROUND_TAXI_LABELS)) {
      map.addLayer({
        id: LYR_GROUND_TAXI_LABELS,
        type: "symbol",
        source: SRC_GROUND,
        minzoom: 14,
        filter: [
          "all",
          ["==", ["get", "k"], "taxiway"],
          ["has", "r"],
        ],
        layout: {
          "symbol-placement": "line",
          "text-field": ["get", "r"],
          "text-font": ["Noto Sans Regular", "Arial Unicode MS Regular"],
          "text-size": ["interpolate", ["linear"], ["zoom"], 14, 9, 18, 14],
          "text-allow-overlap": false,
          "text-padding": 4,
          "symbol-spacing": 220,
        },
        paint: {
          // Farbcodierte Rollwege (Muenchen): das Label traegt die Bodenfarbe,
          // damit man "W1 blau" von "W1 orange" unterscheidet — genau das, was
          // der Pilot am Boden sieht. Ohne Farbe: das uebliche Gruen.
          "text-color": [
            "case",
            ["==", ["get", "c"], "blue"],
            "#93c5fd",
            ["==", ["get", "c"], "orange"],
            "#fdba74",
            "#eaf5ec",
          ],
          "text-halo-color": [
            "case",
            ["==", ["get", "c"], "blue"],
            "#1e3a8a",
            ["==", ["get", "c"], "orange"],
            "#7c2d12",
            "#14532d",
          ],
          "text-halo-width": 1.8,
        },
      });
    }

    if (!map.getSource(SRC_ROUTE)) map.addSource(SRC_ROUTE, { type: "geojson", data: empty });
    if (!map.getSource(SRC_WPTS)) map.addSource(SRC_WPTS, { type: "geojson", data: empty });
    if (!map.getSource(SRC_TRACK)) map.addSource(SRC_TRACK, { type: "geojson", data: empty });
    if (!map.getSource(SRC_TRACK_DOTS)) map.addSource(SRC_TRACK_DOTS, { type: "geojson", data: empty });
    // Nur auf Satellit: dunkle Unterlage UNTER der Route (zuerst hinzufügen =
    // liegt unter der eigentlichen Linie), damit die blaue gestrichelte Route
    // auf hellem/buntem Bild nicht verschwindet. Auf Dark/Light nicht nötig.
    if (sat && !map.getLayer(LYR_ROUTE_CASING)) {
      map.addLayer({
        id: LYR_ROUTE_CASING,
        type: "line",
        source: SRC_ROUTE,
        layout: { "line-cap": "butt", "line-join": "round" },
        paint: { "line-color": "#0b0f16", "line-width": 5, "line-opacity": 0.55 },
      });
    }
    if (!map.getLayer(LYR_ROUTE)) {
      map.addLayer({
        id: LYR_ROUTE,
        type: "line",
        source: SRC_ROUTE,
        // butt-Caps statt round, damit die Striche sauber abgesetzt sind.
        layout: { "line-cap": "butt", "line-join": "round" },
        // Stratos-Look: GEPLANTE Route gestrichelt (die tatsächlich GEFLOGENE
        // Spur ist durchgezogen — so unterscheidet man Plan vs. Ist auf einen Blick).
        paint: { "line-color": accent, "line-width": 2.5, "line-opacity": sat ? 0.95 : 0.7, "line-dasharray": [2, 2] },
      });
    }
    if (!map.getLayer(LYR_TRACK)) {
      map.addLayer({
        id: LYR_TRACK,
        type: "line",
        source: SRC_TRACK,
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": trackColor, "line-width": 3 },
      });
    }
    // Breadcrumb-Punkte entlang des geflogenen Tracks (wie Stratos).
    if (!map.getLayer(LYR_TRACK_DOTS)) {
      map.addLayer({
        id: LYR_TRACK_DOTS,
        type: "circle",
        source: SRC_TRACK_DOTS,
        paint: {
          "circle-radius": 2.4,
          "circle-color": trackColor,
          "circle-stroke-width": 1,
          "circle-stroke-color": haloColor,
        },
      });
    }
    if (!map.getLayer(LYR_WPTS)) {
      map.addLayer({
        id: LYR_WPTS,
        type: "circle",
        source: SRC_WPTS,
        paint: {
          // nächster Wegpunkt größer hervorgehoben (Stratos-Look).
          "circle-radius": [
            "case",
            ["==", ["get", "isNext"], true], 6,
            ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]], 5,
            3,
          ],
          "circle-color": [
            "case",
            ["==", ["get", "isNext"], true], accent,
            ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]], warning,
            accent,
          ],
          "circle-stroke-width": ["case", ["==", ["get", "isNext"], true], 2, 1],
          "circle-stroke-color": haloColor,
        },
      });
    }
    // v0.15.8: Wegpunkt-NAMEN (Ident) — aber erst ab mittlerer Zoomstufe. Bei
    // rausgezoomt (Kontinent/Land) würden die Namen die Karte zukleistern, darum
    // minzoom + Kollisions-Vermeidung (allow-overlap=false). text-optional:
    // wenn kein Platz, bleibt wenigstens der Punkt.
    if (!map.getLayer(LYR_WPT_LABELS)) {
      map.addLayer({
        id: LYR_WPT_LABELS,
        type: "symbol",
        source: SRC_WPTS,
        minzoom: 6.5,
        layout: {
          "text-field": ["get", "ident"],
          "text-font": ["Noto Sans Regular"],
          "text-size": 11,
          "text-offset": [0, 1.1],
          "text-anchor": "top",
          "text-allow-overlap": false,
          "text-optional": true,
          "text-padding": 4,
        },
        paint: {
          "text-color": textColor,
          "text-halo-color": haloColor,
          "text-halo-width": 1.4,
          "text-halo-blur": 0.4,
        },
      });
    }
    pushSources(map, dataRef.current);
    // README §3 EBENEN (Track / Taxiweg): a basemap switch re-runs this whole
    // function (see the re-add sentinel above) and freshly-added layers
    // default to visible — re-assert both current toggles so switching
    // basemaps doesn't silently turn either layer back on.
    const trackVis = showTrackRef.current ? "visible" : "none";
    for (const id of TRACK_LAYERS) {
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", trackVis);
    }
    const taxiVis = showTaxiRef.current ? "visible" : "none";
    for (const id of TAXI_LAYERS) {
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", taxiVis);
    }
  }

  function pushSources(
    map: maplibregl.Map,
    d: { fixes: RouteFix[]; track: [number, number][]; dep?: [number, number]; arr?: [number, number]; nextIdent?: string },
  ) {
    const routeSrc = map.getSource(SRC_ROUTE) as maplibregl.GeoJSONSource | undefined;
    const wptSrc = map.getSource(SRC_WPTS) as maplibregl.GeoJSONSource | undefined;
    const trackSrc = map.getSource(SRC_TRACK) as maplibregl.GeoJSONSource | undefined;
    const dotsSrc = map.getSource(SRC_TRACK_DOTS) as maplibregl.GeoJSONSource | undefined;
    if (!routeSrc || !wptSrc || !trackSrc || !dotsSrc) return;
    let line: [number, number][] = d.fixes.map((f) => [f.lon, f.lat]);
    if (line.length < 2 && d.dep && d.arr) line = [d.dep, d.arr];
    routeSrc.setData({
      type: "FeatureCollection",
      features: line.length >= 2 ? [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: line } }] : [],
    });
    wptSrc.setData({
      type: "FeatureCollection",
      features: d.fixes.map((f) => ({
        type: "Feature",
        properties: {
          ident: f.ident,
          kind: f.ident === "TOC" || f.ident === "TOD" ? f.ident : f.kind,
          isNext: !!d.nextIdent && f.ident === d.nextIdent,
        },
        geometry: { type: "Point", coordinates: [f.lon, f.lat] },
      })),
    });
    trackSrc.setData({
      type: "FeatureCollection",
      features: d.track.length >= 2 ? [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: d.track } }] : [],
    });
    // Breadcrumbs: jeden 8. Track-Punkt als Dot (sonst zu dicht).
    const dots = d.track.filter((_, i) => i % 8 === 0);
    dotsSrc.setData({
      type: "FeatureCollection",
      features: dots.map((p) => ({ type: "Feature", properties: {}, geometry: { type: "Point", coordinates: p } })),
    });
  }

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

  // ---- Taxi-Karte: laden, was gerade im Bild ist ----
  //
  // v0.21.0-beta.2 — DAS IST DER FIX, den Thomas in Berlin gefunden hat.
  //
  // Der erste Wurf lud die Rollwege nur fuer Start- und Zielflughafen eines
  // LAUFENDEN Flugs. Steht man ohne Bid am Gate — genau der Moment, in dem man
  // auf die Karte schaut — blieb sie leer. Dasselbe nach einem Ausweichflug:
  // ausgerechnet der fremde Platz, an dem man tatsaechlich landet, hatte keine
  // Karte.
  //
  // Jetzt zaehlt, was im BILD ist: schiebt man die Karte woandershin, erscheinen
  // dort die Rollwege. Kein Flug noetig, kein Bid, kein Sim.
  //
  // Ab Zoom 11, sonst waeren beim Herauszoomen auf halb Europa hunderte
  // Flughaefen im Bild — und die Layer sind ohnehin erst ab Zoom 12 sichtbar.
  //
  // Der Index ist winzig (ICAO + zwei Zahlen). Die eigentlichen Daten kommen
  // erst, wenn ein Flughafen wirklich sichtbar wird, und liegen danach im
  // lokalen Zwischenspeicher: beim zweiten Mal antwortet der Server mit 304, und
  // ohne Netz kommt die Karte von der Platte.
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;

    let cancelled = false;
    // ICAO → Features. Einmal geladen, bleibt geladen: der Pilot schiebt die
    // Karte hin und her, das darf nicht jedes Mal neu ziehen.
    const loaded = new Map<string, GeoJSON.Feature[]>();
    let index: Array<{ icao: string; lat: number; lon: number }> = [];

    const redraw = () => {
      const features: GeoJSON.Feature[] = [];
      for (const fs of loaded.values()) features.push(...fs);
      const fc: GeoJSON.FeatureCollection = { type: "FeatureCollection", features };
      groundRef.current = fc;
      setGroundLoaded(Array.from(loaded.keys()).sort());
      const src = map.getSource(SRC_GROUND) as maplibregl.GeoJSONSource | undefined;
      src?.setData(fc);
    };

    const syncViewport = async () => {
      if (cancelled || index.length === 0) return;
      if (map.getZoom() < GROUND_MIN_ZOOM - 1) return;

      const b = map.getBounds();
      const visible = index.filter(
        (a) =>
          a.lat >= b.getSouth() &&
          a.lat <= b.getNorth() &&
          a.lon >= b.getWest() &&
          a.lon <= b.getEast(),
      );

      for (const a of visible) {
        if (loaded.has(a.icao)) continue;
        // Platzhalter, damit ein zweites moveend nicht dasselbe nochmal zieht.
        loaded.set(a.icao, []);
        try {
          const r = await invoke<{ icao: string; geojson: string } | null>(
            "airport_ground_get",
            { icao: a.icao },
          );
          if (cancelled) return;
          const fc = r?.geojson
            ? (JSON.parse(r.geojson) as GeoJSON.FeatureCollection)
            : null;
          if (fc && Array.isArray(fc.features)) {
            loaded.set(a.icao, fc.features);
            redraw();
          } else {
            loaded.delete(a.icao);
          }
        } catch {
          // Kein Netz, kein Token, Flughafen nicht importiert: kein Drama.
          loaded.delete(a.icao);
        }
      }
    };

    void (async () => {
      try {
        index = await invoke<Array<{ icao: string; lat: number; lon: number }>>(
          "airport_ground_index",
        );
      } catch {
        index = [];
      }
      if (cancelled) return;
      void syncViewport();
    })();

    const onMoveEnd = () => void syncViewport();
    map.on("moveend", onMoveEnd);
    map.on("zoomend", onMoveEnd);

    return () => {
      cancelled = true;
      map.off("moveend", onMoveEnd);
      map.off("zoomend", onMoveEnd);
    };
  }, [mapReady]);

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
    pushSources(map, { fixes: effFixes, track: effTrackLive, dep: effDep, arr: effArr, nextIdent: nav.nextIdent });

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
    const pts: [number, number][] = [
      ...effFixes.map((f) => [f.lon, f.lat] as [number, number]),
      ...(effDep ? [effDep] : []),
      ...(effArr ? [effArr] : []),
    ];
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

  // ---- VA-Verkehr: pollt /api/acars (wenn eingeblendet) ----
  useEffect(() => {
    if (!showVa) {
      setVaFlights([]);
      return;
    }
    // Verdeckt nicht weiterpollen — die Blasen der Kollegen sieht ohnehin
    // niemand. Der Bestand bleibt stehen und wird beim Wiederauftauchen
    // sofort einmal aufgefrischt.
    if (!sichtbar) return;
    let cancelled = false;
    const poll = async () => {
      try {
        // /api/acars liefert { data: [...] } — defensiv auch flights / Array.
        const data = await invoke<{ data?: VaFlight[]; flights?: VaFlight[] } | VaFlight[]>("va_live_flights");
        const flights = Array.isArray(data) ? data : data?.data ?? data?.flights ?? [];
        if (!cancelled) setVaFlights(flights);
      } catch {
        if (!cancelled) setVaFlights([]);
      }
    };
    void poll();
    // Field feedback (2026-08-03): "nothing happened in 5 seconds", explicit
    // ask to speed this up. handoff_livemap_4a §8 actually states 15-30s as
    // the target for this exact poll — slower than even the pre-existing
    // 12s — so this is a deliberate override of the written spec, confirmed
    // with the user (asked 5s/8s/leave-at-12s; they picked 8s: faster than
    // today without going as aggressive as the un-throttled 5s option).
    const id = setInterval(poll, 8000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [showVa, sichtbar]);

  async function resolveAirportCoord(icao: string): Promise<[number, number] | null> {
    const cache = airportCoordCacheRef.current;
    if (cache.has(icao)) return cache.get(icao)!;
    try {
      const a = await invoke<{ lat?: number | null; lon?: number | null }>("airport_get", { icao });
      const coord: [number, number] | null = a.lat != null && a.lon != null ? [a.lon, a.lat] : null;
      // Nur ERFOLGE dauerhaft merken. Ein "der Flughafen hat keine
      // Koordinaten" ist eine echte, stabile Antwort — die darf gecacht
      // werden.
      cache.set(icao, coord);
      return coord;
    } catch {
      // QS 2026-08-04: hier wurde vorher `cache.set(icao, null)` gesetzt —
      // ein einmaliger Aussetzer (`airport_get` ist ein Result-Command und
      // wirft auch bei fehlendem Client/Netz-Hänger/Timeout, nicht nur bei
      // "Flughafen unbekannt") hat sich damit für die ganze Sitzung
      // eingebrannt: jede Kollegen-Blase zu diesem Ziel zeigte danach
      // dauerhaft "—" als ETA, obwohl ein Versuch eine Sekunde später
      // geklappt hätte. Fehlschläge werden jetzt NICHT gemerkt, der
      // nächste Aufruf versucht es neu — so wie es die Schwester-Abfrage
      // für den eigenen Flug (`lookup()`) oben auch schon macht.
      return null;
    }
  }
  /** README §5's ETA cell for a colleague aircraft — same great-circle +
   *  groundspeed math as the own-flight ETA (`nav`), just resolving the
   *  destination coordinate on demand instead of from the loaded route. */
  async function vaEtaFor(f: VaFlight): Promise<string> {
    const pos = f.position;
    if (!pos || pos.lat == null || pos.lon == null || pos.gs == null || pos.gs <= 30 || !f.arr_airport_id) return "—";
    const arrCoord = await resolveAirportCoord(f.arr_airport_id);
    if (!arrCoord) return "—";
    const dtg = distNm([pos.lon, pos.lat], arrCoord);
    const mins = Math.round((dtg / pos.gs) * 60);
    const arrival = new Date(Date.now() + mins * 60_000);
    return `${arrival.toISOString().slice(11, 16)}z`;
  }

  // ---- VA-Marker rendern (liegen mit auf der einen Karte) ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    vaMarkersRef.current.forEach((m) => m.remove());
    vaMarkersRef.current = [];
    if (vaVisible.length === 0) {
      vaPopupRef.current?.remove();
      vaPopupRef.current = null;
      vaFittedRef.current = false;
    }
    const pts: [number, number][] = [];
    const dr: { marker: maplibregl.Marker; lat: number; lon: number; hdg: number; gs: number }[] = [];
    const popupTargets = new Map<string, { lngLat: [number, number]; f: VaFlight }>();
    for (const f of vaVisible) {
      const lat = f.position?.lat;
      const lon = f.position?.lon;
      if (typeof lat !== "number" || typeof lon !== "number") continue;
      const lngLat: [number, number] = [lon, lat];
      popupTargets.set(String(f.id ?? f.ident ?? f.flight_number ?? ""), { lngLat, f });
      const el = document.createElement("div");
      el.className = "aa-ac-marker aa-ac-marker--va";
      el.innerHTML = aircraftSvg(f.aircraft?.icao);
      el.style.setProperty("--ac-color", phaseColor(f.status_text ?? (f.phase != null ? String(f.phase) : null)));
      el.title = `${f.ident ?? f.flight_number ?? "?"} · ${f.aircraft?.icao ?? ""} · ${f.dpt_airport_id ?? ""}→${f.arr_airport_id ?? ""}`;
      // Klick → Popup mit Flugdaten (ersetzt ein evtl. offenes Popup).
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        vaPopupRef.current?.remove();
        const thisId = String(f.id ?? f.ident ?? f.flight_number ?? "");
        vaPopupIdRef.current = thisId;
        const popup = new maplibregl.Popup({ offset: 16, closeButton: true, className: "aa-vapop", maxWidth: "260px" })
          .setLngLat(lngLat)
          .setHTML(vaPopupHtml(f))
          .addTo(map);
        // manuelles Schließen (X) → Refs aufräumen, sonst zeigt der Rebuild ins Leere.
        popup.on("close", () => {
          if (vaPopupRef.current === popup) {
            vaPopupRef.current = null;
            vaPopupIdRef.current = null;
          }
        });
        vaPopupRef.current = popup;
        // ETA needs an airport-coordinate lookup the popup can't wait on —
        // show "—" first, fill it in once resolved, but only if this exact
        // popup is still the one open (pilot may have clicked elsewhere or
        // closed it by the time the lookup returns).
        void vaEtaFor(f).then((eta) => {
          if (vaPopupRef.current === popup && vaPopupIdRef.current === thisId) popup.setHTML(vaPopupHtml(f, eta));
        });
      });
      const marker = new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat(lngLat).setRotation(f.position?.heading ?? 0).addTo(map);
      vaMarkersRef.current.push(marker);
      dr.push({ marker, lat, lon, hdg: f.position?.heading ?? 0, gs: f.position?.gs ?? 0 });
      pts.push(lngLat);
    }
    vaDrRef.current = dr;
    vaDrT0Ref.current = Date.now();
    // v0.15.6: offenes VA-Popup beim Rebuild an den passenden Flug nachführen
    // (frische Position + Daten) statt es an einem gerade entfernten Marker
    // verwaisen zu lassen; ist der Flug weg, Popup schließen.
    if (vaPopupRef.current && vaPopupIdRef.current) {
      const t = popupTargets.get(vaPopupIdRef.current);
      if (t) {
        const popup = vaPopupRef.current;
        const openId = vaPopupIdRef.current;
        popup.setLngLat(t.lngLat).setHTML(vaPopupHtml(t.f));
        void vaEtaFor(t.f).then((eta) => {
          if (vaPopupRef.current === popup && vaPopupIdRef.current === openId) popup.setHTML(vaPopupHtml(t.f, eta));
        });
      } else {
        vaPopupRef.current.remove();
        vaPopupRef.current = null;
        vaPopupIdRef.current = null;
      }
    }
    // Nur auf die VA-Flieger einpassen, wenn KEIN eigener Flug die Kamera führt
    // (sonst würde es dich vom eigenen Flug wegziehen). Und nur einmal.
    if (pts.length >= 1 && !vaFittedRef.current && !activeFlight) {
      vaFittedRef.current = true;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 60, duration: 600, maxZoom: 6 });
    }
  }, [vaVisible, mapReady, activeFlight]);

  // Dead-Reckoning: VA-Marker zwischen den 12-s-Polls flüssig entlang
  // Heading+Groundspeed weiterbewegen (snappen beim nächsten Poll auf die
  // echte Position). Gibt den Live-Eindruck ohne separaten Live-Endpoint.
  useEffect(() => {
    if (!mapReady) return;
    // Koppelnavigation der VA-Blasen: verdeckt sinnlos, der Bestand wird beim
    // Auftauchen ohnehin frisch geholt.
    if (!sichtbar) return;
    const id = setInterval(() => {
      const dtH = (Date.now() - vaDrT0Ref.current) / 3600000; // Stunden seit Poll
      if (dtH <= 0) return;
      for (const e of vaDrRef.current) {
        if (e.gs < 30) continue; // am Boden/langsam → nicht extrapolieren
        const distNm = e.gs * dtH;
        const th = (e.hdg * Math.PI) / 180;
        const dLat = (distNm * Math.cos(th)) / 60;
        const dLon = (distNm * Math.sin(th)) / (60 * Math.max(0.2, Math.cos((e.lat * Math.PI) / 180)));
        e.marker.setLngLat([e.lon + dLon, e.lat + dLat]);
      }
    }, 1000);
    return () => clearInterval(id);
  }, [mapReady, sichtbar]);

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
  const showTrackRef = useRef(showTrack);
  showTrackRef.current = showTrack;
  const showTaxiRef = useRef(showTaxi);
  showTaxiRef.current = showTaxi;
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    const vis = showTrack ? "visible" : "none";
    for (const id of TRACK_LAYERS) {
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", vis);
    }
  }, [showTrack, mapReady]);
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
          <span className="aa-livemap__header-vacount">{t("livemap.header_va_count", { count: vaVisible.length })}</span>
          <span className="aa-livemap__header-clock">
            {clock}
            <span className="aa-livemap__header-z">z</span>
          </span>
        </div>
      </header>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {/* README §3 — Kartenschalter-Block, one overlay container, four
              named groups. Every switch shows its own state (bold/accent =
              active) — the whole point is that today's unlabeled icon
              buttons didn't. */}
          <div className="aa-livemap-controls aa-livemap-overlay">
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
                {/* Variante B: Symbol immer, Text nur wenn AN.
                    Diese drei sind unabhaengige Ein/Aus-Schalter, anders
                    als die Paare darueber — bei reinen Symbolen haenge der
                    Zustand allein an der Faerbung. Der Preis ist, dass die
                    Leiste beim Umschalten um die Wortbreite zuckt; das ist
                    bewusst so entschieden. */}
                <button
                  type="button"
                  className={`aa-livemap-toggle ${showTrack ? "aa-livemap-toggle--active" : ""}`}
                  aria-pressed={showTrack}
                  aria-label={t("livemap.layer_track")}
                  title={t("livemap.layer_track")}
                  onClick={() => setShowTrack((v) => !v)}
                >
                  <Sym pfad={PFAD.track} />
                  {showTrack && <span className="aa-livemap-toggle__wort">{t("livemap.layer_track")}</span>}
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
                  {showTaxi && <span className="aa-livemap-toggle__wort">{t("livemap.layer_taxi")}</span>}
                </button>
                <button
                  type="button"
                  className={`aa-livemap-toggle ${showVa ? "aa-livemap-toggle--active" : ""}`}
                  aria-pressed={showVa}
                  onClick={() => setShowVa((v) => !v)}
                  aria-label={t("livemap.layer_va")}
                  title={t("livemap.layer_va")}
                >
                  <Sym pfad={PFAD.va} />
                  {showVa && <span className="aa-livemap-toggle__wort">{t("livemap.layer_va")}</span>}
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
              VA traffic — restoring that exact gate here. */}
          {!showOwnContent && vaVisible.length === 0 && (
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
    </section>
  );
}



/** Gelbes Chevron fuer fremde VATSIM-Piloten — identisch zur Webapp-Karte. */
function makeVatsimPlaneImage(): ImageData | null {
  const size = 48; // 24px @ pixelRatio 2
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.clearRect(0, 0, size, size);
  const cx = size / 2;
  ctx.fillStyle = "#f2c14e";
  ctx.strokeStyle = "#1a1205";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(cx, 6);
  ctx.lineTo(size - 8, size - 8);
  ctx.lineTo(cx, size - 16);
  ctx.lineTo(8, size - 8);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  return ctx.getImageData(0, 0, size, size);
}
