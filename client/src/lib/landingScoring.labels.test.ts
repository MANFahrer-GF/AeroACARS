// v0.20.x QS fix: `possible_float`/`firm_positive_touchdown` (added to
// subLandingRate's target-corridor rework) were never added to
// RATIONALE_LABELS/TIP_LABELS — this file's own header comment demands
// these stay 1:1 with the aeroacars-live webapp mirror, which renders
// straight from this table with no i18next fallback (unlike the Tauri
// client's own UI, which never reads this table at all — see
// LandingPanel.tsx's coachTipKey()). Found by code review before the
// v1.4.4 release.
//
// General symmetry check: every RATIONALE_LABELS key must also have a
// TIP_LABELS entry (this is exactly the shape of gap this bug had), so a
// future score band added to any sub*() function without updating both
// tables in lockstep fails CI instead of silently rendering the raw key.

import { describe, it, expect } from "vitest";
import { RATIONALE_LABELS, TIP_LABELS } from "./landingScoring";

describe("RATIONALE_LABELS / TIP_LABELS completeness", () => {
  it("has the two v0.20.x landing-rate corridor keys in both tables", () => {
    expect(RATIONALE_LABELS.possible_float).toBeTruthy();
    expect(RATIONALE_LABELS.firm_positive_touchdown).toBeTruthy();
    expect(TIP_LABELS.possible_float).toBeTruthy();
    expect(TIP_LABELS.firm_positive_touchdown).toBeTruthy();
  });

  it("every rationale key has a matching tip (except the tip-only fallback)", () => {
    const missing = Object.keys(RATIONALE_LABELS).filter((key) => !(key in TIP_LABELS));
    expect(missing).toEqual([]);
  });
});
