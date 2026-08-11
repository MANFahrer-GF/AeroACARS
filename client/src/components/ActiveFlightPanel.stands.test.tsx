// v1.5.5 Stand-Erkennung: die Standzeile unter der Route zeigt erkannte
// Stände (OSM) — und darf bei Flügen ohne Daten (alte Clients, kleine
// Plätze) schlicht fehlen statt "Stand null" zu rendern.
import { describe, it, expect } from "vitest";
import { standsLine } from "./ActiveFlightPanel";

describe("standsLine (v1.5.5 Stand-Erkennung)", () => {
  it("renders both stands when known", () => {
    expect(standsLine("V106", "203")).toBe("Stand V106 → 203");
  });

  it("renders departure-only during the flight", () => {
    expect(standsLine("V106", null)).toBe("Stand V106");
  });

  it("renders arrival-only when the departure stand was never captured", () => {
    expect(standsLine(null, "203")).toBe("Stand → 203");
  });

  it("hides the line entirely without data", () => {
    expect(standsLine(null, null)).toBeNull();
    expect(standsLine(undefined, undefined)).toBeNull();
    // Leere/Whitespace-Strings sind "nicht erkannt", kein "Stand  →".
    expect(standsLine("", " ")).toBeNull();
  });
});
