// P1/8b (Score-Version 26): TS-Zwilling der Rust-Tabelle
// `sub_bounces_mit_leichten` (landing-scoring/src/sub_bounces.rs) — dieselben
// Fälle wie dort. Auf dem alten Stand rot (85/70 für leichte gab es nicht).
import { describe, expect, it } from "vitest";
import { computeSubScores } from "./landingScoring";

const hopser = (normal: number, leicht?: number | null) =>
  computeSubScores({ bounce_count: normal, light_bounce_count: leicht }).find((s) => s.key === "bounces")!;

describe("Teilnote Hopser mit leichten Hopsern", () => {
  it.each([
    [0, null, 100, "clean_set"],
    [0, 0, 100, "clean_set"],
    [0, 1, 85, "one_light_bounce"],
    [0, 2, 70, "light_bounces"],
    [0, 4, 70, "light_bounces"],
    [1, 1, 70, "one_bounce"],
    [1, 3, 70, "one_bounce"],
    [2, 1, 40, "two_bounces"],
    [3, null, 15, "many_bounces"],
  ] as const)("normal %i, leicht %s → %i (%s)", (n, l, p, r) => {
    const s = hopser(n, l);
    expect(s.points).toBe(p);
    expect(s.rationale).toBe(r);
  });
  it("ohne Zählung der leichten (Altbestand) = alte Tabelle", () => {
    for (let n = 0; n < 6; n++) expect(hopser(n, undefined)).toEqual(hopser(n, null));
  });
});
