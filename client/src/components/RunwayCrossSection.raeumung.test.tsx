// Die Räumungsmarke darf keine Seite behaupten, die niemand kennt.
//
// ⚠ Anlass EIN3641 (EGAC 04, ATR 72, 29.08.2026): Räumungspunkt bei 884 m,
// `clearance_side` = null, Räumungsgeschwindigkeit 1,16 kt — das Flugzeug
// war dort schlicht ausgerollt und hat die Bahn nie verlassen. Die Zeichnung
// setzte die Marke trotzdem auf die Kante, weil `null === "left"` falsch ist
// und der else-Zweig die RECHTE Kante nimmt. Aus „wir wissen es nicht" wurde
// „rechts raus". Im Bestand betraf das 6 von 58 Landungen.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { RunwayCrossSection } from "./RunwayCrossSection";

const TOKENS = {
  tdOk: "#22c55e",
  tdWarn: "#f59e0b",
  tdBad: "#ef4444",
  rollout: "#38bdf8",
  grid: "#334155",
  text: "#94a3b8",
  bahn: "#1e293b",
} as unknown as Parameters<typeof RunwayCrossSection>[0]["tokens"];

/** EGAC 04 wie im Bestand: 1829 m lang, 45,1 m breit. */
function egac04() {
  return erzeugeProjektion({ laengeM: 1829, breiteM: 45.1, width: 1200 });
}

const SPUR = [
  { laengs_m: 383, quer_m: -0.2 },
  { laengs_m: 600, quer_m: -3.1 },
  { laengs_m: 884, quer_m: -1.4 },
];

function zeichne(clearanceSide: "left" | "right" | null) {
  return render(
    <RunwayCrossSection
      projektion={egac04()}
      runwayWidthM={45.1}
      trackWidthM={4.1}
      samples={SPUR}
      touchdownM={383}
      touchdownOffsetM={-0.2}
      clearanceM={884.5}
      clearanceSide={clearanceSide}
      width={1200}
      tokens={TOKENS}
    />,
  );
}

/** Kreise, die auf einer der beiden Bahnkanten sitzen. */
function marken(container: HTMLElement) {
  return Array.from(container.querySelectorAll("circle")).map((c) => ({
    x: Number(c.getAttribute("cx")),
    y: Number(c.getAttribute("cy")),
  }));
}

describe("Räumungsmarke", () => {
  it("behauptet ohne bekannte Seite keine Kante", () => {
    const { container } = zeichne(null);
    const alle = marken(container);
    const ys = alle.map((m) => m.y);
    const min = Math.min(...ys);
    const max = Math.max(...ys);
    // Alle Marken müssen im gemessenen Bereich liegen — die Spur bewegt
    // sich zwischen −3,1 m und −0,2 m, also nahe der Mitte. Eine Marke
    // auf der Kante läge weit ausserhalb dieser Spanne.
    const { container: mitSeite } = zeichne("right");
    const kanteY = Math.max(...marken(mitSeite).map((m) => m.y));
    expect(max).toBeLessThan(kanteY - 20);
    expect(min).toBeGreaterThan(0);
  });

  it("setzt sie mit bekannter Seite weiterhin auf die Kante", () => {
    // Die Gegenprobe: Der Fall, für den die Marke gebaut ist, muss
    // unverändert funktionieren.
    const rechts = Math.max(...marken(zeichne("right").container).map((m) => m.y));
    const links = Math.min(...marken(zeichne("left").container).map((m) => m.y));
    expect(rechts).toBeGreaterThan(links);
  });

  it("laesst die Spur auch ohne Seite nicht ohne Endpunkt", () => {
    // Ohne Marke ③ muss der Endpunkt greifen — sonst hört die Spur
    // unkommentiert auf und der Leser sucht das Flugzeug.
    const ohne = marken(zeichne(null).container).length;
    expect(ohne).toBeGreaterThanOrEqual(2);
  });
});
