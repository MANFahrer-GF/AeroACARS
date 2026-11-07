import { describe, expect, it } from "vitest";
import { datumArt, datumText, spurAusTrack } from "./datum";

describe("Gruppendatum", () => {
  const jetzt = new Date(2026, 10, 7, 15, 0).getTime();
  it("heute, gestern, sonst Datum (um Mitternacht, nicht nach 24 Stunden)", () => {
    expect(datumArt(new Date(2026, 10, 7, 0, 5).getTime(), jetzt).art).toBe("heute");
    expect(datumArt(new Date(2026, 10, 6, 23, 59).getTime(), jetzt).art).toBe("gestern");
    expect(datumArt(new Date(2026, 10, 5, 23, 59).getTime(), jetzt).art).toBe("datum");
  });
  it("Datumstext je Sprache", () => {
    expect(datumText(new Date(2026, 10, 7).getTime(), "de")).toBe("07.11.2026");
  });
  it("Track [lon, lat] wird [lat, lon]; Muell wird uebergangen", () => {
    expect(spurAusTrack([[8.5, 50], [9, 49.5], "x", [1]])).toEqual([[50, 8.5], [49.5, 9]]);
    expect(spurAusTrack(null)).toEqual([]);
  });
});
