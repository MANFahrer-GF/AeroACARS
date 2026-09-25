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
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const h = vi.hoisted(() => ({
  marker: [] as Array<{ el: HTMLElement }>,
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
    constructor(opts?: { element?: HTMLElement }) {
      this.el = opts?.element ?? document.createElement("div");
      h.marker.push(this);
    }
    setLngLat() { return this; }
    setRotation() { return this; }
    addTo() { return this; }
    remove() { return this; }
    getElement() { return this.el; }
  }
  class FakePopup {
    setLngLat() { return this; }
    setHTML() { return this; }
    addTo() { h.popupsOffen += 1; return this; }
    remove() { if (h.popupsOffen > 0) h.popupsOffen -= 1; return this; }
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
      if (ev === "load" || ev === "styledata") cb();
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
    setStyle() { return this; }
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
  h.marker = [];
  h.quellen = {};
  h.invokeRufe = [];
  h.popupsOffen = 0;
  h.sichtbarkeit = {};
  h.ebenen = new Set<string>();
  h.ausschnitt = { s: 40, n: 60, w: 0, o: 20 };
  h.mitte = { lng: 6, lat: 48 };
  h.zoom = 4;
  h.einpassungen = [];
});
afterEach(() => cleanup());

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
    const quelle = readFileSync(
      resolve(__dirname, "LiveMapView.tsx"),
      "utf8",
    );
    const angelegt = [...quelle.matchAll(/id: "(fremde-routen-[a-z-]+)"/g)].map(
      (m) => m[1],
    );
    const listeRoh = quelle.slice(
      quelle.indexOf("const FREMDE_ROUTEN_EBENEN"),
      quelle.indexOf("const TRACK_LAYERS"),
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
    // Und er muss oben drauf liegen: Die Leiste traegt z-index 500.
    expect(Number(hinweis.style.zIndex)).toBeGreaterThan(500);
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
