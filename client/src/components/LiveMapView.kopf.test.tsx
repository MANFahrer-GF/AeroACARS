// Kopfzeile, Systemzeile, Zaehler, Namensnennung und verdeckter Reiter der Live-Karte.
//
// Abnahmeliste Live-Karte, Zeilen A10, A11, D7, G2, G4 und G5 (docs/spec/livekarte-umbau-abnahme.md).
// Die echte Karte wird gerendert; maplibre-gl ist ein Nachbau, der aufzeichnet: Konstruktor-Optionen
// (der Stil mit der Esri-Nennung), `addSource` (Nennung der Quellen), Kamera-Aufrufe, `resize` und
// die `on()`-Anmeldungen, damit "sourcedata" so ausgeloest werden kann wie MapLibre es tut.

import { describe, it, expect, beforeEach, afterEach, beforeAll, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

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

const h = vi.hoisted(() => ({
  handlers: [] as Array<{ ev: string; cb: (e?: unknown) => void }>,
  optionen: null as null | Record<string, unknown>,
  quellen: [] as Array<{ id: string; spec: Record<string, unknown> }>,
  ebenen: new Map<string, Record<string, unknown>>(),
  kamera: [] as string[],
  resizes: 0,
  abgemeldet: [] as string[],
  rufe: [] as string[],
  bids: [] as unknown[],
  va: [] as unknown[],
  stil: {} as Record<string, unknown>,
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
  class FakePopup { setLngLat() { return this; } setHTML() { return this; } addTo() { return this; } remove() { return this; } on() { return this; } }
  class FakeBounds { extend() { return this; } }
  class FakeNav {}
  class FakeMap {
    center = { lng: 6, lat: 48 };
    constructor(o: Record<string, unknown>) { h.optionen = o; }
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) { h.handlers.push({ ev, cb }); return this; }
    once() { return this; }
    off(ev: string, cb: (e?: unknown) => void) {
      h.abgemeldet.push(ev);
      h.handlers = h.handlers.filter((x) => !(x.ev === ev && x.cb === cb));
      return this;
    }
    getCanvas() { return { style: {} }; }
    getBounds() { return { getSouth: () => 47, getNorth: () => 49, getWest: () => 5, getEast: () => 7 }; }
    getCenter() { return this.center; }
    getZoom() { return 4; }
    getBearing() { return 0; }
    isStyleLoaded() { return true; }
    getLayer(id: string) { return h.ebenen.get(id); }
    addLayer(spec: { id: string }) { h.ebenen.set(spec.id, spec as unknown as Record<string, unknown>); return this; }
    addSource(id: string, spec: Record<string, unknown>) { h.quellen.push({ id, spec }); return this; }
    getSource(id: string) {
      return h.quellen.some((q) => q.id === id) ? { setData: () => {} } : undefined;
    }
    setFilter() { return this; }
    setLayoutProperty() { return this; }
    setPaintProperty() { return this; }
    addSprite() { return this; }
    removeSprite() { return this; }
    getGlyphs() { return ""; }
    setGlyphs() { return this; }
    removeLayer() { return this; }
    removeSource() { return this; }
    hasImage() { return false; }
    addImage() { return this; }
    easeTo() { h.kamera.push("easeTo"); return this; }
    jumpTo() { h.kamera.push("jumpTo"); return this; }
    flyTo() { h.kamera.push("flyTo"); return this; }
    fitBounds() { h.kamera.push("fitBounds"); return this; }
    resize() { h.resizes += 1; return this; }
    remove() { return this; }
  }
  return { default: { Map: FakeMap, Marker: FakeMarker, Popup: FakePopup, NavigationControl: FakeNav, LngLatBounds: FakeBounds } };
});

vi.mock("../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string) => {
    h.rufe.push(cmd);
    if (cmd === "phpvms_get_bids") return h.bids;
    if (cmd === "va_live_flights") return { data: h.va };
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
vi.mock("./ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../lib/trackStore", () => ({ getTrack: () => [], setTrack: () => {} }));
vi.mock("../lib/aircraftIcon", () => ({ aircraftSvg: () => "<svg></svg>" }));

import { LiveMapView } from "./LiveMapView";

const snap = {
  lat: 49, lon: 9, altitude_msl_ft: 6000, groundspeed_kt: 280, indicated_airspeed_kt: 250,
  heading_deg_true: 295, heading_deg_magnetic: 295, aircraft_icao: "A21N", timestamp: 0,
};
const flug = {
  pirep_id: "P1", airline_icao: "DLH", flight_number: "400", dpt_airport: "EDDF",
  arr_airport: "KJFK", phase: "climb", distance_nm: 120, was_just_resumed: false,
  connection_state: "live", last_position_at: null as string | null, queued_position_count: 0, position_count: 42,
};
const kollege = (id: string, nr: string) => ({
  id, ident: `GSG${nr}`, flight_number: nr, dpt_airport_id: "ENGM", arr_airport_id: "ENEV",
  aircraft: { icao: "A319" }, status_text: "Cruise", position: { lat: 60.2, lon: 11.1, gs: 420, heading: 20 },
});

type Props = {
  activeFlight?: Record<string, unknown> | null; simSnapshot?: typeof snap | null; simKind?: string; sichtbar?: boolean;
};
const ansicht = ({ activeFlight = flug, simSnapshot = snap, simKind = undefined, sichtbar = true }: Props = {}) => (
  <LiveMapView activeFlight={activeFlight as never} simSnapshot={simSnapshot as never} simKind={simKind} onSwitchToBriefing={() => {}} sichtbar={sichtbar} />
);
function laden() {
  act(() => { h.handlers.filter((x) => x.ev === "load").forEach((x) => x.cb()); });
}

beforeEach(() => {
  h.handlers = [];
  h.optionen = null;
  h.quellen = [];
  h.ebenen.clear();
  h.kamera = [];
  h.resizes = 0;
  h.abgemeldet = [];
  h.rufe = [];
  h.bids = [];
  h.va = [];
  h.stil = {};
  localStorage.clear();
  document.documentElement.dataset.theme = "dark";
  vi.stubGlobal("fetch", () => new Promise(() => {})); // Kartenstile: eigener Test
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, closePath() {}, fill() {}, stroke() {},
    getImageData: () => ({ width: 48, height: 48, data: new Uint8ClampedArray(48 * 48 * 4) }),
  } as unknown as CanvasRenderingContext2D);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const text = (sel: string) => document.querySelector(sel)?.textContent;

describe("Kopfzeile (G4)", () => {
  it("nennt den Titel der Karte", async () => {
    render(ansicht());
    laden();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Live-Karte");
  });

  it("zeigt im Flug Rufzeichen, Strecke und Phase", async () => {
    render(ansicht());
    laden();
    expect(text(".aa-livemap__header-callsign")).toBe("DLH400");
    expect(text(".aa-livemap__header-route")).toBe("EDDF → KJFK · Climb");
    expect(document.querySelector(".aa-livemap__resume-hint")).toBeNull();
  });

  it("nimmt ein eigenes Rufzeichen des Fluges vor der Nummer", async () => {
    render(ansicht({ activeFlight: { ...flug, callsign: "DLH4YK" } }));
    laden();
    expect(text(".aa-livemap__header-callsign")).toBe("DLH4YK");
  });

  it("zeigt waehrend der Wiederaufnahme das Sanduhr-Zeichen mit Erklaerung", async () => {
    render(ansicht({ activeFlight: { ...flug, was_just_resumed: true } }));
    laden();
    const hinweis = document.querySelector(".aa-livemap__resume-hint") as HTMLElement;
    expect(hinweis.textContent?.trim()).toBe("⏳");
    expect(hinweis.title).toBe(deCommon.livemap.resume_hint);
    expect(hinweis.title.length).toBeGreaterThan(10);
  });

  it("nennt ohne Flug den naechsten gebuchten Flug mit Abflugzeit", async () => {
    h.bids = [{
      id: 7,
      flight: {
        airline: { icao: "DLH" }, flight_number: "400", dpt_time: "08:15",
        dpt_airport: { icao: "EDDF" }, arr_airport: { icao: "KJFK" },
      },
    }];
    render(ansicht({ activeFlight: null, simSnapshot: null }));
    laden();
    await waitFor(() => expect(text(".aa-livemap__header-route")).toBe("Kein aktiver Flug · nächster DLH400 ab 08:15z"));
    expect(document.querySelector(".aa-livemap__header-callsign")).toBeNull();
  });

  it("sagt ohne Flug und ohne Buchung nur, dass kein Flug aktiv ist", async () => {
    render(ansicht({ activeFlight: null, simSnapshot: null }));
    laden();
    await waitFor(() => expect(h.rufe).toContain("phpvms_get_bids"));
    expect(text(".aa-livemap__header-route")).toBe("Kein aktiver Flug");
  });

  it("zeigt die Uhr in UTC und zaehlt jede Sekunde weiter", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-09T10:00:00Z"));
    render(ansicht());
    laden();
    expect(text(".aa-livemap__header-clock")).toBe("10:00:00z");
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(text(".aa-livemap__header-clock")).toBe("10:00:03z");
  });
});

describe("Zaehler im Kopf (D7)", () => {
  async function mitKollegen(n: number) {
    h.va = Array.from({ length: n }, (_, i) => kollege(`K${i}`, String(100 + i)));
    render(ansicht({ activeFlight: null, simSnapshot: null }));
    laden();
  }
  const zaehler = () => text(".aa-livemap__header-vacount");

  it("nennt die Zahl der Kollegen in der Luft, im Plural", async () => {
    await mitKollegen(3);
    await waitFor(() => expect(zaehler()).toBe("3 GSG-Maschinen sind in der Luft"));
  });

  it("nennt einen einzelnen Kollegen im Singular", async () => {
    await mitKollegen(1);
    await waitFor(() => expect(zaehler()).toBe("1 GSG-Maschine ist in der Luft"));
  });

  it("zaehlt ohne Kollegen null", async () => {
    await mitKollegen(0);
    await waitFor(() => expect(h.rufe).toContain("va_live_flights"));
    expect(zaehler()).toBe("0 GSG-Maschinen sind in der Luft");
  });

  it("zaehlt den eigenen Flug nicht mit, auch wenn er in der Liste steht", async () => {
    h.va = [kollege("P1", "400"), kollege("K2", "101")];
    render(ansicht());
    laden();
    await waitFor(() => expect(zaehler()).toBe("1 GSG-Maschine ist in der Luft"));
  });

  it("geht auf null, wenn der Schalter VA-Verkehr ausgeschaltet wird, und kommt zurueck", async () => {
    await mitKollegen(2);
    await waitFor(() => expect(zaehler()).toBe("2 GSG-Maschinen sind in der Luft"));
    fireEvent.click(screen.getByRole("button", { name: "VA-Verkehr" }));
    await waitFor(() => expect(zaehler()).toBe("0 GSG-Maschinen sind in der Luft"));
    fireEvent.click(screen.getByRole("button", { name: "VA-Verkehr" }));
    await waitFor(() => expect(zaehler()).toBe("2 GSG-Maschinen sind in der Luft"));
  });
});

describe("Systemzeile (G5)", () => {
  const punkt = () => document.querySelector(".aa-livemap-sysline__dot") as HTMLElement;
  const zeile = () => text(".aa-livemap-sysline");

  it("sagt ohne Flug, dass der Recorder inaktiv ist, mit grauem Punkt und ohne Versandanzeige", async () => {
    render(ansicht({ activeFlight: null, simSnapshot: null }));
    laden();
    expect(punkt().className).toContain("aa-livemap-sysline__dot--off");
    expect(zeile()).toContain("Recorder inaktiv");
    expect(document.querySelector(".live-rec")).toBeNull();
  });

  it("meldet im Flug den Recorder aktiv mit Simulator und Uplink OK, gruener Punkt", async () => {
    render(ansicht({ simKind: "msfs2024" }));
    laden();
    expect(punkt().className).toContain("aa-livemap-sysline__dot--ok");
    expect(zeile()).toContain("Recorder aktiv · MSFS · Uplink OK");
  });

  it("nennt den Simulator je Art", async () => {
    const { rerender } = render(ansicht({ simKind: "xplane12" }));
    laden();
    expect(zeile()).toContain("Recorder aktiv · X-PLANE · Uplink OK");
    rerender(ansicht({ simKind: undefined }));
    expect(zeile()).toContain("Recorder aktiv · SIM · Uplink OK");
  });

  it("zeigt bei verlorener oder gesperrter Verbindung roten Punkt und 'Verbindung verloren'", async () => {
    const { rerender } = render(ansicht({ simKind: "msfs2024", activeFlight: { ...flug, connection_state: "failing" } }));
    laden();
    expect(punkt().className).toContain("aa-livemap-sysline__dot--danger");
    expect(zeile()).toContain("Recorder aktiv · MSFS · Verbindung verloren");
    rerender(ansicht({ simKind: "msfs2024", activeFlight: { ...flug, connection_state: "blocked" } }));
    expect(punkt().className).toContain("aa-livemap-sysline__dot--danger");
    expect(zeile()).toContain("Verbindung verloren");
  });

  it("zeigt den letzten Versand: Zustand, Sekunden seit dem Senden und die Summe", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-09T10:00:30Z"));
    render(ansicht({ simKind: "msfs2024", activeFlight: { ...flug, last_position_at: "2026-11-09T10:00:25Z", position_count: 42 } }));
    laden();
    const anzeige = document.querySelector(".live-rec") as HTMLElement;
    expect(anzeige.querySelector(".live-rec__label")?.textContent).toBe("LIVE");
    expect(anzeige.querySelector(".live-rec__detail")?.textContent).toBe("vor 5 s gesendet");
    expect(anzeige.querySelector(".live-rec__count")?.textContent?.replace(/ /g, " ")).toBe("Σ 42");
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(anzeige.querySelector(".live-rec__detail")?.textContent).toBe("vor 8 s gesendet");
  });

  it("zeigt gestaute Positionen als Sync und eine gesperrte Verbindung als Sperre", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-09T10:00:30Z"));
    const { rerender } = render(ansicht({ activeFlight: { ...flug, last_position_at: "2026-11-09T10:00:28Z", queued_position_count: 6 } }));
    laden();
    expect(text(".live-rec__label")).toBe("SYNC");
    expect(text(".live-rec__detail")).toBe("6 Positionen werden gesendet");
    rerender(ansicht({ activeFlight: { ...flug, connection_state: "blocked", last_position_at: "2026-11-09T10:00:28Z" } }));
    expect(text(".live-rec__label")).toBe("GESPERRT");
  });

  it("zeigt eine gescheiterte Sendung als Offline mit der Zahl wartender Positionen", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-09T10:00:30Z"));
    render(ansicht({ activeFlight: { ...flug, connection_state: "failing", last_position_at: "2026-11-09T10:00:28Z", queued_position_count: 9 } }));
    laden();
    expect(text(".live-rec__label")).toBe("OFFLINE");
    expect(text(".live-rec__detail")).toBe("Verbindung verloren — 9 Positionen warten");
  });

  it("rendert die Versandanzeige verdeckt gar nicht", async () => {
    const { rerender } = render(ansicht({ sichtbar: true }));
    laden();
    expect(document.querySelector(".live-rec")).not.toBeNull();
    rerender(ansicht({ sichtbar: false }));
    expect(document.querySelector(".live-rec")).toBeNull();
    rerender(ansicht({ sichtbar: true }));
    expect(document.querySelector(".live-rec")).not.toBeNull();
  });
});

describe("Namensnennung (A10)", () => {
  const nennungen = () => Object.fromEntries(h.quellen.map((q) => [q.id, String(q.spec.attribution ?? "")]));

  it("nennt Esri an den Satellitenkacheln, schon im Startstil der Karte", () => {
    render(ansicht());
    const stil = h.optionen!.style as { sources: Record<string, { attribution?: string }> };
    expect(stil.sources["esri-imagery"].attribution).toBe(
      "Imagery © Esri, Maxar, Earthstar Geographics, USDA, USGS, AeroGRID, IGN, GIS User Community",
    );
  });

  it("nennt OpenStreetMap und die ODbL an den Bodendaten", () => {
    render(ansicht());
    laden();
    expect(nennungen()["aa-ground"]).toBe("Bodendaten © OpenStreetMap contributors (ODbL)");
  });

  it("gibt die Nennung der Kartenstile (CARTO, OpenStreetMap) an die Quellen der Grundkarte weiter", async () => {
    const carto = "© CARTO, © OpenStreetMap contributors";
    const stil = (n: string) => ({
      version: 8, sprite: `https://x/${n}`,
      sources: { carto: { type: "vector", tiles: ["https://x/{z}.mvt"], attribution: carto } },
      layers: [{ id: "wasser", type: "fill", source: "carto", "source-layer": "w" }],
    });
    vi.stubGlobal("fetch", () => Promise.resolve({ ok: true, json: async () => stil("d") }));
    render(ansicht());
    laden();
    await waitFor(() => expect(h.quellen.some((q) => q.id === "grund-carto")).toBe(true));
    expect(nennungen()["grund-carto"]).toBe(carto);
  });

  it("klappt die Nennung beim ersten Auftauchen zu und laesst sie danach dem Piloten", () => {
    render(ansicht());
    laden();
    // MapLibre haengt die Nennung ein und startet (compact) aufgeklappt, sobald die erste Quelle da ist.
    const karte = document.querySelector(".aa-livemap__map") as HTMLElement;
    const nennung = document.createElement("details");
    nennung.className = "maplibregl-ctrl-attrib maplibregl-compact maplibregl-compact-show";
    nennung.setAttribute("open", "");
    karte.appendChild(nennung);
    const feuern = () => act(() => { h.handlers.filter((x) => x.ev === "sourcedata").forEach((x) => x.cb()); });
    feuern();
    expect(nennung.classList.contains("maplibregl-compact-show")).toBe(false);
    expect(nennung.hasAttribute("open")).toBe(false);
    expect(h.abgemeldet).toContain("sourcedata");
    // Klappt der Pilot sie selbst auf, schliesst die Karte sie nicht wieder.
    nennung.classList.add("maplibregl-compact-show");
    nennung.setAttribute("open", "");
    feuern();
    expect(nennung.classList.contains("maplibregl-compact-show")).toBe(true);
    expect(nennung.hasAttribute("open")).toBe(true);
  });

  it("wartet mit dem Zuklappen, bis die Nennung wirklich da ist", () => {
    render(ansicht());
    laden();
    act(() => { h.handlers.filter((x) => x.ev === "sourcedata").forEach((x) => x.cb()); });
    expect(h.abgemeldet).not.toContain("sourcedata");
    expect(h.handlers.some((x) => x.ev === "sourcedata")).toBe(true);
  });
});

describe("Verdeckter Reiter (A11)", () => {
  async function bereit(sichtbar = true) {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-09T10:00:00Z"));
    h.va = [kollege("K1", "101")];
    const r = render(ansicht({ sichtbar }));
    laden();
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    return r;
  }

  it("misst die Leinwand neu, wenn der Reiter wieder vorkommt, und nur dann", async () => {
    const r = await bereit();
    const anfang = h.resizes;
    expect(anfang).toBeGreaterThanOrEqual(1);
    r.rerender(ansicht({ sichtbar: false }));
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(h.resizes, "verdeckt wird nicht vermessen").toBe(anfang);
    r.rerender(ansicht({ sichtbar: true }));
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(h.resizes).toBe(anfang + 1);
  });

  it("macht verdeckt keinen Takt mit: weder Abrufe noch Kamera noch Uhr; sichtbar laufen sie", async () => {
    const r = await bereit();
    // Sichtbar: alle Takte laufen.
    h.rufe = [];
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
    for (const takt of ["va_live_flights", "activity_log_get", "flight_get_track", "flight_get_route_fixes", "hoppie_get_thread"]) {
      expect(h.rufe.filter((c) => c === takt).length, takt).toBeGreaterThan(0);
    }
    expect(text(".aa-livemap__header-clock")).toBe("10:00:30z");

    r.rerender(ansicht({ sichtbar: false }));
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    h.rufe = [];
    h.kamera = [];
    const uhr = text(".aa-livemap__header-clock");
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(h.rufe, "verdeckt keine Abrufe").toEqual([]);
    expect(text(".aa-livemap__header-clock")).toBe(uhr);
    // Eine neue Position bewegt die Kamera verdeckt nicht ...
    r.rerender(ansicht({ sichtbar: false, simSnapshot: { ...snap, lat: 50, lon: 10 } }));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(h.kamera).toEqual([]);
    // ... und beim Auftauchen holt die Karte alles sofort frisch und stellt die Uhr nach.
    r.rerender(ansicht({ sichtbar: true, simSnapshot: { ...snap, lat: 50, lon: 10 } }));
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    expect(h.rufe).toContain("va_live_flights");
    expect(h.kamera.length).toBeGreaterThan(0);
    expect(text(".aa-livemap__header-clock")).not.toBe(uhr);
  });
});

// ── G2 ───────────────────────────────────────────────────────────────────────
describe("Kein gemessenes Layout (G2)", () => {
  // Raster und Reihenfolge sind CSS: jsdom rechnet keine Rasterzeilen. Die Regeln werden darum aus
  // App.css gelesen; was sich am Element zeigen laesst (Inline-Masse), wird am Element geprueft.
  const css = readFileSync(resolve(__dirname, "../App.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  function regel(waehler: string): Record<string, string> {
    for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      if (m[1].split(",").map((s) => s.trim()).includes(waehler)) {
        return Object.fromEntries(m[2].split(";").map((z) => z.split(/:(.*)/s)).filter((p) => p.length > 1).map(([k, v]) => [k.trim(), v.trim()]));
      }
    }
    throw new Error(`${waehler} nicht in App.css`);
  }

  it("legt Leiste in Zeile 1 und Hinweis in Zeile 2 des Rasters, das Panel daneben", () => {
    const ui = regel(".aa-livemap-ui");
    expect(ui.display).toBe("grid");
    // Erste Zeile waechst mit der Leiste (auto), mittlere nimmt den Rest, die letzte gehoert der Systemzeile.
    expect(ui["grid-template-rows"]).toBe("auto minmax(0, 1fr) auto");
    expect(regel(".aa-livemap-controls")["grid-row"]).toBe("1");
    expect(regel(".aa-livemap-hinweis")["grid-row"]).toBe("2");
    expect(regel(".aa-livemap-panel")["grid-row"]).toBe("2");
    expect(regel(".aa-livemap-sysline")["grid-row"]).toBe("3");
  });

  it("rechnet die Lage des Hinweises nicht aus Messwerten", () => {
    expect(css).not.toContain("--aa-panel-oben");
    for (const k of ["top", "bottom", "left", "right", "position"]) {
      expect(regel(".aa-livemap-hinweis")[k], k).toBeUndefined();
      expect(regel(".aa-livemap-controls")[k], k).toBeUndefined();
    }
  });

  it("setzt an Leiste, Panel und Systemzeile keine Lage als Inline-Stil", async () => {
    h.va = [];
    render(ansicht());
    laden();
    const lage = /(^|;)\s*(top|bottom|left|right|position|transform|--aa-panel-oben)\s*:/;
    const ui = document.querySelector(".aa-livemap-ui") as HTMLElement;
    const mitStil = [...ui.querySelectorAll<HTMLElement>("[style]")].filter((e) => lage.test(e.getAttribute("style") ?? ""));
    expect(mitStil.map((e) => e.className)).toEqual([]);
    expect(ui.getAttribute("style") ?? "").not.toMatch(lage);
    // Die Zeilen des Rasters sind alle da: Leiste, Panel, Systemzeile.
    expect(ui.querySelector(".aa-livemap-controls")).not.toBeNull();
    expect(ui.querySelector(".aa-livemap-panel")).not.toBeNull();
    expect(ui.querySelector(".aa-livemap-sysline")).not.toBeNull();
  });
});
