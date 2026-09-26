/**
 * v1.8.1: Der Mapper reicht Raederhoehe und Hoehengruppe aus dem
 * LandingRecord durch — als Wert geprueft, nicht nur als Feldname
 * (Codex-Befund 7).
 */
import { describe, expect, it } from "vitest";
import { mapLandingRecordToV2Props } from "./runwayDiagramV2Mapper";
import { MOCK_LANDING_OPTIONS } from "./mockLandingRecords";

const basis = MOCK_LANDING_OPTIONS.map((o) => o.build()).find((r) => r.runway_match)!;

describe("Mapper: TCH nach Raederhoehe", () => {
  it("reicht tch_rad_ft und tch_hoehengruppe aus dem Record durch", () => {
    const p = mapLandingRecordToV2Props({ ...basis, tch_rad_ft: 20.5, tch_hoehengruppe: 2 })!;
    expect(p.tch_rad_ft).toBe(20.5);
    expect(p.tch_hoehengruppe).toBe(2);
  });

  it("alte Records ohne die Felder ergeben null — die Anzeige nimmt dann die alten Texte", () => {
    const { tch_rad_ft: _a, tch_hoehengruppe: _b, ...alt } = { ...basis, tch_rad_ft: 1, tch_hoehengruppe: 1 };
    void _a;
    void _b;
    const p = mapLandingRecordToV2Props(alt)!;
    expect(p.tch_rad_ft).toBeNull();
    expect(p.tch_hoehengruppe).toBeNull();
  });
});
