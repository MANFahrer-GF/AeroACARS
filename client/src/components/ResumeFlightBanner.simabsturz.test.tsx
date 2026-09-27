// Wiederaufnahme nach Sim-Absturz — SIA 375, 27.09.2026 (Thomas):
//   1. Der Countdown „wird fortgesetzt" lief, obwohl noch kein Simulator
//      verbunden war; erst danach kam die rote Karte. Überflüssig.
//   2. Der gespeicherte Punkt zeigte nur Koordinaten (aus der geleerten
//      Wegstrecken-Basis), Höhe und Kurs fehlten — der Pilot konnte nicht
//      zurückstellen.
import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

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

import { ResumeFlightBanner, fmtGradMinuten } from "./ResumeFlightBanner";
import type { ActiveFlightInfo } from "../types";

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

const FLUG = {
  pirep_id: "p",
  airline_icao: "SIA",
  flight_number: "375",
  callsign: "",
  dpt_airport: "WSSS",
  arr_airport: "EDDM",
  was_just_resumed: true,
  resume_position_suspect: false,
  last_known_lat: 21.2929,
  last_known_lon: 86.2577,
  last_known_alt_ft: 36012,
  last_known_heading_deg: 302,
  last_known_gs_kt: 488,
  last_known_at: new Date(Date.now() - 4 * 60_000).toISOString(),
} as unknown as ActiveFlightInfo;

function simStatus(mitSnapshot: boolean) {
  return (cmd: string) =>
    cmd === "sim_status"
      ? Promise.resolve({ state: "connected", kind: "xplane12", snapshot: mitSnapshot ? { lat: 1.35, lon: 103.98 } : null, last_error: null, available: true })
      : Promise.resolve(null);
}

describe("Wiederaufnahme nach Sim-Absturz", () => {
  beforeEach(() => tauriInvoke.mockReset());

  it("ohne Simulator: kein Countdown, sondern Warten mit dem letzten Punkt", async () => {
    tauriInvoke.mockImplementation(simStatus(false));
    render(<ResumeFlightBanner activeFlight={FLUG} onAdopted={() => {}} onCancelled={() => {}} />);
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.queryByText(/wird der Flug fortgesetzt/)).toBeNull();
    expect(screen.getByText(`${(36012).toLocaleString()} ft`)).toBeTruthy();
    expect(screen.getByText("302°")).toBeTruthy();
    expect(screen.getByText("488 kt")).toBeTruthy();
    expect(screen.getByText("vor 4 min")).toBeTruthy();
    expect(screen.getByText(/21\.2929°N · 86\.2577°E/)).toBeTruthy();
    expect(tauriInvoke).not.toHaveBeenCalledWith("flight_resume_confirm", expect.anything());
  });

  it("mit Simulator und passender Position: Countdown wie bisher, Punkt sichtbar", async () => {
    tauriInvoke.mockImplementation(simStatus(true));
    render(<ResumeFlightBanner activeFlight={FLUG} onAdopted={() => {}} onCancelled={() => {}} />);
    await waitFor(() => expect(screen.getByText(/wird der Flug fortgesetzt/)).toBeTruthy());
    expect(screen.queryByText("Warte auf den Simulator")).toBeNull();
    expect(screen.getByText(/Letzter gespeicherter Punkt/)).toBeTruthy();
  });

  it("solange der Simulatorstatus noch nicht da ist: kein Countdown", async () => {
    tauriInvoke.mockImplementation((cmd: string) => (cmd === "sim_status" ? new Promise(() => undefined) : Promise.resolve(null)));
    render(<ResumeFlightBanner activeFlight={FLUG} onAdopted={() => {}} onCancelled={() => {}} />);
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.queryByText(/wird der Flug fortgesetzt/)).toBeNull();
  });

  it("bricht der Simulator in der letzten Sekunde weg, wird nicht fortgesetzt", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    try {
      const start = Date.now();
      tauriInvoke.mockImplementation((cmd: string) => {
        if (cmd === "sim_status") {
          const da = Date.now() - start < 29_500;
          return Promise.resolve({ state: "connected", kind: "xplane12", snapshot: da ? { lat: 21.29, lon: 86.25 } : null, last_error: null, available: true });
        }
        return Promise.resolve(null);
      });
      render(<ResumeFlightBanner activeFlight={FLUG} onAdopted={() => {}} onCancelled={() => {}} />);
      for (let i = 0; i < 35; i++) {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1000);
        });
      }
      expect(tauriInvoke.mock.calls.some(([c]) => c === "flight_resume_confirm")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("hängt die letzte Abfrage (LAN-Brücke), wird nach 3 s weiter gewartet statt blockiert", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    try {
      const start = Date.now();
      tauriInvoke.mockImplementation((cmd: string) => {
        if (cmd === "sim_status") {
          if (Date.now() - start < 29_500) {
            return Promise.resolve({ state: "connected", kind: "xplane12", snapshot: { lat: 21.29, lon: 86.25 }, last_error: null, available: true });
          }
          return new Promise(() => undefined); // hängt
        }
        return Promise.resolve(null);
      });
      render(<ResumeFlightBanner activeFlight={FLUG} onAdopted={() => {}} onCancelled={() => {}} />);
      for (let i = 0; i < 40; i++) {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1000);
        });
      }
      expect(tauriInvoke.mock.calls.some(([c]) => c === "flight_resume_confirm")).toBe(false);
      // Der Knopf „Jetzt fortsetzen" bleibt bedienbar (Sperre wieder frei).
      expect(screen.getByText(/wird der Flug fortgesetzt|Warte auf den Simulator/)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("Grad und Dezimalminuten wie im Positionsdialog der Simulatoren", () => {
    expect(fmtGradMinuten(21.2929, 86.2577)).toBe("N21°17.57' E086°15.46'");
    expect(fmtGradMinuten(-33.9461, -18.6017)).toBe("S33°56.77' W018°36.10'");
    expect(fmtGradMinuten(1.35, 103.9999999)).toBe("N01°21.00' E104°00.00'");
  });
});
