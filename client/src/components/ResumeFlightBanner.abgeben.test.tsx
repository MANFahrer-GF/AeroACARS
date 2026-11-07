// DAL667, 07.11.2026: Gelandet und geparkt in KSFO, danach lud der Pilot im
// MSFS-Menü eine 777 in Rio. Nach dem App-Neustart bot das Banner nur
// „Flug abbrechen" — der fertige Flug wurde storniert. Ein gelandeter,
// geparkter Flug ohne Sperre muss sich aus dem Banner abgeben lassen.
import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

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

const DAL667 = {
  pirep_id: "BParlVX8j87w96rm",
  airline_icao: "DAL",
  flight_number: "667",
  callsign: "",
  dpt_airport: "KJFK",
  arr_airport: "KSFO",
  phase: "arrived",
  was_just_resumed: true,
  resume_position_suspect: false,
  abgabe_sperre: null,
  divert_hint: null,
  last_known_lat: 37.6155,
  last_known_lon: -122.39,
  last_known_at: new Date(Date.now() - 60 * 60_000).toISOString(),
} as unknown as ActiveFlightInfo;

/** Simulator verbunden (`mitSim`) oder nicht; `flight_end` gelingt. */
function backend(mitSim: boolean) {
  return (cmd: string) => {
    if (cmd === "sim_status") {
      return Promise.resolve({
        state: mitSim ? "connected" : "disconnected",
        kind: "msfs2024",
        snapshot: mitSim ? { lat: -22.81, lon: -43.25 } : null,
        last_error: null,
        available: true,
      });
    }
    return Promise.resolve(null);
  };
}

describe("Resume-Banner: gelandeten Flug abgeben (DAL667)", () => {
  beforeEach(() => tauriInvoke.mockReset());

  it("ohne Simulator: „Flug abgeben“ neben „Flug abbrechen“, Klick gibt ab", async () => {
    tauriInvoke.mockImplementation(backend(false));
    const filed = vi.fn();
    render(
      <ResumeFlightBanner
        activeFlight={DAL667}
        onAdopted={() => {}}
        abgabeMoeglich
        onCancelled={() => {}}
        onFiledSuccess={filed}
      />,
    );
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.getByText("Flug abbrechen")).toBeTruthy();
    fireEvent.click(screen.getByText("Flug abgeben"));
    await waitFor(() => expect(filed).toHaveBeenCalledTimes(1));
    expect(tauriInvoke.mock.calls.some(([c]) => c === "flight_end")).toBe(true);
    expect(tauriInvoke.mock.calls.some(([c]) => c === "flight_cancel")).toBe(false);
    expect(filed.mock.calls[0][0]).toMatchObject({ kind: "filed", dpt: "KJFK", arr: "KSFO" });
  });

  it("Lage passt nicht (anderes Flugzeug in Rio): auch im roten Banner abgebbar", async () => {
    tauriInvoke.mockImplementation(backend(true));
    const filed = vi.fn();
    render(
      <ResumeFlightBanner
        activeFlight={{ ...DAL667, resume_position_suspect: true } as ActiveFlightInfo}
        onAdopted={() => {}}
        abgabeMoeglich
        onCancelled={() => {}}
        onFiledSuccess={filed}
      />,
    );
    fireEvent.click(await screen.findByText("Flug abgeben"));
    await waitFor(() => expect(filed).toHaveBeenCalledTimes(1));
  });

  it("mit Sperre (Sprung vor der Landung) oder vor der Ankunft: kein Abgabe-Knopf", async () => {
    tauriInvoke.mockImplementation(backend(false));
    const { unmount } = render(
      <ResumeFlightBanner
        activeFlight={{ ...DAL667, abgabe_sperre: "sprung" } as ActiveFlightInfo}
        onAdopted={() => {}}
        abgabeMoeglich
        onCancelled={() => {}}
        onFiledSuccess={() => {}}
      />,
    );
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.queryByText("Flug abgeben")).toBeNull();
    unmount();

    render(
      <ResumeFlightBanner
        activeFlight={{ ...DAL667, phase: "cruise" } as ActiveFlightInfo}
        onAdopted={() => {}}
        abgabeMoeglich
        onCancelled={() => {}}
        onFiledSuccess={() => {}}
      />,
    );
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.queryByText("Flug abgeben")).toBeNull();
  });

  it("Simulator verbunden und Lage passt: Knopf auch in der normalen Variante", async () => {
    tauriInvoke.mockImplementation(backend(true));
    render(
      <ResumeFlightBanner
        activeFlight={DAL667}
        onAdopted={() => {}}
        abgabeMoeglich
        onCancelled={() => {}}
        onFiledSuccess={() => {}}
      />,
    );
    await waitFor(() => expect(screen.getByText(/wird der Flug fortgesetzt/)).toBeTruthy());
    expect(screen.getByText("Flug abgeben")).toBeTruthy();
  });

  it("gibt die App gerade selbst ab, ist der verlorene Klick kein Fehler", async () => {
    const alarm = vi.spyOn(window, "alert").mockImplementation(() => {});
    try {
      tauriInvoke.mockImplementation((cmd: string) =>
        cmd === "flight_end"
          ? Promise.reject({ code: "flight_setup_in_progress", message: "busy" })
          : backend(false)(cmd),
      );
      render(
        <ResumeFlightBanner
          activeFlight={DAL667}
          onAdopted={() => {}}
          abgabeMoeglich
        abgabeMoeglich
          onCancelled={() => {}}
          onFiledSuccess={() => {}}
        />,
      );
      fireEvent.click(await screen.findByText("Flug abgeben"));
      await waitFor(() =>
        expect(tauriInvoke.mock.calls.some(([c]) => c === "flight_end")).toBe(true),
      );
      await new Promise((r) => setTimeout(r, 0));
      expect(alarm).not.toHaveBeenCalled();

      // Schon abgegeben (Flug weg): ebenfalls kein Fehler, Banner geht zu.
      const zu = vi.fn();
      tauriInvoke.mockImplementation((cmd: string) =>
        cmd === "flight_end"
          ? Promise.reject({ code: "no_active_flight", message: "weg" })
          : backend(false)(cmd),
      );
      render(
        <ResumeFlightBanner
          activeFlight={DAL667}
          onAdopted={() => {}}
          abgabeMoeglich
        abgabeMoeglich
          onCancelled={zu}
          onFiledSuccess={() => {}}
        />,
      );
      const knoepfe = await screen.findAllByText("Flug abgeben");
      fireEvent.click(knoepfe[knoepfe.length - 1]);
      await waitFor(() => expect(zu).toHaveBeenCalledTimes(1));
      expect(alarm).not.toHaveBeenCalled();
    } finally {
      alarm.mockRestore();
    }
  });

  it("aelteres Programm ohne `abgabe-ankunft-1`: weder Knopf noch Hinweis", async () => {
    tauriInvoke.mockImplementation(backend(false));
    render(
      <ResumeFlightBanner
        activeFlight={DAL667}
        onAdopted={() => {}}
        abgabeMoeglich={false}
        onCancelled={() => {}}
        onFiledSuccess={() => {}}
      />,
    );
    expect(await screen.findByText("Warte auf den Simulator")).toBeTruthy();
    expect(screen.getByText("Flug abbrechen")).toBeTruthy();
    expect(screen.queryByText("Flug abgeben")).toBeNull();
    expect(screen.queryByText(/mit den Werten der Ankunft abgeben/)).toBeNull();
  });
});
