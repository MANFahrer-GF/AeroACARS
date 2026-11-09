// Das Online-Netz auf der Karte: Sektoren, Lotsen, Plaetze und fremde Piloten von VATSIM oder IVAO.
import { useEffect, useRef, useState, type MutableRefObject } from "react";
import maplibregl from "maplibre-gl";
import { buildPilotFeatures, emptyFC, fetchVatsimData, esc as vatsimEsc } from "../../lib/vatsimKarte";
import { ladeSektoren, type Netz } from "../../lib/vatglassesKarte";
import { umrissOhneNaht } from "../../lib/datumsgrenze";
import type { SimSnapshot } from "../../types";
import { ebeneAnlegen } from "./werkzeug";

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

export function netzEbenenAnlegen(map: maplibregl.Map) {
  const empty: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };
  // ── VATSIM-Overlay — dieselbe ruhige Bauform wie auf der Live-Karte
  //    der Webapp (12.08.2026): Flaeche kaum getoент, duenne Kante,
  //    Kennung + Frequenz IN der Flaeche. Quellen immer anlegen; ob
  //    Daten drinstehen, entscheidet der Poll (showVatsim).
  map.addSource("vatsim-sectors", { type: "geojson", data: empty });
  // Eigene Quelle fuer den Rand: ohne die Schnittkante bei ±180°, sonst
  // zieht sich eine Naht mitten durch die Pazifik-Sektoren.
  map.addSource("vatsim-sectors-umriss", {
    type: "geojson",
    data: empty,
  });
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
  ebeneAnlegen(map, {
    id: "vatsim-sectors-line", type: "line", source: "vatsim-sectors-umriss",
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
  map.addSource("vatsim-sector-labels", { type: "geojson", data: empty });
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
  map.addSource("vatsim-atc-airports", { type: "geojson", data: empty });
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
  map.addSource("vatsim-pilots", { type: "geojson", data: empty });
  const flieger = fliegerBild();
  if (flieger) map.addImage("vatsim-plane", flieger, { pixelRatio: 2 });
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

interface NetzAngaben {
  mapRef: MutableRefObject<maplibregl.Map | null>;
  mapReady: boolean;
  sichtbar: boolean;
  zugangTakt: number;
  simSnapshotRef: MutableRefObject<SimSnapshot | null>;
}

/** Holt die Netzdaten, solange ein Netz gewaehlt ist, und haengt Hoehenfilter und Klickfenster an. */
export function useNetzKarte({ mapRef, mapReady, sichtbar, zugangTakt, simSnapshotRef }: NetzAngaben) {
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
  const vatsimPopupRef = useRef<maplibregl.Popup | null>(null);
  // Welche Flugflaeche die Sektoren zeigen. "auto" folgt dem eigenen Flug —
  // die Karte beantwortet dann durchgehend "wer ist fuer MICH zustaendig".
  // Ohne Flug gilt der Regler (Start FL200: dort spielt der Streckenflug).
  const [flModus, setFlModus] = useState<"auto" | "fest">("auto");
  const [flWert, setFlWert] = useState(200);
  const flModusRef = useRef(flModus);
  const flWertRef = useRef(flWert);
  useEffect(() => { flModusRef.current = flModus; flWertRef.current = flWert; }, [flModus, flWert]);
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
      (map.getSource("vatsim-pilots") as maplibregl.GeoJSONSource | undefined)?.setData(pilots);
      (map.getSource("vatsim-atc-airports") as maplibregl.GeoJSONSource | undefined)?.setData(atc);
      (map.getSource("vatsim-sectors") as maplibregl.GeoJSONSource | undefined)?.setData(sectors);
      (map.getSource("vatsim-sectors-umriss") as maplibregl.GeoJSONSource | undefined)?.setData(
        umrissOhneNaht(sectors),
      );
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
          "alle", abbruch.signal, netz,
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
    // `zugangTakt`: nach der Anmeldung am Live-Server sofort neu holen
    // (vorher evtl. 401), nicht erst beim naechsten 30-s-Takt.
  }, [netz, mapReady, sichtbar, zugangTakt]);

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

  return { netz, setNetz, flModus, setFlModus, flWert, setFlWert, netzZeit };
}

/** Gelbes Chevron fuer fremde VATSIM-Piloten — identisch zur Webapp-Karte. */
function fliegerBild(): ImageData | null {
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
