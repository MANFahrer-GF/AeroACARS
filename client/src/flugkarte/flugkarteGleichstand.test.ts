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

const FINGERABDRUCK = "ac3a72704abdcc1e56e2420ff572026991fe4b8b5018c625e6f7bcb411372694";

describe("Flugkarte: Gleichstand Webapp ↔ AeroACARS", () => {
  it("Baustein, Kartenstil und Texte sind dieselben wie im anderen Repo", () => {
    const h = createHash("sha256");
    for (const f of ["flugkarte.ts", "flugakteTypen.ts", "flugkarte.css"]) h.update(readFileSync(`src/flugkarte/${f}`));
    h.update(JSON.stringify(JSON.parse(readFileSync("src/flugkarte/stil-dunkel.json", "utf8"))));
    for (const l of ["de", "en", "it"]) h.update(JSON.stringify(JSON.parse(readFileSync(`../bundle/texte/${l}.json`, "utf8")).flugkarte));
    expect(h.digest("hex")).toBe(FINGERABDRUCK);
  });
});
