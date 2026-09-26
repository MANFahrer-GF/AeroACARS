import { describe, expect, it } from "vitest";
import { LUECKE_MS, hatLuecke, zusammenfuehren } from "./luecken";
import type { Frame } from "./typen";

const f = (t: number, v = 0): Frame => ({ t, z: [v], s: [] });

describe("Luecken im Telemetrie-Strom", () => {
  it("erkennt eine Unterbrechung am Zeitstempel", () => {
    expect(hatLuecke(null, f(0))).toBe(false);
    expect(hatLuecke(f(0), f(100))).toBe(false);
    expect(hatLuecke(f(0), f(LUECKE_MS + 1))).toBe(true);
  });

  it("fuegt nachgeladene Frames zeitrichtig ein, ohne Doppelte", () => {
    const vorhanden = [f(0, 1), f(100, 1), f(3000, 1), f(3100, 1)];
    const nachgeladen = [f(100, 2), f(1000, 2), f(2000, 2), f(3000, 2)];
    const r = zusammenfuehren(vorhanden, nachgeladen);
    expect(r.map((x) => x.t)).toEqual([0, 100, 1000, 2000, 3000, 3100]);
    // Bei gleicher Zeit bleibt die vorhandene Fassung.
    expect(r.find((x) => x.t === 100)!.z[0]).toBe(1);
    expect(r.find((x) => x.t === 1000)!.z[0]).toBe(2);
  });

  it("leere Listen", () => {
    expect(zusammenfuehren([], [f(1)]).map((x) => x.t)).toEqual([1]);
    expect(zusammenfuehren([f(1)], []).map((x) => x.t)).toEqual([1]);
  });
});
