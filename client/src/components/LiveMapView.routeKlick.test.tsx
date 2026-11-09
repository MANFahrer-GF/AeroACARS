// Der Klick auf einen Kollegen — WIRKLICH geklickt, nicht nachgelesen.
//
// Thomas, 20.09.2026: „das mit dem anklicken der route geht nicht … was
// testest du eigentlich". Der Vorwurf saß: Alle bisherigen Prüfungen
// dieses Features waren Quelltext-Wächter (`quelle.indexOf(...)`) und
// Server-Tests. Keine einzige hat je auf einen Marker geklickt. Deshalb
// blieben zwei Fehler unbemerkt, die beim ersten echten Klick auffielen:
// der Server filterte auf eine Spalte, die nie gefüllt ist, und der
// Hinweis lag hinter der Kartenschalter-Leiste.
//
// Dieser Test rendert die Karte, klickt auf das Marker-Element eines
// Kollegen und sieht nach, was tatsächlich passiert:
//   1. Der Klick fragt die Route unter DIESER Kennung ab.
//   2. Kommt eine Route, wird sie gezeichnet; ein zweiter Klick nimmt
//      sie wieder weg.
//   3. Kommt keine, steht der Hinweis im Bild — sichtbar, nicht nur
//      vorhanden.

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, act, cleanup, waitFor } from "@testing-library/react";
import { karteQuelltext } from "./livekarte/quelltext.testhilfe";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const h = vi.hoisted(() => ({
  marker: [] as Array<{ el: HTMLElement; lngLat: [number, number] | null; rotation: number | null; entfernt: boolean }>,
  // Jede Infokarte mit allem, was die Karte ihr gesagt hat: setHTML ist kein Leerlauf mehr.
  popups: [] as Array<{ html: string[]; lngLat: [number, number] | null; offen: boolean }>,
  quellen: {} as Record<string, { type: string; features: unknown[] }>,
  invokeAntworten: {} as Record<string, unknown>,
  invokeRufe: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
  popupsOffen: 0,
  sichtbarkeit: {} as Record<string, string>,
  ebenen: new Set<string>(),
  // Kamera — einstellbar fuer die Datumsgrenzen-Tests.
  ausschnitt: { s: 40, n: 60, w: 0, o: 20 },
  mitte: { lng: 6, lat: 48 },
  zoom: 4,
  /** Jeder `fitBounds`-Aufruf mit den Punkten, die hineingingen. */
  einpassungen: [] as Array<Array<[number, number]>>,
}));

vi.mock("../lib/ipc", () => ({
  isTauri: true,
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.invokeRufe.push({ cmd, args });
    const a = h.invokeAntworten[cmd];
    if (a instanceof Error) return Promise.reject(a);
    return Promise.resolve(a ?? null);
  },
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    el: HTMLElement;
    lngLat: [number, number] | null = null;
    rotation: number | null = null;
    entfernt = false;
    constructor(opts?: { element?: HTMLElement }) {
      this.el = opts?.element ?? document.createElement("div");
      h.marker.push(this);
    }
    setLngLat(ll: [number, number]) { this.lngLat = ll; return this; }
    setRotation(r: number) { this.rotation = r; return this; }
    addTo() { return this; }
    remove() { this.entfernt = true; return this; }
    getElement() { return this.el; }
  }
  class FakePopup {
    rec = { html: [] as string[], lngLat: null as [number, number] | null, offen: false };
    constructor() { h.popups.push(this.rec); }
    setLngLat(ll: [number, number]) { this.rec.lngLat = ll; return this; }
    setHTML(html: string) { this.rec.html.push(html); return this; }
    addTo() { h.popupsOffen += 1; this.rec.offen = true; return this; }
    remove() { if (h.popupsOffen > 0) h.popupsOffen -= 1; this.rec.offen = false; return this; }
    on() { return this; }
  }
  class FakeBounds {
    punkte: Array<[number, number]> = [];
    constructor(a?: [number, number]) { if (a) this.punkte.push(a); }
    extend(p: [number, number]) { this.punkte.push(p); return this; }
  }
  class FakeMap {
    center = { lng: 6, lat: 48 };
    zoom = 4;
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) {
      if (ev === "load") cb();
      return this;
    }
    once() { return this; }
    off() { return this; }
    getBounds() {
      const a = h.ausschnitt;
      return {
        getSouth: () => a.s, getNorth: () => a.n, getWest: () => a.w, getEast: () => a.o,
      };
    }
    getCenter() { return h.mitte; }
    getZoom() { return h.zoom; }
    isStyleLoaded() { return true; }
    // Quellen und Ebenen entstehen erst durch addSource/addLayer.
    //
    // Der erste Nachbau meldete auf `getSource`/`getLayer` IMMER etwas —
    // damit lief `if (!map.getSource("fremde-routen"))` nie, der echte
    // Anlagepfad wurde uebersprungen, und die Tests waeren gruen
    // geblieben, obwohl die Ebenen nie angelegt werden (Codex-Abnahme
    // 20.09.2026).
    addSource(id: string) {
      h.quellen[id] = { type: "FeatureCollection", features: [] };
      return this;
    }
    hasImage() { return false; }
    addImage() { return this; }
    addLayer(spec: { id: string }) {
      h.ebenen.add(spec.id);
      return this;
    }
    getSource(id: string) {
      if (!(id in h.quellen)) return undefined;
      return {
        setData: (d: { type: string; features: unknown[] }) => { h.quellen[id] = d; },
      };
    }
    getLayer(id: string) {
      return h.ebenen.has(id) ? { id } : undefined;
    }
    setPaintProperty() { return this; }
    setLayoutProperty(id: string, _n: string, wert: string) {
      h.sichtbarkeit[id] = wert;
      return this;
    }
    easeTo() { return this; }
    jumpTo() { return this; }
    fitBounds(b: { punkte?: Array<[number, number]> }) {
      h.einpassungen.push([...(b?.punkte ?? [])]);
      return this;
    }
    // Die Komponente ruft `resize()` nach dem Einblenden. Fehlt die
    // Methode, wirft der Aufruf in einem requestAnimationFrame — als
    // UNBEHANDELTE Rejection, die den ganzen Lauf mit Rueckgabewert 1
    // enden laesst, waehrend die Zusammenfassung "grün" meldet
    // (Abnahme 20.09.2026).
    resize() { return this; }
    remove() { return this; }
  }
  return {
    default: {
      Map: FakeMap, Marker: FakeMarker, Popup: FakePopup,
      LngLatBounds: FakeBounds, NavigationControl: class {},
    },
  };
});

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

/** So liefert `fremde_flugroute` einen Wegpunkt — mit Namen. */
const P1 = { lon: 11.1, lat: 60.2, name: "GM604" };
const P2 = { lon: 16.5, lat: 68.5, name: "ENEV" };

const KOLLEGE = {
  id: "PIREP-KOLLEGE",
  ident: "SK4084",
  flight_number: "4084",
  dpt_airport_id: "ENGM",
  arr_airport_id: "ENEV",
  aircraft: { icao: "A319" },
  status_text: "Cruise",
  position: { lat: 60.2, lon: 11.1, gs: 420, heading: 20 },
};

/** Das Marker-Element des Kollegen — dasselbe, das der Pilot anklickt. */
function kollegenMarker(): HTMLElement | null {
  return (
    h.marker.map((m) => m.el).find((el) => el.className.includes("aa-ac-marker--va")) ?? null
  );
}

async function karteMitKollege(routeAntwort: unknown) {
  h.invokeAntworten = {
    va_live_flights: [KOLLEGE],
    fremde_flugroute: routeAntwort,
  };
  const { LiveMapView } = await import("./LiveMapView");
  await act(async () => {
    render(<LiveMapView />);
  });
  await waitFor(() => expect(kollegenMarker()).not.toBeNull(), { timeout: 3000 });
}

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => {})); // Stile der Grundkarte: eigener Test
  h.marker = [];
  h.quellen = {};
  h.invokeRufe = [];
  h.popupsOffen = 0;
  h.popups = [];
  h.sichtbarkeit = {};
  h.ebenen = new Set<string>();
  h.ausschnitt = { s: 40, n: 60, w: 0, o: 20 };
  h.mitte = { lng: 6, lat: 48 };
  h.zoom = 4;
  h.einpassungen = [];
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("Klick auf einen Kollegen", () => {
  it("fragt die Route unter seiner Kennung ab", async () => {
    await karteMitKollege([P1, P2]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    // Die drei Ebenen muessen real angelegt worden sein — sonst prueft
    // alles Weitere eine Karte ohne Darstellung (Codex-Abnahme).
    for (const ebene of [
      "fremde-routen-line",
      "fremde-routen-punkte",
      "fremde-routen-namen",
    ]) {
      expect(h.ebenen.has(ebene), `${ebene} wurde nie angelegt`).toBe(true);
    }
    const ruf = h.invokeRufe.find((r) => r.cmd === "fremde_flugroute");
    expect(ruf, "der Klick hat die Route nie abgefragt").toBeTruthy();
    // Unter DIESER Kennung — nicht unter der Flugnummer oder leer.
    expect(ruf!.args?.pirepId).toBe("PIREP-KOLLEGE");
  });

  it("zeichnet die Route und nimmt sie beim zweiten Klick wieder weg", async () => {
    await karteMitKollege([P1, P2]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const linien = () =>
      (h.quellen["fremde-routen"]?.features ?? []).filter(
        (x) => (x as { geometry: { type: string } }).geometry.type === "LineString",
      ).length;
    await waitFor(() => expect(linien()).toBe(1));
    // Die Koordinaten muessen wirklich ankommen. Nur die Anzahl zu
    // zaehlen liess eine Typaenderung durchgehen, bei der
    // [undefined, undefined] gezeichnet worden waere (20.09.2026).
    const linie = (h.quellen["fremde-routen"]!.features as Array<{
      geometry: { type: string; coordinates: number[][] };
    }>).find((f) => f.geometry.type === "LineString")!;
    expect(linie.geometry.coordinates).toEqual([
      [P1.lon, P1.lat],
      [P2.lon, P2.lat],
    ]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    // Beim Ausschalten geht ALLES weg, Linie wie Punkte.
    await waitFor(() => expect(h.quellen["fremde-routen"]?.features.length).toBe(0));
  });

  it("zeichnet eine Kollegen-Route ueber die Datumsgrenze nicht quer ueber die Welt", async () => {
    // Aleuten, 25.09.2026: 179° O → 179° W lief als waagerechte Linie
    // ueber die ganze Karte. Die Laenge wird weitergezaehlt (181°).
    const OST = { lon: 179, lat: 52, name: "OST" };
    const WEST = { lon: -179, lat: 53, name: "WEST" };
    await karteMitKollege([OST, WEST]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() =>
      expect(
        (h.quellen["fremde-routen"]?.features ?? []).some(
          (x) => (x as { geometry: { type: string } }).geometry.type === "LineString",
        ),
      ).toBe(true),
    );
    const linie = (h.quellen["fremde-routen"]!.features as Array<{
      geometry: { type: string; coordinates: number[][] };
    }>).find((f) => f.geometry.type === "LineString")!;
    expect(linie.geometry.coordinates).toEqual([
      [179, 52],
      [181, 53],
    ]);
  });

  it("zeichnet ZWEI Kollegen gleichzeitig, nicht nur den letzten", async () => {
    // Der Quelltext-Test dazu prueft nur, dass `.entries()` vorkommt —
    // eine Fassung, die trotzdem nur einen Eintrag zeichnet, bliebe dort
    // gruen (Codex-Abnahme 20.09.2026). Hier werden wirklich zwei
    // Kollegen angeklickt.
    const ZWEITER = {
      ...KOLLEGE,
      id: "PIREP-ZWEITER",
      ident: "GSG9",
      position: { lat: 52.0, lon: 9.0, gs: 380, heading: 90 },
    };
    h.invokeAntworten = {
      va_live_flights: [KOLLEGE, ZWEITER],
      fremde_flugroute: [P1, P2],
    };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => {
      render(<LiveMapView />);
    });
    await waitFor(() => {
      const marker = h.marker.filter((m) =>
        m.el.className.includes("aa-ac-marker--va"),
      );
      expect(marker.length).toBe(2);
    });
    const marker = h.marker.filter((m) =>
      m.el.className.includes("aa-ac-marker--va"),
    );
    for (const m of marker) {
      await act(async () => {
        m.el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      });
    }
    await waitFor(() => {
      const linien = (h.quellen["fremde-routen"]?.features ?? []).filter(
        (x) => (x as { geometry: { type: string } }).geometry.type === "LineString",
      );
      expect(linien.length, "nur eine Route gezeichnet").toBe(2);
      // Und beide gehoeren verschiedenen Fluegen.
      const ids = new Set(
        linien.map((x) => (x as { properties: { pirep_id: string } }).properties.pirep_id),
      );
      expect(ids.size).toBe(2);
    });
  });

  it("zeichnet die Wegpunkte mit, nicht nur den Strich", async () => {
    // Thomas, 20.09.2026: „die Route ist sichtbar ohne Punkte die zu
    // sehen sind". Zu einer Route gehoeren ihre Fixe.
    await karteMitKollege([P1, { lon: 13.0, lat: 64.0, name: "VIBUK" }, P2]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() => {
      const f = h.quellen["fremde-routen"]?.features ?? [];
      const linien = f.filter((x) => (x as { geometry: { type: string } }).geometry.type === "LineString");
      const punkte = f.filter((x) => (x as { geometry: { type: string } }).geometry.type === "Point");
      expect(linien.length).toBe(1);
      expect(punkte.length).toBe(3);
      // Mit Namen — sonst stehen unbeschriftete Punkte auf der Karte.
      const namen = punkte.map(
        (x) => (x as { properties: { name?: string } }).properties.name,
      );
      expect(namen).toEqual(["GM604", "VIBUK", "ENEV"]);
    });
  });

  it("baut die Kollegen-Marke beim naechsten Abruf nicht neu (Tablet: Tippen trifft)", async () => {
    const takt = vi.spyOn(globalThis, "setInterval");
    await karteMitKollege([P1, P2]);
    const vorher = kollegenMarker();
    const abruf = takt.mock.calls.find((c) => c[1] === 8000)?.[0] as () => void;
    expect(abruf).toBeTypeOf("function");
    h.invokeAntworten.va_live_flights = [
      { ...KOLLEGE, position: { ...KOLLEGE.position, lat: 61 } },
    ];
    await act(async () => {
      abruf();
      await Promise.resolve();
      await Promise.resolve();
    });
    takt.mockRestore();
    expect(kollegenMarker()).toBe(vorher);
    expect(h.marker.filter((m) => m.el.className.includes("aa-ac-marker--va"))).toHaveLength(1);
  });

  it("schaltet beim zweiten Klick auch die Infokarte wieder zu", async () => {
    // Vorher ging sie erneut auf und verdeckte die Route, und man musste
    // woanders hinklicken, um sie loszuwerden.
    await karteMitKollege([P1, P2]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(h.popupsOffen).toBe(1);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(h.popupsOffen).toBe(0);
  });

  it("versteckt Linie UND Punkte, wenn die Kollegen-Anzeige ausgeht", async () => {
    await karteMitKollege([P1, P2]);
    const schalter = screen.getByRole("button", { name: /VA-Verkehr/i });
    await act(async () => {
      schalter.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() => {
      expect(h.sichtbarkeit["fremde-routen-line"]).toBe("none");
      expect(h.sichtbarkeit["fremde-routen-punkte"]).toBe("none");
      // Auch die BESCHRIFTUNG. Fiel sie aus FREMDE_ROUTEN_EBENEN
      // heraus, blieben die Wegpunkt-Namen als Geisterschrift stehen,
      // waehrend Linie und Punkte verschwinden — und alle Tests blieben
      // gruen (Abnahme 20.09.2026, nachgemessen per Mutation).
      expect(h.sichtbarkeit["fremde-routen-namen"]).toBe("none");
    });
  });

  it("versteckt JEDE Ebene der Kollegen-Routen, keine bleibt uebrig", () => {
    // Nicht nur die drei von heute: Wer morgen eine vierte Ebene anlegt
    // (etwa eine Umrandung), muss sie in FREMDE_ROUTEN_EBENEN eintragen.
    // Sonst bleibt sie beim Ausschalten stehen. Der Vergleich laeuft
    // gegen den Quelltext, weil ein Rendertest eine Ebene, die es noch
    // nicht gibt, nicht kennen kann.
    const quelle = karteQuelltext();
    const angelegt = [...quelle.matchAll(/id: "(fremde-routen-[a-z-]+)"/g)].map(
      (m) => m[1],
    );
    const listeRoh = quelle.slice(
      quelle.indexOf("const FREMDE_ROUTEN_EBENEN"),
      quelle.indexOf("];", quelle.indexOf("const FREMDE_ROUTEN_EBENEN")),
    );
    const gefuehrt = [...listeRoh.matchAll(/"(fremde-routen-[a-z-]+)"/g)].map(
      (m) => m[1],
    );
    expect(angelegt.length).toBeGreaterThanOrEqual(3);
    for (const ebene of new Set(angelegt)) {
      expect(gefuehrt, `${ebene} fehlt in FREMDE_ROUTEN_EBENEN`).toContain(ebene);
    }
  });

  it("sagt es sichtbar, wenn keine Route vorliegt", async () => {
    // Genau der Fall aus dem Betrieb: Der Server antwortet mit 200 und
    // einer leeren Liste. Bis 20.09.2026 lag der Hinweis dazu hinter der
    // Kartenschalter-Leiste — der Klick sah aus, als tue er nichts.
    await karteMitKollege([]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const hinweis = await screen.findByRole("status");
    expect(hinweis.textContent).toMatch(/keine Route/i);
    // Und er steht im Raster unter der Leiste (Lage: LiveMapView.hinweis.test.ts).
    expect(hinweis.className).toContain("aa-livemap-hinweis");
  });
});

describe("Datumsgrenze im Kartenausschnitt", () => {
  it("passt Kollegen beiderseits der Datumsgrenze auf einen kleinen Ausschnitt ein", async () => {
    // Zwei Flieger bei 179° und −179°: 2° auseinander, nicht 358°.
    const WEST = { ...KOLLEGE, id: "PIREP-WEST", position: { ...KOLLEGE.position, lat: 52, lon: 179 } };
    const OST = { ...KOLLEGE, id: "PIREP-OST", ident: "SK1", position: { ...KOLLEGE.position, lat: 53, lon: -179 } };
    h.invokeAntworten = { va_live_flights: [WEST, OST] };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => {
      render(<LiveMapView />);
    });
    await waitFor(() => expect(h.einpassungen.length).toBeGreaterThan(0), { timeout: 3000 });
    const laengen = h.einpassungen[0]!.map((p) => p[0]);
    expect(Math.max(...laengen) - Math.min(...laengen)).toBeLessThanOrEqual(2);
  });

  it("laedt Rollwege eines Platzes jenseits der Datumsgrenze, wenn er im Bild ist", async () => {
    // Ausschnitt 175…185° (Karte ueber die Grenze geschoben), Adak bei −176,6°.
    h.zoom = 12;
    h.ausschnitt = { s: 45, n: 58, w: 175, o: 185 };
    h.mitte = { lng: 180, lat: 51.5 };
    h.invokeAntworten = {
      va_live_flights: [],
      airport_ground_index: [{ icao: "PADK", lat: 51.88, lon: -176.65 }],
      airport_ground_get: null,
    };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => {
      render(<LiveMapView />);
    });
    await waitFor(
      () =>
        expect(
          h.invokeRufe.some((r) => r.cmd === "airport_ground_get" && r.args?.icao === "PADK"),
        ).toBe(true),
      { timeout: 3000 },
    );
  });
});

describe("Bodendaten: Lade-Schwelle", () => {
  async function karteBeiZoom(zoom: number) {
    h.zoom = zoom;
    h.ausschnitt = { s: 49, n: 51, w: 7, o: 10 };
    h.mitte = { lng: 8.57, lat: 50.03 };
    h.invokeAntworten = {
      va_live_flights: [],
      airport_ground_index: [{ icao: "EDDF", lat: 50.03, lon: 8.57 }],
      airport_ground_get: null,
    };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => {
      render(<LiveMapView />);
    });
    // Ohne geladenen Index prueft die Karte gar nichts; erst danach sagt das Ausbleiben etwas aus.
    await waitFor(
      () => expect(h.invokeRufe.some((r) => r.cmd === "airport_ground_index")).toBe(true),
      { timeout: 3000 },
    );
    await act(async () => {
      await new Promise((fertig) => setTimeout(fertig, 50));
    });
    return h.invokeRufe.filter((r) => r.cmd === "airport_ground_get").map((r) => r.args?.icao);
  }

  it("laedt ab Zoom 11", async () => {
    expect(await karteBeiZoom(11)).toEqual(["EDDF"]);
  });

  it("laedt darunter noch nicht", async () => {
    expect(await karteBeiZoom(10.9)).toEqual([]);
  });
});

// ── Abnahmeliste D1, D2, D4, D5: die Kollegen auf der Karte, durch die Komponente ────────────
describe("VA-Verkehr: Abnahme D1, D2, D4, D5", () => {
  const ZWEITER = {
    ...KOLLEGE, id: "PIREP-ZWEITER", ident: "GSG9",
    position: { lat: 52.0, lon: 9.0, gs: 10, heading: 90 }, // rollt: wird nicht fortgeschrieben
  };
  const lebendeVa = () => h.marker.filter((m) => m.el.className.includes("aa-ac-marker--va") && !m.entfernt);
  const linien = () =>
    (h.quellen["fremde-routen"]?.features ?? []).filter((x) => (x as { geometry: { type: string } }).geometry.type === "LineString") as Array<{ properties: { pirep_id: string } }>;
  const eigenerFlug = {
    pirep_id: "PIREP-KOLLEGE", airline_icao: "SKY", flight_number: "4084", dpt_airport: "ENGM", arr_airport: "ENEV",
    phase: "cruise", distance_nm: 10, was_just_resumed: false,
  };
  async function fluss(ms = 20) {
    await act(async () => { await new Promise((r) => setTimeout(r, ms)); });
  }
  /** Karte mit Kollegen; die Takte der Karte und die Uhr stehen unter der Kontrolle des Tests. */
  async function startMitUhr(kollegen: unknown[], props: Record<string, unknown> = {}) {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    vi.setSystemTime(new Date("2026-11-09T10:00:00Z"));
    h.invokeAntworten = { va_live_flights: kollegen, fremde_flugroute: [P1, P2], activity_log_get: [], hoppie_get_thread: [] };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => { render(<LiveMapView {...(props as object)} />); });
    await fluss();
  }
  const abrufe = () => h.invokeRufe.filter((r) => r.cmd === "va_live_flights").length;

  it("D1: ruft die Kollegen alle 8 Sekunden ab, nicht frueher", async () => {
    await startMitUhr([KOLLEGE]);
    expect(abrufe()).toBe(1);
    await act(async () => { vi.advanceTimersByTime(7_999); });
    await fluss();
    expect(abrufe()).toBe(1);
    await act(async () => { vi.advanceTimersByTime(1); });
    await fluss();
    expect(abrufe()).toBe(2);
  });

  it("D1: zeigt den eigenen Flug nicht doppelt, nur die anderen Kollegen", async () => {
    await startMitUhr([KOLLEGE, ZWEITER], { activeFlight: eigenerFlug, simSnapshot: null });
    expect(lebendeVa()).toHaveLength(1);
    expect(lebendeVa()[0].el.title).toContain("GSG9");
  });

  it("D1: rechnet die Kollegen zwischen den Abrufen mit Kurs und Geschwindigkeit weiter und rastet beim naechsten Abruf ein", async () => {
    await startMitUhr([KOLLEGE, ZWEITER]);
    const [schnell, langsam] = lebendeVa();
    expect(schnell.lngLat).toEqual([11.1, 60.2]);
    expect(schnell.rotation).toBe(20);
    // 5 s mit 420 kt auf 020°: 0,5833 nm, davon 0,0091° nach Norden und 0,0067° nach Osten.
    await act(async () => { vi.advanceTimersByTime(5_000); });
    expect(schnell.lngLat![0]).toBeCloseTo(11.1066908, 4);
    expect(schnell.lngLat![1]).toBeCloseTo(60.2091359, 4);
    expect(langsam.lngLat, "unter 30 kt wird nicht fortgeschrieben").toEqual([9.0, 52.0]);
    // Der naechste Abruf nennt die wahre Position, die Marke rastet darauf ein.
    h.invokeAntworten.va_live_flights = [{ ...KOLLEGE, position: { ...KOLLEGE.position, lat: 60.5, heading: 45 } }, ZWEITER];
    await act(async () => { vi.advanceTimersByTime(3_000); });
    await fluss();
    expect(schnell.lngLat).toEqual([11.1, 60.5]);
    expect(schnell.rotation).toBe(45);
  });

  it("D2: die Infokarte nennt Rufzeichen, Pilot, Route, Phase, Hoehe, GS, ETA und Muster", async () => {
    // Zielplatz genau 1° nördlich: 60 nm bei 420 kt = 9 Minuten nach 10:00z.
    h.invokeAntworten = {
      va_live_flights: [{
        ...KOLLEGE, user: { name: "Kari Nordmann" }, aircraft: { icao: "A319", registration: "D-ABCD" },
        position: { lat: 60.2, lon: 11.1, gs: 420, heading: 20, altitude_msl: 36000 },
      }],
      fremde_flugroute: [P1, P2],
      airport_get: { lat: 61.2, lon: 11.1 },
    };
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-11-09T10:00:00Z"));
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => { render(<LiveMapView />); });
    await waitFor(() => expect(kollegenMarker()).not.toBeNull(), { timeout: 3000 });
    await act(async () => { kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await fluss();
    const karte = h.popups[0];
    expect(h.popups).toHaveLength(1);
    expect(karte.lngLat).toEqual([11.1, 60.2]);
    // Zuerst ohne ETA ("—"), weil der Zielplatz erst nachgeschlagen wird ...
    expect(karte.html[0]).toContain('ETA</span><span class="aa-vapop__v">—');
    // ... dann mit allem.
    const html = karte.html.at(-1)!;
    for (const teil of ["SK4084", "Kari Nordmann", "ENGM", "ENEV", "Cruise", "FL360", ">420<", "A319 · D-ABCD"]) expect(html, teil).toContain(teil);
    expect(html).toContain('ETA</span><span class="aa-vapop__v">10:09z');
  });

  it.each([
    ["en", "ALT"],
    ["it", "QUOTA"],
  ])("G8: die Infokarte des Kollegen beschriftet die Hoehe in der Sprache %s", async (sprache, hoehe) => {
    await i18next.changeLanguage(sprache);
    try {
      await karteMitKollege([P1, P2]);
      await act(async () => { kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
      await fluss();
      const html = h.popups[0].html.at(-1)!;
      expect(html).toContain(`${hoehe}</span><span class="aa-vapop__v">`);
      expect(html).not.toContain("HÖHE");
    } finally {
      await i18next.changeLanguage("de");
    }
  });

  it("D2: die offene Infokarte folgt dem Kollegen beim Neuaufbau und schliesst, wenn er weg ist", async () => {
    const takt = vi.spyOn(globalThis, "setInterval");
    await karteMitKollege([P1, P2]);
    const abruf = takt.mock.calls.find((c) => c[1] === 8000)?.[0] as () => void;
    takt.mockRestore();
    await act(async () => { kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    const karte = h.popups[0];
    expect(karte.html.at(-1)).toContain('HÖHE</span><span class="aa-vapop__v">—'); // noch ohne Hoehe
    h.invokeAntworten.va_live_flights = [{ ...KOLLEGE, position: { ...KOLLEGE.position, lat: 60.6, altitude_msl: 35000 } }];
    await act(async () => { abruf(); await Promise.resolve(); await Promise.resolve(); });
    await fluss();
    expect(h.popups, "dieselbe Infokarte, nicht eine neue").toHaveLength(1);
    expect(karte.lngLat).toEqual([11.1, 60.6]);
    expect(karte.html.at(-1)).toContain("FL350");
    // Der Kollege ist gelandet, ein anderer fliegt noch: die Infokarte schliesst trotzdem.
    h.invokeAntworten.va_live_flights = [ZWEITER];
    await act(async () => { abruf(); await Promise.resolve(); await Promise.resolve(); });
    await fluss();
    expect(karte.offen).toBe(false);
  });

  /** Beide Kollegen anklicken; danach liegen zwei Routen auf der Karte. */
  async function zweiRouten() {
    const takt = vi.spyOn(globalThis, "setInterval");
    h.invokeAntworten = { va_live_flights: [KOLLEGE, ZWEITER], fremde_flugroute: [P1, P2] };
    const { LiveMapView } = await import("./LiveMapView");
    await act(async () => { render(<LiveMapView />); });
    await waitFor(() => expect(lebendeVa()).toHaveLength(2), { timeout: 3000 });
    const abruf = takt.mock.calls.find((c) => c[1] === 8000)?.[0] as () => void;
    takt.mockRestore();
    for (const m of lebendeVa()) await act(async () => { m.el.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await waitFor(() => expect(linien()).toHaveLength(2));
    const poll = async () => { await act(async () => { abruf(); await Promise.resolve(); await Promise.resolve(); }); await fluss(); };
    return { poll };
  }

  it("D4: ein gelandeter Kollege verliert seine Route, ein fliegender behaelt sie, auch der letzte raeumt auf", async () => {
    const { poll } = await zweiRouten();
    h.invokeAntworten.va_live_flights = [ZWEITER];
    await poll();
    expect(linien().map((l) => l.properties.pirep_id)).toEqual(["PIREP-ZWEITER"]);
    h.invokeAntworten.va_live_flights = [];
    await poll();
    expect(linien()).toHaveLength(0);
  });

  it("D4: ein fehlgeschlagener Abruf laesst die Routen stehen", async () => {
    const { poll } = await zweiRouten();
    h.invokeAntworten.va_live_flights = new Error("Netz weg");
    await poll();
    expect(linien()).toHaveLength(2);
  });

  it("D4: Ausschalten und Wiedereinschalten laesst die Routen stehen", async () => {
    await zweiRouten();
    const schalter = screen.getByRole("button", { name: /VA-Verkehr/i });
    await act(async () => { schalter.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await fluss();
    expect(linien(), "aus").toHaveLength(2);
    await act(async () => { schalter.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await fluss();
    expect(linien(), "wieder an").toHaveLength(2);
    expect(lebendeVa(), "die Kollegen sind wieder da").toHaveLength(2);
  });

  it("D5: der Schalter VA-Verkehr nimmt Marken und Infokarte weg und bringt sie mit den Routen zurueck", async () => {
    await karteMitKollege([P1, P2]);
    await act(async () => { kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await waitFor(() => expect(linien()).toHaveLength(1));
    expect(lebendeVa()).toHaveLength(1);
    expect(h.popupsOffen).toBe(1);
    const schalter = screen.getByRole("button", { name: /VA-Verkehr/i });
    await act(async () => { schalter.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await waitFor(() => expect(lebendeVa()).toHaveLength(0));
    expect(h.popupsOffen).toBe(0);
    expect(h.sichtbarkeit["fremde-routen-line"]).toBe("none");
    await act(async () => { schalter.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await waitFor(() => expect(lebendeVa()).toHaveLength(1));
    expect(h.sichtbarkeit["fremde-routen-line"]).toBe("visible");
    expect(h.sichtbarkeit["fremde-routen-punkte"]).toBe("visible");
    expect(h.sichtbarkeit["fremde-routen-namen"]).toBe("visible");
  });
});
