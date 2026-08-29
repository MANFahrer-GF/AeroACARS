// Der verwaiste Flug — die Kette, die am 28.08.2026 an THY77 gerissen ist.
//
// Michel hat mitten im Reiseflug (LTFM→KMIA, 12h39m) das Pflicht-Update
// gemacht. Danach kannte der Client den Flug nicht mehr, bot ihn auch nicht
// zur Wiederaufnahme an, und der Pilot saß in Reiseflughöhe vor
// „du musst am Boden sein". Zwei Ursachen im Frontend, beide hier festgehalten:
//
//   1. Der Banner hing in `CockpitView` — und dort im Zweig MIT aktivem Flug.
//      Seine Suche läuft aber nur OHNE aktiven Flug. Zwei Bedingungen, die
//      sich ausschließen: Die Suche konnte NIE laufen.
//   2. Die Suche war ein einziger Versuch, dessen Fehler stumm verschluckt
//      wurde. Lief sie vor der Anmeldung, war der Flug für die ganze
//      Sitzung unauffindbar.
//
// Geprüft wird an der echten Komponente, nicht an nachgebauter Logik.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const tauriInvoke = vi.hoisted(() => {
  (globalThis as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  if (typeof window !== "undefined") {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  }
  return vi.fn();
});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
}));

import { ResumeFlightBanner } from "./ResumeFlightBanner";

const VERWAIST = {
  pirep_id: "p8Q8YeaXqvKgyO6Z",
  flight_number: "77",
  dpt_airport: "LTFM",
  arr_airport: "KMIA",
  status: "ENR",
};

function ohneLeerraum(s: string) {
  return s.replace(/\s+/g, "");
}

describe("Wiederaufnahme eines verwaisten Fluges", () => {
  beforeEach(() => {
    tauriInvoke.mockReset();
  });

  it("bietet den verwaisten Flug an, wenn kein Flug aktiv ist", async () => {
    tauriInvoke.mockImplementation((cmd: string) =>
      cmd === "flight_discover_resumable"
        ? Promise.resolve([VERWAIST])
        : Promise.resolve(null),
    );
    render(
      <ResumeFlightBanner
        activeFlight={null}
        onAdopted={() => {}}
        onCancelled={() => {}}
      />,
    );
    // Die Flugnummer des verwaisten Fluges muss sichtbar werden — genau
    // das, was Michel nie zu sehen bekam.
    await waitFor(() => {
      expect(screen.getByText(/77/)).toBeTruthy();
    });
  });

  it("gibt nach einem Fehlschlag nicht auf, sondern fragt erneut", async () => {
    // Erster Aufruf scheitert (typisch: Sitzung steht noch nicht),
    // der zweite liefert den Flug.
    let rufe = 0;
    tauriInvoke.mockImplementation((cmd: string) => {
      if (cmd !== "flight_discover_resumable") return Promise.resolve(null);
      rufe += 1;
      return rufe === 1
        ? Promise.reject(new Error("not_logged_in"))
        : Promise.resolve([VERWAIST]);
    });
    vi.useFakeTimers();
    render(
      <ResumeFlightBanner
        activeFlight={null}
        onAdopted={() => {}}
        onCancelled={() => {}}
      />,
    );
    await vi.advanceTimersByTimeAsync(2500);
    vi.useRealTimers();
    expect(rufe).toBeGreaterThan(1);
  });

  it("der Banner haengt NICHT mehr in der Cockpit-Ansicht", () => {
    // ⚠ Dort lebte er nur, solange der Reiter offen war — und nur im Zweig
    // mit aktivem Flug, wo die Suche gar nicht erst anläuft.
    const cockpit = ohneLeerraum(
      readFileSync(resolve(__dirname, "CockpitView.tsx"), "utf-8"),
    );
    expect(cockpit.includes("<ResumeFlightBanner")).toBe(false);
  });

  it("der Banner haengt im App-Rahmen, unabhaengig vom Reiter", () => {
    const app = ohneLeerraum(
      readFileSync(resolve(__dirname, "../App.tsx"), "utf-8"),
    );
    expect(app.includes("<ResumeFlightBanner")).toBe(true);
    // Er darf an KEINE Reiterbedingung gebunden sein.
    const stelle = app.indexOf("<ResumeFlightBanner");
    const davor = app.slice(Math.max(0, stelle - 220), stelle);
    expect(davor.includes('tab===')).toBe(false);
  });
});
