// Was sich an den Overlays mit der Grundkarte aendert.
import type maplibregl from "maplibre-gl";
import { LYR_ROUTE, LYR_TRACK, LYR_TRACK_DOTS, LYR_WPTS, LYR_WPT_LABELS } from "./flugEbenen";
import { LYR_GROUND_APRON, LYR_GROUND_RWY, LYR_GROUND_TERMINAL } from "./taxiKarte";
import { cssVar } from "./werkzeug";

/** Alles, was sich mit der Grundkarte aendert. Auf Satellit (dunkel-buntes Bild) gilt immer die
 *  Behandlung der dunklen Karte: heller Track und Text, dunkle Halos, sonst verschwaende alles im Bild. */
export function overlayFarben(map: maplibregl.Map, sat: boolean) {
  const accent = cssVar("--accent", "#0a84ff");
  const trackColor = sat ? "#f2f5fa" : cssVar("--map-track", "#e8e8e8");
  const haloColor = sat ? "#0b0f16" : cssVar("--surface", "#ffffff");
  const textColor = sat ? "#f4f7fc" : cssVar("--text", "#e8edf2");
  const warning = cssVar("--warning", "#ff9f0a");
  const istNaechster = ["==", ["get", "isNext"], true];
  const istToc = ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]];
  const werte: [string, string, unknown][] = [
    // An denselben Kartenstil gekoppelt wie die eigene Route: Fest auf 0,85 war die
    // Kollegen-Linie auf der Vektorkarte deckender als die eigene (Abnahme 20.09.2026).
    ["fremde-routen-line", "line-opacity", sat ? 0.85 : 0.62],
    ["fremde-routen-punkte", "circle-opacity", sat ? 0.85 : 0.62],
    ["fremde-routen-namen", "text-opacity", sat ? 0.9 : 0.75],
    [LYR_GROUND_APRON, "line-color", sat ? "#8aa0b4" : "#3a4a5c"],
    [LYR_GROUND_APRON, "line-opacity", sat ? 0.5 : 0.7],
    [LYR_GROUND_RWY, "line-color", sat ? "#e8edf2" : "#c9ccd1"],
    [LYR_GROUND_RWY, "line-opacity", sat ? 0.75 : 0.9],
    [LYR_GROUND_TERMINAL, "line-color", sat ? "#c4b5fd" : "#8b7fd4"],
    [LYR_ROUTE, "line-color", accent],
    [LYR_ROUTE, "line-opacity", sat ? 0.95 : 0.7],
    [LYR_TRACK, "line-color", trackColor],
    [LYR_TRACK_DOTS, "circle-color", trackColor],
    [LYR_TRACK_DOTS, "circle-stroke-color", haloColor],
    // nächster Wegpunkt größer hervorgehoben (Stratos-Look).
    [LYR_WPTS, "circle-color", ["case", istNaechster, accent, istToc, warning, accent]],
    [LYR_WPTS, "circle-stroke-color", haloColor],
    [LYR_WPT_LABELS, "text-color", textColor],
    [LYR_WPT_LABELS, "text-halo-color", haloColor],
  ];
  for (const [ebene, eigenschaft, wert] of werte) {
    if (map.getLayer(ebene)) map.setPaintProperty(ebene, eigenschaft, wert);
  }
}
