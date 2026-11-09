// Die Taxi-Karte: Rollwege, Stände, Haltepunkte und Bahnen des Platzes, der gerade im Bild ist.
import { useEffect, useState, type MutableRefObject } from "react";
import type maplibregl from "maplibre-gl";
import { invoke } from "../../lib/ipc";
import { laengeNebenVorgaenger } from "../../lib/datumsgrenze";
import { ebeneAnlegen } from "./werkzeug";

// ─── v0.21: Taxi-Karte (Flughafen-Bodendaten) ────────────────────────────
//
// Rollwege, Haltepunkte und Standplaetze aus OpenStreetMap (ODbL), auf dem VPS
// gespiegelt. Der Layer liegt UNTER Route und Track — die Taxi-Karte ist der
// Untergrund, auf dem man rollt, nicht die Hauptsache.
//
// Sichtbar erst ab Zoom 12: darueber ist man im Anflug oder Reiseflug, da
// stoert das Rollweg-Gewirr nur. Beim Rollen zoomt man ohnehin nah heran.
export const SRC_GROUND = "aa-ground";
export const LYR_GROUND_APRON = "aa-ground-apron";
export const LYR_GROUND_TAXI = "aa-ground-taxi";
export const LYR_GROUND_TAXI_LABELS = "aa-ground-taxi-labels";
export const LYR_GROUND_RWY = "aa-ground-runway";
export const LYR_GROUND_STANDS = "aa-ground-stands";
export const LYR_GROUND_STAND_LANES = "aa-ground-stand-lanes";
export const LYR_GROUND_STAND_LABELS = "aa-ground-stand-labels";
export const LYR_GROUND_TERMINAL = "aa-ground-terminal";
export const LYR_GROUND_HOLD = "aa-ground-holding";
export const LYR_GROUND_RWY_LABELS = "aa-ground-runway-labels";
export const GROUND_MIN_ZOOM = 12;

export const TAXI_LAYERS = [
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

export function taxiEbenenAnlegen(map: maplibregl.Map) {
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
  map.addSource(SRC_GROUND, {
    type: "geojson",
    data: empty,
    // ODbL verlangt Namensnennung. An der Quelle statt irgendwo im UI:
    // so kann sie nicht vergessen werden, wenn jemand die Karte umbaut.
    attribution: "Bodendaten © OpenStreetMap contributors (ODbL)",
  });
  ebeneAnlegen(map, {
    id: LYR_GROUND_APRON,
    type: "line",
    source: SRC_GROUND,
    minzoom: GROUND_MIN_ZOOM,
    filter: ["==", ["get", "k"], "apron"],
    paint: {
      "line-width": 1,
    },
  });
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
  ebeneAnlegen(map, {
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
  // Hauptrollwege — gruen. Farbcodierte Rollwege (Muenchen) in ihrer
  // BODENFARBE: kraeftiges Blau bzw. Orange, damit man der Linie folgen kann
  // wie am echten Platz ("folge der blauen Linie zum Gate"). Nur das Label zu
  // faerben war zu unauffaellig — der Pilot orientiert sich an der Linie.
  ebeneAnlegen(map, {
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
  // Stand-Nummern — die stehen im `r`-Feld der parking_position-Linien und
  // gate-Punkte. Erst ab Zoom 15, sonst Zahlensalat auf dem Vorfeld.
  ebeneAnlegen(map, {
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
  ebeneAnlegen(map, {
    id: LYR_GROUND_RWY,
    type: "line",
    source: SRC_GROUND,
    minzoom: GROUND_MIN_ZOOM,
    filter: ["==", ["get", "k"], "runway"],
    layout: { "line-cap": "butt" },
    paint: {
      "line-width": ["interpolate", ["linear"], ["zoom"], 12, 3, 16, 12, 18, 24],
    },
  });
  // Bahn-Bezeichnung ("05/23") entlang der Bahn. OSM hat sie im `r`-Feld — wir
  // luden sie ohnehin schon mit, zeigten sie nur nicht. Weiss auf schwarzem
  // Halo, damit sie auf dem hellen Bahn-Beton (Satellit) UND auf der dunklen
  // Karte lesbar bleibt.
  ebeneAnlegen(map, {
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
  // Terminals: beim Rollen die wichtigste Orientierung ueberhaupt ("ich muss
  // zu Terminal 2"). Sie liegen in OSM als Flaeche vor, der Import speichert
  // sie als Umriss-Linie — als dezente Kontur reicht das voellig.
  ebeneAnlegen(map, {
    id: LYR_GROUND_TERMINAL,
    type: "line",
    source: SRC_GROUND,
    minzoom: 13,
    filter: ["==", ["get", "k"], "terminal"],
    paint: {
      "line-width": 1.4,
      "line-opacity": 0.85,
    },
  });
  // Der fruehere separate blaue Stand-LINIEN-Layer ist weg. Die
  // parking_position-Linien sind die Zuwegung zum Stand — worauf man rollt.
  // Sie werden jetzt vom blauen Stand-Bereich-Layer gezeichnet (IS_STAND_LANE
  // oben), zusammen mit den taxilanes. So bleibt der Stand-Bereich klar vom
  // gruenen Hauptrollweg getrennt und keine Linie wird doppelt gezeichnet.
  ebeneAnlegen(map, {
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
  // (Stand-Nummern werden oben zusammen mit den Stand-Bereichen gezeichnet.)
  // Haltepunkte vor der Bahn — beim Rollen das Wichtigste ueberhaupt.
  // Deshalb gelb und mit Rand, damit sie auf jedem Untergrund herausstechen.
  ebeneAnlegen(map, {
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
  ebeneAnlegen(map, {
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

/** Laedt die Bodendaten fuer den Sichtbereich; Rueckgabe: die geladenen Plaetze (fuer den Tooltip). */
export function useTaxiKarte(mapRef: MutableRefObject<maplibregl.Map | null>, mapReady: boolean): string[] {
  const [groundLoaded, setGroundLoaded] = useState<string[]>([]);
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
      setGroundLoaded(Array.from(loaded.keys()).sort());
      const src = map.getSource(SRC_GROUND) as maplibregl.GeoJSONSource | undefined;
      src?.setData(fc);
    };

    const syncViewport = async () => {
      if (cancelled || index.length === 0) return;
      if (map.getZoom() < GROUND_MIN_ZOOM - 1) return;

      const b = map.getBounds();
      // An der Datumsgrenze reicht der Ausschnitt ueber ±180 hinaus (etwa
      // 175…185°). Den Platz deshalb in dieselbe Weltkopie legen wie die
      // Kartenmitte — sonst faellt ein sichtbarer Platz bei −179° raus
      // (Codex-QS 25.09.2026).
      const mitte = map.getCenter().lng;
      const visible = index.filter((a) => {
        const lon = laengeNebenVorgaenger(a.lon, mitte);
        return (
          a.lat >= b.getSouth() &&
          a.lat <= b.getNorth() &&
          lon >= b.getWest() &&
          lon <= b.getEast()
        );
      });

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
        const geliefert = await invoke<
          Array<{ icao: string; lat: number; lon: number }> | null
        >("airport_ground_index");
        // Nur uebernehmen, was wirklich eine Liste ist. Ein `null` oder
        // ein Objekt liess `index.length` weiter unten werfen — in der
        // Produktion liefert Rust zwar ein Vec, aber der Fehler landete
        // dann als unbehandelte Rejection im Nichts und machte den
        // ganzen Testlauf zu einem Falsch-Gruen (Abnahme 20.09.2026).
        index = Array.isArray(geliefert) ? geliefert : [];
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

  return groundLoaded;
}
