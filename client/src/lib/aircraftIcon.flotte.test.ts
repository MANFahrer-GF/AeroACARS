import { describe, expect, it } from "vitest";
import flotte from "../../../bundle/tabellen/flotte.json";
import { flotteUebernehmen, icaoCategory } from "./aircraftIcon";

describe("Flottentabelle aus dem Laufzeit-Paket (P-3.53)", () => {
  it("eingebauter Stand: Kategorien wie 2.1.24", () => {
    expect(icaoCategory("B77W")).toBe("heavy");
    expect(icaoCategory("H145")).toBe("heli");
    expect(icaoCategory("AT76")).toBe("turboprop");
    expect(icaoCategory("C172")).toBe("light");
    expect(icaoCategory("A320")).toBe("medium");
    expect(icaoCategory(null)).toBe("medium");
  });

  it("ein geladener Stand ersetzt die Listen, eine kaputte Datei nicht", () => {
    const neu = structuredClone(flotte);
    neu.kategorien.heavy.push("A321");
    expect(flotteUebernehmen(neu)).toBe(true);
    expect(icaoCategory("A321")).toBe("heavy");
    expect(flotteUebernehmen({ kategorien: { heavy: "A321" } })).toBe(false);
    expect(icaoCategory("A321")).toBe("heavy");
    expect(flotteUebernehmen(flotte)).toBe(true);
    expect(icaoCategory("A321")).toBe("medium");
  });
});
