import { describe, it, expect } from "vitest";
import { resolveFlightIdent, displayCallsign } from "./callsign";

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

describe("displayCallsign — keine doppelten Airline-Praefixe", () => {
  // Die Faelle stammen 1:1 aus den Rust-Tests zu `with_display_callsign`
  // (panel_server.rs, v1.5.6). Beide Seiten muessen dieselbe Regel fahren,
  // sonst zeigt das HUD etwas anderes als das Fenster daneben.
  it("setzt das Praefix, wenn der Bezeichner es nicht traegt", () => {
    expect(displayCallsign("GEC", "0", "4TK")).toBe("GEC4TK");
    expect(displayCallsign("CFG", "0", "7ME")).toBe("CFG7ME");
  });

  it("doppelt das Praefix NICHT, wenn die VA es schon mitliefert", () => {
    expect(displayCallsign("GEC", "0", "GEC4TK")).toBe("GEC4TK");
    expect(displayCallsign("DLH", "155", "DLH155")).toBe("DLH155");
  });

  it("faellt auf die Flugnummer zurueck, wenn kein Rufzeichen da ist", () => {
    expect(displayCallsign("DLH", "155")).toBe("DLH155");
    expect(displayCallsign("DLH", "155", "   ")).toBe("DLH155");
  });

  it("kommt ohne Airline-Code aus", () => {
    expect(displayCallsign(null, "155")).toBe("155");
    expect(displayCallsign("", "0", "7ME")).toBe("7ME");
  });

  it("liefert den Airline-Code, wenn sonst nichts da ist", () => {
    expect(displayCallsign("DLH", "")).toBe("DLH");
  });

  it("greift auch im Landungsbericht, wo flight_number schon aufgeloest ist", () => {
    // `build_landing_record` schreibt den aufgeloesten Bezeichner in
    // `flight_number` — traegt der das Praefix, darf es nicht doppeln.
    expect(displayCallsign("GEC", "GEC4TK")).toBe("GEC4TK");
  });
});
