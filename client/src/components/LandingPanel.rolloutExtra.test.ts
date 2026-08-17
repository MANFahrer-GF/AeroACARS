// v0.12.0 (#runway-utilization-refinement) — Tests für die TS-gerenderten
// Bahn-Auslastungs-Extra-Zeilen (Spec LE4/LE5).
//
// Fokus: pure functions. `buildRolloutExtraLines` / `buildRolloutValueLabel`
// bauen ab score_algorithm_version >= 3 die Card-Zeilen aus den Record-
// Feldern statt aus den hardcoded-DE `extra`-Strings des Rust-Crates.
// Geprüft wird gegen die ECHTEN DE-Texte (Mini-Interpolation), damit der
// Test auch die i18n-Wortlaute mit abdeckt.

import { describe, it, expect } from "vitest";
import {
  buildRolloutExtraLines,
  buildRolloutValueLabel,
  isRolloutV3,
  rolloutLdaMeters,
} from "./LandingPanel";
import type { LandingRecord, LandingRunwayMatch } from "./LandingPanel";
import deCommon from "../locales/de/common.json";

// Mini-t: interpoliert {{x}}-Platzhalter gegen die echten DE-Texte.
function t(key: string, opts?: Record<string, string | number>): string {
  const path = key.split(".");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let node: any = deCommon;
  for (const p of path) node = node?.[p];
  let s = typeof node === "string" ? node : key;
  if (opts) {
    for (const [k, v] of Object.entries(opts)) {
      s = s.split(`{{${k}}}`).join(String(v));
    }
  }
  return s;
}

// BTX8815-Geometrie: LDA 2849,88 m = (9350 ft − 0 ft) × 0.3048.
const LOWS_15: LandingRunwayMatch = {
  airport_ident: "LOWS",
  runway_ident: "15",
  surface: "ASP",
  length_ft: 9350,
  centerline_distance_m: 1.2,
  centerline_distance_abs_ft: 4,
  side: "L",
  touchdown_distance_from_threshold_ft: 1774,
  displaced_threshold_ft: 0,
};

function record(over: Partial<LandingRecord>): LandingRecord {
  return {
    score_algorithm_version: 3,
    runway_match: LOWS_15,
    td_distance_from_threshold_m: 540.85,
    rollout_distance_m: 442.5,
    ...over,
  } as LandingRecord;
}

describe("isRolloutV3", () => {
  it("true ab score_algorithm_version 3", () => {
    expect(isRolloutV3(record({ score_algorithm_version: 3 }))).toBe(true);
    expect(isRolloutV3(record({ score_algorithm_version: 4 }))).toBe(true);
  });

  it("false für v2 / pre-v0.10 / fehlend", () => {
    expect(isRolloutV3(record({ score_algorithm_version: 2 }))).toBe(false);
    expect(isRolloutV3(record({ score_algorithm_version: 1 }))).toBe(false);
    expect(isRolloutV3(record({ score_algorithm_version: null }))).toBe(false);
    expect(isRolloutV3(record({ score_algorithm_version: undefined }))).toBe(
      false,
    );
  });
});

describe("rolloutLdaMeters", () => {
  it("LDA = (length_ft − displaced_ft) × 0.3048", () => {
    const lda = rolloutLdaMeters(LOWS_15);
    expect(lda).not.toBeNull();
    expect(lda!).toBeCloseTo(2849.88, 1);
  });

  it("zieht den Displaced Threshold ab", () => {
    const lda = rolloutLdaMeters({ ...LOWS_15, displaced_threshold_ft: 1000 });
    expect(lda!).toBeCloseTo((9350 - 1000) * 0.3048, 1);
  });

  it("null bei kaputter Geometrie (LDA ≤ 0 / length ≤ 0)", () => {
    expect(rolloutLdaMeters({ ...LOWS_15, displaced_threshold_ft: 9999 })).toBe(
      null,
    );
    expect(rolloutLdaMeters({ ...LOWS_15, length_ft: 0 })).toBe(null);
  });
});

describe("buildRolloutExtraLines", () => {
  it("BTX8815: drei Zeilen — Aufsetzpunkt, Ausrollstrecke, Bahn", () => {
    const lines = buildRolloutExtraLines(record({}), t);
    expect(lines).toHaveLength(3);
    expect(lines[0]).toBe("Aufsetzpunkt: 541 m hinter der Schwelle");
    expect(lines[1]).toBe("Ausrollstrecke ab Aufsetzen: 443 m");
    expect(lines[2]).toBe("Bahn: LOWS 15 · LDA 2850 m");
  });

  it("R2-P2: negativer td_distance → „vor der Schwelle\" mit Betrag", () => {
    const lines = buildRolloutExtraLines(
      record({ td_distance_from_threshold_m: -50.4 }),
      t,
    );
    // kein „−50 m hinter …" — Vorzeichen wählt den Key, {{m}} ist der Betrag.
    expect(lines[0]).toBe("Aufsetzpunkt: 50 m vor der Schwelle");
    expect(lines[0]).not.toContain("-");
  });

  it("ohne runway_match entfällt nur die Bahn-Zeile", () => {
    const lines = buildRolloutExtraLines(record({ runway_match: null }), t);
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain("Aufsetzpunkt");
    expect(lines[1]).toContain("Ausrollstrecke");
  });

  it("fehlende Einzelfelder lassen ihre Zeile weg", () => {
    const lines = buildRolloutExtraLines(
      record({ td_distance_from_threshold_m: null, rollout_distance_m: null }),
      t,
    );
    expect(lines).toHaveLength(1);
    expect(lines[0]).toContain("Bahn: LOWS 15");
  });
});

describe("buildRolloutValueLabel", () => {
  it("BTX8815: echte Auslastung 983 m / 2850 m / 35 %", () => {
    // used = max(540.85 + 442.5, 442.5) = 983.35 → 983 m
    // pct  = round(983.35 / 2849.88 × 100) = round(34.5) = 35
    expect(buildRolloutValueLabel(record({}), t)).toBe(
      "35 % der Bahn · 983 m von 2850 m LDA",
    );
  });

  it("null wenn Pflichtfelder fehlen (Caller fällt auf Rust-value zurück)", () => {
    expect(buildRolloutValueLabel(record({ runway_match: null }), t)).toBe(
      null,
    );
    expect(
      buildRolloutValueLabel(record({ td_distance_from_threshold_m: null }), t),
    ).toBe(null);
  });
});

// ── v1.6.8: versetzte Schwelle — Anzeige und Punkte aus EINER Zahl ──────
//
// Der Client meldet den Versatz jetzt auch dann, wenn er in der
// Navdaten-Geometrie steckt statt im Zahlenfeld. Die Kachel muss
// dieselbe nutzbare Laenge und denselben Prozentsatz zeigen wie der
// Score-Kern im Rust-Crate, sonst faellt beides wieder auseinander.
describe("versetzte Schwelle in der Kachel", () => {
  // EDDH 33 aus den aktiven Navdaten: 12028 ft Bahn, 1464 ft Versatz.
  const EDDH_33: LandingRunwayMatch = {
    airport_ident: "EDDH",
    runway_ident: "33",
    surface: "ASP",
    length_ft: 12028,
    centerline_distance_m: 0.4,
    centerline_distance_abs_ft: 1,
    side: "CENTER",
    touchdown_distance_from_threshold_ft: 1969,
    displaced_threshold_ft: 1464,
  };
  const r = record({
    runway_match: EDDH_33,
    td_distance_from_threshold_m: 600,
    rollout_distance_m: 1400,
  });

  it("zieht den Versatz von der nutzbaren Laenge ab", () => {
    // 12028 ft = 3666 m, minus 1464 ft = 446 m → 3220 m
    expect(Math.round(rolloutLdaMeters(EDDH_33)!)).toBe(3220);
  });

  it("rechnet den Prozentsatz gegen die verkuerzte Bahn", () => {
    // genutzt 600 + 1400 = 2000 m von 3220 m = 62 %
    // (gegen die volle Bahn waeren es 55 % — ein Band besser)
    const label = buildRolloutValueLabel(r, t)!;
    expect(label).toContain("62 %");
    expect(label).toContain("2000 m");
    expect(label).toContain("3220 m");
  });

  it("nennt in der Bahn-Zeile dieselbe nutzbare Laenge", () => {
    const zeilen = buildRolloutExtraLines(r, t);
    const bahn = zeilen.find((z) => z.includes("EDDH"))!;
    expect(bahn).toContain("3220");
  });

  it("ohne Versatz bleibt alles wie bisher", () => {
    const ohne = record({
      runway_match: { ...EDDH_33, displaced_threshold_ft: 0 },
      td_distance_from_threshold_m: 600,
      rollout_distance_m: 1400,
    });
    expect(Math.round(rolloutLdaMeters({ ...EDDH_33, displaced_threshold_ft: 0 })!)).toBe(3666);
    expect(buildRolloutValueLabel(ohne, t)!).toContain("55 %");
  });
});
