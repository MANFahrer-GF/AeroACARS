// Q2 (Audit 2026-06-27) — Golden-vector regression test for the FROZEN
// legacy TS scoring fallback in `computeSubScores`.
//
// Context: the live source of truth for landing scores is the Rust crate
// `landing-scoring` (its sub_scores are passed 1:1 into the UI for every
// v0.7.1+ PIREP — see LandingPanel.getSubScores). This TS `computeSubScores`
// runs ONLY as the legacy fallback for pre-v0.7.1 PIREPs and is intentionally
// NOT kept in sync with the current Rust algorithm (e.g. Rust rollout is now
// LDA-/weight-class-based, while this legacy path keeps the old absolute-metre
// thresholds — that divergence is by design, not a bug).
//
// Purpose of this test: PIN the legacy behaviour so an *unintended* change to
// the frozen fallback breaks CI. It is deliberately NOT a Rust↔TS equality
// test (the two are different algorithms now).

import { describe, it, expect } from "vitest";

import { computeSubScores } from "./landingScoring";

describe("landingScoring TS legacy fallback — golden vectors (Q2)", () => {
  it("pins the full sub-score vector for a representative clean landing", () => {
    // v0.20.x: landing_rate is NOT frozen (only the rollout piece below
    // is — see the file header) — it tracks the live target-corridor
    // redesign, so this line moved from 90/firm_but_clean to
    // 100/firm_positive_touchdown (|150| is within the 90-250 corridor).
    const subs = computeSubScores({
      vs_fpm: -150, // |150| in [90, 250) → 100 / firm_positive_touchdown
      scored_g_load: 1.3, // ≥ 1.20, < 1.40 → 85 / comfortable_g
      bounce_count: 0, // → 100 / clean_set
      approach_vs_stddev_fpm: 80, // < 100 → band 100
      approach_bank_stddev_deg: 1.5, // < 2 → band 100; min → very_stable
      rollout_distance_m: 1000, // 800..<1200 → 80 / good_stop
      fuel_efficiency_pct: 1.0, // |1.0| < 2 → 100 / on_plan
    });
    expect(subs.map((s) => [s.key, s.points, s.band, s.rationale])).toEqual([
      ["landing_rate", 100, "good", "firm_positive_touchdown"],
      ["g_force", 85, "good", "comfortable_g"],
      ["bounces", 100, "good", "clean_set"],
      ["stability", 100, "good", "very_stable"],
      ["rollout", 80, "good", "good_stop"],
      ["fuel", 100, "good", "on_plan"],
    ]);
  });

  it("freezes the legacy rollout thresholds (the diverged piece)", () => {
    const ro = (m: number) =>
      computeSubScores({ rollout_distance_m: m }).find(
        (s) => s.key === "rollout",
      )!;
    const got = [799, 800, 1199, 1200, 1799, 1800, 2499, 2500].map((m) => {
      const r = ro(m);
      return [r.points, r.band, r.rationale];
    });
    expect(got).toEqual([
      [100, "good", "excellent_stop"],
      [80, "good", "good_stop"],
      [80, "good", "good_stop"],
      [55, "ok", "long_rollout"],
      [55, "ok", "long_rollout"],
      [25, "bad", "very_long_rollout"],
      [25, "bad", "very_long_rollout"],
      [5, "bad", "marginal_runway"],
    ]);
  });
});
