// Regressionstest: Landungs-Tab ueber die LAN-Bruecke mit langsamer Antwort.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { LandingRecord } from "./LandingPanel";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { LandingPanel } from "./LandingPanel";

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

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

function record(over: Partial<LandingRecord>): LandingRecord {
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

describe("LandingPanel — langsame Antwort ueber die LAN-Bruecke", () => {
  // Feldbefund 28.10.2026 (Thomas, Tablet): Landungs-Tab ueber die Bruecke leer.
  // Die Liste aller Landungen braucht dort laenger als der 5-s-Takt; jede neue
  // Abfrage verwarf die vorige, also kam nie eine an.
  it("zeigt die Landungen, auch wenn jede Antwort 7 s braucht", async () => {
    vi.useFakeTimers();
    const rec = record({ pirep_id: "lan-1", flight_number: "3333" });
    let offen = 0, maxOffen = 0;
    const langsam = <T,>(wert: T) => {
      offen++; maxOffen = Math.max(maxOffen, offen);
      return new Promise<T>((res) => setTimeout(() => { offen--; res(wert); }, 7000));
    };
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_list") return langsam([rec]);
      if (cmd === "landing_get_current") return langsam(null);
      if (cmd === "landing_list_stand") return langsam("1-100-1");
      return Promise.resolve(null);
    });

    render(<LandingPanel />);
    for (let i = 0; i < 6; i++) {
      await act(async () => {
        vi.advanceTimersByTime(5000);
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });
    }

    expect(screen.getByText(/GS3333/)).toBeInTheDocument();
    // Takt startet keine zweite Abfrage, solange eine laeuft (Leitung nicht
    // stapeln): hoechstens Vorschau + Merker gleichzeitig offen, nie doppelt.
    expect(maxOffen).toBeLessThanOrEqual(2);
    // Merker bleibt gleich: die volle Liste nur einmal
    expect(invokeMock.mock.calls.filter((c) => c[0] === "landing_list")).toHaveLength(1);
  });

  it("holt die volle Liste nur, wenn sich der Stand-Merker aendert", async () => {
    vi.useFakeTimers();
    let stand = "100-1";
    let liste = [record({ pirep_id: "a", flight_number: "4444" })];
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_list") return Promise.resolve(liste);
      if (cmd === "landing_list_stand") return Promise.resolve(stand);
      return Promise.resolve(null);
    });
    const takt = async () => {
      await act(async () => {
        vi.advanceTimersByTime(5000);
        for (let i = 0; i < 6; i++) await Promise.resolve();
      });
    };
    const listenAbrufe = () => invokeMock.mock.calls.filter((c) => c[0] === "landing_list").length;

    render(<LandingPanel />);
    await act(async () => { for (let i = 0; i < 6; i++) await Promise.resolve(); });
    for (let i = 0; i < 4; i++) await takt();
    expect(listenAbrufe()).toBe(1);
    expect(screen.getByText(/GS4444/)).toBeInTheDocument();

    stand = "200-2";
    liste = [record({ pirep_id: "b", flight_number: "5555" }), ...liste];
    await takt();
    expect(listenAbrufe()).toBe(2);
    expect(screen.getByText(/GS5555/)).toBeInTheDocument();
  });

  it("fragt neu, wenn ein Abruf laenger als eine Minute haengt", async () => {
    vi.useFakeTimers();
    const rec = record({ pirep_id: "h-1", flight_number: "6666" });
    let erster = true;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_get_current") {
        if (erster) { erster = false; return new Promise(() => {}); } // haengt fuer immer
        return Promise.resolve(null);
      }
      if (cmd === "landing_list_stand") return Promise.resolve("1-1-1");
      if (cmd === "landing_list") return Promise.resolve([rec]);
      return Promise.resolve(null);
    });
    render(<LandingPanel />);
    for (let i = 0; i < 14; i++) {
      await act(async () => {
        vi.advanceTimersByTime(5000);
        for (let k = 0; k < 6; k++) await Promise.resolve();
      });
    }
    expect(screen.getByText(/GS6666/)).toBeInTheDocument();
  });

  it("staut bei dauerhaft haengenden Abrufen hoechstens zwei auf", async () => {
    vi.useFakeTimers();
    invokeMock.mockImplementation(() => new Promise(() => {})); // alles haengt fuer immer
    render(<LandingPanel />);
    for (let i = 0; i < 120; i++) { // 10 Minuten
      await act(async () => {
        vi.advanceTimersByTime(5000);
        for (let k = 0; k < 3; k++) await Promise.resolve();
      });
    }
    expect(invokeMock.mock.calls.filter((c) => c[0] === "landing_get_current").length).toBeLessThanOrEqual(2);
  });
});

describe("LandingPanel — schlanke Liste, volle Landung beim Antippen", () => {
  const warten = async () => {
    await act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); });
  };

  it("holt die volle Landung per landing_get und zeigt sie", async () => {
    const schlank = record({ pirep_id: "p-7", flight_number: "7777" });
    const voll = record({ pirep_id: "p-7", flight_number: "7777", arr_airport: "LOWW" });
    invokeMock.mockImplementation((cmd: string, args?: { pirepId?: string }) => {
      if (cmd === "landing_list") return Promise.resolve([schlank]);
      if (cmd === "landing_get") return Promise.resolve(args?.pirepId === "p-7" ? voll : null);
      return Promise.resolve(null);
    });
    render(<LandingPanel />);
    await warten();
    expect(screen.queryAllByText(/LOWW/)).toHaveLength(0);
    fireEvent.click(screen.getByText(/GS7777/).closest("tr")!);
    await warten();
    expect(invokeMock).toHaveBeenCalledWith("landing_get", { pirepId: "p-7" });
    // Nur die VOLLE Landung hat LOWW — die Detailansicht zeigt sie, nicht den Listen-Datensatz
    expect(screen.queryAllByText(/LOWW/).length).toBeGreaterThan(0);
  });

  it("zeigt die G-Kraft nach der Sinkrate, bei nicht bewertbarer Landung einen Strich", async () => {
    const gut = record({ pirep_id: "g-1", flight_number: "1001", vs_at_edge_fpm: -120, landing_scored_g_force: 1.31 } as Partial<LandingRecord>);
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_list") return Promise.resolve([gut]);
      return Promise.resolve(null);
    });
    render(<LandingPanel />);
    await warten();
    const kopf = screen.getAllByRole("columnheader").map((h) => h.textContent ?? "");
    const iRate = kopf.findIndex((k) => /Sinkrate/.test(k));
    expect(kopf[iRate + 1]).toMatch(/G-Kraft/);
    const zeile = screen.getByText(/GS1001/).closest("tr")!;
    const zellen = Array.from(zeile.querySelectorAll("td")).map((td) => td.textContent ?? "");
    // gleiche Schreibweise wie die Detailansicht (fmtNumber, TouchdownAbschnitt)
    expect(zellen[iRate + 1]).toBe("1.31G");
  });
});

