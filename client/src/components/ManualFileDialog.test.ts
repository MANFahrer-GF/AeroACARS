// v0.19.x FIX: numOrNull used to collapse both "12.500" and "12,500" to
// 12.5 (blind comma->dot replace, no thousands-grouping awareness) — a
// pilot typing a normal block-fuel figure would silently file a PIREP
// with 12.5 kg of fuel instead of 12,500 kg.

import { describe, it, expect } from "vitest";
import { numOrNull } from "./ManualFileDialog";

describe("numOrNull", () => {
  it("parses a German thousands-grouped value (dot) as the full number", () => {
    expect(numOrNull("12.500")).toBe(12500);
  });

  it("parses a US/comma thousands-grouped value as the full number", () => {
    expect(numOrNull("12,500")).toBe(12500);
  });

  it("parses a German decimal comma as a fraction", () => {
    expect(numOrNull("12,5")).toBe(12.5);
  });

  it("parses a plain dot decimal as a fraction", () => {
    expect(numOrNull("437.3")).toBeCloseTo(437.3);
  });

  it("parses a full German-style grouped decimal (dot thousands, comma decimal)", () => {
    expect(numOrNull("12.500,5")).toBeCloseTo(12500.5);
  });

  it("parses a full US-style grouped decimal (comma thousands, dot decimal)", () => {
    expect(numOrNull("12,500.5")).toBeCloseTo(12500.5);
  });

  it("parses repeated thousands separators of the same kind", () => {
    expect(numOrNull("1.234.567")).toBe(1234567);
    expect(numOrNull("1,234,567")).toBe(1234567);
  });

  it("passes through a plain integer or negative number unchanged", () => {
    expect(numOrNull("-180")).toBe(-180);
    expect(numOrNull("35000")).toBe(35000);
  });

  it("returns null for empty or invalid input", () => {
    expect(numOrNull("")).toBeNull();
    expect(numOrNull("   ")).toBeNull();
    expect(numOrNull("abc")).toBeNull();
  });
});
