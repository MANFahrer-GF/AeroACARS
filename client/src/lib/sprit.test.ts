/**
 * v1.7.35 — der Farbton der Sprit-Phasen.
 *
 * Diese Zusicherungen halten fest, was das Release ausmacht: Der Sprit wird
 * gezeigt, nicht angeklagt. Ohne sie könnte jemand die Warnfarbe
 * zurückbringen, ohne dass ein Test es merkt.
 */
import { describe, expect, it } from "vitest";
import { contingencyGenutzt, dezimal, minuten, pct, phaseTon, reserveAbstand } from "./sprit";
import type { SpritAuswertung, SpritPhase } from "./sprit";

const phase = (pct: number): SpritPhase => ({
  ist_kg: 1000,
  plan_kg: 1000,
  abweichung_pct: pct,
  als_kg: false,
});

describe("phaseTon", () => {
  it("gibt für sparsame Flüge die ruhige Farbe — bis Sinkflug", () => {
    expect(phaseTon(phase(-8.5), "bis_sinkflug")).toBe("ok");
    expect(phaseTon(phase(3), "bis_sinkflug")).toBe("ok");
  });

  it("warnt NIE — auch nicht bei extremem Mehrverbrauch", () => {
    // Die Achse ist aus der Note geflogen, weil sie die Flugsicherung
    // benotete. Eine Warnfarbe wäre derselbe Vorwurf in Rot.
    for (const p of [3.1, 20, 47, 192.8, 342.2]) {
      expect(phaseTon(phase(p), "bis_sinkflug")).toBe("neutral");
      expect(phaseTon(phase(p), "anflug")).toBe("neutral");
    }
  });

  it("färbt den Anflug auch bei Sparsamkeit nicht ein", () => {
    // Der Anflug misst überwiegend die Radarführung — in beide Richtungen
    // ist das keine Aussage über den Piloten.
    expect(phaseTon(phase(-67.6), "anflug")).toBe("neutral");
    expect(phaseTon(phase(-10), "anflug")).toBe("neutral");
  });

  it("gibt ohne Phase neutral zurück", () => {
    expect(phaseTon(null)).toBe("neutral");
    expect(phaseTon(null, "anflug")).toBe("neutral");
  });

  it("behandelt eine Phase ohne Angabe der Art wie „bis Sinkflug“", () => {
    expect(phaseTon(phase(-5))).toBe("ok");
    expect(phaseTon(phase(50))).toBe("neutral");
  });
});

describe("dezimal", () => {
  it("behält das Vorzeichen — als echtes Minus", () => {
    // Bis v1.7.36-Entwurf fiel es still weg (QS-Vorschlag V-e).
    expect(dezimal(-3.4, 1)).toBe("−3,4");
    expect(dezimal(3.4, 1)).toBe("3,4");
    // Eine auf null gerundete Zahl bekommt keins.
    expect(dezimal(-0.04, 1)).toBe("0,0");
  });

  it("setzt in pct genau ein Vorzeichen", () => {
    expect(pct(-3.4)).toBe("−3,4 %");
    expect(pct(192.8)).toBe("+192,8 %");
    expect(pct(0)).toBe("0,0 %");
  });
});

describe("reserveAbstand", () => {
  const basis = (ueber: Partial<SpritAuswertung>) =>
    ({ reserve: { status: "intakt", quote_pct: 338.3 }, landing_fuel_kg: 2476, reserve_kg: 732, ...ueber }) as SpritAuswertung;

  it("nimmt den Wert aus der Rechnung — nicht den Rückfall", () => {
    // Bewusst ein anderer Wert als 2 476 − 732: Sonst bliebe der Test grün,
    // wenn das Feld ignoriert würde (QS v1.7.37).
    expect(reserveAbstand(basis({ reserve_abstand_kg: 1700 }))).toBe(1700);
  });

  it("zeigt bei knapper Unterschreitung nie „− 0 kg“", () => {
    // Gerundet gleich, trotzdem darunter — dieselbe Klemme wie sprit.rs.
    expect(reserveAbstand(basis({ reserve: { status: "unterschritten", quote_pct: 99.9 }, landing_fuel_kg: 732 }))).toBe(-1);
  });

  it("gibt älteren Datensätzen dieselbe Zahl aus den gespeicherten Werten", () => {
    // BIT348 wurde mit v1.7.36 gerechnet — ohne das Feld.
    expect(reserveAbstand(basis({}))).toBe(2476 - 732);
    expect(reserveAbstand(basis({ reserve: { status: "unterschritten", quote_pct: 83.6 }, landing_fuel_kg: 612 }))).toBe(-120);
  });

  it("nennt keinen Abstand, wenn die Reserve nicht prüfbar ist", () => {
    expect(reserveAbstand(basis({ reserve: { status: "nicht_pruefbar", grund: "kein_ofp" }, reserve_abstand_kg: 5 }))).toBeNull();
    expect(reserveAbstand(basis({ reserve_kg: null }))).toBeNull();
  });
});

describe("minuten", () => {
  it("rundet auf ganze Minuten, zeigt gemessene Kurzzeit nie als 0", () => {
    expect(minuten(14.3)).toBe("14");
    expect(minuten(15.8)).toBe("16");
    expect(minuten(0.3)).toBe("< 1");
    expect(minuten(0)).toBe("0");
  });
});

describe("contingencyGenutzt — Rückfall für Altbestand", () => {
  const leiter = { taxi_kg: 227, trip_kg: 2115, contingency_kg: 238, alternate_kg: 1927, reserve_kg: 1130, extra_kg: 0, block_kg: 5637, sonstiges_kg: 0, uebertankung_kg: 0 };
  const alt = (ldg: number) => ({ contingency_verbraucht: false, landing_fuel_kg: ldg, leiter }) as unknown as SpritAuswertung;
  it("rechnet wie sprit.rs und begrenzt auf die Contingency", () => {
    expect(contingencyGenutzt(alt(3209))).toBe(86);
    expect(contingencyGenutzt(alt(3400))).toBe(0);
    // Weit über die Contingency hinaus: gedeckelt, nicht 395.
    expect(contingencyGenutzt(alt(2900))).toBe(238);
  });
  it("nimmt das Feld aus der Rechnung, wenn es da ist", () => {
    expect(contingencyGenutzt({ ...alt(3209), contingency_genutzt_kg: 50 } as SpritAuswertung)).toBe(50);
  });
  it("sagt nichts ohne Aussage der Rechnung", () => {
    expect(contingencyGenutzt({ ...alt(3209), contingency_verbraucht: null } as SpritAuswertung)).toBeNull();
  });
});
