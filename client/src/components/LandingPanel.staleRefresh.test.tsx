// v0.20.x QS fix: `2bf9f48` added a `refreshSeqRef` sequence-number guard to
// LandingPanel's 5-s polling `refresh()` (see the "QS 2026-08-04" comment
// right above `refreshSeqRef` in LandingPanel.tsx), so an in-flight request
// that resolves out of order can no longer clobber a fresher one already
// applied to state. That commit bundled it as an undocumented 4th fix with
// no dedicated test — this is the missing regression test, found by the
// second, wider QS pass over the 19-commit release-scope gap.
//
// Reproduces the exact race: the mount-time refresh() (call #1) is slow;
// before it resolves, the 5s interval fires refresh() #2, which resolves
// fast with newer data; only THEN does call #1 resolve, with stale data.
// Without the seq guard, call #1's stale response would land last and win.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
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

describe("LandingPanel — stale-response guard on the 5s polling refresh", () => {
  it("a late-resolving earlier refresh must not overwrite a newer refresh's records", async () => {
    vi.useFakeTimers();

    const staleRecord = record({ pirep_id: "stale-1", flight_number: "1111" });
    const freshRecord = record({ pirep_id: "fresh-1", flight_number: "2222" });

    let resolveStaleList!: (v: LandingRecord[]) => void;
    let resolveStaleCurrent!: (v: LandingRecord | null) => void;
    const staleListPromise = new Promise<LandingRecord[]>((res) => {
      resolveStaleList = res;
    });
    const staleCurrentPromise = new Promise<LandingRecord | null>((res) => {
      resolveStaleCurrent = res;
    });

    const listQueue: Array<Promise<LandingRecord[]>> = [
      staleListPromise,
      Promise.resolve([freshRecord]),
    ];
    const currentQueue: Array<Promise<LandingRecord | null>> = [
      staleCurrentPromise,
      Promise.resolve(null),
    ];

    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_list") return listQueue.shift() ?? Promise.resolve([]);
      if (cmd === "landing_get_current") return currentQueue.shift() ?? Promise.resolve(null);
      return Promise.resolve(null);
    });

    render(<LandingPanel />);

    // Mount's refresh() (#1) has fired and is now awaiting the still-pending
    // stale promises.
    await act(async () => {
      await Promise.resolve();
    });

    // Advance past the 5s interval: refresh() #2 fires and resolves
    // immediately with the fresh record.
    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText(/GS2222/)).toBeInTheDocument();
    expect(screen.queryByText(/GS1111/)).not.toBeInTheDocument();

    // NOW the slow mount-refresh (#1) resolves, with STALE data arriving
    // last. The seq guard must discard it.
    await act(async () => {
      resolveStaleList([staleRecord]);
      resolveStaleCurrent(null);
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText(/GS2222/)).toBeInTheDocument();
    expect(screen.queryByText(/GS1111/)).not.toBeInTheDocument();
  });
});
