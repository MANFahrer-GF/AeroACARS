// Routen der Kollegen: per Klick auf den Flieger ein- und ausblendbar.
import type maplibregl from "maplibre-gl";
import { ohneDatumsgrenzenSprung } from "../../lib/datumsgrenze";
import { ebeneAnlegen } from "./werkzeug";

export /** Ein Wegpunkt einer fremden Route, wie `fremde_flugroute` ihn liefert. */
interface FremderPunkt {
  lon: number;
  lat: number;
  /** Kann leer sein — dann bleibt der Punkt unbeschriftet. */
  name?: string;
}

/** Die Ebenen der Kollegen-Routen — Linie, Wegpunkte und Beschriftung. */
export const FREMDE_ROUTEN_EBENEN = [
  "fremde-routen-line",
  "fremde-routen-punkte",
  "fremde-routen-namen",
];
export function fremdeRoutenAnlegen(map: maplibregl.Map) {
  // Routen der Kollegen — unter den Flugzeugen, damit sie nichts verdecken.
  map.addSource("fremde-routen", {
    type: "geojson",
    data: { type: "FeatureCollection", features: [] },
  });
  ebeneAnlegen(map, {
    id: "fremde-routen-line",
    type: "line",
    source: "fremde-routen",
    filter: ["==", ["geometry-type"], "LineString"],
    layout: { "line-cap": "round", "line-join": "round" },
    paint: {
      // 1,4 px bei 70 % in Graublau waren auf dunkler Karte praktisch
      // unsichtbar (Thomas, 20.09.2026: „die Linie ist nicht zu sehen
      // (kaum)"). Am Bild gegen die eigene Route abgewogen: Sie soll
      // klar da sein, aber die eigene (2,5 px / 95 % / Akzentfarbe)
      // nicht uebertoenen.
      "line-color": "#8fb0d4",
      "line-width": 2.0,
      // An denselben Kartenstil gekoppelt wie die eigene Route
      // (0,95 / 0,7): Fest auf 0,85 war die Kollegen-Linie auf der
      // Vektorkarte DECKENDER als die eigene — das Gegenteil der
      // Absicht und der Zusage in den Release-Notes (Abnahme
      // 20.09.2026). Sie bleibt jetzt in beiden Stilen darunter.
      "line-dasharray": [4, 3],
    },
  });
  ebeneAnlegen(map, {
    id: "fremde-routen-punkte",
    type: "circle",
    source: "fremde-routen",
    filter: ["==", ["geometry-type"], "Point"],
    paint: {
      "circle-radius": 2.8,
      "circle-color": "#8fb0d4",
    },
  });
  ebeneAnlegen(map, {
    id: "fremde-routen-namen",
    type: "symbol",
    source: "fremde-routen",
    filter: ["==", ["geometry-type"], "Point"],
    // Erst ab mittlerem Zoom: Eine Langstrecke hat 39 Fixe, und
    // mehrere Kollegen gleichzeitig wuerden die Karte sonst
    // zupflastern (Thomas fragte am 20.09.2026 nach den Namen).
    minzoom: 5,
    layout: {
      "text-field": ["get", "name"],
      "text-size": 10,
      "text-offset": [0, 0.9],
      "text-anchor": "top",
      // Lieber weglassen als uebereinanderlegen.
      "text-allow-overlap": false,
      "text-optional": true,
    },
    paint: {
      "text-color": "#8fb0d4",
      "text-halo-color": "rgba(10,16,24,0.85)",
      "text-halo-width": 1.2,
    },
  });

}

/** Die eingeblendeten Routen in die Karte schreiben. */
export function fremdeRoutenZeichnen(map: maplibregl.Map, routen: Map<string, FremderPunkt[]>) {
  const quelle = map.getSource("fremde-routen") as maplibregl.GeoJSONSource | undefined;
  if (!quelle) return;
  quelle.setData({
    type: "FeatureCollection",
    // Linie UND Wegpunkte. Ohne die Punkte war nur ein Strich zu sehen,
    // und man konnte nicht erkennen, wo der Kollege seine Fixe hat
    // (Thomas, 20.09.2026: „ohne Punkte die zu sehen sind").
    features: [...routen.entries()]
      .filter(([, punkte]) => punkte.length >= 2)
      .flatMap(([id, punkte]) => [
        {
          type: "Feature" as const,
          properties: { pirep_id: id },
          geometry: {
            type: "LineString" as const,
            coordinates: ohneDatumsgrenzenSprung(
              punkte.map((p) => [p.lon, p.lat] as [number, number]),
            ),
          },
        },
        ...punkte.map((p) => ({
          type: "Feature" as const,
          // Der Name wandert mit — die Beschriftung liest ihn von hier.
          properties: { pirep_id: id, name: p.name ?? "" },
          geometry: { type: "Point" as const, coordinates: [p.lon, p.lat] as [number, number] },
        })),
      ]),
  });
}

