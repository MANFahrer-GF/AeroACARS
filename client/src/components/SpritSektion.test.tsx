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
    fassung: 1,
    bis_sinkflug: { ist_kg: 18688, plan_kg: 19353, abweichung_pct: -3.4 },
    anflug: { ist_kg: 5461, plan_kg: 1865, abweichung_pct: 192.8 },
    zeit_unter_schwelle_min: 15.8,
    schwelle_ft: 9487,
    strecke_anflug_nm: 182,
    plan_strecke_anflug_nm: 191,
    reserve: { status: "intakt", quote_pct: 341.1 },
    reserve_kg: 4916,
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

describe("SpritSektion", () => {
  it("zeigt beide Phasen mit Ist, Plan und Abweichung", () => {
    render(<SpritSektion sprit={dlh370()} />);
    const t = screen.getByTestId("sprit-sektion").textContent ?? "";
    expect(t).toContain("−3,4 %");
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
    // Sechs Segmente der Leiter.
    expect(svg!.querySelectorAll("rect").length).toBe(6);
    // Landemarke als eigene Linie.
    expect(svg!.querySelectorAll("line").length).toBe(1);
    expect(svg!.textContent).toContain("gelandet mit");
    expect(svg!.textContent).toContain("Contingency verbraucht");
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
    // Bei DLH370 bleiben 3581 kg Extra ungenutzt. Von rechts gemessen liegt
    // die Marke damit genau an der Grenze Extra|Alternate — also NICHT im
    // Alternate-Block, passend zur Zeile „Alternate + Final Reserve
    // unangetastet". Genau dieser Widerspruch war ein QS-Befund.
    const a = dlh370();
    const { container } = render(<SpritSektion sprit={a} />);
    const svg = container.querySelector("svg")!;
    const rects = [...svg.querySelectorAll("rect")];
    const W = 560;
    const l = a.leiter!;
    const x = (v: number) => (v / l.block_kg) * W;
    // Reihenfolge der Segmente: Taxi, Trip, Contingency, Extra, Alternate, Reserve.
    const breiten = rects.map((r) => Number(r.getAttribute("width")));
    expect(breiten[3]).toBeCloseTo(x(l.extra_kg), 1);
    expect(breiten[4]).toBeCloseTo(x(l.alternate_kg), 1);
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
    const markX = Number(svg.querySelector("line")!.getAttribute("x1"));
    const grenzeExtraAlternate = W - x(l.alternate_kg + l.reserve_kg);
    const extraBeginn = x(l.taxi_kg + l.trip_kg + l.contingency_kg);
    expect(markX).toBeCloseTo(W - x(a.landing_fuel_kg!), 1);
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
    const markX = Number(container.querySelector("svg")!.querySelector("line")!.getAttribute("x1"));
    expect(markX).toBeGreaterThanOrEqual(0);
    expect(markX).toBeLessThanOrEqual(560);
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
