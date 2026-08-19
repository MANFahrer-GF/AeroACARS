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
    expect(p.initialClimb).toBe("5000FT");
    expect(p.depFreq).toBe("121.150");
    expect(p.atis).toBe("K");
    expect(p.qnh).toBe("1011");
    expect(p.ctot).toBe("1436");
    expect(p.recognized).toBe(true);
  });

  it("keeps every unrecognized stretch, unchanged, in original order", () => {
    const p = parseUplink(PDC_REPLY);
    expect(p.conditions).toEqual([
      "CLD BTI4TK",
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

// --- v1.6.12 (#pdc-station): the real LROP clearance from 19.08.2026 ---
//
// Verbatim off the wire, '@' line breaks and all — including the sender's
// missing space in "WMT4TKCLRD". Every finding below was visible on the
// pilot's screen: an empty INITIAL CLIMB cell, an ATIS letter of "R"
// that ATC never sent, and the whole header sitting in the conditions
// line as if the controller had attached it.
const LROP_CLEARANCE =
  "CLD 0843 260819 LROP PDC 001 WMT4TKCLRD TO@EDDB@OFF@08L@VIA@SOKRU1K@CLIMB@FL280@" +
  "SQUAWK@1000@NEXT FREQ@121.855@ATIS REQ STARTUP ON@121.855";

describe("parseUplink — LROP field case", () => {
  it("takes the flight-level climb out of the text and into the grid", () => {
    expect(parseUplink(LROP_CLEARANCE).initialClimb).toBe("FL280");
  });

  it("never reads an ATIS letter out of a word", () => {
    const p = parseUplink(LROP_CLEARANCE);
    expect(p.atis).toBeNull();
    expect(p.conditions).toContain("ATIS REQ STARTUP ON 121.855");
  });

  it("still reads a genuine ATIS letter", () => {
    expect(parseUplink("CLRD TO EDDB ATIS B SQUAWK 1000").atis).toBe("B");
  });

  it("picks up the destination even when glued to the callsign", () => {
    expect(parseUplink(LROP_CLEARANCE).dest).toBe("EDDB");
  });

  it("reads the header as a reference, not as a condition", () => {
    const p = parseUplink(LROP_CLEARANCE);
    expect(p.header).toEqual({
      kind: "CLD",
      time: "0843",
      date: "260819",
      icao: "LROP",
      ref: "PDC 001",
    });
    expect(p.conditions.join(" ")).not.toContain("PDC 001");
  });

  it("leaves exactly the station's own instruction in the conditions", () => {
    expect(parseUplink(LROP_CLEARANCE, "WMT4TK").conditions).toEqual([
      "ATIS REQ STARTUP ON 121.855",
    ]);
  });

  it("drops our own callsign only when it stands alone", () => {
    expect(parseUplink(LROP_CLEARANCE).conditions).toContain("WMT4TK");
    const withInstruction = parseUplink("WMT4TK CONTACT GROUND SQUAWK 1000", "WMT4TK");
    expect(withInstruction.conditions).toEqual(["WMT4TK CONTACT GROUND"]);
  });

  it("reads the rest of the clearance as before", () => {
    const p = parseUplink(LROP_CLEARANCE);
    expect(p.squawk).toBe("1000");
    expect(p.sid).toBe("SOKRU1K");
    expect(p.rwy).toBe("08L");
    expect(p.depFreq).toBe("121.855");
    expect(p.ctot).toBeNull();
    expect(p.qnh).toBeNull();
  });

  it("does not turn an FSM status message into an empty grid", () => {
    const fsm = parseUplink(
      "FSM 0853 260819 LROP WMT4TK@WMT4TK@ACK NOT RECEIVED@CLEARANCE CANCELLED@REVERT TO VOICE PROCEDURES",
    );
    expect(fsm.recognized).toBe(false);
    expect(fsm.header?.kind).toBe("FSM");
  });
});
