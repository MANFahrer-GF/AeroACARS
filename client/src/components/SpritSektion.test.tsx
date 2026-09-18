/**
 * v1.7.35 — Badge im Kopf und Sektion im Landungs-Tab rendern
 * `record.sprit` und rechnen nichts nach. Zahlen: DLH370, 17.09.2026.
 */
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { SpritBadge, SpritSektion } from "./SpritSektion";
import type { SpritAuswertung } from "../lib/sprit";
import { kg, pct } from "../lib/sprit";

function dlh370(): SpritAuswertung {
  return {
    fassung: 2,
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
    badge: "gruen",
  };
}

describe("SpritBadge", () => {
  it("steht grün, wenn die Reserve intakt ist", () => {
    render(<SpritBadge sprit={dlh370()} />);
    const b = screen.getByTestId("sprit-badge");
    expect(b.dataset.ton).toBe("gruen");
    expect(b.textContent).toContain("Reserve intakt");
    // Kein Zahlenwert im Badge — die Zahl steht in der Sektion.
    expect(b.textContent).not.toMatch(/\d/);
  });

  it("steht gelb bei Unterschreitung und niemals rot", () => {
    const a = dlh370();
    a.badge = "gelb";
    a.reserve = { status: "unterschritten", quote_pct: 69.3 };
    render(<SpritBadge sprit={a} />);
    const b = screen.getByTestId("sprit-badge");
    expect(b.dataset.ton).toBe("gelb");
    expect(b.textContent).toContain("unter Final Reserve");
    expect(b.innerHTML).not.toContain("#ff5c4d");
  });

  it("steht grau, wenn nichts prüfbar ist", () => {
    const a = dlh370();
    a.badge = "grau";
    a.reserve = { status: "nicht_pruefbar", grund: "kein_ofp" };
    render(<SpritBadge sprit={a} />);
    expect(screen.getByTestId("sprit-badge").textContent).toContain("nicht prüfbar");
  });

  it("rendert nichts ohne Auswertung (Altbestand)", () => {
    const { container } = render(<SpritBadge sprit={null} />);
    expect(container.firstChild).toBeNull();
  });
});

/**
 * Die Skala der Leiter — sie traegt seit v1.7.36 die Uebertankung mit.
 *
 * Wer mehr tankt als geplant, hat Sprit an Bord, den der Plan-Block nicht
 * kennt. Ohne ihn liefen Grafik und Textzeile 273 kg auseinander (DLH 370),
 * und die Abhebe-Marke wurde beim Tankern auf x=0 geklemmt.
 */
function skalaVon(a: SpritAuswertung): number {
  // Aus den Daten, nicht nachgerechnet — so wie die Anzeige selbst.
  const l = a.leiter!;
  return l.block_kg + (l.uebertankung_kg ?? 0);
}

describe("SpritSektion", () => {
  it("zeigt beide Phasen mit Ist, Plan und Abweichung", () => {
    render(<SpritSektion sprit={dlh370()} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("−3,4 %");
    // Die Hauptzahl des Anflugs steht in Kilogramm — wie in der Live-
    // Übersicht und im Bericht. Der Prozentwert steht daneben in der
    // Nebenzeile. Vorher prüfte dieser Test NUR die Nebenzeile und wäre
    // auch grün geblieben, wenn hauptzahl() gar nicht gegriffen hätte.
    expect(t).toContain("+3\u202f596 kg");
    expect(t).toContain("+192,8 %");
    expect(t).toContain("18\u202f688");
    expect(t).toContain("19\u202f353");
  });

  it("nennt Zeit unter der Schwelle und die Anflugstrecke", () => {
    render(<SpritSektion sprit={dlh370()} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("15,8 min");
    expect(t).toContain("9\u202f487 ft");
    expect(t).toContain("182 NM");
  });

  it("zeigt Reserve und Extra als Klartext", () => {
    render(<SpritSektion sprit={dlh370()} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("intakt");
    expect(t).toContain("341 %");
    expect(t).toContain("5\u202f178");
    expect(t).toContain("3\u202f308");
  });

  it("sagt ausdrücklich, dass es keine Note gibt", () => {
    render(<SpritSektion sprit={dlh370()} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("keine Note");
    expect(t).not.toMatch(/PTS|Punkte/);
  });

  it("zeichnet die Sprit-Leiter mit Landemarke", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    // Sieben Segmente: die sechs Plan-Bloecke plus die Uebertankung — diese
    // Vorlage ist mit 273 kg ueber dem Block angelassen worden (41 917 kg).
    // Beim echten DLH 370 war der Tank beim Anlassen genau der Block.
    expect(svg!.querySelectorAll("rect").length).toBe(7);
    // Zwei Marken: womit abgehoben (gestrichelt), womit gelandet (kräftig).
    expect(svg!.querySelectorAll("line").length).toBe(2);
    expect(svg!.textContent).toContain("gelandet mit");
    expect(svg!.textContent).toContain("Contingency verbraucht");
  });


  it("zeichnet die Landemarke auf den tatsächlichen Spritstand", () => {
    // Vorher rechnete die Grafik den Landesprit gegen die geplante Leiter,
    // die Zeile daneben gegen den echten Abhebe-Tankstand — 273 kg
    // Widerspruch bei DLH370 (QS-Befund P2-2).
    const a = dlh370();
    const { container } = render(<SpritSektion sprit={a} />);
    const svg = container.querySelector("svg")!;
    const W = 560;
    const x = (v: number) => (v / skalaVon(a)) * W;
    const landung = [...svg.querySelectorAll("line")].find(
      (e) => e.getAttribute("stroke-width") === "2",
    )!;
    const markX = Number(landung.getAttribute("x1"));
    // Die Marke ist eine Tatsache: der Spritstand beim Aufsetzen.
    expect(markX).toBeCloseTo(W - x(a.landing_fuel_kg!), 1);
  });

  it("zeigt bei moderatem Anflug den Prozentwert als Hauptzahl", () => {
    // Die kg-Darstellung greift erst bei unlesbaren Prozenten (> 60 %).
    const a = dlh370();
    a.anflug = { ist_kg: 2000, plan_kg: 1865, abweichung_pct: 7.2, als_kg: false };
    render(<SpritSektion sprit={a} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("+7,2 %");
    expect(t).not.toContain("+135 kg");
  });


  it("setzt die Landemarke auf den echten Spritstand, auch bei unverbrauchter Contingency", () => {
    // Der Prüfer der vierten Runde hat belegt, dass die vorige Fassung bei
    // unverbrauchter Contingency — dem Normalfall — fest auf deren Kante
    // stand, unabhängig vom tatsächlichen Landesprit. Das Bild behauptete
    // „Contingency verbraucht", während die Zeile darunter „unberührt"
    // schrieb. Jetzt zeigt die Marke eine Tatsache.
    const a = dlh370();
    a.landing_fuel_kg = 20000;
    a.contingency_verbraucht = false;
    a.extra_genutzt_kg = 0;
    a.extra_ungenutzt_kg = 5178; // gekappt auf das volle Extra
    const { container } = render(<SpritSektion sprit={a} />);
    const svg = container.querySelector("svg")!;
    const W = 560;
    const l = a.leiter!;
    const x = (v: number) => (v / skalaVon(a)) * W;
    const linien = [...svg.querySelectorAll("line")];
    // Die kräftige Marke ist die Landung.
    const landung = linien.find((e) => e.getAttribute("stroke-width") === "2")!;
    const markX = Number(landung.getAttribute("x1"));
    expect(markX).toBeCloseTo(W - x(20000), 1);
    // Und gerade NICHT auf der Contingency/Extra-Kante.
    const kante = x(l.taxi_kg + l.trip_kg + l.contingency_kg);
    expect(Math.abs(markX - kante)).toBeGreaterThan(5);
    // Der Text im SVG widerspricht ihr nicht.
    expect(svg.textContent).toContain("Contingency unberührt");
  });

  it("zeigt zwei Marken an der richtigen Stelle, mit den richtigen Zahlen", () => {
    // Dieser Test prüft Position UND Wert. Die frühere Fassung prüfte nur,
    // DASS eine gestrichelte Linie existiert — und blieb grün, während die
    // Abhebe-Marke um 3 204 kg falsch lag (QS-Abnahme).
    const a = dlh370();
    const { container } = render(<SpritSektion sprit={a} />);
    const svg = container.querySelector("svg")!;
    const W = 560;
    const x = (v: number) => (v / skalaVon(a)) * W;
    const linien = [...svg.querySelectorAll("line")];
    expect(linien.length).toBe(2);

    const abheben = linien.find((e) => e.getAttribute("stroke-dasharray"))!;
    const landung = linien.find((e) => e.getAttribute("stroke-width") === "2")!;
    expect(Number(abheben.getAttribute("x1"))).toBeCloseTo(W - x(40919), 1);
    expect(Number(landung.getAttribute("x1"))).toBeCloseTo(W - x(16770), 1);

    // Die Etiketten nennen dieselben Zahlen.
    expect(svg.textContent).toContain("40\u202f919");
    expect(svg.textContent).toContain("16\u202f770");

    // Und die zugesagte Eigenschaft gilt: der Abstand IST der Verbrauch.
    const abstandKg =
      ((Number(landung.getAttribute("x1")) - Number(abheben.getAttribute("x1"))) / W) *
      skalaVon(a);
    expect(abstandKg).toBeCloseTo(40919 - 16770, 0);
  });

  it("lässt die Abhebe-Marke weg, wenn der Tankstand fehlt", () => {
    const a = dlh370();
    a.takeoff_fuel_kg = null;
    const { container } = render(<SpritSektion sprit={a} />);
    expect([...container.querySelectorAll("svg line")].length).toBe(1);
    expect(container.querySelector("svg")!.textContent).not.toContain("abgehoben mit");
  });

  it("kommt ohne Phasen aus und zeigt trotzdem die Reserve", () => {
    const a = dlh370();
    a.bis_sinkflug = null;
    a.anflug = null;
    render(<SpritSektion sprit={a} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("intakt");
    expect(t).toContain("—");
  });


  it("zeichnet die Leiter in Verbrauchsreihenfolge, sodass die Landemarke den Text trifft", () => {
    // In dieser Vorlage (273 kg ueber Block angelassen) bleiben **3 308 kg**
    // Extra ungenutzt — das ist die Zahl aus der Auswertung. Bis v1.7.35 stand hier 3 581: die Grafik rechnete
    // gegen den geplanten Block, die Zeile gegen den tatsächlichen
    // Abhebestand, und die 273 kg Übertankung dazwischen fehlten in der
    // Leiter ganz. Seit die Übertankung ein eigenes Segment hat, treffen
    // beide denselben Punkt.
    const a = dlh370();
    const { container } = render(<SpritSektion sprit={a} />);
    const svg = container.querySelector("svg")!;
    const rects = [...svg.querySelectorAll("rect")];
    const W = 560;
    const l = a.leiter!;
    const x = (v: number) => (v / skalaVon(a)) * W;
    // Reihenfolge: Taxi, Trip, Contingency, Extra, Übertankung, Alternate,
    // Reserve. Die Übertankung steht zwischen Extra und Alternate, weil
    // `sprit.rs` sie genauso verrechnet.
    const breiten = rects.map((r) => Number(r.getAttribute("width")));
    expect(breiten[3]).toBeCloseTo(x(l.extra_kg), 1);
    expect(breiten[4]).toBeCloseTo(x(273), 1);
    expect(breiten[5]).toBeCloseTo(x(l.alternate_kg), 1);
    // Von der Marke bis zum rechten Rand steht, was gelandet ist. Rechts
    // liegen Alternate + Reserve (13 189 kg, unangetastet), davor das noch
    // vorhandene Extra. Die Marke muss also im EXTRA-Block liegen — vorher
    // lag sie im Alternate-Block und widersprach damit der Zeile daneben
    // „Alternate + Final Reserve unangetastet" (QS-Befund P2-2).
    //
    // Sie misst den TATSÄCHLICHEN Landesprit gegen die GEPLANTE Leiter. Wer
    // anders tankt als geplant, bei dem weicht die Marke leicht von der
    // Textzeile „X kg Extra genutzt" ab — die rechnet gegen den echten
    // Abhebe-Tankstand. Beides ist richtig, es misst nur Verschiedenes.
    // Die Marke muss im EXTRA-Block liegen: rechts von ihr stehen das noch
    // vorhandene Extra sowie Alternate und Reserve. Vorher lag sie im
    // Alternate-Block und widersprach damit der Zeile daneben „Alternate +
    // Final Reserve unangetastet". Die genaue Position prüft der Test
    // „zeichnet die Landemarke aus derselben Zahl wie die Textzeile".
    const landung = [...svg.querySelectorAll("line")].find(
      (e) => e.getAttribute("stroke-width") === "2",
    )!;
    const markX = Number(landung.getAttribute("x1"));
    const grenzeExtraAlternate = W - x(l.alternate_kg + l.reserve_kg + 273);
    const extraBeginn = x(l.taxi_kg + l.trip_kg + l.contingency_kg);
    expect(markX).toBeGreaterThan(extraBeginn);
    expect(markX).toBeLessThan(grenzeExtraAlternate);
  });

  it("färbt die Landemarke nie in der Fehlerfarbe", () => {
    const { container } = render(<SpritSektion sprit={dlh370()} />);
    expect(container.querySelector("svg")!.innerHTML).not.toContain("#ff5c4d");
  });

  it("hält die Landemarke im Bild, wenn mehr gelandet wurde als geplant", () => {
    const a = dlh370();
    a.landing_fuel_kg = 99999; // OFP-Mismatch / getankt
    const { container } = render(<SpritSektion sprit={a} />);
    const linien = [...container.querySelectorAll("svg line")];
    for (const l of linien) {
      const v = Number(l.getAttribute("x1"));
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(560);
    }
  });

  it("kommt ohne Leiter aus (Manual-Flug ohne OFP)", () => {
    const a = dlh370();
    a.leiter = null;
    const { container } = render(<SpritSektion sprit={a} />);
    expect(container.querySelector("svg")).toBeNull();
    expect(screen.getByTestId("sprit-sektion").textContent).toContain("intakt");
  });

  it("rendert nichts ohne Auswertung (Altbestand behält den alten Balken)", () => {
    const { container } = render(<SpritSektion sprit={undefined} />);
    expect(container.firstChild).toBeNull();
  });
});

describe("Formatierer", () => {
  it("setzt Vorzeichen und deutsche Dezimalstelle", () => {
    expect(pct(-3.4)).toBe("−3,4 %");
    expect(pct(192.8)).toBe("+192,8 %");
    expect(pct(0)).toBe("0,0 %");
    expect(pct(null)).toBe("—");
  });

  it("trennt Tausender und rundet auf ganze kg", () => {
    expect(kg(18688.4)).toBe("18\u202f688");
    expect(kg(null)).toBe("—");
  });
});
