// v0.7.8 Phase 1 — Component-Tests fuer SinkrateForensik
// Spec: docs/spec/v0.7.8-landing-rate-explainability.md §8.1
//
// Fokus: pure functions (Bucket-Math, Trend-Detection, Coaching-Tipp-
// Selektor, hasForensics, Score-Basis-Cascade). Render-Tests fuer
// Missing-Data-Cases.

import { describe, it, expect, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import {
  hasForensics,
  computeBuckets,
  isMonotonAccelerating,
  pickCoachingTip,
  pickForensicsLegacyKey,
  scoreBasisVs,
  selectTraceSamples,
  vsTone,
  SinkrateForensik,
} from "./SinkrateForensik";
import type { LandingProfilePoint, LandingRecord } from "./LandingPanel";

// v0.7.8 Phase 4 QS-Fix: i18n-Setup fuer Render-Tests damit
// useTranslation() funktioniert. Echte DE-Keys werden geladen damit
// Tests die Spec-Wortlaute pruefen koennen.
import deCommon from "../locales/de/common.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next
      .use(initReactI18next)
      .init({
        lng: "de",
        fallbackLng: "de",
        resources: { de: { common: deCommon } },
        defaultNS: "common",
        interpolation: { escapeValue: false },
      });
  }
});

// ───────────────────────────────────────────────────────────────────────────
// GSG-218-Fixture (Live-DB-Werte, Spec §8.3)
// ───────────────────────────────────────────────────────────────────────────
const GSG_218 = {
  landing_rate_fpm: -358,
  landing_peak_vs_fpm: -343,
  landing_source: "vs_at_impact",
  vs_at_edge_fpm: -343,
  vs_smoothed_250ms_fpm: -344,
  vs_smoothed_500ms_fpm: -314,
  vs_smoothed_1000ms_fpm: -266,
  vs_smoothed_1500ms_fpm: -229,
  peak_vs_pre_flare_fpm: -347,
  peak_g_post_500ms: 1.82,
  peak_g_post_1000ms: 1.82,
  flare_reduction_fpm: 0,
  forensic_sample_count: 469,
};

const EMPTY_LEGACY = {
  forensic_sample_count: null,
  vs_smoothed_250ms_fpm: null,
  vs_smoothed_500ms_fpm: null,
  vs_smoothed_1000ms_fpm: null,
  vs_smoothed_1500ms_fpm: null,
  vs_at_edge_fpm: null,
};

// ───────────────────────────────────────────────────────────────────────────
// v0.7.20 GSG-219 QS-Befund: forensics_version-aware Notice-Text-Branch
// ───────────────────────────────────────────────────────────────────────────
describe("pickForensicsLegacyKey — Notice-Text-Differenzierung (v0.7.20)", () => {
  it("returns no_edge_buffer_text fuer v2-Fluege ohne Sampler-Edge-Buffer", () => {
    // GSG219-Fall: forensics_version=2 ist gesetzt (v2-Cascade lief),
    // aber forensic_sample_count etc. fehlen → kein "alter Flug"-Text.
    expect(pickForensicsLegacyKey({ forensics_version: 2 })).toBe(
      "no_edge_buffer_text",
    );
  });
  it("returns no_edge_buffer_text auch fuer forensics_version >= 3 (forward-compat)", () => {
    expect(pickForensicsLegacyKey({ forensics_version: 3 })).toBe(
      "no_edge_buffer_text",
    );
  });
  it("returns legacy_notice_text fuer pre-v0.7.0 PIREPs (forensics_version = 1)", () => {
    expect(pickForensicsLegacyKey({ forensics_version: 1 })).toBe(
      "legacy_notice_text",
    );
  });
  it("returns legacy_notice_text wenn forensics_version fehlt (alter Record)", () => {
    expect(pickForensicsLegacyKey({})).toBe("legacy_notice_text");
  });
  it("returns legacy_notice_text wenn forensics_version = 0", () => {
    expect(pickForensicsLegacyKey({ forensics_version: 0 })).toBe(
      "legacy_notice_text",
    );
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("hasForensics — Render-Gate", () => {
  it("true wenn forensic_sample_count gesetzt", () => {
    expect(hasForensics({ ...EMPTY_LEGACY, forensic_sample_count: 469 })).toBe(true);
  });
  it("true wenn nur vs_smoothed_500ms gesetzt", () => {
    expect(hasForensics({ ...EMPTY_LEGACY, vs_smoothed_500ms_fpm: -314 })).toBe(true);
  });
  it("true wenn nur vs_at_edge_fpm gesetzt", () => {
    expect(hasForensics({ ...EMPTY_LEGACY, vs_at_edge_fpm: -343 })).toBe(true);
  });
  it("false wenn ALLE Felder null (Legacy)", () => {
    expect(hasForensics(EMPTY_LEGACY)).toBe(false);
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("computeBuckets — Disjoint-Bucket-Differenz", () => {
  it("GSG-218: korrekte Bucket-Werte aus Live-DB-Fixture", () => {
    const buckets = computeBuckets(-229, -266, -314, -344, -343);
    expect(buckets).not.toBeNull();
    // (1500*-229 - 1000*-266) / 500 = -155
    expect(Math.round(buckets![0]!.vs)).toBe(-155);
    // (1000*-266 - 500*-314) / 500 = -218
    expect(Math.round(buckets![1]!.vs)).toBe(-218);
    // (500*-314 - 250*-344) / 250 = -284
    expect(Math.round(buckets![2]!.vs)).toBe(-284);
    // Edge direkt
    expect(buckets![3]!.vs).toBe(-343);
  });

  it("null wenn auch nur ein Mittelwert fehlt (Bucket-Math braucht alle 4)", () => {
    expect(computeBuckets(null, -266, -314, -344, -343)).toBeNull();
    expect(computeBuckets(-229, null, -314, -344, -343)).toBeNull();
    expect(computeBuckets(-229, -266, -314, null, -343)).toBeNull();
  });

  it("Edge-Fallback: wenn vs_at_edge fehlt, nimm vs_smoothed_250ms", () => {
    const buckets = computeBuckets(-229, -266, -314, -344, null);
    expect(buckets![3]!.vs).toBe(-344);
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("isMonotonAccelerating — Trend ueber BETRAG", () => {
  it("GSG-218: |155|→|218|→|284|→|343| alle Deltas >20 = true", () => {
    const buckets = computeBuckets(-229, -266, -314, -344, -343)!;
    expect(isMonotonAccelerating(buckets)).toBe(true);
  });

  it("Stabile Landung (gleichmaessig, kein Drop): false", () => {
    // Konstruiert: 4 Buckets mit |VS|-Werten 100, 105, 110, 115 fpm
    // Deltas: 5, 5, 5 — alle <20 → KEIN monotones Beschleunigen
    const buckets = [
      { label: "a", vs: -100 },
      { label: "b", vs: -105 },
      { label: "c", vs: -110 },
      { label: "d", vs: -115 },
    ];
    expect(isMonotonAccelerating(buckets)).toBe(false);
  });

  it("Flare wirkt (Betrag faellt am Ende): false", () => {
    // 4 Buckets: -200, -250, -300, -150 — letzter Bucket weicher
    const buckets = [
      { label: "a", vs: -200 },
      { label: "b", vs: -250 },
      { label: "c", vs: -300 },
      { label: "d", vs: -150 },
    ];
    expect(isMonotonAccelerating(buckets)).toBe(false);
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("pickCoachingTip — Prioritaet (erster Match gewinnt)", () => {
  it("GSG-218: flare_lost wegen monoton beschleunigendem Trend", () => {
    const buckets = computeBuckets(-229, -266, -314, -344, -343)!;
    const tip = pickCoachingTip({
      buckets,
      peakGPost500ms: 1.82,
      flareReductionFpm: 0,
      vsAtEdgeFpm: -343,
      vsSmoothed1500ms: -229,
    });
    expect(tip).toBe("flare_lost");
  });

  it("Hard-G ohne Bucket-Trend: hard_g triggert", () => {
    const buckets = [
      { label: "a", vs: -100 },
      { label: "b", vs: -100 },
      { label: "c", vs: -100 },
      { label: "d", vs: -100 },
    ];
    const tip = pickCoachingTip({
      buckets,
      peakGPost500ms: 1.85,
      flareReductionFpm: 100,
      vsAtEdgeFpm: -100,
      vsSmoothed1500ms: -100,
    });
    expect(tip).toBe("hard_g");
  });

  it("Sanfte Landung ohne Drop: clean", () => {
    const buckets = [
      { label: "a", vs: -90 },
      { label: "b", vs: -100 },
      { label: "c", vs: -110 },
      { label: "d", vs: -120 },
    ];
    const tip = pickCoachingTip({
      buckets,
      peakGPost500ms: 1.2,
      flareReductionFpm: 150,
      vsAtEdgeFpm: -120,
      vsSmoothed1500ms: -90,
    });
    expect(tip).toBe("clean");
  });

  it("Late-Drop ueber Math.abs (v1.3-Fix): triggert bei |343|-|229|=114 > 100", () => {
    // Kein Bucket-Trend, kein Hard-G, kein No-Flare — nur Late-Drop
    const buckets = [
      { label: "a", vs: -100 },
      { label: "b", vs: -100 },
      { label: "c", vs: -100 },
      { label: "d", vs: -343 },
    ];
    const tip = pickCoachingTip({
      buckets: null, // explicit kein Trend-Trigger
      peakGPost500ms: 1.3,
      flareReductionFpm: 100,
      vsAtEdgeFpm: -343,
      vsSmoothed1500ms: -229,
    });
    expect(tip).toBe("late_drop");
    // Buckets ungenutzt — Test demonstriert dass Late-Drop unabhaengig
    // vom Trend triggern kann
    expect(buckets).toBeTruthy();
  });

  it("No-Flare: flare_reduction < 50", () => {
    const tip = pickCoachingTip({
      buckets: null,
      peakGPost500ms: 1.3,
      flareReductionFpm: 30,
      vsAtEdgeFpm: -200,
      vsSmoothed1500ms: -180,
    });
    expect(tip).toBe("no_flare");
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("scoreBasisVs — Cascade-Chain", () => {
  it("nimmt landing_peak_vs_fpm wenn gesetzt", () => {
    expect(scoreBasisVs({
      vs_at_edge_fpm: null,
      landing_peak_vs_fpm: -343,
      landing_rate_fpm: -358,
    })).toBe(-343);
  });

  it("Fallback auf landing_rate_fpm wenn landing_peak_vs_fpm null", () => {
    expect(scoreBasisVs({
      vs_at_edge_fpm: null,
      landing_peak_vs_fpm: null,
      landing_rate_fpm: -358,
    })).toBe(-358);
  });

  // v0.7.17 (B-015): vs_at_edge_fpm muss Vorrang haben.
  it("bevorzugt vs_at_edge_fpm (50-Hz-Edge, FAR 25.473) vor allen anderen", () => {
    expect(scoreBasisVs({
      vs_at_edge_fpm: -265,
      landing_peak_vs_fpm: -311,
      landing_rate_fpm: -358,
    })).toBe(-265);
  });

  it("ignoriert positive vs_at_edge_fpm (= Bumping/Ballooning, kein echter TD)", () => {
    expect(scoreBasisVs({
      vs_at_edge_fpm: 12,
      landing_peak_vs_fpm: -311,
      landing_rate_fpm: -358,
    })).toBe(-311);
  });

  it("EIN799-Regression: -265 statt -311", () => {
    // Tester-Befund 2026-05-12: Pilot-Client zeigte -311, Webapp -265
    // fuer denselben EIN799-Touchdown. Dieser Test fixiert dass der
    // Pilot-Client jetzt auch -265 zeigt.
    expect(scoreBasisVs({
      vs_at_edge_fpm: -265,
      landing_peak_vs_fpm: -311,
      landing_rate_fpm: -311,
    })).toBe(-265);
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("vsTone — Bands nach landingScoring.ts:128-131", () => {
  it("|150| → good (< 200)", () => expect(vsTone(-150)).toBe("good"));
  it("|343| → neutral (< 400, GSG-218-Fall)", () => expect(vsTone(-343)).toBe("neutral"));
  it("|500| → warn (< 600)", () => expect(vsTone(-500)).toBe("warn"));
  it("|800| → err (< 1000)", () => expect(vsTone(-800)).toBe("err"));
  it("|1200| → err-severe (>= 1000)", () => expect(vsTone(-1200)).toBe("err-severe"));
  it("null → null", () => expect(vsTone(null)).toBeNull());
});

// ───────────────────────────────────────────────────────────────────────────
describe("selectTraceSamples — touchdown_profile Filter [-3500, 0]", () => {
  it("waehlt nearest-neighbor bei -3000/-2000/-1000/-500/-100", () => {
    const profile: LandingProfilePoint[] = [
      { t_ms: -3100, vs_fpm: -110, agl_ft: 18, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -2100, vs_fpm: -144, agl_ft: 12, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -1100, vs_fpm: -171, agl_ft: 8, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -600, vs_fpm: -249, agl_ft: 3, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -100, vs_fpm: -348, agl_ft: 1, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
    ];
    const trace = selectTraceSamples(profile);
    expect(trace.length).toBe(5);
    expect(trace[0]!.t_ms).toBe(-3100);
    expect(trace[4]!.t_ms).toBe(-100);
  });

  it("leeres Array wenn profile < 3 Samples", () => {
    expect(selectTraceSamples([])).toEqual([]);
    expect(selectTraceSamples([
      { t_ms: -100, vs_fpm: -200, agl_ft: 5, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
    ])).toEqual([]);
  });

  it("leeres Array wenn alle Samples ausserhalb [-3500, 0]", () => {
    const profile: LandingProfilePoint[] = [
      { t_ms: -5000, vs_fpm: -110, agl_ft: 200, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -4000, vs_fpm: -120, agl_ft: 100, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
      { t_ms: -3700, vs_fpm: -130, agl_ft: 50, on_ground: false,
        g_force: 1, heading_true_deg: 0, groundspeed_kt: 130, indicated_airspeed_kt: 130, pitch_deg: 2, bank_deg: 0 },
    ];
    expect(selectTraceSamples(profile)).toEqual([]);
  });

  it("null/undefined profile → leeres Array", () => {
    expect(selectTraceSamples(null)).toEqual([]);
    expect(selectTraceSamples(undefined)).toEqual([]);
  });
});

// ───────────────────────────────────────────────────────────────────────────
describe("GSG-218 End-to-End — kompletter Fixture-Trockenlauf (Spec §8.3)", () => {
  it("Score-Basis ist -343 (Cascade), nicht -358 (Rate)", () => {
    expect(scoreBasisVs(GSG_218)).toBe(-343);
  });
  it("Score-Basis-Tone ist neutral (|343| in [200, 400))", () => {
    expect(vsTone(scoreBasisVs(GSG_218))).toBe("neutral");
  });
  it("Buckets matchen die Live-DB-Werte 1:1", () => {
    const buckets = computeBuckets(
      GSG_218.vs_smoothed_1500ms_fpm,
      GSG_218.vs_smoothed_1000ms_fpm,
      GSG_218.vs_smoothed_500ms_fpm,
      GSG_218.vs_smoothed_250ms_fpm,
      GSG_218.vs_at_edge_fpm,
    )!;
    expect(buckets.map((b) => Math.round(b.vs))).toEqual([-155, -218, -284, -343]);
  });
  it("Coaching-Tipp ist flare_lost (Trend dominiert)", () => {
    const buckets = computeBuckets(
      GSG_218.vs_smoothed_1500ms_fpm,
      GSG_218.vs_smoothed_1000ms_fpm,
      GSG_218.vs_smoothed_500ms_fpm,
      GSG_218.vs_smoothed_250ms_fpm,
      GSG_218.vs_at_edge_fpm,
    );
    expect(pickCoachingTip({
      buckets,
      peakGPost500ms: GSG_218.peak_g_post_500ms,
      flareReductionFpm: GSG_218.flare_reduction_fpm,
      vsAtEdgeFpm: GSG_218.vs_at_edge_fpm,
      vsSmoothed1500ms: GSG_218.vs_smoothed_1500ms_fpm,
    })).toBe("flare_lost");
  });
});

// ───────────────────────────────────────────────────────────────────────────
// Render-Tests mit @testing-library/react — DOM/UI-Verhalten absichern
// (v0.7.8 QS-Round-9 P2-Fix)
// ───────────────────────────────────────────────────────────────────────────

function makeRecord(overrides: Partial<LandingRecord>): LandingRecord {
  return {
    pirep_id: "test",
    touchdown_at: "2026-05-11T15:00:00Z",
    recorded_at: "2026-05-11T15:00:00Z",
    flight_number: "TST123",
    airline_icao: "TST",
    dpt_airport: "EDDF",
    arr_airport: "EDDM",
    aircraft_registration: null,
    aircraft_icao: null,
    aircraft_title: null,
    sim_kind: null,
    score_numeric: 80,
    score_label: "smooth",
    grade_letter: "B",
    landing_rate_fpm: -200,
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
    forensic_sample_count: null,
    vs_at_edge_fpm: null,
    vs_smoothed_250ms_fpm: null,
    vs_smoothed_500ms_fpm: null,
    vs_smoothed_1000ms_fpm: null,
    vs_smoothed_1500ms_fpm: null,
    peak_g_post_500ms: null,
    peak_g_post_1000ms: null,
    peak_vs_pre_flare_fpm: null,
    vs_at_flare_end_fpm: null,
    flare_reduction_fpm: null,
    flare_dvs_dt_fpm_per_sec: null,
    flare_quality_score: null,
    flare_detected: null,
    landing_source: null,
    ...overrides,
  } as LandingRecord;
}

describe("SinkrateForensik render — Legacy + Missing-Data + Source-Pill", () => {
  it("Legacy-Notice wenn keine Forensik-Felder gesetzt", () => {
    render(<SinkrateForensik record={makeRecord({})} />);
    expect(screen.getByText(/Forensik-Daten noch nicht gespeichert/i)).toBeInTheDocument();
  });

  it("Voll-Render wenn forensic_sample_count gesetzt", () => {
    render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      landing_peak_vs_fpm: -343,
    })} />);
    expect(screen.getByText(/Sinkrate-Forensik/i)).toBeInTheDocument();
    expect(screen.getByText(/Welche Sinkrate ist die/i)).toBeInTheDocument();
  });

  it("Missing-Data: 3 von 4 Smoothed-Tiles gesetzt → 4 Tiles im DOM, 1 mit Em-Dash + Tooltip", () => {
    const { container } = render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      vs_smoothed_1500ms_fpm: -229,
      vs_smoothed_1000ms_fpm: -266,
      vs_smoothed_500ms_fpm: -314,
      vs_smoothed_250ms_fpm: null, // ein Tile fehlt
      vs_at_edge_fpm: -343,
    })} />);
    const tiles = container.querySelectorAll(".sinkrate-tile");
    expect(tiles.length).toBe(4);
    const naTile = container.querySelector(".sinkrate-tile--na");
    expect(naTile).not.toBeNull();
    expect(naTile?.textContent).toContain("—");
    // Tooltip via title-Attribut
    expect(naTile?.getAttribute("title")).toBeTruthy();
    expect(naTile?.getAttribute("title")).toMatch(/Wert nicht erfasst/i);
  });

  it("Source-Pill nur wenn landing_source gesetzt", () => {
    const { rerender } = render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      landing_peak_vs_fpm: -343,
      landing_source: "vs_at_impact",
    })} />);
    expect(screen.getByText(/vs_at_impact/i)).toBeInTheDocument();

    rerender(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      landing_peak_vs_fpm: -343,
      landing_source: null, // kein Pill erwartet
    })} />);
    expect(screen.queryByText(/vs_at_impact/)).toBeNull();
  });

  it("Bucket-Sektion ausgeblendet wenn nicht alle 5 Werte gesetzt", () => {
    const { container } = render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      vs_smoothed_1500ms_fpm: -229,
      // andere fehlen
    })} />);
    expect(container.querySelector(".sinkrate-buckets")).toBeNull();
  });

  // v0.16.22 FIX 3: the panel now reads the CANONICAL backend
  // `flare_reduction_fpm` directly (not a recompute from peak/edge), so it
  // agrees with LandingPanel.tsx and `flare_detected`. These tests drive the
  // backend value and assert the display semantics are preserved.
  it("Flare-Reduktion zeigt POSITIVE Zahl aus dem kanonischen Backend-Wert", () => {
    // Backend liefert flare_reduction_fpm = 93 (Flare hat reduziert).
    render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      landing_peak_vs_fpm: -142,
      vs_at_edge_fpm: -142,
      peak_vs_pre_flare_fpm: -235,
      flare_reduction_fpm: 93,
    })} />);
    // Wortlaut: "Flare hat Sinkrate um 93 fpm reduziert" — kein Minus
    expect(screen.getByText(/93 fpm reduziert/i)).toBeInTheDocument();
    // Kein "−93" / "-93" im Reduktions-Text
    const reduction = screen.queryByText(/-93 fpm reduziert/);
    expect(reduction).toBeNull();
  });

  it("Flare hat NICHT reduziert: alternativer Text wenn Backend-Wert ≤ 0", () => {
    // Backend liefert flare_reduction_fpm = 0 → "nicht reduziert".
    render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      landing_peak_vs_fpm: -343,
      vs_at_edge_fpm: -343,
      peak_vs_pre_flare_fpm: -200,
      flare_reduction_fpm: 0,
    })} />);
    expect(screen.getByText(/Flare hat die Sinkrate nicht reduziert/i)).toBeInTheDocument();
  });

  // v0.16.22 FIX 3 regression: the OLD recompute (|peak| − |edge|) would have
  // shown a POSITIVE reduction here (|−659| − |−423| = 236) and DISAGREED
  // with the backend, which de-lagged the edge and computed its own value.
  // The panel must now honour the backend `flare_reduction_fpm` (188) so it
  // matches LandingPanel.tsx and `flare_detected`.
  it("FIX 3: nutzt Backend-Wert (188), nicht den Recompute aus peak/edge (236)", () => {
    render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      vs_at_edge_fpm: -423,            // de-lagged edge
      peak_vs_pre_flare_fpm: -659,     // AGL-derived peak
      flare_reduction_fpm: 188,        // canonical backend reduction
      flare_detected: true,
    })} />);
    expect(screen.getByText(/188 fpm reduziert/i)).toBeInTheDocument();
    // The stale recompute value (236) must NOT appear.
    expect(screen.queryByText(/236 fpm reduziert/i)).toBeNull();
  });

  it("Kein DLHv im Render-Output (V7-Acceptance)", () => {
    const { container } = render(<SinkrateForensik record={makeRecord({
      forensic_sample_count: 469,
      vs_smoothed_1500ms_fpm: -229,
      vs_smoothed_1000ms_fpm: -266,
      vs_smoothed_500ms_fpm: -314,
      vs_smoothed_250ms_fpm: -344,
      vs_at_edge_fpm: -343,
      landing_peak_vs_fpm: -343,
    })} />);
    expect(container.textContent).not.toMatch(/DLHv/i);
    expect(container.textContent).not.toMatch(/SmartCARS/i);
    expect(container.textContent).toMatch(/Volanta/);
  });
});

