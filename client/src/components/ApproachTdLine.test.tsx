import { describe, it, expect } from "vitest";
import { approachTdLineIndex } from "./LandingPanel";

// v0.13.15 (Pilot-Befund ViolonC 2026-05-31): die rote TD-Linie im
// ANFLUG-STABILITÄT-Chart stand bei +0.5 s statt 0.0 s, weil das letzte
// Approach-Sample mit dem verspäteten Streamer-Tick gestempelt wird und
// `findIndex(t_ms >= 0)` auf dieses ~0.5 s zu späte Sample sprang.
describe("approachTdLineIndex", () => {
  it("interpolates between the last pre-TD and first post-TD sample (the real bug)", () => {
    // letztes Pre-TD-Sample bei -200 ms (= '0.0 s vor TD'), nächstes bei
    // +500 ms. TD (t=0) liegt 200/700 ≈ 0.286 zwischen Index 2 und 3.
    const samples = [
      { t_ms: -1200 },
      { t_ms: -700 },
      { t_ms: -200 },
      { t_ms: 500 },
    ];
    const idx = approachTdLineIndex(samples);
    expect(idx).toBeCloseTo(2 + 200 / 700, 5);
    // Entscheidend: die Linie sitzt NÄHER am letzten Pre-TD-Sample (Index 2)
    // als am verspäteten +0.5-s-Sample (Index 3) — genau Adrians Befund.
    expect(idx).toBeLessThan(2.5);
  });

  it("sits exactly on a sample that lands on t=0", () => {
    const samples = [{ t_ms: -500 }, { t_ms: 0 }, { t_ms: 500 }];
    expect(approachTdLineIndex(samples)).toBe(1);
  });

  it("falls back to the last sample when no post-TD sample exists", () => {
    const samples = [{ t_ms: -900 }, { t_ms: -400 }, { t_ms: -50 }];
    expect(approachTdLineIndex(samples)).toBe(2);
  });

  it("falls back to the last sample when t_ms is missing everywhere", () => {
    const samples = [{ t_ms: null }, {}, { t_ms: undefined }];
    expect(approachTdLineIndex(samples)).toBe(2);
  });

  it("returns 0 when the first sample is already at/after TD", () => {
    expect(approachTdLineIndex([{ t_ms: 0 }, { t_ms: 400 }])).toBe(0);
    expect(approachTdLineIndex([{ t_ms: 120 }, { t_ms: 400 }])).toBe(0);
  });

  it("handles empty input", () => {
    expect(approachTdLineIndex([])).toBe(0);
  });
});
