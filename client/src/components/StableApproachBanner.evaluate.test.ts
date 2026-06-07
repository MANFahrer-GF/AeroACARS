import { describe, it, expect } from "vitest";
import { evaluateApproach, glideslopeScaleFactor } from "./StableApproachBanner";
import type { SimSnapshot } from "../types";

// Minimaler Snapshot — evaluateApproach liest nur diese Felder.
function snap(o: Partial<SimSnapshot>): SimSnapshot {
  return {
    on_ground: false,
    altitude_agl_ft: 800,
    vertical_speed_fpm: -700,
    bank_deg: 0,
    gear_position: 1,
    flaps_position: 1,
    ...o,
  } as unknown as SimSnapshot;
}

const GS_55 = glideslopeScaleFactor(5.5); // ≈ 1.837 (London City)

describe("glideslopeScaleFactor (= Backend gs_factor)", () => {
  it("3° / unbekannt / außerhalb 2–7,5° → 1 (keine Skalierung)", () => {
    expect(glideslopeScaleFactor(3)).toBeCloseTo(1, 6);
    expect(glideslopeScaleFactor(null)).toBe(1);
    expect(glideslopeScaleFactor(undefined)).toBe(1);
    expect(glideslopeScaleFactor(1.5)).toBe(1); // < 2°
    expect(glideslopeScaleFactor(8)).toBe(1); // > 7,5°
  });
  it("steiler Winkel skaliert tan-proportional", () => {
    expect(glideslopeScaleFactor(5.5)).toBeCloseTo(1.837, 2);
    expect(glideslopeScaleFactor(4)).toBeCloseTo(1.334, 2);
  });
});

describe("evaluateApproach — gleitwinkel-aware Sink-Schwellen", () => {
  // DER Fix: steiler, aber profil-konformer Anflug am 1000-ft-Gate.
  const steepAt1000 = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -1500 });

  it("3° (gsFactor=1): −1500 fpm @1000 ft wird als instabil geflaggt", () => {
    expect(evaluateApproach(steepAt1000, "approach", 1)?.key).toBe("gate1000_unstable");
  });

  it("5,5° (skaliert): DASSELBE −1500 fpm wird NICHT geflaggt (auf Profil)", () => {
    expect(evaluateApproach(steepAt1000, "approach", GS_55)).toBeNull();
  });

  it("sub-100 ft: steiler on-profile Sink nicht fälschlich 'pull up' (skaliert)", () => {
    const s = snap({ altitude_agl_ft: 80, vertical_speed_fpm: -1100 });
    expect(evaluateApproach(s, "final", 1)?.key).toBe("sink_rate_pull_up");
    expect(evaluateApproach(s, "final", GS_55)).toBeNull();
  });

  it("echt exzessiver Sink wird auch skaliert noch geflaggt", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -2500 });
    expect(evaluateApproach(s, "approach", GS_55)?.key).toBe("gate1000_unstable");
  });

  it("Bank-Überschreitung triggert unabhängig von der Skalierung", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -700, bank_deg: 8 });
    expect(evaluateApproach(s, "approach", GS_55)?.reason).toContain("Bank");
  });

  it("Config (Gear/Flaps) triggert unabhängig von der Skalierung", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -700, flaps_position: 0.5 });
    expect(evaluateApproach(s, "approach", GS_55)?.reason).toContain("Flaps");
  });

  it("nicht in der Approach-Phase → keine Advisory", () => {
    expect(evaluateApproach(snap({ vertical_speed_fpm: -3000 }), "cruise", 1)).toBeNull();
  });
});
