// Aufsetzpunkt und erste Spurprobe sind DERSELBE Ort.
//
// ⚠ Diese Eigenschaft muss auf JEDER Bahn gelten — und sie ist die
// Prüfung, die den Fehler von LAN273 sofort gefunden hätte.
//
// Der Payload führt zwei Bezugspunkte nebeneinander, ohne dass irgendwo
// stand, welcher wo gilt:
//
//   ab Landeschwelle:  td_distance_from_threshold_m, aim_point_m
//   ab Bahnanfang:     lateral_samples[].laengs_m, mess_ende_laengs_m,
//                      scoring_cutoff_m, clearance_point_m, Ausfahrten
//
// Auf Bahnen OHNE versetzte Schwelle sind beide gleich — deshalb fiel es
// nie auf. Auf TJPS 12 (573 m) lagen Aufsetzmarke und Rollspur exakt um
// diese 573 m auseinander.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { RunwayCrossSection } from "./RunwayCrossSection";

const TOKENS = {
  tdOk: "#22c55e", tdWarn: "#f59e0b", tdBad: "#ef4444", rollout: "#38bdf8",
  grid: "#334155", text: "#94a3b8", bahn: "#1e293b",
} as unknown as Parameters<typeof RunwayCrossSection>[0]["tokens"];

/**
 * Zeichnet eine Landung und gibt zurück, wie weit Aufsetzmarke und erste
 * Spurprobe im Bild auseinanderliegen — in Pixeln.
 */
function abstandImBild(opts: {
  laengeM: number;
  ddsM: number;
  breiteM: number;
  ersteProbeAbBahnanfangM: number;
}) {
  const { laengeM, ddsM, breiteM, ersteProbeAbBahnanfangM } = opts;
  // Der Aufsetzpunkt ist DIESELBE Stelle, nur ab der Schwelle gerechnet.
  const touchdownAbSchwelleM = ersteProbeAbBahnanfangM - ddsM;
  const projektion = erzeugeProjektion({
    lengthM: laengeM - ddsM,
    ddsM,
    padX: 70,
    innerW: 1060,
  });
  const { container } = render(
    <RunwayCrossSection
      projektion={projektion}
      runwayWidthM={breiteM}
      trackWidthM={7.6}
      samples={[
        { laengs_m: ersteProbeAbBahnanfangM, quer_m: 2.9 },
        { laengs_m: ersteProbeAbBahnanfangM + 200, quer_m: 4.3 },
        { laengs_m: ersteProbeAbBahnanfangM + 400, quer_m: 1.0 },
      ]}
      touchdownM={touchdownAbSchwelleM}
      touchdownOffsetM={2.9}
      width={1200}
      tokens={TOKENS}
    />,
  );
  const kreise = Array.from(container.querySelectorAll("circle"));
  expect(kreise.length).toBeGreaterThan(0);
  // Die Aufsetzmarke ist der erste gezeichnete Kreis.
  const td = Number(kreise[0]!.getAttribute("cx"));

  // ⚠ Die Spurposition wird aus der GEZEICHNETEN Grafik gelesen, nicht
  // nachgerechnet. Ein erster Entwurf dieses Tests hat sie selbst mit
  // `mAbBahnanfangZuX` bestimmt — und damit meine Rechnung gegen meine
  // Rechnung geprueft. Die Gegenprobe blieb gruen, obwohl der Fehler
  // wieder eingebaut war.
  const pfade = Array.from(container.querySelectorAll("path"))
    .map((el) => el.getAttribute("d") ?? "")
    .filter((d) => d.startsWith("M"));
  expect(pfade.length).toBeGreaterThan(0);
  // Der erste Punkt des ersten Spurpfades: "M <x> <y> ..."
  const ersterX = Math.min(
    ...pfade.map((d) => Number(d.slice(1).trim().split(/[\s,]+/)[0])),
  );
  return Math.abs(td - ersterX);
}

describe("Bezugspunkt", () => {
  it("Aufsetzmarke und Spur liegen bei versetzter Schwelle zusammen", () => {
    // TJPS 12 (LAN273, 30.08.2026): 2439 m lang, 573 m versetzte
    // Schwelle, 45,7 m breit. Aufgesetzt 337 m hinter dem Bahnanfang.
    const abstand = abstandImBild({
      laengeM: 2439,
      ddsM: 572.7,
      breiteM: 45.72,
      ersteProbeAbBahnanfangM: 337.1,
    });
    expect(abstand).toBeLessThan(2);
  });

  it("und ebenso ohne versetzte Schwelle", () => {
    // LIRF 16L (ITY81): keine versetzte Schwelle — hier waren die
    // beiden Bezugspunkte schon immer gleich, deshalb fiel der Fehler
    // jahrelang nicht auf. Der Fall muss weiter halten.
    const abstand = abstandImBild({
      laengeM: 3902,
      ddsM: 0,
      breiteM: 60.05,
      ersteProbeAbBahnanfangM: 1573.9,
    });
    expect(abstand).toBeLessThan(2);
  });

  it("eine sehr lange versetzte Schwelle verschiebt nichts", () => {
    // Gegenprobe mit einem Extremwert: Je groesser die versetzte
    // Schwelle, desto groesser waere der Fehler gewesen.
    const abstand = abstandImBild({
      laengeM: 4000,
      ddsM: 1500,
      breiteM: 45,
      ersteProbeAbBahnanfangM: 1800,
    });
    expect(abstand).toBeLessThan(2);
  });
});
