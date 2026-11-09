// Der eigene Flug auf der Karte: geplante Route, Wegpunkte mit TOC/TOD, geflogene Spur.
import type maplibregl from "maplibre-gl";
import { ohneDatumsgrenzenSprung } from "../../lib/datumsgrenze";
import { ebeneAnlegen } from "./werkzeug";

export interface RouteFix {
  ident: string;
  lat: number;
  lon: number;
  kind: string;
}

export const SRC_ROUTE = "aa-planned-route";
export const SRC_WPTS = "aa-planned-wpts";
export const SRC_TRACK = "aa-flown-track";
export const SRC_TRACK_DOTS = "aa-flown-track-dots";
export const LYR_ROUTE = "aa-planned-route-line";
export const LYR_WPTS = "aa-planned-wpts-circles";
export const LYR_WPT_LABELS = "aa-planned-wpts-labels";
export const LYR_TRACK = "aa-flown-track-line";
export const LYR_TRACK_DOTS = "aa-flown-track-dots-circles";
export const LYR_ROUTE_CASING = "aa-planned-route-casing"; // dunkle Unterlage nur auf Satellit

export const TRACK_LAYERS = [LYR_ROUTE, LYR_ROUTE_CASING, LYR_WPTS, LYR_WPT_LABELS, LYR_TRACK, LYR_TRACK_DOTS];

export function flugEbenenAnlegen(map: maplibregl.Map) {
  const empty: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };
  map.addSource(SRC_ROUTE, { type: "geojson", data: empty });
  map.addSource(SRC_WPTS, { type: "geojson", data: empty });
  map.addSource(SRC_TRACK, { type: "geojson", data: empty });
  map.addSource(SRC_TRACK_DOTS, { type: "geojson", data: empty });
  // Nur auf Satellit: dunkle Unterlage UNTER der Route (zuerst hinzufügen =
  // liegt unter der eigentlichen Linie), damit die blaue gestrichelte Route
  // auf hellem/buntem Bild nicht verschwindet. Auf Dark/Light nicht nötig.
  ebeneAnlegen(map, {
    id: LYR_ROUTE_CASING,
    type: "line",
    source: SRC_ROUTE,
    layout: { "line-cap": "butt", "line-join": "round" },
    paint: { "line-color": "#0b0f16", "line-width": 5, "line-opacity": 0.55 },
  });
  ebeneAnlegen(map, {
    id: LYR_ROUTE,
    type: "line",
    source: SRC_ROUTE,
    // butt-Caps statt round, damit die Striche sauber abgesetzt sind.
    layout: { "line-cap": "butt", "line-join": "round" },
    // Stratos-Look: GEPLANTE Route gestrichelt (die tatsächlich GEFLOGENE
    // Spur ist durchgezogen — so unterscheidet man Plan vs. Ist auf einen Blick).
    paint: { "line-width": 2.5, "line-dasharray": [2, 2] },
  });
  ebeneAnlegen(map, {
    id: LYR_TRACK,
    type: "line",
    source: SRC_TRACK,
    layout: { "line-cap": "round", "line-join": "round" },
    paint: { "line-width": 3 },
  });
  // Breadcrumb-Punkte entlang des geflogenen Tracks (wie Stratos).
  ebeneAnlegen(map, {
    id: LYR_TRACK_DOTS,
    type: "circle",
    source: SRC_TRACK_DOTS,
    paint: {
      "circle-radius": 2.4,
      "circle-stroke-width": 1,
    },
  });
  ebeneAnlegen(map, {
    id: LYR_WPTS,
    type: "circle",
    source: SRC_WPTS,
    paint: {
      "circle-radius": [
        "case",
        ["==", ["get", "isNext"], true], 6,
        ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]], 5,
        3,
      ],
      "circle-stroke-width": ["case", ["==", ["get", "isNext"], true], 2, 1],
    },
  });
  // v0.15.8: Wegpunkt-NAMEN (Ident) — aber erst ab mittlerer Zoomstufe. Bei
  // rausgezoomt (Kontinent/Land) würden die Namen die Karte zukleistern, darum
  // minzoom + Kollisions-Vermeidung (allow-overlap=false). text-optional:
  // wenn kein Platz, bleibt wenigstens der Punkt.
  ebeneAnlegen(map, {
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
      "text-halo-width": 1.4,
      "text-halo-blur": 0.4,
    },
  });
}

export interface FlugDaten {
  fixes: RouteFix[];
  track: [number, number][];
  dep?: [number, number];
  arr?: [number, number];
  nextIdent?: string;
}

export function flugDatenSetzen(map: maplibregl.Map, d: FlugDaten) {
  const routeSrc = map.getSource(SRC_ROUTE) as maplibregl.GeoJSONSource | undefined;
  const wptSrc = map.getSource(SRC_WPTS) as maplibregl.GeoJSONSource | undefined;
  const trackSrc = map.getSource(SRC_TRACK) as maplibregl.GeoJSONSource | undefined;
  const dotsSrc = map.getSource(SRC_TRACK_DOTS) as maplibregl.GeoJSONSource | undefined;
  if (!routeSrc || !wptSrc || !trackSrc || !dotsSrc) return;
  let line: [number, number][] = d.fixes.map((f) => [f.lon, f.lat]);
  if (line.length < 2 && d.dep && d.arr) line = [d.dep, d.arr];
  // Ueber die Datumsgrenze weiterzaehlen, sonst laeuft die Linie quer
  // ueber die ganze Karte (siehe `lib/datumsgrenze.ts`).
  line = ohneDatumsgrenzenSprung(line);
  const track = ohneDatumsgrenzenSprung(d.track);
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
    features: track.length >= 2 ? [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: track } }] : [],
  });
  // Breadcrumbs: jeden 8. Track-Punkt als Dot (sonst zu dicht).
  const dots = d.track.filter((_, i) => i % 8 === 0);
  dotsSrc.setData({
    type: "FeatureCollection",
    features: dots.map((p) => ({ type: "Feature", properties: {}, geometry: { type: "Point", coordinates: p } })),
  });
}
