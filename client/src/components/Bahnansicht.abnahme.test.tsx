// Abnahme Bahnansicht Variante B (Thomas 22.10.2026, Spec v1.7.0 §8.3,
// `handoff/bahnansicht/vorschlag.md` §9.3): die zehn echten Landungen als
// feste Render-Tests. Geprüft wird die GEZEICHNETE Geometrie der Draufsicht
// (Polylinien `rad-links`/`rad-rechts` gegen das gezeichnete Bahnrechteck),
// nicht die Rechnung.
//
// Auf dem alten Stand (v2.1.6) ist jede Geometrie-Zusicherung hier ROT: Die
// Draufsicht zeichnete keine Radlinien (nur eine waagerechte Ausroll-Linie
// auf Aufsetzhöhe), jede Bahn mit 45 m und den Aufsetzpunkt auf ±22,5 m
// geklemmt (B1); die Queransicht zeigte das Band nur bis zur Radmitte (B2)
// und setzte das Messfenster-Ende von #1905 bei 3418 m (B3).
//
// Fälle: touchdowns.payload_json von live, lesend exportiert (`dev/bahnansicht/fixtures`).

import { describe, expect, it } from "vitest";
import { render, cleanup } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { RunwayDiagramV2, MIN_ANZEIGE_BREITE_PX, BAHN_GRUND, type RunwayDiagramV2Props } from "./RunwayDiagramV2";
import { mapTouchdownPayloadToV2Props } from "../dev/bahnansicht/payloadMapper";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { aussenkanteHalbM } from "../lib/bahnQuerMassstab";

const FAELLE = import.meta.glob("../dev/bahnansicht/fixtures/*.json", { eager: true, import: "default" }) as Record<
  string,
  Record<string, unknown>
>;
const payload = (id: number) => FAELLE[`../dev/bahnansicht/fixtures/${id}.json`]!;
const fall = (id: number) => {
  const pl = payload(id);
  return mapTouchdownPayloadToV2Props(pl, Array.isArray(pl.sub_scores) ? (pl.sub_scores as never) : null)!;
};

function zeichne(props: RunwayDiagramV2Props) {
  cleanup();
  const { container } = render(<RunwayDiagramV2 {...props} />);
  const svg = container.querySelector("svg")!;
  const bahn = svg.querySelector('[data-rolle="bahnflaeche"]')!;
  const top = Number(bahn.getAttribute("y"));
  const bot = top + Number(bahn.getAttribute("height"));
  const linie = (rolle: string) => {
    const el = svg.querySelector(`[data-rolle="${rolle}"]`);
    if (!el) return null;
    return el
      .getAttribute("points")!
      .trim()
      .split(/\s+/)
      .map((p) => p.split(",").map(Number) as [number, number]);
  };
  const projektion = erzeugeProjektion({
    lengthM: props.length_m,
    ddsM: props.displaced_threshold_m ?? 0,
    spurVersatzM: props.spur_nullpunkt_versatz_m ?? 0,
    padX: 70,
    innerW: 1060,
  });
  return { container, svg, top, bot, links: linie("rad-links"), rechts: linie("rad-rechts"), mitte: linie("spur-mitte"), projektion };
}

function yBei(linie: Array<[number, number]>, x: number): number | null {
  for (let i = 1; i < linie.length; i++) {
    const [x0, y0] = linie[i - 1]!;
    const [x1, y1] = linie[i]!;
    if ((x0 - x) * (x1 - x) <= 0 && x1 !== x0) return y0 + ((x - x0) / (x1 - x0)) * (y1 - y0);
  }
  return null;
}

function draussen(z: ReturnType<typeof zeichne>, seite: "links" | "rechts", m: number): boolean | null {
  const l = seite === "links" ? z.links : z.rechts;
  if (!l) return null;
  const y = yBei(l, z.projektion.mAbBahnanfangZuX(m));
  if (y == null) return null;
  return seite === "links" ? y < z.top : y > z.bot;
}

function irgendwoDraussen(z: ReturnType<typeof zeichne>, bisM: number): boolean | null {
  if (!z.links || !z.rechts) return null;
  const bisX = z.projektion.mAbBahnanfangZuX(bisM);
  return (
    z.links.some(([x, y]) => x <= bisX && y < z.top - 0.5) ||
    z.rechts.some(([x, y]) => x <= bisX && y > z.bot + 0.5)
  );
}

// Erwartung je Fall, aus den gespeicherten Proben nachgerechnet
// (Randabstand = halbe Bahn − |quer| − Reifen-Aussenkante).
const RAUS: Array<{ id: number; seite: "links" | "rechts"; drin: number[]; draussen: number[] }> = [
  { id: 1914, seite: "rechts", draussen: [1534], drin: [1300, 1750] },
  { id: 1662, seite: "links", draussen: [400], drin: [700] },
  { id: 1842, seite: "rechts", draussen: [1000], drin: [1300] },
  { id: 1779, seite: "links", draussen: [1767], drin: [1000] },
  { id: 1738, seite: "links", draussen: [300], drin: [] },
];
const SAUBER: Array<{ id: number; bisM: number }> = [
  { id: 1884, bisM: 1824 },
  { id: 1909, bisM: 1897 },
  { id: 1916, bisM: 1795 },
  // scoring_cutoff; mess_ende (3418) stammt aus dem Touch-and-go davor (B3).
  { id: 1905, bisM: 1600 },
];

describe("Bahnansicht B — zehn echte Landungen (golden)", () => {
  for (const f of RAUS) {
    it(`#${f.id}: äusseres Rad ${f.seite} draussen bei ${f.draussen.join("/")} m, drin bei ${f.drin.join("/") || "—"} (F1)`, () => {
      const z = zeichne(fall(f.id));
      // Alter Stand: keine Radlinien in der Draufsicht → null → rot.
      for (const m of f.draussen) expect(draussen(z, f.seite, m), `${m} m`).toBe(true);
      for (const m of f.drin) expect(draussen(z, f.seite, m), `${m} m`).toBe(false);
    });
  }

  for (const f of SAUBER) {
    it(`#${f.id}: sauber, kein Rad ausserhalb bis ${f.bisM - 60} m`, () => {
      // Die letzten 60 m vor dem Ende sind ausgenommen: Überhöhungsartefakt
      // an steilen Ausfahrten (B4), siehe Nachweis.
      expect(irgendwoDraussen(zeichne(fall(f.id)), f.bisM - 60)).toBe(false);
    });
  }

  it("#1914: erster Kantenübertritt zwischen 1350 und 1450 m (gemessen ab 1393)", () => {
    const z = zeichne(fall(1914));
    const erster = z.rechts!.find(([, y]) => y > z.bot)!;
    expect(erster[0]).toBeGreaterThanOrEqual(z.projektion.mAbBahnanfangZuX(1350));
    expect(erster[0]).toBeLessThanOrEqual(z.projektion.mAbBahnanfangZuX(1450));
  });

  for (const id of [1738, 1779, 1914, 1884]) {
    it(`#${id}: echte Bahnbreite und Band bis Reifen-Aussenkante (B1, B2)`, () => {
      const props = fall(id);
      const z = zeichne(props);
      const m = z.mitte!;
      let i = 1;
      for (let k = 1; k < Math.min(m.length - 1, 60); k++)
        if (Math.abs(m[k + 1]![1] - m[k - 1]![1]) < Math.abs(m[i + 1]![1] - m[i - 1]![1])) i = k;
      const bandM = (Math.abs(z.rechts![i]![1] - z.links![i]![1]) / (z.bot - z.top)) * props.runway_width_m!;
      // B2: Aussenkante, NICHT Radmitte (Spurweite). Für A320 7,59 m:
      // 8,69 m statt 7,59 m — der Unterschied liegt über der Toleranz.
      expect(bandM).toBeCloseTo(2 * aussenkanteHalbM(props.track_width_m!), 0);
      const paket = 2 * aussenkanteHalbM(props.track_width_m!) - props.track_width_m!;
      expect(Math.abs(bandM - props.track_width_m!)).toBeGreaterThan(paket / 2);
      // B1: Bahnhöhe folgt der echten Breite (27 m ≠ 61 m ≠ 45 m).
      expect(z.bot - z.top).not.toBeCloseTo(210, 0);
    });
  }

  it("#1662: Aufsetzpunkt ausserhalb der gezeichneten Kante, nicht auf ±22,5 m geklemmt (B1)", () => {
    const z = zeichne(fall(1662));
    const td = z.svg.querySelector('[data-rolle="td-punkt"]')!;
    const cy = Number(td.getAttribute("cy"));
    // −26,2 m auf einer 45,7-m-Bahn: Mitte des Flugzeugs jenseits der linken Kante.
    expect(cy).toBeLessThan(z.top);
  });

  it("#1914: Zahl an der knappsten Stelle = gespeicherter min_edge_clearance_m, bei ~1534 m", () => {
    const z = zeichne(fall(1914));
    const rs = z.svg.querySelector('[data-rolle="randstelle"]')!;
    expect(Number(rs.getAttribute("data-randabstand"))).toBeCloseTo(payload(1914).min_edge_clearance_m as number, 2);
    expect(Math.abs(Number(rs.getAttribute("data-laengs")) - 1534)).toBeLessThan(30);
    expect(z.container.querySelector('[data-rolle="randstelle-text"]')!.textContent).toMatch(/Rad 5,8 m neben der Bahn \(rechts\)/);
  });

  it("#1662: zweite Zahl „aufgesetzt X m neben der Bahn“ nur bei off_runway_surface (Entscheidung 11)", () => {
    const t1662 = zeichne(fall(1662)).container.querySelector('[data-rolle="aufgesetzt-neben"]');
    // −26,2 m Versatz + 4,35 m Aussenkante (A320) − 22,86 m halbe Bahn ≈ 7,6 m
    expect(t1662?.textContent).toMatch(/aufgesetzt 7,\d m neben der Bahn/);
    // Gegenprobe: #1914 hat Ausrichtung off_centerline, #1884 sauber → keine zweite Zahl.
    expect(zeichne(fall(1914)).container.querySelector('[data-rolle="aufgesetzt-neben"]')).toBeNull();
    expect(zeichne(fall(1884)).container.querySelector('[data-rolle="aufgesetzt-neben"]')).toBeNull();
  });

  it("#1905: Messfenster-Marke am frühesten Ende (1604 m), nicht bei 3418 m aus dem Touch-and-go (B3)", () => {
    const z = zeichne(fall(1905));
    const d = z.svg.querySelector('[data-rolle="mess-ende"]')!.getAttribute("d")!;
    const x = Number(/^M ([\d.]+)/.exec(d)![1]);
    expect(x).toBeCloseTo(z.projektion.mAbBahnanfangZuX(1604.46), 0);
    // und der Touch-and-go als violetter früherer Durchgang
    expect(z.svg.querySelector('[data-rolle="durchgang"]')).not.toBeNull();
  });

  it("#1738: Marken überdecken sich nie (2 bei 345 m, 3 bei 350 m am Bildrand)", () => {
    // Alter Stand (Sichtprüfung 22.10.2026): Marke 2 lag genau unter Marke 3.
    const z = zeichne(fall(1738));
    const kreise = [...z.svg.querySelectorAll("[data-marke] circle")].map((c) => [
      Number(c.getAttribute("cx")),
      Number(c.getAttribute("cy")),
    ]);
    const td = z.svg.querySelector('[data-rolle="td-punkt"]')!;
    kreise.push([Number(td.getAttribute("cx")), Number(td.getAttribute("cy"))]);
    expect(kreise.length).toBeGreaterThanOrEqual(3);
    for (let i = 0; i < kreise.length; i++)
      for (let j = i + 1; j < kreise.length; j++)
        expect(Math.hypot(kreise[i]![0]! - kreise[j]![0]!, kreise[i]![1]! - kreise[j]![1]!), `${i}/${j}`).toBeGreaterThanOrEqual(18);
  });

  it("Handy: Hinweiszeilen stehen als Text ausserhalb der rollenden Grafik", () => {
    const z = zeichne(fall(1662));
    const kasten = z.container.querySelector('[data-rolle="bahn-grafik"]')!;
    for (const r of ["randstelle-text", "aufgesetzt-neben"]) {
      const el = z.container.querySelector(`[data-rolle="${r}"]`)!;
      expect(el.tagName.toLowerCase(), r).toBe("div");
      expect(kasten.contains(el), r).toBe(false);
    }
  });

  it("#1062 (ohne Spurdaten): gestrichelt mit Hinweis, keine Behauptung „mittig“", () => {
    const z = zeichne(fall(1062));
    expect(z.container.querySelector('[data-rolle="hinweis-ohne-spur"]')?.textContent).toMatch(/Keine Spurdaten/);
    expect([...z.svg.querySelectorAll("line")].some((l) => l.getAttribute("stroke-dasharray") === "8 6")).toBe(true);
  });

  it("#1842 (Spur-Nullpunkt 195 m): Aufsetzpunkt sitzt auf der ersten Spurprobe", () => {
    const z = zeichne(fall(1842));
    const tdx = Number(z.svg.querySelector('[data-rolle="td-punkt"]')!.getAttribute("cx"));
    expect(Math.abs(z.mitte![0]![0] - tdx)).toBeLessThan(3);
  });

  it("Inventar §3: alle Einzelstücke der Queransicht stehen in der Draufsicht", () => {
    const z = zeichne(fall(1909));
    const text = z.container.textContent ?? "";
    expect(z.svg.querySelector('[data-rolle="querskala"]')).not.toBeNull();
    expect(z.svg.querySelector('[data-rolle="ueberhoehung"]')!.textContent).toMatch(/Spurweite 7\.6 m · quer \d+,\d× überhöht · Bahn 46 m/);
    // Alle 31 Ausfahrten als Stummel, und benannt (zusammengefasst, gezählt).
    expect(z.svg.querySelectorAll('[data-rolle="ausfahrt"]').length).toBeGreaterThan(20);
    const namen = [...z.svg.querySelectorAll('[data-rolle="ausfahrt-name"]')].map((n) => n.textContent ?? "");
    const behauptet = namen.reduce((a, s) => a + s.replace(/\s*\+\d+$/, "").split("/").length + Number(/\+(\d+)$/.exec(s)?.[1] ?? 0), 0);
    expect(behauptet).toBeGreaterThan(20);
    // Marke 1 sitzt als Ziffer auf dem Aufsetzpunkt, 2 und 3 im Overlay.
    const ziffern = [...z.svg.querySelectorAll('text[fill="#0B0F17"]')].map((t) => t.textContent);
    expect(["1", "2", "3"].every((n) => ziffern.includes(n))).toBe(true);
    expect(z.svg.querySelector('[data-rolle="mess-ende"]')).not.toBeNull();
    // Eigene Legende direkt unter der Draufsicht; keine zweite überhöhte Ansicht mehr.
    expect(text).toMatch(/Band reicht bis zur Außenkante der äußeren Reifen/);
    expect(text).not.toMatch(/QUER — WO DIE RÄDER LIEFEN/);
    // „am Bildrand angehalten“ — #1738 (Graspiste, Spur fast ganz ausserhalb)
    expect(zeichne(fall(1738)).container.querySelector('[data-rolle="laeuft-weiter"]')?.textContent).toMatch(/am Bildrand angehalten/);
  });

  it("B7: Handy — Grafik mit Mindestbreite, kleinste Schrift ≥ 5,5 px (vorher 2,7 px bei 360 px)", () => {
    const z = zeichne(fall(1914));
    expect(z.svg.style.minWidth).toBe(`${MIN_ANZEIGE_BREITE_PX}px`);
    const kasten = z.container.querySelector('[data-rolle="bahn-grafik"]') as HTMLElement;
    expect(kasten.style.overflowX).toBe("auto");
    const kleinste = Math.min(
      ...[...z.svg.querySelectorAll("text")].map((t) => Number(t.getAttribute("font-size"))).filter((n) => n > 0),
    );
    const vb = Number(z.svg.getAttribute("viewBox")!.split(" ")[2]);
    expect((kleinste * MIN_ANZEIGE_BREITE_PX) / vb).toBeGreaterThanOrEqual(5.5);
  });

  it("B9: helles Thema — die Grafik steht auf ihrem eigenen dunklen Grund", () => {
    const z = zeichne(fall(1914));
    const kasten = z.container.querySelector('[data-rolle="bahn-grafik"]') as HTMLElement;
    // Vorher rgba(0,0,0,0.25): im hellen Thema Hellgrau unter hellgrauer Schrift.
    expect(kasten.style.background).toMatch(/rgb\(12, 22, 40\)|#0c1628/i);
    expect(BAHN_GRUND).toBe("#0c1628");
  });
});

describe("Neue Beschriftungen nur mit Zeichen aus B612 Mono", () => {
  it("runway_v2.b.* und die Marke „neben der Bahn“ (de/en/it)", () => {
    const vorrat = new Set(
      (JSON.parse(readFileSync("src/assets/fonts/b612mono-zeichen.json", "utf8")).zeichen as number[]).map((c) =>
        String.fromCodePoint(c),
      ),
    );
    const fehlt: string[] = [];
    for (const l of ["de", "en", "it"]) {
      const d = JSON.parse(readFileSync(`src/locales/${l}/common.json`, "utf8"));
      const texte: string[] = [
        ...Object.values(d.runway_v2.b as Record<string, string>),
        ...Object.entries(d.landing.flag as Record<string, string>)
          .filter(([k]) => k.startsWith("neben_") || k === "bounce_leicht")
          .map(([, v]) => v),
      ];
      for (const s of texte)
        for (const c of s.replace(/\{\{[^}]+\}\}/g, "")) if (!vorrat.has(c)) fehlt.push(`${l}: ${JSON.stringify(c)} in „${s.slice(0, 40)}“`);
    }
    expect(fehlt).toEqual([]);
  });
});
