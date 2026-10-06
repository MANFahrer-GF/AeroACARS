// Dot-Streifen der Anfluggrafik (05.10.2026).
//
// Geprüft wird, was der Pilot sieht: Mit Gleitpfad-Kurve erscheint der
// Streifen auf derselben Zeitachse wie die Sinkrate, markiert wird der
// größte Wert im Stable Gate (nicht der empfindliche Wert kurz vor der
// Schwelle). Ohne Kurve kein Streifen, dafür der Hinweis.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { AnflugGrafikAbschnitt, ApproachChart, dotsZurZeit } from "./AnflugGrafik";
import type { ApproachSample } from "../lib/landungsDatensatz";
import type { GleitpfadPunkt } from "./AnflugForensikInfo";

/** 81 Proben im Sekundentakt, 80 s bis zum Aufsetzen. */
const proben: ApproachSample[] = Array.from({ length: 81 }, (_, i) => ({
  vs_fpm: -700,
  bank_deg: 0,
  t_ms: (i - 80) * 1000,
  agl_ft: 1000 - i * 12.5,
  is_scored_gate: i < 77,
  is_flare: i >= 77,
  gs_kt: 135,
}));

/** THY39-artig: im Gate bis +2,22, unter 200 ft bis +4,98. */
const verlauf: GleitpfadPunkt[] = [
  { t: -78, h: 990, d: 0.3 },
  { t: -40, h: 500, d: 1.1 },
  { t: -20, h: 209, d: 2.22 },
  { t: -14, h: 113, d: 4.14 },
  { t: -13, h: 102, d: 4.98 },
];

describe("Dot-Streifen", () => {
  it("interpoliert die Dots zu einer Zeit, außerhalb der Kurve nichts", () => {
    expect(dotsZurZeit(verlauf, -30_000)).toBeCloseTo(1.66, 2);
    expect(dotsZurZeit(verlauf, -20_000)).toBe(2.22);
    expect(dotsZurZeit(verlauf, -5_000)).toBeNull();
    expect(dotsZurZeit(verlauf, -90_000)).toBeNull();
  });

  it("zeichnet den Streifen und markiert den größten Wert im Gate", () => {
    const { container } = render(
      <ApproachChart samples={proben} gleitpfadVerlauf={verlauf} />,
    );
    const text = container.textContent ?? "";
    expect(text).toContain("Gleitpfad in Dots");
    // Kurve: ein Pfad mit fünf Punkten in der Streifenfarbe.
    const kurve = container.querySelector('path[stroke="#8b5cf6"]');
    expect(kurve?.getAttribute("d")?.match(/[ML]/g)?.length).toBe(5);
    // Markiert wird 2,22 bei 209 ft, nicht 4,98 kurz vor der Schwelle.
    expect(text).toContain("größte Abweichung im Gate +2.22 bei 209 ft");
    expect(text).not.toContain("4.98");
    // Achse wächst bis ±5 mit (größter Wert 4,98) — nichts klebt am Rand.
    // Kreis der Marke bei +2,22 Dots: yDot = 346 + 100 − 2,22/5 · 100 = 401,6.
    const kreis = container.querySelector('circle[r="5"][fill="#8b5cf6"]');
    expect(Number(kreis?.getAttribute("cy"))).toBeCloseTo(401.6, 1);
    expect(text).toContain("+5");
    expect(container.querySelector("svg")?.getAttribute("viewBox")).toBe("0 0 1120 580");
  });

  it("ohne Kurve: kein Streifen, Grafik so hoch wie bisher, Hinweis darunter", () => {
    const { container } = render(
      <AnflugGrafikAbschnitt samples={proben} profile={null} gleitpfadVerlauf={null} />,
    );
    expect(container.textContent).not.toContain("Gleitpfad in Dots (");
    expect(container.querySelector("svg")?.getAttribute("viewBox")).toBe("0 0 1120 320");
    expect(container.textContent).toContain("für diesen Flug nicht aufgezeichnet");
  });

  // QS 06.10.2026: Spur ohne Zeiten — der Streifen lässt sich nicht
  // ausrichten. Dann auch kein (i) zum Streifen und nicht „nicht
  // aufgezeichnet" (die Kurve gibt es ja).
  it("Spur ohne Zeiten: weder Streifen noch (i) dazu noch falscher Hinweis", () => {
    const ohneZeit = proben.map((p) => ({ ...p, t_ms: undefined as unknown as number }));
    const { container } = render(
      <AnflugGrafikAbschnitt samples={ohneZeit} profile={null} gleitpfadVerlauf={verlauf} />,
    );
    expect(container.textContent).not.toContain("Gleitpfad in Dots");
    expect(container.textContent).not.toContain("nicht aufgezeichnet");
    // Gegenprobe: mit Zeiten erscheint das (i) zum Streifen.
    const mit = render(<AnflugGrafikAbschnitt samples={proben} profile={null} gleitpfadVerlauf={verlauf} />);
    expect(mit.container.textContent).toContain("Gleitpfad in Dots");
  });
});
