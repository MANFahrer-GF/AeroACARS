// QS 2026-08-04 — Regressions-Test fuer die Landungs-Kategorie.
//
// Vorgeschichte in drei Stufen, jede hat die naechste Abweichung erzeugt:
//   v2  Bericht + Uebersicht lasen `record.score_label` woertlich. Das Feld
//       wird beim Einreichen eingefroren; nach der v0.20.0-Umstellung der
//       Schwellen trugen Alt-Fluege fuer immer das alte Wort.
//   v3  Die Uebersicht rechnete die Leiter in TypeScript nach. Damit stand
//       dieselbe Regel zweimal im Code — und Uebersicht ("SMOOTH") und
//       Bericht ("ACCEPTABLE") widersprachen sich fuer DIESELBE Landung
//       (real gemessen: 2 von 31 Fluegen, beide 88 Punkte / Note A).
//   v4  Geloest an der Quelle: `landing_list` (Rust) berechnet die
//       abgeleiteten Felder beim Lesen neu. Genau EINE Regel, in Rust.
//
// Dieser Test haelt die v4-Eigenschaft fest: das Frontend leitet die
// Kategorie NUR noch aus dem gelieferten Label ab und baut die Leiter NICHT
// wieder selbst nach. Wuerde jemand hier erneut aus `score_numeric` rechnen,
// schlaegt der erste Fall fehl.
import { describe, it, expect } from "vitest";
import { recordCategory, rateCategoryWord } from "./LandingPanel";
import type { LandingRecord } from "../lib/landingScoring";

const rec = (score_label: string, score_numeric: number) =>
  ({ score_label, score_numeric }) as unknown as LandingRecord;

describe("recordCategory — eine Quelle fuer Uebersicht und Bericht", () => {
  it("folgt dem gelieferten Label und rechnet NICHT selbst aus der Punktzahl", () => {
    // Punktzahl und Label absichtlich widerspruechlich: das Backend ist die
    // Autoritaet. Wer hier wieder eine eigene Leiter einbaut, faellt auf.
    expect(recordCategory(rec("smooth", 12))).toBe("smooth");
    expect(recordCategory(rec("severe", 99))).toBe("severe");
  });

  it("bildet alle fuenf Kategorien ab", () => {
    for (const c of ["smooth", "acceptable", "firm", "hard", "severe"] as const) {
      expect(recordCategory(rec(c, 80))).toBe(c);
    }
  });

  it("faengt ein unbekanntes Label ab, ohne die Anzeige zu sprengen", () => {
    expect(recordCategory(rec("was-ganz-neues", 80))).toBe("firm");
  });

  it("Uebersicht und Bericht zeigen zwangslaeufig dasselbe Wort", () => {
    // Beide Anzeigestellen gehen durch dieselben zwei Funktionen — hier
    // nachgestellt fuer den real gemessenen Fall (88 Punkte, Note A).
    const r = rec("smooth", 88);
    const uebersicht = rateCategoryWord(recordCategory(r));
    const bericht = rateCategoryWord(recordCategory(r));
    expect(uebersicht).toBe("SMOOTH");
    expect(bericht).toBe(uebersicht);
  });
});
