// Eine Landung ohne aufgezeichneten Aufsetzmoment bekommt keine Zahl.
//
// Anlass 12.09.2026, CFG 2090 (EDDF→KPDX, A339, X-Plane 12, 10:44 h):
// Zwischen der letzten Probe in der Luft (6,4 ft) und dem ersten
// Bodenkontakt lagen 0,92 s ohne jede Messung. Die Bewertung nahm den
// ersten Frame danach — −10,39 fpm — und machte daraus 97 Punkte, Note A+,
// "butterweich". Der Anflug war zugleich als zu steil markiert.
//
// Die Sperre sitzt im Rust-Teil; hier wird geprüft, dass der Client sie
// nicht unterläuft: Er rechnet für Altdatensätze selbst weiter, und genau
// dort entstünde sonst wieder eine Zahl, die daneben niemand verantwortet.
import { describe, expect, it } from "vitest";
import { istBewertbar, scoreBasisVs } from "./SinkrateForensik";

const GEMESSEN = {
  vs_at_edge_fpm: -152,
  landing_peak_vs_fpm: -160,
  landing_rate_fpm: -160,
};

describe("Landung ohne ausreichende Aufzeichnung", () => {
  it("liefert keine Sinkrate, auch wenn die alten Felder gefüllt sind", () => {
    // Genau der Fall aus dem Bestand: Die Zahlen stehen da, sie sind nur
    // nichts wert. Ein Client, der sie trotzdem nimmt, zeigt etwas anderes
    // als PIREP und Webapp — derselbe Riss wie beim PIA3452-Split.
    const record = {
      ...GEMESSEN,
      landung_nicht_bewertbar: { groesste_luecke_ms: 920, proben: 5 },
    };
    expect(scoreBasisVs(record)).toBeNull();
    expect(istBewertbar(record)).toBe(false);
  });

  it("lässt eine sauber gemessene Landung unangetastet", () => {
    expect(scoreBasisVs(GEMESSEN)).toBe(-152);
    expect(istBewertbar(GEMESSEN)).toBe(true);
  });

  it("behandelt fehlende Angabe als bewertbar", () => {
    // Altdatensätze von vor dieser Änderung haben das Feld nicht. Sie
    // bleiben, wie sie sind — rückwirkend wird nichts entwertet.
    expect(istBewertbar({ ...GEMESSEN, landung_nicht_bewertbar: null })).toBe(true);
    expect(istBewertbar(GEMESSEN)).toBe(true);
  });
});
