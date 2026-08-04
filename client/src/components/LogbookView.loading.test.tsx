// Ladeanzeige beim Öffnen eines Fluges.
//
// Feldbefund Thomas, 25.07.2026: „warum das so lange dauert, das Logbuch zu
// öffnen … dann müssen wir irgendwas einbauen, damit der User sieht, dass es
// geladen wird, dass man da nicht zwanzigmal draufklickt."
//
// Gemessen war es kein Gefühl: Der Detailabruf brauchte für einen
// Langstreckenflug 7,1 s, für einen durchschnittlichen 3–4 s. Serverseitig ist
// das inzwischen auf 2,9 s bzw. 1,7–2,0 s gefallen (`GsgLogbook`-Controller),
// aber ein bis drei Sekunden bleiben — der phpVMS-Start allein kostet 1,3 s.
//
// Sichtbar war davon nichts. `openDetail()` setzte zwar einen Ladezustand, aber
// der einzige Ort, der ihn anzeigte, war der Text im Blätterbalken UNTER der
// Tabelle. Wer auf eine Zeile klickte, sah oben nichts passieren — und klickte
// erneut. Jeder weitere Klick schickte eine weitere Abfrage los.
//
// Diese Tests halten beides fest: dass die Rückmeldung SOFORT kommt (nicht erst
// mit der Antwort), und dass ein zweiter Klick keine zweite Abfrage auslöst.

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

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
  /** Aufgelöst von Hand, damit der Zustand WÄHREND des Ladens prüfbar ist. */
  pending: [] as Array<(v: unknown) => void>,
  calls: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
}));

vi.mock("../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.calls.push({ cmd, args });
    if (cmd === "logbook_stats") return Promise.resolve({ total_flights: 2 });
    if (cmd === "logbook_pireps") {
      return Promise.resolve({
        total: 2,
        items: [
          { id: "AAA", date: "2026-07-20T10:00:00Z", dep_icao: "EDDF", arr_icao: "LEPA", callsign: "GSG101", aircraft_icao: "B738", aircraft_reg: "D-ABAA", duration_min: 130, distance_nm: 800, landing_rate_fpm: -180, status: "accepted" },
          { id: "BBB", date: "2026-07-19T10:00:00Z", dep_icao: "KCLT", arr_icao: "EDDM", callsign: "GSG102", aircraft_icao: "A359", aircraft_reg: "D-ABAB", duration_min: 520, distance_nm: 4099, landing_rate_fpm: -403, status: "accepted" },
        ],
      });
    }
    // Der Detailabruf bleibt offen, bis der Test ihn auflöst.
    return new Promise((resolve) => h.pending.push(resolve));
  },
}));

vi.mock("maplibre-gl", () => {
  class FakeMap {
    on() { return this; }
    remove() {}
    addSource() {}
    addLayer() {}
    fitBounds() {}
  }
  class FakeMarker {
    setLngLat() { return this; }
    addTo() { return this; }
  }
  class FakeBounds {
    extend() { return this; }
  }
  return {
    default: { Map: FakeMap, Marker: FakeMarker, LngLatBounds: FakeBounds, NavigationControl: class {} },
    Map: FakeMap, Marker: FakeMarker, LngLatBounds: FakeBounds,
  };
});
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));
vi.mock("./FlightProfile", () => ({ FlightProfile: () => null }));

import { LogbookView } from "./LogbookView";

/** Wartet, bis React die aufgelösten Promises verarbeitet hat. */
const settle = async () => { await act(async () => { await Promise.resolve(); }); };

const rowFor = (callsign: string) => screen.getByText(callsign).closest("tr")!;

describe("Ladeanzeige beim Öffnen eines Fluges", () => {
  beforeEach(async () => {
    h.pending.length = 0;
    h.calls.length = 0;
    render(<LogbookView />);
    await settle();
  });
  afterEach(cleanup);

  it("zeigt die Rückmeldung sofort beim Klick, nicht erst mit der Antwort", async () => {
    const row = rowFor("GSG102");
    expect(row.className, "vor dem Klick ist nichts markiert").not.toContain("is-opening");
    expect(document.querySelector(".aa-lb-spin")).toBeNull();

    fireEvent.click(row);
    await settle(); // Antwort steht noch aus — der Abruf hängt bewusst.

    expect(rowFor("GSG102").className, "die angeklickte Zeile hebt sich ab").toContain("is-opening");
    expect(
      within(rowFor("GSG102")).getByLabelText("lädt"),
      "der Kreisel sitzt in der angeklickten Zeile, nicht irgendwo auf der Seite",
    ).toBeInTheDocument();
    expect(document.querySelector(".aa-lb-progress"), "Balken an der Kartenoberkante").not.toBeNull();
  });

  it("markiert nur die angeklickte Zeile", async () => {
    fireEvent.click(rowFor("GSG102"));
    await settle();
    expect(rowFor("GSG101").className).not.toContain("is-opening");
    expect(within(rowFor("GSG101")).queryByLabelText("lädt")).toBeNull();
  });

  // Der eigentliche Grund für den Befund: „dass man da nicht zwanzigmal
  // draufklickt". Klicken darf man — es darf nur nichts auslösen.
  it("verwirft weitere Klicks, solange ein Abruf läuft", async () => {
    const detailCalls = () => h.calls.filter((c) => c.cmd === "logbook_pirep").length;

    fireEvent.click(rowFor("GSG102"));
    await settle();
    expect(detailCalls()).toBe(1);

    fireEvent.click(rowFor("GSG102"));
    fireEvent.click(rowFor("GSG102"));
    fireEvent.click(rowFor("GSG101")); // auch ein ANDERER Flug nicht
    await settle();

    expect(detailCalls(), "ein laufender Abruf blockiert weitere").toBe(1);
  });

  it("nimmt die Anzeige zurück, sobald der Flug da ist", async () => {
    fireEvent.click(rowFor("GSG102"));
    await settle();
    expect(h.pending).toHaveLength(1);

    await act(async () => {
      h.pending[0]({ id: "BBB", callsign: "GSG102", dep_icao: "KCLT", arr_icao: "EDDM", route: [], log: [] });
      await Promise.resolve();
    });

    expect(document.querySelector(".aa-lb-spin"), "Kreisel weg").toBeNull();
    expect(document.querySelector(".aa-lb-progress"), "Balken weg").toBeNull();
    expect(screen.getByText("← Logbuch"), "die Detailansicht steht").toBeInTheDocument();
  });

  // Scheitert der Abruf, darf die Anzeige nicht ewig weiterlaufen — sonst
  // sieht es aus, als lade es noch, und niemand kommt auf die Idee, es
  // erneut zu versuchen.
  it("gibt die Tabelle nach einem Fehler wieder frei", async () => {
    fireEvent.click(rowFor("GSG102"));
    await settle();

    await act(async () => {
      h.pending[0](Promise.reject(new Error("Verbindung fehlgeschlagen")));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(document.querySelector(".aa-lb-spin")).toBeNull();
    fireEvent.click(rowFor("GSG101"));
    await settle();
    expect(
      h.calls.filter((c) => c.cmd === "logbook_pirep").length,
      "nach dem Fehler ist ein neuer Versuch möglich",
    ).toBe(2);
  });
});
