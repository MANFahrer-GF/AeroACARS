// Gleichstand der Flugkarte zwischen Admin-Webapp (aeroacars-live) und AeroACARS (aeroacars-src).
//
// Pilot und Admin sollen exakt dasselbe sehen (Thomas, 28.10.2026). Darum sind
// Baustein, Typen, CSS, Kartenstil und die Texte unter `flugkarte.*` in beiden
// Repos Kopien voneinander. Dieser Fingerabdruck steht in BEIDEN Repos gleich:
// Wer eine Seite ändert, muss die Änderung in die andere übernehmen und den
// Wert hier und dort gemeinsam neu setzen — sonst laufen die Sichten still auseinander.
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const FINGERABDRUCK = "37b5a46dea73c9cd040c9dc0a81b24f7fadfd7a87e08d1e252b0eda4f4b47001";

describe("Flugkarte: Gleichstand Webapp ↔ AeroACARS", () => {
  it("Baustein, Kartenstil und Texte sind dieselben wie im anderen Repo", () => {
    const h = createHash("sha256");
    for (const f of ["flugkarte.ts", "flugakteTypen.ts", "flugkarte.css"]) h.update(readFileSync(`src/flugkarte/${f}`));
    h.update(JSON.stringify(JSON.parse(readFileSync("src/flugkarte/stil-dunkel.json", "utf8"))));
    for (const l of ["de", "en", "it"]) h.update(JSON.stringify(JSON.parse(readFileSync(`src/locales/${l}/common.json`, "utf8")).flugkarte));
    expect(h.digest("hex")).toBe(FINGERABDRUCK);
  });
});
