// Feld-Bug (2026-08-05, macOS): der "PDF exportieren"-Button in der
// Landungs-Detailansicht tat nach dem ersten Klick GAR NICHTS mehr.
// Ursache: der `printing`-State wurde ausschliesslich ueber das
// `afterprint`-Event zurueckgesetzt — feuert dieses Event einmal nicht
// (in WKWebView/macOS-Tauri-Build nicht zuverlaessig), bleibt `printing`
// fuer immer `true`, und jeder weitere Klick ist ein no-op (React sieht
// keine State-Aenderung, der Effect laeuft nie wieder, `window.print()`
// wird nie wieder aufgerufen).
//
// Diese Tests reproduzieren beide Haerte-Faelle GEGEN DEN ALTEN CODE
// (nur `afterprint`, kein try/catch, kein Fallback-Timeout):
//   1. `window.print()` wirft (z. B. weil die Funktion in diesem WebView
//      nicht existiert) — muss sofort einen sichtbaren Fehler zeigen und
//      den Button reaktivieren, statt den State haengen zu lassen.
//   2. `afterprint` feuert nie — der Fallback-Timeout muss den Zustand
//      nach spaetestens 20s zuruecksetzen, statt fuer immer zu haengen.

import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { LandingRecord } from "./LandingPanel";
import { LandingDetail } from "./LandingPanel";

vi.mock("../lib/ipc", () => ({ invoke: vi.fn() }));
vi.mock("../lib/sentry", () => ({
  Sentry: { captureException: vi.fn() },
}));

beforeAllInit();

function beforeAllInit() {
  if (!i18next.isInitialized) {
    i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function record(over: Partial<LandingRecord> = {}): LandingRecord {
  return {
    pirep_id: "id",
    touchdown_at: "2026-08-03T19:53:43.561494400Z",
    recorded_at: "2026-08-03T19:53:43.561494400Z",
    flight_number: "0000",
    airline_icao: "GS",
    dpt_airport: "EDDH",
    arr_airport: "LOWS",
    touchdown_airport: null,
    touchdown_airport_source: null,
    touchdown_distance_to_destination_nm: null,
    touchdown_nearest_distance_nm: null,
    aircraft_registration: null,
    aircraft_icao: "A320",
    aircraft_title: null,
    sim_kind: null,
    score_numeric: 94,
    score_label: "acceptable",
    grade_letter: "A",
    landing_rate_fpm: -150,
    landing_peak_vs_fpm: null,
    landing_g_force: null,
    landing_peak_g_force: null,
    landing_pitch_deg: null,
    landing_bank_deg: null,
    landing_speed_kt: null,
    landing_heading_deg: null,
    landing_weight_kg: null,
    touchdown_sideslip_deg: null,
    bounce_count: 0,
    headwind_kt: null,
    crosswind_kt: null,
    approach_vs_stddev_fpm: null,
    approach_bank_stddev_deg: null,
    rollout_distance_m: null,
    planned_block_fuel_kg: null,
    planned_burn_kg: null,
    planned_tow_kg: null,
    planned_ldw_kg: null,
    planned_zfw_kg: null,
    actual_trip_burn_kg: null,
    fuel_efficiency_kg_diff: null,
    fuel_efficiency_pct: null,
    takeoff_weight_kg: null,
    takeoff_fuel_kg: null,
    landing_fuel_kg: null,
    block_fuel_kg: null,
    runway_match: null,
    touchdown_profile: [],
    approach_samples: [],
    ...over,
  } as LandingRecord;
}

describe("LandingDetail — PDF-Export darf nie dauerhaft haengen bleiben", () => {
  it("zeigt einen sichtbaren Fehler und reaktiviert den Button, wenn window.print() wirft", async () => {
    const printSpy = vi.spyOn(window, "print").mockImplementation(() => {
      throw new Error("print unavailable");
    });

    render(
      <LandingDetail
        record={record()}
        allRecords={[]}
        onBack={() => {}}
        isPreview={false}
      />,
    );

    const button = screen.getByRole("button", { name: /PDF exportieren/i });

    // Der Effect wartet einen echten requestAnimationFrame ab, bevor er
    // window.print() ruft — dafuer pollen statt eine feste Wartezeit zu
    // raten (robuster gegen langsame CI-Laeufer).
    await act(async () => {
      button.click();
    });
    await waitFor(() => expect(printSpy).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByText(/PDF-Export nicht möglich/)).toBeInTheDocument(),
    );

    // Der Button muss NACH dem Fehlschlag sofort wieder funktionieren —
    // das genau war der alte Bug: nach einem Fehlschlag blieb `printing`
    // haengen und jeder weitere Klick war ein no-op.
    printSpy.mockClear();
    await act(async () => {
      button.click();
    });
    await waitFor(() => expect(printSpy).toHaveBeenCalledTimes(1));
  });

  it("erholt sich vom Fallback-Timeout, wenn afterprint nie feuert", async () => {
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "requestAnimationFrame",
        "cancelAnimationFrame",
      ],
    });
    const printSpy = vi.spyOn(window, "print").mockImplementation(() => {
      // Simuliert das WKWebView-Verhalten: window.print() kehrt zurueck,
      // aber `afterprint` feuert nie.
    });

    render(
      <LandingDetail
        record={record()}
        allRecords={[]}
        onBack={() => {}}
        isPreview={false}
      />,
    );

    const button = screen.getByRole("button", { name: /PDF exportieren/i });

    await act(async () => {
      button.click();
      // Der Effect wartet einen requestAnimationFrame ab, bevor er
      // window.print() ruft.
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(printSpy).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/PDF-Export nicht möglich/)).not.toBeInTheDocument();

    // Ohne Fallback wuerde `printing` hier fuer immer `true` bleiben.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(screen.getByText(/PDF-Export nicht möglich/)).toBeInTheDocument();

    // Und der Button muss jetzt wieder auf einen neuen Klick reagieren.
    printSpy.mockClear();
    await act(async () => {
      button.click();
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(printSpy).toHaveBeenCalledTimes(1);
  });
});
