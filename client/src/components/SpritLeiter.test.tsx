/**
 * v1.7.36 — die Sprit-Leiter hält, was sie zeigt.
 *
 * Zwei Zusagen stehen im Code der Leiter, und beide waren bis v1.7.35
 * gebrochen, ohne dass ein Test es merkte:
 *
 *  1. „Der Abstand zwischen den Marken IST der Verbrauch." Die Abhebe-Marke
 *     verschwand bei kleinem Abstand ganz — wo nichts steht, liest niemand
 *     einen Abstand ab.
 *  2. Die Landemarke trifft denselben Punkt, den die Zeile darunter nennt.
 *     Sie tat es nicht: Bei DLH 370 las die Grafik 3 581 kg übriges Extra,
 *     die Zeile sagte 3 308 — die Übertankung von 273 kg fehlte in der
 *     Grafik.
 */
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { SpritSektion } from "./SpritSektion";
import type { SpritAuswertung } from "../lib/sprit";

function dlh370(ueber: Partial<SpritAuswertung> = {}): SpritAuswertung {
  return {
    fassung: 3,
    bis_sinkflug: { ist_kg: 18688, plan_kg: 19353, abweichung_pct: -3.4, als_kg: false },
    anflug: { ist_kg: 5461, plan_kg: 1865, abweichung_pct: 192.8, als_kg: true },
    zeit_unter_schwelle_min: 15.8,
    schwelle_ft: 9487,
    strecke_anflug_nm: 182,
    plan_strecke_anflug_nm: 139,
    reserve: { status: "intakt", quote_pct: 341.1 },
    reserve_kg: 4916,
    takeoff_fuel_kg: 40919,
    landing_fuel_kg: 16770,
    extra_getankt_kg: 5178,
    extra_genutzt_kg: 1870,
    extra_ungenutzt_kg: 3308,
    contingency_verbraucht: true,
    alternate_und_reserve_intakt: true,
    leiter: {
      taxi_kg: 998,
      trip_kg: 21218,
      contingency_kg: 1061,
      alternate_kg: 8273,
      reserve_kg: 4916,
      extra_kg: 5178,
      block_kg: 41644,
      sonstiges_kg: 0,
      // Aus der Rechnung: 40 919 abgehoben + 998 Taxi − 41 644 Block.
      uebertankung_kg: 273,
    },
    rollen_vor_start: null,
    rollen_nach_landung_kg: null,
    badge: "gruen",
    ...ueber,
  } as SpritAuswertung;
}

/** Die x-Position einer gestrichelten Linie (Abhebe-Marke). */
function abhebeMarke(c: HTMLElement): number | null {
  const l = c.querySelector('line[stroke-dasharray]');
  return l ? Number(l.getAttribute("x1")) : null;
}

/** Die x-Position der Landemarke (durchgezogene Linie). */
function landeMarke(c: HTMLElement): number | null {
  const l = Array.from(c.querySelectorAll("line")).find(
    (e) => !e.getAttribute("stroke-dasharray"),
  );
  return l ? Number(l.getAttribute("x1")) : null;
}

describe("Sprit-Leiter", () => {
  it("die Landemarke trifft, was die Zeile darunter sagt", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const W = 560;
    // Die Skala trägt die Übertankung: 40 919 + 998 − 41 644 = 273 kg.
    const skala = 41644 + 273;
    const mark = landeMarke(container);
    expect(mark).not.toBeNull();

    // Von rechts gemessen steht die Marke bei 16 770 kg Landesprit.
    const kgAnDerMarke = (1 - mark! / W) * skala;
    expect(kgAnDerMarke).toBeCloseTo(16770, 0);

    // Und das Extra, das dabei übrig bleibt, ist genau die Zahl aus der
    // Auswertung — nicht 3 581, wie die Grafik bis v1.7.35 auswies.
    const untenImExtra = 4916 + 8273 + 273; // Reserve + Alternate + Übertank
    expect(kgAnDerMarke - untenImExtra).toBeCloseTo(3308, 0);
  });

  it("der Abstand zwischen den Marken IST der Verbrauch", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const W = 560;
    const skala = 41644 + 273;
    const ab = abhebeMarke(container);
    const mark = landeMarke(container);
    expect(ab).not.toBeNull();
    expect(mark).not.toBeNull();
    const verbrauchtLautGrafik = ((mark! - ab!) / W) * skala;
    expect(verbrauchtLautGrafik).toBeCloseTo(40919 - 16770, 0);
  });

  it("zeigt die Abhebe-Marke auch bei kurzem Flug", () => {
    // Abheben und Landen fast gleich — bis v1.7.35 verschwand die Marke.
    const kurz = dlh370({ landing_fuel_kg: 40800 });
    const { container } = render(<SpritSektion sprit={kurz} />);
    expect(abhebeMarke(container)).not.toBeNull();
  });

  it("hält die Abhebe-Marke im Bild, wenn über Plan getankt wurde", () => {
    // 43 919 kg abgehoben bei 41 644 kg Plan-Block: Ohne die mitwachsende
    // Skala wurde die Marke auf x=0 geklemmt, während ihr Etikett den
    // echten Wert nannte.
    const basis = dlh370();
    const tanker = dlh370({
      takeoff_fuel_kg: 43919,
      // Die Rechnung liefert die Übertankung mit: 43 919 + 998 − 41 644.
      leiter: { ...basis.leiter!, uebertankung_kg: 3273 },
    });
    const { container } = render(<SpritSektion sprit={tanker} />);
    const ab = abhebeMarke(container);
    expect(ab).not.toBeNull();
    expect(ab!).toBeGreaterThan(0);
    const W = 560;
    const skala = 41644 + (43919 + 998 - 41644);
    expect((1 - ab! / W) * skala).toBeCloseTo(43919, 0);
  });
});
