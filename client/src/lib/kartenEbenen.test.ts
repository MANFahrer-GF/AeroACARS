// Kartenebenen: keine Ausdrücke an Eigenschaften, die keine nehmen.
//
// Diese Fehlerklasse hat zweimal die ganze Karte geleert:
//   1. `["==", ["get","ohne_hoehe"], 1]` bei fill-opacity — null gegen
//      Zahl, MapLibre wirft.
//   2. `["case", ["has","gebucht"], …]` bei line-dasharray — das ist
//      eine CrossFadedProperty und nimmt gar keine Ausdrücke.
//
// Beide Male riss der Fehler ALLE nachfolgenden Ebenen mit: keine
// Sektoren, keine Lotsen, keine Türme, nur noch Verkehr. Der Test
// verhindert die Wiederholung, indem er die Quelldatei selbst prüft.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/** Eigenschaften, die in MapLibre KEINE datengetriebenen Ausdrücke
 *  annehmen — ein `case`/`get`/`has` darin lässt addLayer werfen. */
const OHNE_AUSDRUECKE = [
  "line-dasharray",
  "line-gradient",
  "fill-pattern",
  "line-pattern",
  "background-pattern",
];

// Vom Projektwurzelverzeichnis aus — der Test läuft unter vitest, das
// die Wurzel als Arbeitsverzeichnis setzt.
const QUELLE = resolve(process.cwd(), "src/components/LiveMapView.tsx");

describe("Kartenebenen", () => {
  const text = readFileSync(QUELLE, "utf8");

  for (const eigenschaft of OHNE_AUSDRUECKE) {
    it(`"${eigenschaft}" bekommt keinen Ausdruck`, () => {
      const muster = new RegExp(
        `"${eigenschaft}"\\s*:\\s*\\[\\s*"(case|match|step|interpolate|get|has|coalesce)`,
        "g",
      );
      const treffer = [...text.matchAll(muster)];
      expect(
        treffer.length,
        `${eigenschaft} mit Ausdruck — MapLibre wirft beim Anlegen und ` +
        `alle nachfolgenden Ebenen fehlen`,
      ).toBe(0);
    });
  }

  it("die VATSIM-Ebenen laufen über die abgesicherte Anlage", () => {
    // Eine kaputte Ebene darf fehlen, aber nicht die Karte leeren.
    const ungeschuetzt = [...text.matchAll(/map\.addLayer\(\{\s*\n\s*id: "vatsim-/g)];
    expect(
      ungeschuetzt.length,
      "VATSIM-Ebene wird ohne ebeneAnlegen() angelegt",
    ).toBe(0);
  });
});
