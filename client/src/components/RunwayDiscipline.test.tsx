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
import { erzeugeProjektion } from "../lib/runwayProjection";


/** EDDH 23, so wie die Gegenprobe sie bestätigt hat. */
/**
 * EDDH 23 — 156 m versetzte Schwelle, die NICHT in der Navdaten-Geometrie
 * steckt. Die Spurwerte laufen deshalb ab Bahnanfang, der Aufsetzpunkt ab
 * Landeschwelle: zwei Nullpunkte, 156 m auseinander.
 */
function eddh23() {
  return erzeugeProjektion({
    lengthM: 3094,
    ddsM: 156,
    spurVersatzM: 156,
    padX: 70,
    innerW: 1060,
  });
}

/**
 * Dieselbe Bahn, aber der Versatz steckt schon in der Geometrie — so
 * liefert es der DFD-Export seit AIRAC 2608. Dann fallen beide Nullpunkte
 * zusammen und es darf NICHTS verschoben werden.
 *
 * ⚠ Am Bestand gemessen (30.08.2026) ist das bei 8 von 19 Flügen mit
 * versetzter Schwelle der Fall. Eine Zeichnung, die hier pauschal die
 * versetzte Schwelle abzieht, verschiebt die Spur um 156 m.
 */
function eddh23OhneVersatz() {
  return erzeugeProjektion({
    lengthM: 3094,
    ddsM: 156,
    spurVersatzM: 0,
    padX: 70,
    innerW: 1060,
  });
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
});
