import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

/**
 * Jeder Schlüssel, den die Bewertung erzeugt, braucht in ALLEN drei
 * Sprachen einen Text.
 *
 * ⚠ Warum das ein eigener Test ist
 *
 * i18next hat keinen Ersatzbehandler und `fallbackLng: "en"` hilft
 * nicht, wenn auch Englisch den Schlüssel nicht kennt: Dann rendert die
 * Oberfläche den ROHEN Schlüssel. Der Pilot liest dann wörtlich
 * `landing.warn.runway_axis_unverified` in Bernstein.
 *
 * Genau das ist am 30.08.2026 mit v1.7.12 passiert — zwei neue Zustände
 * (`runway_axis_unverified`, Skip-Grund `diverted`) kamen ohne einen
 * einzigen Spracheintrag. Die Rust-Tests waren grün, die Frontend-Tests
 * auch: Niemand hat die beiden Seiten gegeneinander gehalten.
 *
 * Dieser Test tut es — er liest die Schlüssel aus dem RUST-Quelltext,
 * nicht aus einer gepflegten Liste. Eine Liste würde beim nächsten Mal
 * genauso vergessen wie die Sprachdatei.
 */
const WURZEL = path.resolve(__dirname, "../..");
const SCORING = path.join(WURZEL, "src-tauri/crates/landing-scoring/src");

function rustQuellen(): string {
  return fs
    .readdirSync(SCORING)
    .filter((f) => f.endsWith(".rs"))
    .map((f) => fs.readFileSync(path.join(SCORING, f), "utf-8"))
    .join("\n");
}

function sprache(code: string): Record<string, unknown> {
  return JSON.parse(
    fs.readFileSync(path.join(WURZEL, "src/locales", code, "common.json"), "utf-8"),
  );
}

/** `landing.warn.x` / `landing.skipped_reason.x` nachschlagen. */
function hatText(daten: any, gruppe: string, schluessel: string): boolean {
  const wert = daten?.landing?.[gruppe]?.[schluessel];
  return typeof wert === "string" && wert.trim().length > 0;
}

const SPRACHEN = ["de", "en", "it"];

describe("Beschriftungen für erzeugte Bewertungs-Schlüssel", () => {
  const quelle = rustQuellen();

  it("findet überhaupt Schlüssel im Rust-Quelltext", () => {
    // Ohne diesen Riegel bestünde der Test auch dann, wenn die
    // Suchmuster nichts mehr finden — er prüfte dann nichts.
    expect(quelle.length).toBeGreaterThan(1000);
  });

  const skipGruende = [...quelle.matchAll(/::skipped\([^)]*?,\s*"([a-z0-9_]+)"\s*\)/g)].map(
    (m) => m[1]!,
  );
  const warnungen = [...quelle.matchAll(/warning\s*[:=]\s*Some\(\s*"([a-z0-9_]+)"/g)].map(
    (m) => m[1]!,
  );

  it("erzeugt mindestens die bekannten Schlüssel", () => {
    expect(skipGruende).toContain("no_planned_burn");
    expect(warnungen).toContain("planned_burn_may_be_off");
  });

  for (const code of SPRACHEN) {
    const daten = sprache(code);
    it(`${code}: jeder Skip-Grund hat einen Text`, () => {
      const fehlend = [...new Set(skipGruende)].filter(
        (k) => !hatText(daten, "skipped_reason", k),
      );
      expect(
        fehlend,
        `ohne Eintrag zeigt die Landeansicht den rohen Schlüssel ` +
          `landing.skipped_reason.<name>`,
      ).toEqual([]);
    });

    it(`${code}: jede Warnung hat einen Text`, () => {
      const fehlend = [...new Set(warnungen)].filter((k) => !hatText(daten, "warn", k));
      expect(
        fehlend,
        `ohne Eintrag zeigt die Landeansicht den rohen Schlüssel ` +
          `landing.warn.<name>`,
      ).toEqual([]);
    });
  }
});
