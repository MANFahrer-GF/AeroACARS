// v1.3.0 (#Hoppie-PDC-CPDLC) — '@' is a line break, not a character.
//
// Pinned to the real clearance vSMR emits (SMRPlugin.cpp:206-236). Before
// this, the pilot saw the whole thing as one unreadable line.

import { describe, it, expect } from "vitest";
import { datalinkLines, formatDatalinkText } from "./datalink";

// Verbatim shape of what the VATSIM UK controller plugin sends.
const VSMR_CLEARANCE =
  "CLR TO @EGLL@ RWY @27R@ DEP @DVR1G@ INIT CLB @FL060@ SQUAWK @1234@";

describe("datalinkLines", () => {
  it("breaks a vSMR clearance into readable lines", () => {
    expect(datalinkLines(VSMR_CLEARANCE)).toEqual([
      "CLR TO",
      "EGLL",
      "RWY",
      "27R",
      "DEP",
      "DVR1G",
      "INIT CLB",
      "FL060",
      "SQUAWK",
      "1234",
    ]);
  });

  it("renders an empty field as N/A instead of swallowing it", () => {
    // A controller who left the SID blank sends '@@'.
    expect(datalinkLines("CLR TO @EGLL@ DEP @@ SQUAWK @1234@")).toEqual([
      "CLR TO",
      "EGLL",
      "DEP",
      "N/A",
      "SQUAWK",
      "1234",
    ]);
  });

  it("leaves text without markers untouched", () => {
    expect(datalinkLines("CLIMB TO AND MAINTAIN FL240")).toEqual([
      "CLIMB TO AND MAINTAIN FL240",
    ]);
  });

  it("survives an odd number of markers", () => {
    // A paired-delimiter parser would misread this; '@' is just a break.
    expect(datalinkLines("MONITOR @118.500")).toEqual(["MONITOR", "118.500"]);
  });

  it("drops the gap a trailing marker would leave", () => {
    expect(datalinkLines("WILCO@")).toEqual(["WILCO"]);
    expect(datalinkLines("")).toEqual([]);
  });

  it("joins back to newline-separated text", () => {
    expect(formatDatalinkText("CLR TO @EGLL@")).toBe("CLR TO\nEGLL");
  });
});
