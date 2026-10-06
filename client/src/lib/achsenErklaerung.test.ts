// QS 06.10.2026: Jede bewertete Achse hat ihre (i)-Erklärung.
//
// LandungsBewertung.tsx zeigt an jeder Teilnote (außer `rollout`, das ein
// eigenes Hilfe-Fenster hat) `t("landing.info.<key>")`. Für „Abfangen" (neu
// in Score-Version 19) und „Aufsetzpunkt" fehlte der Text — im Fenster stand
// der rohe Schlüssel. Die Achsen kommen hier aus der Gewichtstabelle der
// Bewertung selbst, nicht aus einer abgeschriebenen Liste.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import de from "../locales/de/common.json";
import en from "../locales/en/common.json";
import it_ from "../locales/it/common.json";

const quelle = readFileSync(
  resolve(__dirname, "../../src-tauri/crates/landing-scoring/src/lib.rs"),
  "utf8",
);
// Der `match` in `gewichtetes_mittel` — bis zum Default-Zweig.
const block = quelle.slice(
  quelle.indexOf("fn gewichtetes_mittel"),
  quelle.indexOf("_ => 1.0", quelle.indexOf("fn gewichtetes_mittel")),
);
// `flare` wird seit v0.7.x nicht mehr ausgegeben (ruhendes Gewicht).
const ACHSEN = [...block.matchAll(/"([a-z_]+)" => [0-9.]+,/g)]
  .map((m) => m[1]!)
  .filter((k) => k !== "flare" && k !== "rollout");

describe("Erklärung je bewerteter Achse", () => {
  it("die Achsen-Liste ist echt (Gegenprobe)", () => {
    expect(ACHSEN).toContain("abfangen");
    expect(ACHSEN).toContain("touchdown_point");
    expect(ACHSEN.length).toBeGreaterThanOrEqual(8);
  });
  for (const [sprache, d] of [["de", de], ["en", en], ["it", it_]] as const) {
    it(`${sprache}: landing.info.<achse> vorhanden`, () => {
      const info = (d as { landing: { info: Record<string, unknown> } }).landing.info;
      const fehlt = ACHSEN.filter((k) => typeof info[k] !== "string" || (info[k] as string).length < 20);
      expect(fehlt).toEqual([]);
    });
  }
});
