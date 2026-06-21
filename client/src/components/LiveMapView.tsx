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
import maplibregl, { type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import { ActivityLogPanel } from "./ActivityLogPanel";
import { setTrack } from "../lib/trackStore";
import { aircraftSvg } from "../lib/aircraftIcon";
import { phaseColor, phaseLabel as formatPhase } from "../lib/phaseColors";

const BASEMAP_DARK = "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json";
const BASEMAP_LIGHT = "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json";
// Satellit (Esri World Imagery + Namens-Overlay, kein API-Key). Manuell wählbar
// über den Karten-Toggle. glyphs auf die CARTO-Fonts (haben "Open Sans Regular"
// wie dark/light — demotiles hat den Font NICHT, sonst fehlten Waypoint-Namen).
const BASEMAP_SAT: StyleSpecification = {
  version: 8,
  glyphs: "https://tiles.basemaps.cartocdn.com/fonts/{fontstack}/{range}.pbf",
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
};

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
/** Theme-fähiges Popup-HTML mit den Flugdaten aus /api/acars. */
function vaPopupHtml(f: VaFlight): string {
  const cs = f.ident || [f.airline?.icao, f.flight_number].filter(Boolean).join("") || f.flight_number || "—";
  const ac = [f.aircraft?.icao, f.aircraft?.registration].filter(Boolean).join(" · ");
  const pilot = f.user?.name ?? "";
  const phaseTxt = f.status_text ?? "";
  const pcol = phaseColor(f.status_text ?? (f.phase != null ? String(f.phase) : null));
  const pos = f.position ?? {};
  const gs = pos.gs != null ? `${Math.round(pos.gs)}` : "—";
  const ias = pos.ias != null ? `${Math.round(pos.ias)}` : "—";
  const hdg = pos.heading != null ? `${Math.round(pos.heading)}°` : "—";
  const vs = pos.vs != null ? `${pos.vs > 0 ? "+" : ""}${Math.round(pos.vs)}` : "—";
  const cell = (k: string, v: string, u = "") =>
    `<div class="aa-vapop__cell"><span class="aa-vapop__k">${k}</span><span class="aa-vapop__v">${escHtml(v)}${u ? `<i>${u}</i>` : ""}</span></div>`;
  return (
    `<div class="aa-vapop__head">` +
    `<span class="aa-vapop__cs">${escHtml(cs)}</span>` +
    (phaseTxt ? `<span class="aa-vapop__badge" style="--p:${pcol}">${escHtml(phaseTxt)}</span>` : "") +
    `</div>` +
    (ac ? `<div class="aa-vapop__sub">${escHtml(ac)}</div>` : "") +
    (pilot ? `<div class="aa-vapop__pilot">${escHtml(pilot)}</div>` : "") +
    `<div class="aa-vapop__route">${escHtml(f.dpt_airport_id ?? "—")}<span class="aa-vapop__arrow">→</span>${escHtml(f.arr_airport_id ?? "—")}</div>` +
    `<div class="aa-vapop__grid">` +
    cell("ALT", flLabel(pos.altitude_msl ?? pos.altitude)) +
    cell("GS", gs, " kt") +
    cell("IAS", ias, " kt") +
    cell("HDG", hdg) +
    cell("V/S", vs, " fpm") +
    `</div>`
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

interface Props {
  activeFlight: ActiveFlightInfo | null;
  simSnapshot: SimSnapshot | null;
}

export function LiveMapView({ activeFlight, simSnapshot }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const acMarkerRef = useRef<maplibregl.Marker | null>(null);
  const pinMarkersRef = useRef<maplibregl.Marker[]>([]);
  const vaMarkersRef = useRef<maplibregl.Marker[]>([]);
  const vaPopupRef = useRef<maplibregl.Popup | null>(null);
  const vaPopupIdRef = useRef<string | null>(null); // welcher VA-Flug das offene Popup zeigt
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
  const basemapRef = useRef<"auto" | "sat">(basemap);
  const [showVa, setShowVa] = useState(true); // VA-Verkehr ein-/ausblenden
  const [theme, setTheme] = useState<"dark" | "light">(readTheme());
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
    const dtg = activeFlight?.distance_nm;
    const gs = simSnapshot?.groundspeed_kt;
    let eta = "—";
    if (dtg != null && gs != null && gs > 30) {
      const mins = Math.round((dtg / gs) * 60);
      eta = mins >= 60 ? `${Math.floor(mins / 60)}h ${String(mins % 60).padStart(2, "0")}m` : `${mins}m`;
    }
    return { nextIdent: next?.ident, nextLabel, eta };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effFixes, effAircraft, effArr, activeFlight?.distance_nm, simSnapshot?.groundspeed_kt]);

  // dataRef für die styledata-Re-Adds aktuell halten
  dataRef.current = { fixes: effFixes, track: effTrackLive, dep: effDep, arr: effArr, nextIdent: nav.nextIdent };

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: basemapRef.current === "sat" ? BASEMAP_SAT : readTheme() === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT,
      center: [6, 48],
      zoom: 4,
      attributionControl: { compact: true },
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
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
      addOverlays(map);
      setMapReady(true);
    });
    map.on("styledata", () => {
      if (map.isStyleLoaded()) addOverlays(map);
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
    // setStyle wischt alle Layer — der styledata-Handler baut die Overlays
    // (Track/Route/Waypoints) danach idempotent wieder auf, für jede Basemap.
    mapRef.current?.setStyle(
      basemap === "sat" ? BASEMAP_SAT : theme === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT,
    );
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
          "text-font": ["Open Sans Regular"],
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
  }, [pirepId]);

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
  }, [pirepId]);

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

  // ---- Redraw: eigener Flug (Quellen + Flugzeug-Marker + Pins) ----
  // Läuft immer; VA-Flieger liegen als zusätzliche Marker mit drauf (eigene Effekte).
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
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
      // Follow: bei gesetztem Haken IMMER auf den Flieger zentrieren — egal wie
      // weit man rausgezoomt hat. Gate NUR am `follow`-State (ein einziger Zustand,
      // nichts kann mehr desyncen). was_just_resumed unterdrückt das Folgen der
      // evtl. falschen Reload-Position direkt nach einem Resume.
      if (follow && !activeFlight?.was_just_resumed) {
        const c = map.getCenter();
        const far = Math.abs(c.lng - lngLat[0]) > 2 || Math.abs(c.lat - lngLat[1]) > 2;
        if (far) {
          // großer Versatz (Erst-Lock / weit draußen) → hart auf den Flieger,
          // phasen-passender Zoom (Boden nah, Reiseflug weit).
          map.jumpTo({
            center: lngLat,
            zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
          });
          followEngageRef.current = false;
        } else if (followEngageRef.current) {
          // bewusst (wieder) eingeschaltet und Flieger schon nah → sanft zentrieren
          // und EINMAL den phasen-Zoom setzen.
          map.easeTo({
            center: lngLat,
            zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
            duration: 350,
          });
          followEngageRef.current = false;
        } else {
          // laufendes Folgen → NUR schwenken. Dein manueller Zoom bleibt erhalten
          // (kein Zurückziehen mehr auf den Phasen-Zoom — genau das war der Bug).
          map.easeTo({ center: lngLat, duration: 400 });
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
    const id = setInterval(poll, 12000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [showVa]);

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
        vaPopupIdRef.current = String(f.id ?? f.ident ?? f.flight_number ?? "");
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
        vaPopupRef.current.setLngLat(t.lngLat).setHTML(vaPopupHtml(t.f));
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
  }, [mapReady]);

  // ---- Stats ----
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
      spd: fmt(s?.indicated_airspeed_kt, " kts"),
      hdg: s ? `${Math.round(s.heading_deg_magnetic)}°` : "—",
      gs: fmt(s?.groundspeed_kt, " kts"),
      dtg: activeFlight?.distance_nm != null ? `${Math.round(activeFlight.distance_nm)} nm` : "—",
    };
  }, [simSnapshot, activeFlight]);

  const showOwnContent = !!activeFlight;

  return (
    <section className="aa-livemap">
      <div className="aa-livemap__topbar">
        <div className="aa-livemap__title">
          <span className="aa-stat__label">FLUG</span>
          <span className="aa-livemap__title-value">
            {activeFlight
              ? `${activeFlight.airline_icao}${activeFlight.flight_number} · ${activeFlight.dpt_airport}→${activeFlight.arr_airport}`
              : "Live-Karte"}
            {activeFlight?.was_just_resumed && (
              <span className="aa-livemap__resume-hint" title="warte auf Sim-Position">
                {" "}
                ⏳
              </span>
            )}
          </span>
        </div>

        {showOwnContent && (
          <div className="aa-livemap__stats">
            <Stat label="ALT" value={stats.alt} />
            <Stat label="IAS" value={stats.spd} />
            <Stat label="HDG" value={stats.hdg} />
            <Stat label="GS" value={stats.gs} />
            <Stat label="DTG" value={stats.dtg} />
            <Stat label="NEXT" value={nav.nextLabel} />
            <Stat label="ETA" value={nav.eta} />
            <Stat label="PHASE" value={phaseLabel} />
            <Stat label="POS" value={effAircraft ? `${effAircraft.lat.toFixed(2)}/${effAircraft.lon.toFixed(2)}` : "—"} />
          </div>
        )}

        <div className="aa-livemap__right">
          {activeFlight && effAircraft && (
            <button
              type="button"
              className="aa-livemap__iconbtn"
              data-tip="Karte aufs Flugzeug zentrieren"
              aria-label="Auf mein Flugzeug zentrieren"
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
              {/* Fadenkreuz = „jetzt zentrieren" */}
              <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                <circle cx="12" cy="12" r="7" />
                <path d="M12 2v3M12 19v3M2 12h3M19 12h3" />
                <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
              </svg>
            </button>
          )}
          {activeFlight && (
            <button
              type="button"
              className="aa-livemap__iconbtn aa-livemap__iconbtn--toggle"
              data-active={follow}
              aria-pressed={follow}
              data-tip={
                follow
                  ? "Folgen: AN — Karte bleibt am Flugzeug (klicken zum Ausschalten)"
                  : "Folgen: AUS — klicken, damit die Karte dem Flugzeug folgt"
              }
              aria-label="Folgen"
              onClick={() => {
                const next = !follow;
                setFollow(next);
                followEngageRef.current = next;
                suppressRouteFitRef.current = next;
              }}
            >
              {/* Flugzeug = „dem eigenen Flieger folgen" */}
              <svg viewBox="0 0 24 24" width="17" height="17" fill="currentColor" aria-hidden="true">
                <path d="M21 15.5v-1.7l-7.5-4.6V4.2a1.5 1.5 0 0 0-3 0v5L3 13.8v1.7l7.5-2.2v4.3l-2 1.4v1.3l3.5-1 3.5 1v-1.3l-2-1.4v-4.3z" />
              </svg>
            </button>
          )}
          <button
            type="button"
            className="aa-livemap__iconbtn aa-livemap__iconbtn--toggle"
            data-active={showVa}
            aria-pressed={showVa}
            data-tip={
              showVa
                ? `VA-Verkehr: AN — ${vaVisible.length} online (klicken zum Ausblenden)`
                : "VA-Verkehr: AUS — klicken zum Anzeigen anderer Piloten"
            }
            aria-label="VA-Verkehr anzeigen"
            onClick={() => setShowVa(!showVa)}
          >
            {/* Radar = „anderer VA-Verkehr live" */}
            <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
              <path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7M6 18a9 9 0 0 1 0-12M18 6a9 9 0 0 1 0 12" />
            </svg>
            {/* Zähler-Slot IMMER da (feste Breite) → Ein-/Ausschalten oder
                1-↔2-stellige Zahl verschiebt die Nachbar-Buttons nicht mehr. */}
            <span className="aa-livemap__vacount">{showVa ? vaVisible.length : ""}</span>
          </button>
          <button
            type="button"
            className="aa-livemap__iconbtn aa-livemap__iconbtn--toggle"
            data-active={basemap === "sat"}
            aria-pressed={basemap === "sat"}
            data-tip={
              basemap === "sat"
                ? "Satellitenkarte: AN — klicken für Standard (dunkel/hell)"
                : "Satellitenkarte: AUS — klicken für echtes Satellitenbild"
            }
            aria-label="Satellitenkarte umschalten"
            onClick={() => setBasemap((b) => (b === "sat" ? "auto" : "sat"))}
          >
            {/* Globus = Satellit/echtes Bild */}
            <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <circle cx="12" cy="12" r="9" />
              <path d="M3 12h18M12 3c2.6 2.7 2.6 15.3 0 18M12 3c-2.6 2.7-2.6 15.3 0 18" />
            </svg>
          </button>
        </div>
      </div>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {!effAircraft && vaVisible.length === 0 && (
            <div className="aa-livemap__empty">
              {showVa
                ? "Kein aktiver Flug und gerade kein VA-Verkehr."
                : "Kein aktiver Flug — starte einen Flug, um ihn live zu verfolgen."}
            </div>
          )}
        </div>
        <aside className="aa-livemap__log">
          <ActivityLogPanel />
        </aside>
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="aa-stat">
      <span className="aa-stat__label">{label}</span>
      <span className="aa-stat__value">{value}</span>
    </div>
  );
}

