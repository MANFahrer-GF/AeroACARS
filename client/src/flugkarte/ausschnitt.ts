// Kartenausschnitt des Bilder-Tabs (P-11.36): derselbe Kartenstil wie die Flugkarte
// (stil-dunkel.json mit FARBEN_DUNKEL), dazu Spur des Flugs, Bildmarken und Fokus.
// Eigene kleine Funktion statt der Wiedergabe-Flugkarte: sie braucht nur Spur, Marken,
// Fokus und den Umschalter Karte/Weltkugel (`setProjection` mit `globe`, MapLibre 5).
import maplibregl, { type StyleSpecification } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { FARBEN_DUNKEL } from "./flugakteTypen";

export interface BildMarke {
  id: string;
  name: string;
  lat: number;
  lon: number;
  aktiv: boolean;
}

export interface AusschnittEingabe {
  /** Spur des Flugs als [lat, lon]; null = nicht verfuegbar. */
  spur: Array<[number, number]> | null;
  marken: BildMarke[];
  /** Mitte und Zoom; fehlt der Fokus, passt die Karte Spur und Marken ein. */
  fokus: { lat: number; lon: number } | null;
  kugel: boolean;
}

export interface AusschnittOptionen {
  stilUrl: string;
  /** Klick auf eine Bildmarke. */
  beiMarke?: (id: string) => void;
  /** Klick auf die Karte (Detailspalte: Karte-Tab oeffnen). */
  beiKlick?: () => void;
  /** Zoom des Fokus; Vorgabe 8. */
  zoom?: number;
  /** Nur Tests: Zugriff auf die Karte. */
  beiKarte?: (karte: maplibregl.Map) => void;
  interaktiv?: boolean;
}

export interface Ausschnitt {
  aktualisieren(e: AusschnittEingabe): void;
  aufraeumen(): void;
}

/** Spur als GeoJSON; Laengen werden fortlaufend gemacht (Datumsgrenze), damit die Linie nicht um die Welt springt. */
export function spurGeoJson(spur: Array<[number, number]> | null): GeoJSON.Feature<GeoJSON.LineString> {
  const koord: number[][] = [];
  let vorher: number | null = null;
  for (const [lat, lon] of spur ?? []) {
    let l = lon;
    if (vorher !== null) l += 360 * Math.round((vorher - lon) / 360);
    koord.push([l, lat]);
    vorher = l;
  }
  return { type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: koord } };
}

export function bildKarteAnzeigen(root: HTMLElement, opt: AusschnittOptionen): Ausschnitt {
  let beendet = false;
  let bereit = false;
  let letzte: AusschnittEingabe | null = null;
  let marker: maplibregl.Marker[] = [];
  const map = new maplibregl.Map({
    container: root,
    attributionControl: false,
    interactive: opt.interaktiv !== false,
    renderWorldCopies: true,
    center: [0, 30],
    zoom: 1.5,
    style: { version: 8, sources: {}, layers: [{ id: "bg", type: "background", paint: { "background-color": "#0b121b" } }] },
  });
  map.addControl(new maplibregl.AttributionControl({ compact: true }), "bottom-right");
  opt.beiKarte?.(map);
  if (opt.beiKlick) map.on("click", () => opt.beiKlick?.());

  function markenZeichnen(e: AusschnittEingabe) {
    marker.forEach((m) => m.remove());
    marker = e.marken.map((m) => {
      const el = document.createElement("button");
      el.type = "button";
      el.className = "bt-marke" + (m.aktiv ? " bt-marke--aktiv" : "");
      el.title = m.name;
      el.setAttribute("aria-label", m.name);
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        opt.beiMarke?.(m.id);
      });
      return new maplibregl.Marker({ element: el }).setLngLat([m.lon, m.lat]).addTo(map);
    });
  }

  function anwenden() {
    const e = letzte;
    if (!e || !bereit || beendet) return;
    (map.getSource("bt-spur") as maplibregl.GeoJSONSource | undefined)?.setData(spurGeoJson(e.spur));
    markenZeichnen(e);
    try {
      map.setProjection({ type: e.kugel ? "globe" : "mercator" });
    } catch { /* alte Fassung ohne Projektionen: Karte bleibt flach */ }
    if (e.fokus) {
      map.jumpTo({ center: [e.fokus.lon, e.fokus.lat], zoom: e.kugel ? 3 : opt.zoom ?? 8 });
    } else {
      const pts = [...(e.spur ?? []).map(([la, lo]) => [lo, la]), ...e.marken.map((m) => [m.lon, m.lat])];
      if (pts.length > 0) {
        const b = pts.reduce((a, p) => a.extend(p as [number, number]), new maplibregl.LngLatBounds(pts[0] as [number, number], pts[0] as [number, number]));
        map.fitBounds(b, { padding: 30, maxZoom: 9, duration: 0 });
      }
    }
  }

  map.on("load", async () => {
    let stil: StyleSpecification | null = null;
    try { stil = await (await fetch(opt.stilUrl)).json(); } catch { /* ohne Grundkarte weiter */ }
    if (beendet) return;
    if (stil) {
      if (stil.glyphs) map.setGlyphs(stil.glyphs);
      for (const [id, q] of Object.entries(stil.sources)) if (!map.getSource(id)) map.addSource(id, q as never);
      for (const l of stil.layers) {
        if (l.type === "background") continue;
        const lay: Record<string, unknown> = { ...l, id: "v:" + l.id };
        const f = FARBEN_DUNKEL[l.id];
        const paint = { ...((l as { paint?: Record<string, unknown> }).paint ?? {}), ...(f ?? {}) };
        for (const k of Object.keys(paint)) if (k.endsWith("-pattern")) delete paint[k];
        lay.paint = paint;
        if (l.id.startsWith("place_")) lay.paint = { ...paint, "text-color": "#c3cfdb", "text-halo-color": "rgba(10,16,24,0.92)", "text-halo-width": 1.4 };
        map.addLayer(lay as never);
      }
    }
    map.addSource("bt-spur", { type: "geojson", data: spurGeoJson(null) });
    map.addLayer({ id: "bt-spur-linie", type: "line", source: "bt-spur", paint: { "line-color": "#5fb0ff", "line-width": 2.5, "line-opacity": 0.9 }, layout: { "line-cap": "round", "line-join": "round" } });
    bereit = true;
    anwenden();
  });

  return {
    aktualisieren(e) {
      letzte = e;
      anwenden();
    },
    aufraeumen() {
      beendet = true;
      marker.forEach((m) => m.remove());
      map.remove();
    },
  };
}
