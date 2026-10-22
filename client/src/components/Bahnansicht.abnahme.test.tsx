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
import { RunwayDiagramV2, BAHN_GRUND, type RunwayDiagramV2Props } from "./RunwayDiagramV2";
import { mapTouchdownPayloadToV2Props } from "../dev/bahnansicht/payloadMapper";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { aussenkanteHalbM } from "../lib/bahnQuerMassstab";

const FAELLE = import.meta.glob("../dev/bahnansicht/fixtures/*.json", { eager: true, import: "default" }) as Record<
  string,
  Record<string, unknown>
>;
const payload = (id: number) => FAELLE[`../dev/bahnansicht/fixtures/${id}.json`]!;
// Der Typ steht nicht im Touchdown-Payload; die Webapp ergänzt ihn aus
// `flight_sessions` (`LandingAnalysis`), der Client aus dem LandingRecord.
// Live gelesen (sqlite3 -readonly, 22.10.2026).
const TYP: Record<number, string> = { 1936: "A320" };
const fall = (id: number) => {
  const pl = payload(id);
  const p = mapTouchdownPayloadToV2Props(pl, Array.isArray(pl.sub_scores) ? (pl.sub_scores as never) : null)!;
  return TYP[id] ? { ...p, aircraft_icao: p.aircraft_icao ?? TYP[id] } : p;
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
  // Ab 2.1.8 legt die Grafik ihren Zeichenbereich offen (Platz hinter dem
  // Bahnende, Skala rechts) — sonst die feste Geometrie bis 2.1.7.
  const zahl = (n: string) => (svg.hasAttribute(n) ? Number(svg.getAttribute(n)) : null);
  const projektion = erzeugeProjektion({
    lengthM: props.length_m,
    ddsM: props.displaced_threshold_m ?? 0,
    spurVersatzM: props.spur_nullpunkt_versatz_m ?? 0,
    padX: zahl("data-pad-x") ?? 70,
    innerW: zahl("data-inner-w") ?? 1060,
    nachM: zahl("data-nach-m") ?? 0,
    vorM: zahl("data-vor-m") ?? 0,
  } as Parameters<typeof erzeugeProjektion>[0]);
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
    // Kopf seit 2.1.8 (Entscheidung 7): Komma im Deutschen, „gestreckt“.
    expect(z.svg.querySelector('[data-rolle="ueberhoehung"]')!.textContent).toMatch(/Spurweite 7,6 m · Bahn 46 m breit · quer \d+-fach gestreckt/);
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
    expect(z.container.querySelector('[data-legende-eintrag="band"]')?.textContent).toMatch(/Band bis Reifen-Außenkante/);
    expect(text).not.toMatch(/QUER — WO DIE RÄDER LIEFEN/);
    // „am Bildrand angehalten“ — #1738 (Graspiste, Spur fast ganz ausserhalb)
    expect(zeichne(fall(1738)).container.querySelector('[data-rolle="laeuft-weiter"]')?.textContent).toMatch(/am Bildrand angehalten/);
  });

  it("B7/Entscheidung 7: Handy — gestaucht statt gerollt, kleinste Schrift ≥ 5,5 px bei 358 px", () => {
    // jsdom misst nicht — der Kasten meldet seine Breite über ResizeObserver.
    const echt = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(private cb: ResizeObserverCallback) {}
      observe() {
        this.cb([{ contentRect: { width: 358 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
    try {
      const z = zeichne(fall(1914));
      expect(z.svg.style.minWidth).toBe("");
      const kasten = z.container.querySelector('[data-rolle="bahn-grafik"]') as HTMLElement;
      expect(kasten.style.overflowX).not.toBe("auto");
      const vb = Number(z.svg.getAttribute("viewBox")!.split(" ")[2]);
      const kleinste = Math.min(
        ...[...z.svg.querySelectorAll("text")].map((t) => Number(t.getAttribute("font-size"))).filter((n) => n > 0),
      );
      expect((kleinste * 358) / vb).toBeGreaterThanOrEqual(5.5);
      // Die ganze Bahn ist im Bild, nicht rechts abgeschnitten.
      expect(Number(z.svg.getAttribute("data-pad-x")) + Number(z.svg.getAttribute("data-inner-w"))).toBeLessThanOrEqual(vb);
    } finally {
      globalThis.ResizeObserver = echt;
    }
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

// ── #1936 (DLH 250, EDDG 25, A320, Client 2.1.6) als elfter fester Fall ──
// Kritik Thomas 22.10.2026 (`handoff/bahnansicht/kritik-2026-10-22.md`),
// Entscheidungen 1–7 (`entscheidungen-2026-10-22.md`). Jede Zusicherung ist
// auf dem Stand v2.1.7 ROT (Beleg im Nachweis). Die Phantom-Ausfahrt rechts
// bei 1839,6 m prüft der Rust-Test `ausfahrten::tests::eddg_*`: Der
// gespeicherte Payload trägt sie, und die Szenerie-Züge sind nicht gespeichert.

/** Textbreite geschätzt wie in `bahnHilfen.gruppiere` (0,62 em je Zeichen). */
function textKasten(t: Element) {
  const fs = Number(t.getAttribute("font-size") ?? 10);
  const x = Number(t.getAttribute("x") ?? 0);
  const y = Number(t.getAttribute("y") ?? 0);
  const b = (t.textContent ?? "").length * fs * 0.62;
  const anker = t.getAttribute("text-anchor") ?? "start";
  const links = anker === "middle" ? x - b / 2 : anker === "end" ? x - b : x;
  return { links, rechts: links + b, oben: y - fs, unten: y + fs * 0.25 };
}

function draufsicht() {
  const z = zeichne(fall(1936));
  const vb = z.svg.getAttribute("viewBox")!.split(" ").map(Number);
  return { ...z, W: vb[2]!, H: vb[3]! };
}

describe("#1936 EDDG 25 — Kritik 22.10.2026 (golden)", () => {
  it("1 · ein Ausfahrtname überall: Kopf, Stummel, Lupe und Pfeil sagen „B“ (nicht „B/S1“)", () => {
    const z = draufsicht();
    const texte = [...z.svg.querySelectorAll("text")].map((t) => t.textContent ?? "");
    const kopf = texte.map((s) => /Ausfahrt (\S+) (links|rechts)/.exec(s)?.[1]).find(Boolean);
    const stummel = [...z.svg.querySelectorAll('[data-rolle="ausfahrt-name"][font-weight="700"]')].map((t) => t.textContent);
    const lupe = [...z.container.querySelectorAll('svg[data-ansicht="lupe"] text[font-weight="700"]')].map((t) => t.textContent);
    expect(kopf).toBe("B");
    expect(stummel).toEqual(["B"]);
    expect(lupe).toEqual(["B"]);
    expect(z.svg.querySelector('[data-rolle="weiter-pfeil"]')?.textContent).toBe("weiter auf Rollweg B");
    // Doppelname an derselben Abzweigung: ein Name, nirgends „S1“.
    const namen = [...z.svg.querySelectorAll('[data-rolle="ausfahrt-name"]')].map((t) => t.textContent ?? "");
    expect(namen.some((n) => n.includes("S1") || n.includes("/"))).toBe(false);
  });

  it("3 · kein Text ausserhalb des Bildes (ausdrücklich der Räumpunkt-Kopf)", () => {
    const z = draufsicht();
    const raus = [...z.svg.querySelectorAll("text")]
      .filter((t) => (t.textContent ?? "").trim() !== "")
      .filter((t) => {
        const k = textKasten(t);
        return k.links < -0.5 || k.rechts > z.W + 0.5 || k.oben < -0.5 || k.unten > z.H + 0.5;
      })
      .map((t) => t.textContent);
    expect(raus).toEqual([]);
  });

  it("3b · kein unbenutzter Randstreifen: keine Spalte rechts, Ränder so breit wie nötig (Thomas: „rechts zu viel Platz“)", () => {
    // Bedarf links: die Kennung „25“ (+ 14 Luft). Rechts: nur der Platz, den
    // die Ausfahrt hinter dem Bahnende braucht — keine Textspalte daneben.
    const z = draufsicht();
    const pad = Number(z.svg.getAttribute("data-pad-x"));
    const bildRand = pad + Number(z.svg.getAttribute("data-inner-w"));
    expect(z.W - bildRand).toBeLessThanOrEqual(8);
    const kaesten = [...z.svg.querySelectorAll("text")]
      .filter((t) => (t.textContent ?? "").trim() !== "")
      .map((t) => ({ t: t.textContent ?? "", ...textKasten(t) }));
    // Kein Text rechts neben dem Zeichenbereich.
    expect(kaesten.filter((k) => k.links > bildRand - 0.5).map((k) => k.t)).toEqual([]);
    // Links: nur die Kennung, so breit wie sie selbst.
    const kennung = kaesten.find((k) => k.t === "25")!;
    expect(pad - (kennung.rechts - kennung.links)).toBeLessThanOrEqual(16);
  });

  it("4 · Ausfahrt ganz im Bild: Band, Marke 3 und Stummel im Zeichenbereich, Bahn nicht bündig am Rand", () => {
    const z = draufsicht();
    const bildRand = Number(z.svg.getAttribute("data-pad-x")) + Number(z.svg.getAttribute("data-inner-w"));
    const xs = [
      ...(z.links ?? []).map(([x]) => x),
      ...(z.rechts ?? []).map(([x]) => x),
      ...[...z.svg.querySelectorAll('[data-marke="3"] circle')].map((c) => Number(c.getAttribute("cx")) + Number(c.getAttribute("r"))),
      ...[...z.svg.querySelectorAll('[data-rolle="ausfahrt"]')].map((r) => Number(r.getAttribute("x")) + Number(r.getAttribute("width"))),
    ];
    expect(Math.max(...xs)).toBeLessThanOrEqual(bildRand);
    // Platz hinter dem Bahnende (Entscheidung 1): die Bahn endet vor dem Zeichenbereich.
    expect(z.svg.getAttribute("data-nach-m")).not.toBeNull();
    expect(Number(z.svg.getAttribute("data-nach-m"))).toBeGreaterThan(0);
  });

  it("5 · Band in der Kurve nirgends schmaler als in der Geraden (wie 2.1.7, senkrecht zur Linie)", () => {
    // Thomas 22.10.2026: Quer abgesetzt wurde die Ausfahrtskurve zur Linie
    // gequetscht („wieder bei Punkt Null“). Die Bandbreite senkrecht zur
    // gezeichneten Linie bleibt überall gleich — auch in der Ausfahrt.
    const z = draufsicht();
    const l = z.links!;
    const r = z.rechts!;
    const breiten = l.map(([x1, y1], i) => Math.hypot(r[i]![0] - x1, r[i]![1] - y1));
    const gerade = breiten[Math.floor(breiten.length / 2)]!;
    const x = (m: number) => z.projektion.mAbBahnanfangZuX(m);
    const kurve = z.mitte!.map(([px], i) => ({ px, b: breiten[i]! })).filter((q) => q.px >= x(1925));
    expect(kurve.length).toBeGreaterThan(2);
    for (const q of kurve) expect(q.b, `x=${q.px}`).toBeGreaterThanOrEqual(gerade * 0.97);
  });

  it("6 · kein missverständliches „real bis 102 m“ — stattdessen der Pfeil an der Austrittsstelle", () => {
    const z = draufsicht();
    expect(z.container.textContent).not.toMatch(/real bis/);
    const pfeil = z.svg.querySelector('[data-rolle="weiter-pfeil"]');
    expect(pfeil).not.toBeNull();
    const letzte = z.mitte![z.mitte!.length - 1]!;
    const spitze = pfeil!.querySelector("polygon")!.getAttribute("points")!.split(" ")[0]!.split(",").map(Number);
    expect(Math.abs(spitze[0]! - letzte[0])).toBeLessThan(12);
  });

  it("8 · keine Linie ins Leere (kein Strich und keine gestrichelte Linie an Marke 2)", () => {
    const z = draufsicht();
    expect(z.svg.querySelectorAll('[data-rolle="randstelle"] line').length).toBe(0);
    const insLeere = [...z.svg.querySelectorAll("line")].filter((l) => Number(l.getAttribute("y2")) > z.H - 12);
    expect(insLeere.length).toBe(0);
  });

  it("9 · Quelle der Rollwege richtig benannt: nie mehr pauschal OpenStreetMap (Draufsicht und Lupe)", () => {
    // Gespeichert (2.1.6): keine Quelle — die Legende sagt das, statt „OSM“ zu raten.
    const z = draufsicht();
    const eintrag = () => z.container.querySelector('[data-rolle="legende"] [data-legende-eintrag="rollwege"]')?.textContent;
    expect(eintrag()).toMatch(/Quelle nicht gespeichert/);
    expect(z.container.textContent).not.toMatch(/OpenStreetMap|\(OSM\)/);
    // Ab 2.1.8 meldet der Client sie: #1936 kam aus der MSFS-Szenerie.
    cleanup();
    const { container } = render(<RunwayDiagramV2 {...fall(1936)} runway_exits_quelle="szenerie" />);
    expect(container.querySelector('[data-legende-eintrag="rollwege"]')?.textContent).toMatch(/aus der Szenerie/);
    expect(container.querySelector('svg[data-ansicht="lupe"]')!.parentElement!.textContent).toMatch(/Rollweg aus der Szenerie/);
    expect(container.textContent).not.toMatch(/OpenStreetMap|\(OSM\)/);
  });

  it("10 · Marke 2 mit Zahl daneben und Farbe grün (16,3 m Rand)", () => {
    const z = draufsicht();
    const m2 = z.svg.querySelector('[data-marke="2"]')!;
    expect(m2.querySelector("circle")!.getAttribute("fill")).toBe("#22c55e");
    expect(z.svg.querySelector('[data-rolle="marke2-text"]')?.textContent).toBe("16,3 m zur Kante");
  });

  it("Dreieck „Bewertung bis hier“ beschriftet, Spur danach blasser (Entscheidung 4)", () => {
    const z = draufsicht();
    expect(z.svg.querySelector('[data-rolle="mess-ende-text"]')?.textContent).toBe("Bewertung bis hier");
    const vor = z.svg.querySelector('[data-rolle="band"]')!;
    const nach = z.svg.querySelector('[data-rolle="band-nach"]')!;
    expect(Number(nach.getAttribute("fill-opacity"))).toBeLessThan(Number(vor.getAttribute("fill-opacity")) / 2);
  });

  it("Kopf, Querskala und Handy (Entscheidung 7)", () => {
    const z = draufsicht();
    // Der Faktor ist gerechnet (mit der Querskala rechts ist die Bahn etwas
    // kürzer gezeichnet: 9 statt 8,2 wie im Bildschirmfoto).
    expect(z.svg.querySelector('[data-rolle="ueberhoehung"]')!.textContent).toMatch(
      /^A320 · Spurweite 7,6 m · Bahn 45 m breit · quer \d-fach gestreckt$/,
    );
    // Kantenangaben im Grünstreifen; „Mitte“ entfällt (Thomas 22.10.2026).
    const skala = [...z.svg.querySelectorAll('[data-rolle="querskala"] text')].map((t) => t.textContent);
    expect(skala).toEqual(["Kante 22,6 m links", "Kante 22,6 m rechts"]);
    // „07“ und Länge in der Zeile von BAHN-ENDE, nicht in einer Spalte.
    const texte = [...z.svg.querySelectorAll("text")];
    const ende = texte.find((t) => t.textContent === "BAHN-ENDE")!;
    for (const k of ["07", "1970 m"])
      expect(texte.find((t) => t.textContent === k)!.getAttribute("y"), k).toBe(ende.getAttribute("y"));
    // Handy: gestaucht statt im Kasten gerollt.
    expect(z.svg.style.minWidth).toBe("");
    expect((z.container.querySelector('[data-rolle="bahn-grafik"]') as HTMLElement).style.overflowX).not.toBe("auto");
  });

  it("eine Legende direkt unter der Draufsicht, ein Satz statt Fliesstext (Entscheidung 6)", () => {
    const z = draufsicht();
    const legenden = z.container.querySelectorAll('[data-rolle="legende"]');
    expect(legenden.length).toBe(1);
    expect(z.container.textContent).toMatch(
      /Quer stark gestreckt, damit schon ein, zwei Meter sichtbar werden\. Echte Winkel zeigt die Lupe darunter\./,
    );
    expect(z.container.textContent).not.toMatch(/Blick von oben auf die ganze Bahn/);
    // Die Legende steht VOR der Lupe.
    const lupe = z.container.querySelector('svg[data-ansicht="lupe"]')!;
    expect(legenden[0]!.compareDocumentPosition(lupe) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

// ── 7 · Jedes gezeichnete Element steht in der Legende ─────────────────
// Fängt auch KÜNFTIGE Elemente: Jede Form im Bild (rect, line, path,
// polyline, polygon, circle) muss über sich oder einen Vorfahren ein
// `data-legende` tragen. Entweder einen Schlüssel, der in der Legende unter
// der Grafik steht, oder `beschriftung` (Skalen, Klammern, Bezugsstriche —
// sie tragen ihren Text selbst). Eine neue Form ohne Zuordnung wird rot.
describe("Legende vollständig — jedes gezeichnete Element (alle elf Fälle)", () => {
  const FORMEN = "rect, line, path, polyline, polygon, circle";
  for (const id of [1062, 1662, 1738, 1779, 1842, 1884, 1905, 1909, 1914, 1916, 1936]) {
    it(`#${id}`, () => {
      const z = zeichne(fall(id));
      const eintraege = new Set(
        [...z.container.querySelectorAll("[data-legende-eintrag]")].map((e) => e.getAttribute("data-legende-eintrag")),
      );
      const fehlt = new Set<string>();
      for (const f of z.svg.querySelectorAll(FORMEN)) {
        if (f.closest("defs, clipPath, pattern")) continue;
        const k = f.closest("[data-legende]")?.getAttribute("data-legende");
        if (!k) fehlt.add(`${f.tagName} ohne Zuordnung (${f.parentElement?.getAttribute("data-rolle") ?? "?"})`);
        else if (k !== "beschriftung" && !eintraege.has(k)) fehlt.add(`${k} fehlt in der Legende`);
      }
      expect([...fehlt]).toEqual([]);
    });
  }
});

// ── Thomas 22.10.2026 (nach den 2.1.8-Bildern) ─────────────────────────
describe("Kantenangabe überdeckt nichts (alle elf Fälle)", () => {
  for (const id of [1062, 1662, 1738, 1779, 1842, 1884, 1905, 1909, 1914, 1916, 1936]) {
    it(`#${id}: „Kante … m“ liegt auf keinem Stummel und keinem anderen Text`, () => {
      const z = zeichne(fall(id));
      const kanten = [...z.svg.querySelectorAll('[data-rolle="querskala"] text')];
      const kasten = (t: Element) => textKasten(t);
      const ueber = (a: { links: number; rechts: number; oben: number; unten: number }, b: typeof a) =>
        !(a.rechts <= b.links || b.rechts <= a.links || a.unten <= b.oben || b.unten <= a.oben);
      const stummel = [...z.svg.querySelectorAll('[data-rolle="ausfahrt"]')].map((r) => {
        const x = Number(r.getAttribute("x"));
        const y = Number(r.getAttribute("y"));
        return { links: x, rechts: x + Number(r.getAttribute("width")), oben: y, unten: y + Number(r.getAttribute("height")) };
      });
      const andere = [...z.svg.querySelectorAll("text")].filter((t) => !kanten.includes(t) && (t.textContent ?? "").trim());
      for (const k of kanten) {
        const kk = kasten(k);
        expect(stummel.filter((s) => ueber(kk, s)).length, `${k.textContent} auf Stummel`).toBe(0);
        const treffer = andere.filter((t) => ueber(kk, kasten(t))).map((t) => t.textContent);
        expect(treffer, `${k.textContent} auf Text`).toEqual([]);
      }
    });
  }
});

describe("#1936 Altbestand: kein Rollwegstück auf der Bahn als Ausfahrt", () => {
  it("unten bei ~1840 m kein Stummel und kein „B“, in der Lupe unten kein „B“", () => {
    const z = draufsicht();
    const unten = [...z.svg.querySelectorAll('[data-rolle="ausfahrt"]')].filter(
      (r) => Number(r.getAttribute("y")) >= z.bot - 0.5,
    );
    const x1840 = z.projektion.mAbBahnanfangZuX(1839.6);
    expect(unten.filter((r) => Math.abs(Number(r.getAttribute("x")) + 4 - x1840) < 6).length).toBe(0);
    const namenUnten = [...z.svg.querySelectorAll('[data-rolle="ausfahrt-name"]')]
      .filter((t) => Number(t.getAttribute("y")) > z.bot)
      .map((t) => t.textContent);
    expect(namenUnten).not.toContain("B");
    // Lupe: Namen unter der Bahn (rechte Seite) — kein „B“ mehr.
    const lupe = z.container.querySelector('svg[data-ansicht="lupe"]')!;
    const H = Number(lupe.getAttribute("viewBox")!.split(" ")[3]);
    const lupeUnten = [...lupe.querySelectorAll("text")]
      .filter((t) => t.textContent === "B")
      .map((t) => Number(t.getAttribute("y")));
    expect(lupeUnten.filter((yy) => yy > H / 2)).toEqual([]);
  });
});

// ── Codex 22.10.2026 ───────────────────────────────────────────────────
describe("Codex-Befunde 4 und 5", () => {
  it("4 · ist clearance_exit gesetzt, zeigt die Anzeige NUR diesen Namen — kein anderer Kandidat", () => {
    // Der gemeldete Name liegt nicht in der Liste (andere Revision): Kopf
    // nennt ihn trotzdem, kein Stummel wird stattdessen hervorgehoben.
    cleanup();
    const { container } = render(<RunwayDiagramV2 {...fall(1936)} clearance_exit="Q9" runway_exits_quelle="szenerie" />);
    const svg = container.querySelector("svg")!;
    const kopf = [...svg.querySelectorAll("text")].map((t) => /Ausfahrt (\S+) (links|rechts)/.exec(t.textContent ?? "")?.[1]).find(Boolean);
    expect(kopf).toBe("Q9");
    expect([...svg.querySelectorAll('[data-rolle="ausfahrt-name"][font-weight="700"]')].map((t) => t.textContent)).toEqual([]);
    expect(svg.querySelector('[data-rolle="weiter-pfeil"]')?.textContent).toBe("weiter auf Rollweg Q9");
  });

  it("5 · die beschlossenen Legendeneinträge stehen fest da (Entscheidung 6), unabhängig vom DOM", () => {
    // Schwelle, TDZ, AIM, Band, Flugzeugmitte, Grünstreifen, Rollwege,
    // Marken 1/2/3, Dreieck — bei #1936 alle gezeichnet, also alle Pflicht.
    const z = draufsicht();
    const da = [...z.container.querySelectorAll('[data-rolle="legende"] [data-legende-eintrag]')].map((e) =>
      e.getAttribute("data-legende-eintrag"),
    );
    for (const k of ["schwelle", "tdz", "aim", "td", "band", "mitte", "gruen", "rollwege", "marke2", "marke3", "dreieck"])
      expect(da, k).toContain(k);
  });
});
