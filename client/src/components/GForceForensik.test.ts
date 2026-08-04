// v0.19.x FIX: showsVsLeadsNote used to gate the "master score not dragged
// down by G" reassurance at 1.40 (T_G_FIRM), but classify_landing's actual
// B-009 guarantee (V/S smooth -> category stays exactly what V/S alone
// produces) only holds up to T_G_HARD (1.70). Between 1.40 and 1.70 the
// note used to claim a guarantee that was no longer necessarily true.

import { describe, it, expect } from "vitest";
import { showsVsLeadsNote, gTone } from "./GForceForensik";

describe("showsVsLeadsNote", () => {
  it("does NOT show the note in the 1.40-1.70 G band (the exact bug)", () => {
    expect(showsVsLeadsNote(1.4, -116)).toBe(false);
    expect(showsVsLeadsNote(1.55, -116)).toBe(false);
    expect(showsVsLeadsNote(1.69, -116)).toBe(false);
  });

  it("shows the note once G reaches T_G_HARD (1.70) with smooth V/S", () => {
    expect(showsVsLeadsNote(1.7, -116)).toBe(true);
    expect(showsVsLeadsNote(2.3, -116)).toBe(true);
  });

  it("does not show the note when G is genuinely low (nothing to explain)", () => {
    expect(showsVsLeadsNote(1.0, -116)).toBe(false);
  });

  it("does not show the note when V/S is not smooth (>= 200 fpm)", () => {
    expect(showsVsLeadsNote(2.3, -450)).toBe(false);
  });

  it("does not show the note when either value is missing", () => {
    expect(showsVsLeadsNote(null, -116)).toBe(false);
    expect(showsVsLeadsNote(2.3, null)).toBe(false);
    expect(showsVsLeadsNote(undefined, undefined)).toBe(false);
  });
});

// v0.19.x FIX: SinkrateForensik.tsx used to define its OWN hardcoded
// 3-band G-color function (1.4/1.7 only) instead of importing this one —
// the same peak-G value could read as a different color on the two
// forensics panels of one report (e.g. 1.25 G: "good" in one, "neutral"
// here). These pin the real 5-band ladder both panels now share.
describe("gTone", () => {
  it("returns the full 5-band ladder matching the T_G_* thresholds", () => {
    expect(gTone(1.0)).toBe("good");
    expect(gTone(1.25)).toBe("neutral"); // the exact value from the bug report
    expect(gTone(1.5)).toBe("warn");
    expect(gTone(1.9)).toBe("err");
    expect(gTone(2.3)).toBe("err-severe");
  });

  it("returns null for a missing value", () => {
    expect(gTone(null)).toBeNull();
    expect(gTone(undefined)).toBeNull();
  });
});
