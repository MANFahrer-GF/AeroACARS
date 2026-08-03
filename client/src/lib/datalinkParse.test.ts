// v1.3.5 (#Datalink-3a) — parser regression tests.
//
// Fixtures here double as the "test data" the field couldn't supply for
// the datalink redesign (no live CPDLC traffic was on hand to demo
// against) — a synthetic but wire-realistic PDC reply and a synthetic
// CPDLC WU uplink, both built to the same shapes real Hoppie/vSMR traffic
// uses (see hoppie-protocol's own fixtures for the verified formats).

import { describe, it, expect } from "vitest";
import { parseUplink, formatCtot } from "./datalinkParse";

const PDC_REPLY =
  "CLD BTI4TK CLRD TO EDDM OFF 14L VIA DOMUX2N SQUAWK 4231 INITIAL CLIMB 5000FT NEXT FREQ 121.150 ATIS K QNH 1011 CTOT 1436 SET SQUAWK BEFORE PUSH CONTACT EDDK_GND FOR PUSH";

describe("parseUplink", () => {
  it("extracts all eight fields regardless of position", () => {
    const p = parseUplink(PDC_REPLY);
    expect(p.rwy).toBe("14L");
    expect(p.sid).toBe("DOMUX2N");
    expect(p.squawk).toBe("4231");
    expect(p.initialClimb).toBe("5000");
    expect(p.depFreq).toBe("121.150");
    expect(p.atis).toBe("K");
    expect(p.qnh).toBe("1011");
    expect(p.ctot).toBe("1436");
    expect(p.recognized).toBe(true);
  });

  it("keeps every unrecognized stretch, unchanged, in original order", () => {
    const p = parseUplink(PDC_REPLY);
    expect(p.conditions).toEqual([
      "CLD BTI4TK CLRD TO EDDM",
      "FT",
      "SET SQUAWK BEFORE PUSH CONTACT EDDK_GND FOR PUSH",
    ]);
  });

  it("never invents a value for a field the telex doesn't contain", () => {
    const p = parseUplink("CLD BTI4TK CLRD TO EDDM SQUAWK 4231");
    expect(p.squawk).toBe("4231");
    expect(p.sid).toBeNull();
    expect(p.ctot).toBeNull();
    expect(p.rwy).toBeNull();
  });

  it("only matches SQUAWK when followed by exactly four digits", () => {
    // "SET SQUAWK BEFORE PUSH" must not be mistaken for a squawk value.
    const p = parseUplink("SET SQUAWK BEFORE PUSH SQUAWK 7000");
    expect(p.squawk).toBe("7000");
    expect(p.conditions).toEqual(["SET SQUAWK BEFORE PUSH"]);
  });

  it("reports parsing failure when no field is recognized at all", () => {
    const p = parseUplink("STANDBY FOR FURTHER INSTRUCTIONS");
    expect(p.recognized).toBe(false);
    expect(p.conditions).toEqual(["STANDBY FOR FURTHER INSTRUCTIONS"]);
    expect(p.squawk).toBeNull();
  });

  it("keeps the trimmed raw text verbatim alongside the parse", () => {
    const p = parseUplink("  SQUAWK 4231  \n");
    expect(p.raw).toBe("SQUAWK 4231");
  });

  it("handles a synthetic CPDLC-style uplink with only some fields present", () => {
    // Not a real GOLD element string — a free-text uplink that slipped
    // through without a recognized /data2/ header (see ThreadEntry's
    // "kind" doc comment), which is exactly when this parser applies to
    // CPDLC traffic too.
    const p = parseUplink("PROCEED DIRECT TO UDROS MAINTAIN FL050 QNH 1013");
    expect(p.qnh).toBe("1013");
    expect(p.squawk).toBeNull();
    expect(p.recognized).toBe(true);
    expect(p.conditions).toEqual(["PROCEED DIRECT TO UDROS MAINTAIN FL050"]);
  });
});

describe("formatCtot", () => {
  it("inserts the colon and UTC marker", () => {
    expect(formatCtot("1436")).toBe("14:36z");
  });

  it("pads a three-digit value", () => {
    expect(formatCtot("905")).toBe("09:05z");
  });

  it("passes through anything that isn't 3-4 digits unchanged", () => {
    expect(formatCtot("N/A")).toBe("N/A");
  });
});
