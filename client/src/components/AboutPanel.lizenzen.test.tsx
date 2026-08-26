import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/**
 * Die Lizenzvermerke müssen im „Über"-Bereich stehen bleiben.
 *
 * # Warum das ein Test ist und kein Kommentar
 *
 * Zwei der Datenquellen verlangen die Nennung nicht als Höflichkeit,
 * sondern als Bedingung:
 *
 *   * **OpenStreetMap** (ODbL) — Nennung der Mitwirkenden und der Lizenz.
 *   * **X-Plane apt.dat** (GNU GPL) — „The complete copyright message
 *     must be left intact if you redistribute this data." Der Vermerk
 *     lautet auf Robin A. Peel.
 *
 * Wir geben beide Bestände an alle Piloten weiter. Fällt der Vermerk bei
 * einem Umbau der Seite heraus, merkt das niemand — sichtbar ist er nur
 * für den, der ihn sucht. Deshalb hält ihn eine Prüfung fest und nicht
 * die Aufmerksamkeit.
 */
describe("Über — Lizenzvermerke", () => {
  const quelle = readFileSync(
    resolve(__dirname, "AboutPanel.tsx"),
    "utf-8",
  );

  const NOETIG: Array<{ was: string; muss: string[] }> = [
    {
      was: "OpenStreetMap (ODbL)",
      muss: ["OpenStreetMap", "Open Database License", "opendatacommons.org"],
    },
    {
      was: "X-Plane apt.dat (GPL)",
      muss: [
        "Robin A. Peel",
        "General Public License",
        "gateway.x-plane.com",
      ],
    },
  ];

  for (const { was, muss } of NOETIG) {
    it(`nennt ${was}`, () => {
      const fehlt = muss.filter((m) => !quelle.includes(m));
      expect(
        fehlt,
        `Im „Über"-Bereich fehlt: ${fehlt.join(", ")} — das ist eine ` +
          `Lizenzbedingung, keine Formsache.`,
      ).toEqual([]);
    });
  }

  it("sagt, dass von X-Plane nur die Bezeichnungen stammen", () => {
    // Der Unterschied ist wichtig: Die Geometrie bleibt OSM. Stünde das
    // nicht da, läse sich der Vermerk so, als käme die ganze Rollkarte
    // aus X-Plane — und die ODbL-Nennung ginge ins Leere.
    expect(
      /designator/i.test(quelle) && /geometry stays/i.test(quelle),
      "der Vermerk unterscheidet nicht zwischen Bezeichnung und Geometrie",
    ).toBe(true);
  });
});
