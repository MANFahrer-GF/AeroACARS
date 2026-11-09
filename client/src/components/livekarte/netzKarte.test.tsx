// Das Online-Netz auf der Karte — durch die echte Karte und die echte Leiste bedient.
//
// Abnahmeliste Live-Karte, Zeilen E1, E2, E3, E5, E6 (docs/spec/livekarte-umbau-abnahme.md).
// maplibre-gl ist ein Nachbau, der aufzeichnet: addLayer-Spezifikationen, setData je Quelle,
// setFilter, die `on()`-Anmeldungen (damit Klicks so ausgeloest werden koennen, wie MapLibre sie
// verteilt) und jedes Popup samt Inhalt. Netz-Abrufe laufen durch die echten Funktionen
// (`fetchVatsimData`, `ladeSektoren`); nur Server und Datafeed sind ersetzt.

import { describe, it, expect, beforeEach, afterEach, beforeAll, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

type Merkmale = Record<string, unknown>;
interface Treffer { properties: Merkmale }
const h = vi.hoisted(() => ({
  handlers: [] as Array<{ ev: string; layer?: string; cb: (e?: unknown) => void }>,
  ebenen: new Map<string, Record<string, unknown>>(),
  quellen: new Set<string>(),
  daten: new Map<string, { type: string; features: Array<Record<string, unknown>> }>(),
  datenLog: [] as Array<{ id: string; features: number }>,
  filter: [] as Array<{ id: string; filter: unknown }>,
  popups: [] as Array<{ html: string; offen: boolean; opts: Record<string, unknown> }>,
  rufe: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
  vatsimAbrufe: 0,
  bilder: [] as Array<{ id: string; pixelRatio?: number }>,
  feed: {} as Record<string, unknown>,
  sektoren: {} as Record<string, unknown>,
  ivaoAntwort: null as Promise<unknown> | null,
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    el: HTMLElement;
    constructor(opts?: { element?: HTMLElement }) { this.el = opts?.element ?? document.createElement("div"); }
    setLngLat() { return this; }
    setRotation() { return this; }
    addTo() { return this; }
    remove() { return this; }
    getElement() { return this.el; }
  }
  class FakePopup {
    eintrag: { html: string; offen: boolean; opts: Record<string, unknown> };
    constructor(opts?: Record<string, unknown>) {
      this.eintrag = { html: "", offen: false, opts: opts ?? {} };
      h.popups.push(this.eintrag);
    }
    setLngLat() { return this; }
    setHTML(html: string) { this.eintrag.html = html; return this; }
    addTo() { this.eintrag.offen = true; return this; }
    remove() { this.eintrag.offen = false; return this; }
    on() { return this; }
  }
  class FakeBounds { extend() { return this; } }
  class FakeNav {}
  class FakeMap {
    center = { lng: 6, lat: 48 };
    zoom = 4;
    canvas = { style: { cursor: "" } };
    addControl() { return this; }
    on(ev: string, a: string | ((e?: unknown) => void), b?: (e?: unknown) => void) {
      if (typeof a === "string") h.handlers.push({ ev, layer: a, cb: b! });
      else h.handlers.push({ ev, cb: a });
      return this;
    }
    once(ev: string, cb: (e?: unknown) => void) { h.handlers.push({ ev, cb }); return this; }
    off(ev: string, a: string | ((e?: unknown) => void), b?: (e?: unknown) => void) {
      const cb = typeof a === "string" ? b : a;
      h.handlers = h.handlers.filter((x) => !(x.ev === ev && x.cb === cb));
      return this;
    }
    getCanvas() { return this.canvas; }
    getBounds() {
      return { getSouth: () => 47, getNorth: () => 49, getWest: () => 5, getEast: () => 7 };
    }
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    getBearing() { return 0; }
    isStyleLoaded() { return true; }
    getLayer(id: string) { return h.ebenen.get(id); }
    addLayer(spec: { id: string }) { h.ebenen.set(spec.id, spec as unknown as Record<string, unknown>); return this; }
    addSource(id: string) { h.quellen.add(id); return this; }
    getSource(id: string) {
      if (!h.quellen.has(id)) return undefined;
      return {
        setData: (d: { type: string; features: Array<Record<string, unknown>> }) => {
          h.daten.set(id, d);
          h.datenLog.push({ id, features: d.features.length });
        },
      };
    }
    setFilter(id: string, filter: unknown) { h.filter.push({ id, filter }); return this; }
    setLayoutProperty() { return this; }
    setPaintProperty() { return this; }
    addSprite() { return this; }
    removeSprite() { return this; }
    getGlyphs() { return ""; }
    setGlyphs() { return this; }
    removeLayer() { return this; }
    removeSource() { return this; }
    hasImage() { return false; }
    addImage(id: string, _bild: unknown, opts?: { pixelRatio?: number }) { h.bilder.push({ id, pixelRatio: opts?.pixelRatio }); return this; }
    easeTo() { return this; }
    jumpTo() { return this; }
    flyTo() { return this; }
    fitBounds() { return this; }
    resize() { return this; }
    remove() { return this; }
  }
  return { default: { Map: FakeMap, Marker: FakeMarker, Popup: FakePopup, NavigationControl: FakeNav, LngLatBounds: FakeBounds } };
});

vi.mock("../../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    h.rufe.push({ cmd, args });
    if (cmd === "live_inhalt") return h.ivaoAntwort && String(args?.pfad).includes("netz=ivao") ? h.ivaoAntwort : h.sektoren;
    if (cmd === "va_live_flights") return { data: [] };
    if (cmd === "flight_get_route_fixes") return [];
    if (cmd === "flight_get_track") return [];
    if (cmd === "airport_get") return { lat: null, lon: null };
    if (cmd === "airport_ground_index") return [];
    if (cmd === "activity_log_get") return [];
    if (cmd === "hoppie_get_thread") return [];
    return null;
  }),
  listen: vi.fn(async () => () => {}),
  openExternal: vi.fn(async () => {}),
}));
vi.mock("../ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../../lib/trackStore", () => ({ getTrack: () => [], setTrack: () => {} }));
vi.mock("../../lib/aircraftIcon", () => ({ aircraftSvg: () => "<svg></svg>" }));
vi.mock("../../lib/phaseColors", () => ({ phaseColor: () => "#ffffff", phaseLabel: () => "Steigflug" }));

import { LiveMapView } from "../LiveMapView";
import { _vatsimCacheLeeren } from "../../lib/vatsimKarte";
import { karteQuelltext } from "./quelltext.testhilfe";

const fc = (features: Array<Record<string, unknown>> = []) => ({ type: "FeatureCollection", features });
const flaeche = (ruf: string, farbe: string, ring: number[][], mehr: Merkmale = {}) => ({
  type: "Feature",
  properties: { ruf, farbe, frequenz: "125.500", gesprochen: ruf, fl_von: 0, fl_bis: 660, ...mehr },
  geometry: { type: "Polygon", coordinates: [ring] },
});
const punkt = (lon: number, lat: number, eigen: Merkmale) => ({
  type: "Feature", properties: eigen, geometry: { type: "Point", coordinates: [lon, lat] },
});

const snap = {
  lat: 49, lon: 9, altitude_msl_ft: 6000, groundspeed_kt: 280, indicated_airspeed_kt: 250,
  heading_deg_true: 295, heading_deg_magnetic: 295, aircraft_icao: "A21N", timestamp: 0,
};
const flight = {
  pirep_id: "P1", airline_icao: "THY", flight_number: "155", dpt_airport: "LTFM",
  arr_airport: "LYTV", phase: "climb", distance_nm: 120, was_just_resumed: false,
};

beforeEach(() => {
  h.handlers = [];
  h.ebenen.clear();
  h.quellen.clear();
  h.daten.clear();
  h.datenLog = [];
  h.ivaoAntwort = null;
  h.filter = [];
  h.popups = [];
  h.rufe = [];
  h.vatsimAbrufe = 0;
  h.bilder = [];
  // jsdom kennt keine Zeichenflaeche; das Flieger-Symbol braucht eine, also ein Stummel.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, closePath() {}, fill() {}, stroke() {},
    getImageData: () => ({ width: 48, height: 48, data: new Uint8ClampedArray(48 * 48 * 4) }),
  } as unknown as CanvasRenderingContext2D);
  h.feed = {
    pilots: [{
      callsign: "DLH123", latitude: 50, longitude: 8, altitude: 35000, groundspeed: 450, heading: 270,
      flight_plan: { departure: "EDDF", arrival: "EGLL", aircraft_short: "A320" },
    }],
    controllers: [], atis: [],
  };
  h.sektoren = {
    flaechen: fc([
      flaeche("EDGG_CTR", "#f43f5e", [[8, 49], [9, 49], [9, 50], [8, 49]]),
      flaeche("EDMM_CTR", "#22d3ee", [[11, 48], [12, 48], [12, 49], [11, 48]]),
      // Pazifik-Sektor, bei ±180° geteilt: die senkrechte Kante liegt genau auf der Datumsgrenze.
      flaeche("NZZO_OCEANIC", "#a3e635", [[170, 10], [180, 10], [180, 20], [170, 20], [170, 10]]),
    ]),
    marken: fc([punkt(8.5, 49.3, { ruf: "EDGG_CTR", frequenz: "125.500", farbe: "#f43f5e" })]),
    abgedeckt: [],
    gebucht: fc(),
    tracon: { flaechen: fc(), marken: fc(), abgedeckt: [] },
    atc: {
      plaetze: fc([punkt(8.57, 50.03, { icao: "EDDF", tone: "twr", count: 1, hat_app: 1 })]),
      firFlaechen: fc([flaeche("LFFF", "#123456", [[2, 47], [3, 47], [3, 48], [2, 47]], { fir: "LFFF", fir_name: "Paris" })]),
      firMarken: fc([punkt(2.5, 47.5, { ruf: "LFFF", frequenz: "" })]),
    },
  };
  _vatsimCacheLeeren();
  localStorage.clear();
  document.documentElement.dataset.theme = "dark";
  vi.stubGlobal("fetch", (url: string) => {
    if (String(url).includes("data.vatsim.net")) {
      h.vatsimAbrufe += 1;
      return Promise.resolve({ ok: true, json: async () => h.feed });
    }
    return new Promise(() => {}); // Kartenstile: eigener Test
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function ansicht(simSnapshot: typeof snap | null = snap, sichtbar = true) {
  return <LiveMapView activeFlight={flight as never} simSnapshot={simSnapshot as never} simKind={undefined} onSwitchToBriefing={() => {}} sichtbar={sichtbar} />;
}
async function karte(simSnapshot: typeof snap | null = snap) {
  const r = render(ansicht(simSnapshot));
  act(() => { h.handlers.filter((x) => x.ev === "load" && !x.layer).forEach((x) => x.cb()); });
  await waitFor(() => expect(h.ebenen.has("vatsim-sectors-fill")).toBe(true));
  return r;
}
const knopf = (name: string) => screen.getByRole("button", { name });
const gedrueckt = () =>
  Object.fromEntries(["Aus", "VATSIM", "IVAO"].map((n) => [n, knopf(n).getAttribute("aria-pressed")]));
const sektorAbrufe = () =>
  h.rufe.filter((r) => r.cmd === "live_inhalt").map((r) => String(r.args?.pfad));
const quelle = (id: string) => h.daten.get(id);

/** So verteilt MapLibre einen Klick: jede Ebene mit Treffern ruft ihre Behandler in der Reihenfolge
 *  der Anmeldung, alle bekommen dasselbe urspruengliche Ereignis. */
function klick(treffer: Record<string, Merkmale>) {
  const ereignis = { lngLat: { lng: 9, lat: 49 }, originalEvent: {} };
  act(() => {
    for (const x of h.handlers.filter((y) => y.ev === "click" && y.layer)) {
      const t = treffer[x.layer!];
      if (t) x.cb({ ...ereignis, features: [{ properties: t } satisfies Treffer] });
    }
  });
}
const offenePopups = () => h.popups.filter((p) => p.offen);

const filterFuer = (fl: number) => ["any",
  ["has", "ohne_hoehe"],
  ["has", "tracon"],
  ["all",
    ["<=", ["coalesce", ["get", "fl_von"], 0], fl],
    [">=", ["coalesce", ["get", "fl_bis"], 999], fl],
  ],
];
const letzterFilter = (id: string) => [...h.filter].reverse().find((f) => f.id === id)?.filter;

describe("Online-Netz: Aus / VATSIM / IVAO (E1)", () => {
  it("startet immer auf Aus und holt dann nichts", async () => {
    // Auch ein frueher gemerkter Wert darf die Karte nicht mit einem Netz starten lassen.
    for (const k of ["aaLivemapNetz", "aaLivemapVatsim", "aaLivemapIvao"]) localStorage.setItem(k, "vatsim");
    await karte();
    expect(gedrueckt()).toEqual({ Aus: "true", VATSIM: "false", IVAO: "false" });
    await act(async () => { await new Promise((r) => setTimeout(r, 50)); });
    expect(sektorAbrufe()).toEqual([]);
    expect(h.vatsimAbrufe).toBe(0);
    expect(quelle("vatsim-pilots")?.features.length ?? 0).toBe(0);
  });

  it("laesst nie zwei Netze zugleich zu und zeigt je Klick genau eines", async () => {
    await karte();
    const zustaende: Array<Record<string, string | null>> = [];
    for (const wahl of ["VATSIM", "IVAO", "VATSIM", "Aus", "IVAO"]) {
      fireEvent.click(knopf(wahl));
      zustaende.push(gedrueckt());
    }
    for (const z of zustaende) expect(Object.values(z).filter((v) => v === "true")).toHaveLength(1);
    expect(zustaende.map((z) => Object.keys(z).find((k) => z[k] === "true"))).toEqual(["VATSIM", "IVAO", "VATSIM", "Aus", "IVAO"]);
  });

  it("wechselt von VATSIM auf IVAO ohne Umweg ueber Aus: die Daten des alten Netzes verschwinden sofort, nicht erst mit der neuen Antwort", async () => {
    let antworten: (wert: unknown) => void = () => {};
    h.ivaoAntwort = new Promise((ok) => { antworten = ok; });
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-sectors")?.features.length).toBeGreaterThan(0));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBeGreaterThan(0));
    klick({ "vatsim-sectors-fill": { ruf: "EDGG_CTR", gesprochen: "Langen", frequenz: "125.500", farbe: "#f43f5e" } });
    expect(offenePopups()).toHaveLength(1);

    fireEvent.click(knopf("IVAO"));
    await waitFor(() => expect(sektorAbrufe()).toEqual(["/api/vatglasses?fl=alle", "/api/vatglasses?fl=alle&netz=ivao"]));
    // Die Antwort steht noch aus: Unter „IVAO“ darf nichts von VATSIM mehr liegen ...
    for (const id of ["vatsim-pilots", "vatsim-atc-airports", "vatsim-sectors", "vatsim-sectors-umriss", "vatsim-sector-labels"]) {
      expect(quelle(id)?.features, id).toEqual([]);
    }
    expect(offenePopups(), "Klickfenster des alten Netzes").toEqual([]);
    // ... und die Bedienung war nie auf Aus.
    expect(gedrueckt().IVAO).toBe("true");
    expect(gedrueckt().Aus).toBe("false");

    await act(async () => {
      antworten({
        ...h.sektoren,
        flaechen: fc([flaeche("LFPG_CTR", "#00ff00", [[2, 48], [3, 48], [3, 49], [2, 48]])]),
        piloten: [{ callsign: "IVA77", latitude: 50, longitude: 8, altitude: 36000, groundspeed: 450, heading: 90, flight_plan: null }],
      });
    });
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.map((f) => (f.properties as Merkmale).callsign)).toEqual(["IVA77"]));
  });

  it("leert die Quellen nicht, wenn dasselbe Netz neu holt (Reiter weg und zurueck)", async () => {
    const r = await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBeGreaterThan(0));
    await waitFor(() => expect(sektorAbrufe()).toHaveLength(1));
    const marke = h.datenLog.length;

    r.rerender(ansicht(snap, false));
    r.rerender(ansicht(snap, true));
    await waitFor(() => expect(sektorAbrufe()).toHaveLength(2));
    await waitFor(() => expect(h.datenLog.length).toBeGreaterThan(marke));
    expect(h.datenLog.slice(marke).filter((e) => e.features === 0 && e.id === "vatsim-pilots")).toEqual([]);
    expect(quelle("vatsim-pilots")?.features.length).toBeGreaterThan(0);
  });

  it("E7: meldet die Messung genau einmal ans Log und zeigt sie nirgends an", async () => {
    await karte();
    const messungen = () => h.rufe.filter((r) => r.cmd === "netz_messung");
    const ruhe = () => act(() => { h.handlers.filter((x) => x.ev === "idle").forEach((x) => x.cb()); });
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-sectors")?.features.length).toBeGreaterThan(0));
    await waitFor(() => expect(h.handlers.some((x) => x.ev === "idle")).toBe(true));
    expect(messungen(), "erst wenn die Karte zur Ruhe kommt").toEqual([]);
    ruhe();
    expect(messungen()).toHaveLength(1);
    expect(messungen()[0].args).toEqual({ netz: "vatsim", holenMs: expect.any(Number), zeichnenMs: expect.any(Number) });
    expect(document.body.textContent).not.toMatch(/holen|zeichnen|erstmals/i);

    fireEvent.click(knopf("IVAO"));
    await waitFor(() => expect(sektorAbrufe()).toHaveLength(2));
    await waitFor(() => expect(quelle("vatsim-sectors")?.features.length).toBeGreaterThan(0));
    ruhe();
    expect(messungen(), "nur das allererste Zuschalten").toHaveLength(1);
  });

  it("G8: die Messzeile kehrt nicht in die Oberflaeche zurueck", () => {
    const quelltext = karteQuelltext();
    expect(quelltext).not.toMatch(/\} s (holen|zeichnen)|Netz \(erstmals|netzZeit|netz_zeit_hint/);
  });

  it("raeumt beim Wechsel auf Aus alle fuenf Quellen leer", async () => {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-sectors")?.features.length).toBeGreaterThan(0));
    fireEvent.click(knopf("Aus"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(0));
    for (const id of ["vatsim-pilots", "vatsim-atc-airports", "vatsim-sectors", "vatsim-sectors-umriss", "vatsim-sector-labels"]) {
      expect(quelle(id)?.features.length, id).toBe(0);
    }
  });
});

describe("Sektoren auf der Karte (E2)", () => {
  async function mitVatsim() {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-sectors")?.features.length).toBeGreaterThan(0));
  }

  it("zeichnet den Rand aus dem Umriss ohne Naht bei ±180°, die Flaeche aus den vollen Polygonen", async () => {
    await mitVatsim();
    expect((h.ebenen.get("vatsim-sectors-line") as { source: string }).source).toBe("vatsim-sectors-umriss");
    expect((h.ebenen.get("vatsim-sectors-fill") as { source: string }).source).toBe("vatsim-sectors");
    const flaechen = quelle("vatsim-sectors")!.features;
    const polygon = flaechen.find((f) => (f.properties as Merkmale).ruf === "NZZO_OCEANIC")!;
    expect((polygon.geometry as { type: string }).type).toBe("Polygon");
    expect((polygon.geometry as { coordinates: number[][][] }).coordinates[0]).toContainEqual([180, 20]);

    const umriss = quelle("vatsim-sectors-umriss")!.features;
    const pazifik = umriss.find((f) => (f.properties as Merkmale).ruf === "NZZO_OCEANIC")!;
    expect((pazifik.geometry as { type: string }).type).toBe("MultiLineString");
    const linien = (pazifik.geometry as { coordinates: number[][][] }).coordinates;
    // Keine Strecke, die beide Punkte auf 180° hat: die Naht fehlt, der Rest steht.
    for (const linie of linien) {
      for (let i = 1; i < linie.length; i++) expect(linie[i - 1][0] === 180 && linie[i][0] === 180).toBe(false);
    }
    expect(linien.flat()).toContainEqual([170, 10]);
    // Ein gewoehnlicher Sektor behaelt seinen ganzen Rand.
    const gewoehnlich = umriss.find((f) => (f.properties as Merkmale).ruf === "EDGG_CTR")!;
    expect((gewoehnlich.geometry as { coordinates: number[][][] }).coordinates.flat()).toHaveLength(4);
  });

  it("faerbt jeden Sektor in der Farbe seiner Station und die groben FIR-Flaechen gruen", async () => {
    await mitVatsim();
    const farben = Object.fromEntries(
      quelle("vatsim-sectors")!.features.map((f) => [(f.properties as Merkmale).ruf, (f.properties as Merkmale).farbe]),
    );
    expect(farben).toMatchObject({ EDGG_CTR: "#f43f5e", EDMM_CTR: "#22d3ee", NZZO_OCEANIC: "#a3e635", LFFF: "#4ade80" });
    // Die FIR-Flaeche bekommt die Felder des Klickfensters und gilt ohne Hoehenband.
    const fir = quelle("vatsim-sectors")!.features.find((f) => (f.properties as Merkmale).ruf === "LFFF")!;
    expect(fir.properties).toMatchObject({ gesprochen: "Paris", ohne_hoehe: 1 });
    // Die Ebenen lesen die Farbe je Flaeche aus den Daten.
    const paint = (id: string) => (h.ebenen.get(id) as { paint: Record<string, unknown> }).paint;
    expect(paint("vatsim-sectors-fill")["fill-color"]).toEqual(["get", "farbe"]);
    expect(paint("vatsim-sectors-line")["line-color"]).toEqual(["get", "farbe"]);
    expect(paint("vatsim-sector-labels-symbol")["text-color"]).toEqual(["get", "farbe"]);
  });

  it("beschriftet jeden Sektor mit Kennung und Frequenz, die FIR-Marke in Gruen", async () => {
    await mitVatsim();
    const ebene = h.ebenen.get("vatsim-sector-labels-symbol") as { source: string; layout: Record<string, unknown> };
    expect(ebene.source).toBe("vatsim-sector-labels");
    expect(ebene.layout["text-field"]).toEqual(["format", ["get", "ruf"], {}, "\n", {}, ["get", "frequenz"], { "font-scale": 0.8 }]);
    const marken = quelle("vatsim-sector-labels")!.features.map((f) => f.properties as Merkmale);
    expect(marken.find((m) => m.ruf === "EDGG_CTR")).toMatchObject({ frequenz: "125.500", farbe: "#f43f5e" });
    expect(marken.find((m) => m.ruf === "LFFF")).toMatchObject({ farbe: "#4ade80" });
  });
});

describe("Lotsen und Plaetze (E4)", () => {
  type Ebene = { type: string; source: string; filter?: unknown; layout: Record<string, unknown>; paint: Record<string, unknown> };
  const ebene = (id: string) => h.ebenen.get(id) as unknown as Ebene;

  it("faerbt den Platzpunkt nach Stationsart: Turm blau, Rollkontrolle violett, sonst Turm-Blau", async () => {
    await karte();
    const punkt = ebene("vatsim-atc-airports-circle");
    expect(punkt.type).toBe("circle");
    expect(punkt.source).toBe("vatsim-atc-airports");
    expect(punkt.paint["circle-color"]).toEqual(["match", ["get", "tone"], "twr", "#38bdf8", "gnd", "#818cf8", "#38bdf8"]);
    expect(punkt.paint["circle-radius"]).toEqual(["case", [">", ["get", "count"], 1], 7, 5.5]);
  });

  it("legt den Anflug (APP) als gelben Ring um den Platz, nur wo es eine APP-Station gibt", async () => {
    await karte();
    const ring = ebene("vatsim-atc-app-ring");
    expect(ring.type).toBe("circle");
    expect(ring.source).toBe("vatsim-atc-airports");
    expect(ring.filter).toEqual(["==", ["get", "hat_app"], 1]);
    expect(ring.paint["circle-color"]).toBe("rgba(0,0,0,0)");
    expect(ring.paint["circle-stroke-color"]).toBe("#fbbf24");
    expect(ring.paint["circle-stroke-width"]).toBe(1.6);
    expect(ring.paint["circle-radius"]).toEqual(["interpolate", ["linear"], ["zoom"], 4, 9, 8, 20]);
  });

  it("schreibt ICAO und die Stationsbuchstaben A, D, G, T in festen Farben in eine Zeile", async () => {
    await karte();
    const label = ebene("vatsim-atc-airports-label");
    expect(label.type).toBe("symbol");
    expect(label.layout["text-field"]).toEqual([
      "format",
      ["get", "icao"], {},
      "  ", {},
      ["get", "t_atis"], { "text-color": "#eab308" },
      ["get", "t_del"], { "text-color": "#60a5fa" },
      ["get", "t_gnd"], { "text-color": "#4ade80" },
      ["get", "t_twr"], { "text-color": "#f87171" },
    ]);
  });

  it("gibt dem Platz Stationsart, APP-Zeichen und Buchstaben aus den Serverdaten mit auf die Karte", async () => {
    const platz = punkt(8.57, 50.03, { icao: "EDDF", tone: "gnd", count: 4, hat_app: 1, t_atis: "A", t_del: "D", t_gnd: "G", t_twr: "T" });
    (h.sektoren.atc as { plaetze: unknown }).plaetze = fc([platz]);
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-atc-airports")?.features.length).toBe(1));
    expect(quelle("vatsim-atc-airports")!.features[0].properties).toEqual({
      icao: "EDDF", tone: "gnd", count: 4, hat_app: 1, t_atis: "A", t_del: "D", t_gnd: "G", t_twr: "T",
    });
  });

  it("zeigt im Klickfenster des Platzes die Stationen in der Reihenfolge ATIS, DEL, GND, TWR, APP mit Info-Buchstabe", async () => {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(1));
    const stationen = [
      { tag: "APP", callsign: "EDDF_APP", frequency: "125.350" },
      { tag: "TWR", callsign: "EDDF_TWR", frequency: "118.700" },
      { tag: "ATIS", callsign: "EDDF_ATIS", frequency: "118.020", code: "K" },
      { tag: "GND", callsign: "EDDF_GND", frequency: "121.800" },
    ];
    klick({ "vatsim-atc-airports-circle": { icao: "EDDF", positions: JSON.stringify(stationen) } });
    const html = offenePopups()[0].html;
    const folge = ["EDDF_ATIS", "EDDF_GND", "EDDF_TWR", "EDDF_APP"].map((c) => html.indexOf(c));
    expect(folge.every((i) => i > 0)).toBe(true);
    expect([...folge].sort((a, b) => a - b)).toEqual(folge);
    expect(html).toContain("INFO: K");
    expect(html).toContain("vatsim-pop-chip--atis");
  });
});

describe("Hoehenregler (E3)", () => {
  const regler = () => screen.queryByRole("slider") as HTMLInputElement | null;
  const anzeige = () => document.querySelector(".aa-livemap-flwahl__wert")?.textContent;

  it("steht nur bei VATSIM da: nicht bei Aus, nicht bei IVAO", async () => {
    await karte();
    expect(regler()).toBeNull();
    fireEvent.click(knopf("VATSIM"));
    expect(regler()).not.toBeNull();
    expect(regler()!.min).toBe("0");
    expect(regler()!.max).toBe("450");
    expect(regler()!.step).toBe("10");
    fireEvent.click(knopf("IVAO"));
    expect(regler()).toBeNull();
    expect(screen.queryByRole("button", { name: "eigene Höhe" })).toBeNull();
    fireEvent.click(knopf("Aus"));
    expect(regler()).toBeNull();
  });

  it("wirkt sofort: filtert die geladenen Sektoren neu, ohne einen weiteren Abruf", async () => {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(letzterFilter("vatsim-sectors-fill")).toEqual(filterFuer(60)));
    const abrufe = sektorAbrufe().length;
    // Mit Flug folgt der Regler der Hoehe und ist gesperrt; "eigene Hoehe" abschalten macht ihn frei.
    expect(regler()!.disabled).toBe(true);
    fireEvent.click(knopf("eigene Höhe"));
    expect(regler()!.disabled).toBe(false);
    fireEvent.change(regler()!, { target: { value: "330" } });
    expect(anzeige()).toBe("FL330");
    for (const id of ["vatsim-sectors-fill", "vatsim-sectors-line", "vatsim-sector-labels-symbol"]) {
      expect(letzterFilter(id), id).toEqual(filterFuer(330));
    }
    expect(sektorAbrufe().length).toBe(abrufe);
    // "eigene Hoehe" ist dabei aus.
    expect(knopf("eigene Höhe").getAttribute("aria-pressed")).toBe("false");
  });

  it("folgt mit „eigene Höhe“ dem Flug: Anzeige und Filter wandern mit der Flughoehe", async () => {
    const r = await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(letzterFilter("vatsim-sectors-fill")).toEqual(filterFuer(60)));
    expect(anzeige()).toBe("FL060");
    const abrufe = sektorAbrufe().length;
    r.rerender(ansicht({ ...snap, altitude_msl_ft: 24000 }));
    expect(anzeige()).toBe("FL240");
    await waitFor(() => expect(letzterFilter("vatsim-sectors-fill")).toEqual(filterFuer(240)));
    expect(sektorAbrufe().length).toBe(abrufe);
  });

  it("gilt ohne Sim-Verbindung immer der Regler (Start FL200) und ist dann bedienbar", async () => {
    await karte(null);
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(letzterFilter("vatsim-sectors-fill")).toEqual(filterFuer(200)));
    expect(anzeige()).toBe("FL200");
    expect(regler()!.disabled).toBe(false);
  });
});

describe("Fremde Piloten (E5)", () => {
  it("legt die Pilotenebene ab Zoom 4,5 an, mit Flieger, der in Kartenrichtung nach seinem Kurs gedreht ist", async () => {
    await karte();
    const ebene = h.ebenen.get("vatsim-pilots-symbol") as {
      type: string; source: string; minzoom: number; layout: Record<string, unknown>;
    };
    expect(ebene.type).toBe("symbol");
    expect(ebene.source).toBe("vatsim-pilots");
    expect(ebene.minzoom).toBe(4.5);
    expect(ebene.layout["icon-image"]).toBe("vatsim-plane");
    expect(ebene.layout["icon-rotate"]).toEqual(["get", "hdg"]);
    expect(ebene.layout["icon-rotation-alignment"]).toBe("map");
  });

  it("registriert das Flieger-Bild der Ebene in doppelter Aufloesung", async () => {
    await karte();
    expect(h.bilder).toEqual([{ id: "vatsim-plane", pixelRatio: 2 }]);
  });

  it("gibt jedem Piloten seinen Kurs aus dem Datafeed mit", async () => {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(1));
    const f = quelle("vatsim-pilots")!.features[0];
    expect(f.properties).toMatchObject({ callsign: "DLH123", hdg: 270, dep: "EDDF", arr: "EGLL", actype: "A320" });
    expect((f.geometry as { coordinates: number[] }).coordinates).toEqual([8, 50]);
  });

  it("nimmt bei IVAO die Piloten aus der Serverantwort und holt keinen VATSIM-Feed", async () => {
    h.sektoren = {
      ...(h.sektoren as object),
      piloten: [{
        callsign: "IVA77", latitude: -33.9, longitude: 18.6, altitude: 36132, groundspeed: 462, heading: 234,
        flight_plan: { departure: "VTBS", arrival: "FACT", aircraft_short: "A359" },
      }],
    };
    await karte();
    fireEvent.click(knopf("IVAO"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(1));
    const f = quelle("vatsim-pilots")!.features[0];
    expect(f.properties).toMatchObject({ callsign: "IVA77", hdg: 234, dep: "VTBS", arr: "FACT", actype: "A359" });
    expect((f.geometry as { coordinates: number[] }).coordinates).toEqual([18.6, -33.9]);
    expect(h.vatsimAbrufe).toBe(0);
  });

  it("zeigt bei IVAO keine VATSIM-Piloten, auch wenn der Server noch keine Piloten mitschickt", async () => {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(1));
    fireEvent.click(knopf("IVAO"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(0));
  });
});

describe("Klickfenster (E6)", () => {
  const PILOT = { callsign: "DLH123", dep: "EDDF", arr: "EGLL", actype: "A320", alt: 35000, gs: 450 };
  const PLATZ = { icao: "EDDF", positions: JSON.stringify([{ tag: "TWR", callsign: "EDDF_TWR", frequency: "118.700" }]) };
  const SEKTOR = { ruf: "EDGG_CTR", gesprochen: "Langen Radar", frequenz: "125.500", fl_von: 245, fl_bis: 660 };

  async function bereit() {
    await karte();
    fireEvent.click(knopf("VATSIM"));
    await waitFor(() => expect(quelle("vatsim-pilots")?.features.length).toBe(1));
  }

  it("meldet sich auf den drei Ebenen an: Pilot, Platz, Sektor", async () => {
    await bereit();
    const ebenen = h.handlers.filter((x) => x.ev === "click" && x.layer).map((x) => x.layer);
    expect(ebenen).toEqual(["vatsim-pilots-symbol", "vatsim-atc-airports-circle", "vatsim-sectors-fill"]);
  });

  it("zeigt bei Pilot, Platz und Sektor in einem Punkt nur das Fenster des Piloten", async () => {
    await bereit();
    klick({ "vatsim-pilots-symbol": PILOT, "vatsim-atc-airports-circle": PLATZ, "vatsim-sectors-fill": SEKTOR });
    expect(offenePopups()).toHaveLength(1);
    const html = offenePopups()[0].html;
    expect(html).toContain("DLH123");
    expect(html).toContain("EDDF");
    expect(html).toContain("FL350");
    expect(html).toContain("450 kt");
    expect(html).not.toContain("EDDF — ATC");
    expect(html).not.toContain("Langen Radar");
  });

  it("zeigt bei Platz und Sektor das Fenster des Platzes, nicht das des Sektors", async () => {
    await bereit();
    klick({ "vatsim-atc-airports-circle": PLATZ, "vatsim-sectors-fill": SEKTOR });
    expect(offenePopups()).toHaveLength(1);
    const html = offenePopups()[0].html;
    expect(html).toContain("EDDF — ATC");
    expect(html).toContain("EDDF_TWR");
    expect(html).toContain("118.700");
    expect(html).not.toContain("Langen Radar");
  });

  it("zeigt nur einen Sektor mit Sprechname, Frequenz und Hoehenband", async () => {
    await bereit();
    klick({ "vatsim-sectors-fill": SEKTOR });
    expect(offenePopups()).toHaveLength(1);
    const html = offenePopups()[0].html;
    expect(html).toContain("EDGG_CTR");
    expect(html).toContain("Langen Radar");
    expect(html).toContain("125.500");
    expect(html).toContain("FL245–FL660");
  });

  it("ersetzt beim naechsten Klick das alte Fenster, statt ein zweites daneben zu oeffnen", async () => {
    await bereit();
    klick({ "vatsim-sectors-fill": SEKTOR });
    klick({ "vatsim-pilots-symbol": PILOT });
    expect(h.popups).toHaveLength(2);
    expect(offenePopups()).toHaveLength(1);
    expect(offenePopups()[0].html).toContain("DLH123");
    expect(h.popups[0].offen).toBe(false);
  });

  it("macht beim Abschalten des Netzes das offene Fenster zu", async () => {
    await bereit();
    klick({ "vatsim-pilots-symbol": PILOT });
    expect(offenePopups()).toHaveLength(1);
    fireEvent.click(knopf("Aus"));
    await waitFor(() => expect(offenePopups()).toHaveLength(0));
  });
});
