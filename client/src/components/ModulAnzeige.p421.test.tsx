// v3 (P-4.21): Für 3.0-Flüge zeigt der Client Soll-Band und Farbgrenzen
// aus der Modulausgabe des Datensatzes (`modul_sollband`, `modul_grenzen`);
// nur 2.x-Datensätze ohne diese Felder nehmen den Altbestand-Pfad.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { ApproachChart } from "./AnflugGrafik";
import { gTone, pickGCoachingTip } from "./GForceForensik";
import { vsTone } from "./SinkrateForensik";
import { GRENZEN_ALTBESTAND_2X, grenzenAus, type Grenzen } from "../lib/bewertungsGrenzen";
import type { ApproachSample, ModulSollband } from "../lib/landungsDatensatz";

const proben: ApproachSample[] = Array.from({ length: 30 }, (_, i) => ({
  vs_fpm: -700, bank_deg: 0, t_ms: (i - 30) * 1000, agl_ft: 900 - i * 25,
  is_scored_gate: true, is_flare: false, gs_kt: 135,
}));

const bandPfade = (c: HTMLElement) =>
  [...c.querySelectorAll("path")].map((p) => p.getAttribute("d") ?? "").filter((d) => d.endsWith("Z"));

describe("Soll-Band aus dem Modul", () => {
  it("zeichnet die Punkte des Moduls, nicht die eigene Rechnung", () => {
    const ohne = render(<ApproachChart samples={proben} />);
    const altBand = bandPfade(ohne.container);
    expect(altBand.length).toBe(1);
    // Ein Modul mit anderem Band (−300 fpm, ±50): die Grafik folgt ihm.
    const modul: ModulSollband = {
      punkte: proben.map((_, index) => ({ index, soll: -300, oben: -250, unten: -350 })),
      faktor: 1,
      toleranz: 50,
    };
    const mit = render(<ApproachChart samples={proben} modulSollband={modul} />);
    const neuBand = bandPfade(mit.container);
    expect(neuBand.length).toBe(1);
    expect(neuBand[0]).not.toBe(altBand[0]);
    expect(mit.container.textContent).toContain("±50");
  });

  it("gleiche Modulpunkte wie die alte Rechnung ergeben dasselbe Bild", () => {
    const ohne = render(<ApproachChart samples={proben} />);
    const punkte = proben.map((p, index) => ({ index, soll: -p.gs_kt! * 5.31, oben: -p.gs_kt! * 5.31 + 100, unten: -p.gs_kt! * 5.31 - 100 }));
    const mit = render(<ApproachChart samples={proben} modulSollband={{ punkte, faktor: 1, toleranz: 100 }} />);
    expect(mit.container.innerHTML).toBe(ohne.container.innerHTML);
  });
});

describe("Farbgrenzen aus dem Modul", () => {
  const modul: Grenzen = { vs_fpm: [150, 300, 500, 900], g: [1.1, 1.3, 1.6, 2.0] };

  it("3.0-Datensatz: Grenzen des Moduls, 2.x: Altbestand", () => {
    expect(grenzenAus({ modul_grenzen: modul })).toBe(modul);
    expect(grenzenAus({})).toBe(GRENZEN_ALTBESTAND_2X);
    // 1,35 G ist beim Altbestand „neutral" (< 1,4), beim Modul „warn" (>= 1,3).
    expect(gTone(1.35)).toBe("neutral");
    expect(gTone(1.35, modul)).toBe("warn");
    // 450 fpm: Altbestand „warn" (400–600), Modul „warn" (300–500), 550: „err" nur beim Modul.
    expect(vsTone(550)).toBe("warn");
    expect(vsTone(550, modul)).toBe("err");
    expect(pickGCoachingTip({ vsAtEdgeFpm: -350, gPeak: 1.35, gMedian: 1.2, gP95: 1.3, maxGearForceN: 0 })).toBe("clean");
    expect(pickGCoachingTip({ vsAtEdgeFpm: -350, gPeak: 1.35, gMedian: 1.2, gP95: 1.3, maxGearForceN: 0, grenzen: modul })).toBe("real_impact");
  });
});
