import { describe, it, expect } from "vitest";
import { resolveFlightIdent } from "./callsign";

describe("resolveFlightIdent", () => {
  it("prefers a non-empty callsign over flight_number", () => {
    expect(resolveFlightIdent("0", "7ME")).toBe("7ME");
  });

  it("falls back to flight_number when callsign is null", () => {
    expect(resolveFlightIdent("1434", null)).toBe("1434");
  });

  it("falls back to flight_number when callsign is undefined", () => {
    expect(resolveFlightIdent("1434", undefined)).toBe("1434");
  });

  it("falls back to flight_number when callsign is an empty string", () => {
    expect(resolveFlightIdent("1434", "")).toBe("1434");
  });

  it("falls back to flight_number when callsign is only whitespace", () => {
    expect(resolveFlightIdent("1434", "   ")).toBe("1434");
  });

  it("trims a callsign with surrounding whitespace", () => {
    expect(resolveFlightIdent("0", "  7ME  ")).toBe("7ME");
  });

  it("still returns flight_number '0' when nothing else is available (never fabricates)", () => {
    expect(resolveFlightIdent("0", null)).toBe("0");
  });
});
