// Prüfungen für die Bahndisziplin-Anzeige.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.4 (eine Projektion) und
// §8.6 (Lesbarkeit als harte Anforderung).
//
// # Warum das Tests sind und keine Sichtprüfung
//
// Beide Fehlerklassen, gegen die hier geprüft wird, sind am 23.08.2026
// tatsächlich aufgetreten — und beide sahen für sich betrachtet plausibel aus:
//
//   * Der Aim-Marker stand 209 m an der falschen Stelle. Auffällig wurde das
//     erst, als beide Ansichten untereinander standen und nicht fluchteten.
//   * Eine Kollisionsprüfung, die nur Text gegen Text testete, meldete
//     „0 Kollisionen", während die Versatz-Beschriftung quer über der
//     Fahrspur lag.

import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { RunwayCrossSection } from "./RunwayCrossSection";

const TOKENS = {
  tarmac: "#1e293b",
  tarmacBorder: "#475569",
  centerline: "#e2e8f0",
  rollout: "#38bdf8",
  tdPerfect: "#22c55e",
  tdWarn: "#f59e0b",
  tdSevere: "#ef4444",
};

/** EDDH 23, so wie die Gegenprobe sie bestätigt hat. */
function eddh23() {
  return erzeugeProjektion({ lengthM: 3094, ddsM: 156, padX: 70, innerW: 1060 });
}

describe("§8.4 — eine Projektion für beide Ansichten", () => {
  it("bildet die Landeschwelle hinter den Bahnanfang ab", () => {
    const p = eddh23();
    // Bei versetzter Schwelle liegt der Nullpunkt NICHT am linken Rand.
    expect(p.thresholdX).toBeGreaterThan(p.bahnAnfangX);
    // …und zwar um genau den Versatz.
    expect(p.thresholdX - p.bahnAnfangX).toBeCloseTo(156 * p.pxProMeter, 6);
  });

  it("legt Bahnanfang und Bahnende auf die Ränder des Zeichenbereichs", () => {
    const p = eddh23();
    expect(p.bahnAnfangX).toBe(70);
    expect(p.bahnEndeX).toBe(70 + 1060);
    // Das Bahnende ist der letzte Meter der nutzbaren Länge.
    expect(p.mToX(3094)).toBeCloseTo(p.bahnEndeX, 6);
  });

  it("begrenzt beim Zeichnen, aber nicht beim Messen", () => {
    const p = eddh23();
    // Zeichnen: nichts läuft aus dem Bild.
    expect(p.mToX(99999)).toBeCloseTo(p.bahnEndeX, 6);
    expect(p.mToX(-99999)).toBeCloseTo(p.bahnAnfangX, 6);
    // Messen: ein Überrollen bleibt sichtbar als Zahl.
    expect(p.mToXUnbegrenzt(3200)).toBeGreaterThan(p.bahnEndeX);
  });

  it("hält die Untergrenze von einer echten kurzen Bahn fern", () => {
    // 292 m ist die kürzeste nutzbare Länge in den Navdaten — sie muss
    // unverändert durchkommen. Die früheren 500 m überschrieben sie.
    const kurz = erzeugeProjektion({ lengthM: 292, ddsM: 0, padX: 70, innerW: 1060 });
    expect(kurz.lengthM).toBe(292);
    // Ein kaputter Kleinstwert wird dagegen abgefangen.
    const kaputt = erzeugeProjektion({ lengthM: 0.5, ddsM: 0, padX: 70, innerW: 1060 });
    expect(kaputt.lengthM).toBe(100);
    const nan = erzeugeProjektion({ lengthM: NaN, ddsM: 0, padX: 70, innerW: 1060 });
    expect(Number.isFinite(nan.lengthM)).toBe(true);
  });

  it("bildet denselben Meter in beiden Ansichten auf dasselbe X ab", () => {
    // Das ist das Prüfkriterium aus §8.4: Der Aufsetzpunkt oben muss
    // senkrecht über der Marke in der Queransicht liegen. Er hält, weil
    // beide Ansichten DIESELBE Funktion bekommen — nicht zwei gleich
    // aussehende.
    const p = eddh23();
    const { container } = render(
      <RunwayCrossSection
        projektion={p}
        runwayWidthM={46}
        trackWidthM={7.59}
        // ⚠ EDDH 23 hat 156 m versetzte Schwelle, und die beiden Werte
        // haben VERSCHIEDENE Bezugspunkte: `laengs_m` ab Bahnanfang,
        // `touchdownM` ab Landeschwelle. Derselbe Ort ist also 656 bzw.
        // 500 — vorher standen hier zweimal 500, und der Test prueft
        // seitdem, dass beide Ansichten denselben Meter treffen.
        // Siehe docs/spec/runway-diagram-v2.contract.md, Abschnitt
        // „Zwei Bezugspunkte".
        samples={[
          { laengs_m: 656, quer_m: -8.7 },
          { laengs_m: 1156, quer_m: 26.8 },
        ]}
        touchdownM={500}
        touchdownOffsetM={-8.7}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const kreis = container.querySelector("circle");
    expect(kreis).not.toBeNull();
    expect(Number(kreis!.getAttribute("cx"))).toBeCloseTo(p.mToX(500), 1);
  });
});

describe("§8.6 — Lesbarkeit", () => {
  it("zeichnet ohne Bahnbreite gar nichts, statt zu raten", () => {
    const { container } = render(
      <RunwayCrossSection
        projektion={eddh23()}
        runwayWidthM={0}
        trackWidthM={7.59}
        samples={[{ laengs_m: 500, quer_m: 0 }]}
        touchdownM={500}
        touchdownOffsetM={0}
        width={1200}
        tokens={TOKENS}
      />,
    );
    expect(container.querySelector("svg")).toBeNull();
  });

  it("hält allen Inhalt im Zeichenbereich", () => {
    // §8.6.2. Im ersten Entwurf standen die Bahnkennungen halb ausserhalb.
    const p = eddh23();
    const { container } = render(
      <RunwayCrossSection
        projektion={p}
        runwayWidthM={46}
        trackWidthM={10.7}
        // Ein Extremfall, wie er im Bestand vorkommt: Der EDDL-Fall lag bei
        // 52,6 m Versatz auf einer 45-m-Bahn. Solche Werte werden zwar von
        // der Bewertung uebersprungen (`implausible_lateral_track`), aber
        // gezeichnet wird die Spur trotzdem -- und sie darf das Bild nicht
        // verlassen. Ohne Begrenzung in `querZuY` lief sie hier heraus.
        samples={[
          { laengs_m: 200, quer_m: -120 },
          { laengs_m: 1500, quer_m: 90 },
        ]}
        touchdownM={200}
        touchdownOffsetM={-120}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const svg = container.querySelector("svg")!;
    const [, , vbW, vbH] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    for (const el of Array.from(svg.querySelectorAll("circle, text, rect, line"))) {
      for (const [attr, max] of [
        ["cx", vbW!],
        ["x", vbW!],
        ["x1", vbW!],
        ["x2", vbW!],
        ["cy", vbH!],
        ["y", vbH!],
        ["y1", vbH!],
        ["y2", vbH!],
      ] as const) {
        const v = el.getAttribute(attr);
        if (v == null) continue;
        const n = Number(v);
        if (!Number.isFinite(n)) continue;
        expect(n).toBeGreaterThanOrEqual(0);
        expect(n).toBeLessThanOrEqual(max);
      }
    }
  });

  it("beschriftet die Seiten mit Wörtern, nicht mit Vorzeichen", () => {
    // §8.6: „Ein Pilot denkt in Seiten, nicht in Koordinaten." Im ersten
    // Entwurf war die Skala mathematisch beschriftet (+ oben) — dieselbe
    // Seite lag damit in einer Ansicht oben und in der anderen unten.
    const { container } = render(
      <RunwayCrossSection
        projektion={eddh23()}
        runwayWidthM={46}
        trackWidthM={7.59}
        samples={[
          { laengs_m: 400, quer_m: 0 },
          { laengs_m: 900, quer_m: 5 },
        ]}
        touchdownM={400}
        touchdownOffsetM={0}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const texte = Array.from(container.querySelectorAll("text")).map((e) => e.textContent);
    expect(texte).toContain("LINKS");
    expect(texte).toContain("RECHTS");
    expect(texte.some((s) => s?.includes("+"))).toBe(false);
  });

  it("legt oben nach links, wie die Längsansicht", () => {
    // Die verbindliche Seitenkonvention aus §8.6: `oben = links in
    // Landerichtung`. Intern bleibt `quer > 0 = rechts` — ein Punkt rechts
    // muss also WEITER UNTEN landen als einer links.
    const p = eddh23();
    const { container } = render(
      <RunwayCrossSection
        projektion={p}
        runwayWidthM={46}
        trackWidthM={7.59}
        samples={[
          { laengs_m: 400, quer_m: -15 },
          { laengs_m: 900, quer_m: 15 },
        ]}
        touchdownM={400}
        touchdownOffsetM={-15}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const svg = container.querySelector("svg")!;
    const linksY = Number(
      Array.from(svg.querySelectorAll("text"))
        .find((e) => e.textContent === "LINKS")!
        .getAttribute("y"),
    );
    const rechtsY = Number(
      Array.from(svg.querySelectorAll("text"))
        .find((e) => e.textContent === "RECHTS")!
        .getAttribute("y"),
    );
    expect(linksY).toBeLessThan(rechtsY);

    // Und die Marke des Aufsetzpunkts (15 m links) liegt oberhalb der Mitte.
    // Die Mitte kommt aus dem viewBox, nicht als feste Zahl: Sonst haengt
    // der Test an der Bildhoehe und wird beim naechsten Vergroessern rot,
    // ohne dass sich am Verhalten etwas geaendert haette.
    const [, , , vbH] = svg.getAttribute("viewBox")!.split(" ").map(Number);
    const kreis = svg.querySelector("circle")!;
    expect(Number(kreis.getAttribute("cy"))).toBeLessThan(vbH! / 2);
  });

  it("gibt jeder Nummer der Liste eine Marke im Bild", () => {
    // §8.5: Die Liste verweist auf die Marken. Eine Nummer in der Liste
    // ohne Marke im Bild laesst den Leser suchen — aufgefallen am 23.08.
    // beim Rendern der Varianten: Der Ueberroll-Eintrag ④ stand in der
    // Liste, im Bild gab es nur ① bis ③.
    const { container } = render(
      <RunwayCrossSection
        projektion={eddh23()}
        runwayWidthM={46}
        trackWidthM={5.72}
        samples={[
          { laengs_m: 1850, quer_m: 0.4 },
          { laengs_m: 3094, quer_m: 3.0 },
        ]}
        touchdownM={1850}
        touchdownOffsetM={0.4}
        maxLateralOffsetM={3.0}
        overrunM={84}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const ziffern = Array.from(container.querySelectorAll("text"))
      .map((e) => e.textContent)
      .filter((s) => s != null && /^\d$/.test(s));
    expect(ziffern).toContain("4");
  });

  it("zeichnet die Spur als ein Band, nicht als drei Linien", () => {
    // §8.5: Drei getrennte Linien laufen bei steilen Abschnitten optisch
    // auseinander und sehen aus, als kreuzten sie einander.
    const { container } = render(
      <RunwayCrossSection
        projektion={eddh23()}
        runwayWidthM={46}
        trackWidthM={10.7}
        samples={[
          { laengs_m: 400, quer_m: 0 },
          { laengs_m: 800, quer_m: 12 },
          { laengs_m: 1200, quer_m: 2 },
        ]}
        touchdownM={400}
        touchdownOffsetM={0}
        width={1200}
        tokens={TOKENS}
      />,
    );
    const pfade = Array.from(container.querySelectorAll("path"));
    const band = pfade.find((p) => p.getAttribute("fill") !== "none");
    expect(band).toBeDefined();
    // Ein geschlossener Umriss — Hin- und Rückweg plus `Z`.
    expect(band!.getAttribute("d")).toMatch(/Z$/);
  });
});
