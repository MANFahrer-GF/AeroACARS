// AeroACARS 3.0, Etappe 5 (P-8.21): Die Flugkarte selbst gibt es nur noch einmal —
// im Laufzeit-Paket (Client-Repo, ui/flugkarte/); die Admin-Webapp laedt sie von dort.
// Ihre Texte `flugkarte.*` (ohne `fenster`, das gehoert zum jeweiligen Rahmen) stehen
// aber noch an zwei Stellen: bundle/texte (die Karte zeigt sie) und
// webapp/src/locales (der Recorder prueft daran seinen deutschen Rueckfall der
// Flugakte, flugakte.test.ts). Dieser Fingerabdruck steht in BEIDEN Repos gleich;
// wer einen Text aendert, zieht die andere Seite und beide Werte mit.
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const FINGERABDRUCK = "d6dbaaa749c9600e83e3ec356de77f41b7879277ea12ad2dc30095b058ad2af4";

describe("Flugkarte: Texte gleich in Paket (Client) und Webapp-Referenz", () => {
  it("flugkarte.* ohne fenster hat den vereinbarten Fingerabdruck", () => {
    const h = createHash("sha256");
    for (const l of ["de", "en", "it"]) {
      const fk = { ...JSON.parse(readFileSync(`../bundle/texte/${l}.json`, "utf8")).flugkarte };
      delete fk.fenster;
      h.update(JSON.stringify(fk));
    }
    expect(h.digest("hex")).toBe(FINGERABDRUCK);
  });
});
