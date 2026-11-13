// Shared Cockpit auf der Live-Karte des Clients (Konzept Shared Cockpit, Abschnitt 6b).
//
// Das GSG-Modul SharedCockpit legt in /api/acars eine Crew zu EINEM Eintrag zusammen
// (Gastgeber, mit `shared_cockpit`), der Spurtraeger-Bericht des Partners faellt raus.
// Geprueft durch die echte Komponente: Marker klicken, aufgezeichnetes setHTML lesen,
// Marker-Titel lesen, und beim Partner muss der Gastgeber-Eintrag als eigener Flug fehlen.

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, act, cleanup, waitFor } from "@testing-library/react";
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

const GASTGEBER_EINTRAG = {
  id: "PIREP-GASTGEBER",
  ident: "GSG123",
  flight_number: "123",
  dpt_airport_id: "EDDF",
  arr_airport_id: "EDDM",
  aircraft: { icao: "A320" },
  status_text: "Cruise",
  user: { name: "Anna B & Jonas W" },
  position: { lat: 49.5, lon: 10.0, gs: 430, heading: 120 },
};
const sc = (rolle: unknown, extra: Record<string, unknown> = {}) => ({
  crew_id: 7,
  status: "laeuft",
  rolle_gastgeber: rolle,
  gastgeber: { user_id: 1, pilot_id: "GSG0001", name: "Anna B" },
  partner: { user_id: 2, pilot_id: "GSG0002", name: "Jonas W" },
  fuehrender_pirep_id: "PIREP-GASTGEBER",
  spur_pirep_id: "PIREP-SPUR",
  partner_pirep_id: "PIREP-PARTNER",
  simulator: "msfs",
  ...extra,
});

const vaMarker = () => h.marker.filter((m) => !m.entfernt && m.el.className.includes("aa-ac-marker--va"));

async function karte(eintraege: unknown[], activeFlight?: { pirep_id: string }) {
  h.invokeAntworten = { va_live_flights: eintraege, activity_log_get: [], hoppie_get_thread: [] };
  const { LiveMapView } = await import("./LiveMapView");
  const flug = activeFlight
    ? { pirep_id: activeFlight.pirep_id, airline_icao: "GSG", flight_number: "999", dpt_airport: "EDDH", arr_airport: "EDDK" }
    : null;
  await act(async () => {
    render(<LiveMapView activeFlight={flug as never} />);
  });
}

/** Klickt den (einzigen) Kollegen an und gibt das HTML seiner Infokarte zurueck. */
async function kaertchen(): Promise<string> {
  await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
  await act(async () => {
    vaMarker()[0].el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  const offen = h.popups.find((p) => p.offen);
  expect(offen, "Infokarte ging nicht auf").toBeTruthy();
  return offen!.html[offen!.html.length - 1];
}

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => {}));
  h.marker = [];
  h.quellen = {};
  h.invokeRufe = [];
  h.popupsOffen = 0;
  h.popups = [];
  h.sichtbarkeit = {};
  h.ebenen = new Set<string>();
  h.einpassungen = [];
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("Shared Cockpit im Kollegen-Kaertchen", () => {
  it("zeigt die Plakette mit PF und PM, wenn der Gastgeber PF ist", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF") }]);
    const html = await kaertchen();
    expect(html).toContain('class="aa-vapop__sc"');
    expect(html).toContain(deCommon.livemap.shared_cockpit);
    expect(html).toContain("PF Anna B · PM Jonas W");
  });

  it("dreht die Rollen, wenn der Gastgeber PM ist", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PM") }]);
    const html = await kaertchen();
    expect(html).toContain("PF Jonas W · PM Anna B");
  });

  it("escaped die Namen", async () => {
    await karte([{
      ...GASTGEBER_EINTRAG,
      shared_cockpit: sc("PF", { gastgeber: { name: '<b>Anna</b> & "J"' }, partner: { name: "<img src=x>" } }),
    }]);
    const html = await kaertchen();
    expect(html).toContain("PF &lt;b&gt;Anna&lt;/b&gt; &amp; &quot;J&quot; · PM &lt;img src=x&gt;");
    expect(html).not.toContain("<b>Anna");
    expect(html).not.toContain("<img");
  });

  it("zeigt ohne shared_cockpit keine Plakette, der Pilot bleibt wie bisher", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, user: { name: "Anna B" } }]);
    const html = await kaertchen();
    expect(html).not.toContain("aa-vapop__sc");
    expect(html).toContain("Anna B");
    expect(vaMarker()[0].el.title).not.toContain("SC");
  });

  for (const [fall, wert] of [
    ["ein Text", "x"],
    ["null", null],
    ["ein Array", [1, 2]],
    ["ohne Namen", sc("PF", { gastgeber: {}, partner: { name: "" } })],
    ["ohne Partner", sc("PF", { partner: null })],
    ["mit unbekannter Rolle", sc("CPT")],
    ["ohne Rolle", sc(undefined)],
  ] as Array<[string, unknown]>) {
    it(`stuerzt bei ${fall} nicht ab und zeigt keine Plakette`, async () => {
      await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: wert }]);
      const html = await kaertchen();
      expect(html).toContain("GSG123");
      expect(html).not.toContain("aa-vapop__sc");
      expect(vaMarker()[0].el.title).not.toContain("SC");
    });
  }
});

describe("Shared Cockpit am Marker", () => {
  it("haengt dem Marker-Titel „· SC“ an", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF") }]);
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
    expect(vaMarker()[0].el.title).toBe("GSG123 · A320 · EDDF→EDDM · SC");
  });
});

describe("Eigenes Flugzeug beim Partner nicht doppelt", () => {
  const ANDERER = { ...GASTGEBER_EINTRAG, id: "PIREP-ANDERER", ident: "GSG777", user: { name: "Dritter" }, position: { lat: 52, lon: 8, gs: 300, heading: 0 } };

  it("blendet den Gastgeber-Eintrag aus, wenn spur_pirep_id meine Berichtsnummer ist", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF") }, ANDERER], { pirep_id: "PIREP-SPUR" });
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
    expect(vaMarker()[0].el.title).toContain("GSG777");
  });

  it("blendet ihn auch aus, wenn partner_pirep_id meine Berichtsnummer ist", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF") }, ANDERER], { pirep_id: "PIREP-PARTNER" });
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
    expect(vaMarker()[0].el.title).toContain("GSG777");
  });

  it("blendet ihn auch aus, wenn die Namen fehlen (Ausblenden braucht nur die Nummern)", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc(undefined, { gastgeber: null, partner: null }) }, ANDERER], { pirep_id: "PIREP-SPUR" });
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
    expect(vaMarker()[0].el.title).toContain("GSG777");
  });

  it("zeigt ihn einem fremden Piloten mit eigenem Flug weiter an", async () => {
    await karte([{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF") }, ANDERER], { pirep_id: "PIREP-FREMD" });
    await waitFor(() => expect(vaMarker().length).toBe(2), { timeout: 3000 });
    expect(vaMarker().map((m) => m.el.title).some((t) => t.includes("GSG123") && t.endsWith("· SC"))).toBe(true);
  });

  it("vergleicht leere oder fehlende Nummern nicht mit „null“", async () => {
    await karte(
      [{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF", { spur_pirep_id: null, partner_pirep_id: "" }) }],
      { pirep_id: "null" },
    );
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
  });

  it("blendet bei leerer eigener Nummer und leerer Partnernummer nicht aus", async () => {
    await karte(
      [{ ...GASTGEBER_EINTRAG, shared_cockpit: sc("PF", { spur_pirep_id: null, partner_pirep_id: "" }) }],
      { pirep_id: "" },
    );
    await waitFor(() => expect(vaMarker().length).toBe(1), { timeout: 3000 });
  });
});
