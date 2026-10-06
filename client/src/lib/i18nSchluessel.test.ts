import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { ABZUG_MIT_TEXT, DECKEL_MIT_TEXT, deckelText } from "./landungsUrteil";

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

  // Deckel-Gruende der Gesamtnote (`master_deckel`): Literale vor einer
  // DECKEL_-Obergrenze und die Kennungs-Konstanten ANFLUG_* (05.10.2026).
  const deckel = [
    ...[...quelle.matchAll(/Some\(\(\s*"([a-z0-9_]+)",\s*DECKEL_/g)].map((m) => m[1]!),
    ...[...quelle.matchAll(/pub const (?:ANFLUG|GEFAHR)_[A-Z_]+: &str = "([a-z0-9_]+)"/g)].map(
      (m) => m[1]!,
    ),
  ];
  // Score-Version 21: Abzug statt Deckel — Literale vor einer ABZUG_-Zahl,
  // die Gefahr-Kennungen und „Anflug nicht gemessen".
  const abzug = [
    ...[...quelle.matchAll(/Some\(\(\s*"([a-z0-9_]+)",\s*ABZUG_/g)].map((m) => m[1]!),
    ...[...quelle.matchAll(/pub const GEFAHR_[A-Z_]+: &str = "([a-z0-9_]+)"/g)].map((m) => m[1]!),
    ...[...quelle.matchAll(/pub const (ANFLUG_NICHT_GEMESSEN): &str = "([a-z0-9_]+)"/g)].map(
      (m) => m[2]!,
    ),
  ];
  // Score-Version 19: „schwächster Teil begrenzt" — `teil_mittel_<achse>`.
  const teilDeckel = [...quelle.matchAll(/=> "(teil_(?:mittel|schlecht)[a-z_]*)"/g)].map(
    (m) => m[1]!,
  );

  it("erzeugt mindestens die bekannten Schlüssel", () => {
    expect(skipGruende).toContain("no_planned_burn");
    expect(warnungen).toContain("planned_burn_may_be_off");
    expect(deckel).toEqual(
      // Seit Score-Version 21 vergibt Rust keine Deckel mehr; die alten
      // Kennungen bleiben fuer Altbestand (DECKEL_MIT_TEXT).
      expect.arrayContaining(["anflug_partial_gesamt", "vor_der_schwelle", "mehrfach_hopser"]),
    );
    expect(teilDeckel).toEqual(
      expect.arrayContaining(["teil_mittel_touchdown_point", "teil_schlecht_rollout", "teil_mittel"]),
    );
  });

  it("kennt die Abzugs-Gründe", () => {
    expect(abzug).toEqual(
      expect.arrayContaining([
        "ueberlast",
        "harte_landung",
        "anflug_partial",
        "anflug_unstable",
        "anflug_nicht_gemessen",
        "vor_der_schwelle",
        "mehrfach_hopser",
      ]),
    );
    // Teil-Gründe (`teil_mittel_<achse>`) setzt deckelText zusammen.
    const ohneText = [...new Set(abzug)].filter((k) => !ABZUG_MIT_TEXT.has(k) && !k.startsWith("teil_"));
    expect(ohneText, "Grund fehlt in ABZUG_MIT_TEXT (lib/landungsUrteil.ts)").toEqual([]);
  });

  it("jeder Deckel-Grund erscheint im Landungsbericht", () => {
    // Feste Gründe stehen in DECKEL_MIT_TEXT, Teil-Gründe zeigt deckelText().
    const fehlend = [...new Set(deckel)].filter(
      (k) => !DECKEL_MIT_TEXT.has(k) && deckelText((x) => x, k) == null,
    );
    expect(fehlend, "Grund fehlt in DECKEL_MIT_TEXT (lib/landungsUrteil.ts)").toEqual([]);
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

    it(`${code}: jeder Deckel-Grund hat einen Text`, () => {
      const fehlend = [...DECKEL_MIT_TEXT, "teil_mittel", "teil_schlecht", "ein_teil"].filter(
        (k) => !hatText(daten, "deckel", k),
      );
      expect(fehlend, `ohne Eintrag zeigt der Bericht landing.deckel.<name>`).toEqual([]);
    });

    it(`${code}: jeder Abzug hat einen Text, lang und kurz`, () => {
      for (const gruppe of ["abzug", "abzug_kurz"]) {
        const fehlend = [...ABZUG_MIT_TEXT, "teil_mittel", "teil_schlecht"].filter(
          (k) => !hatText(daten, gruppe, k),
        );
        expect(fehlend, `landing.${gruppe}.<name>`).toEqual([]);
      }
    });

    it(`${code}: Teil-Abzüge nennen den Teil, mehrere Abzüge stehen zusammen`, () => {
      const t = (k: string, o?: Record<string, unknown>) => {
        const [gruppe, name] = k.split(".").slice(1);
        const w = (daten as any)?.landing?.[gruppe!]?.[name!];
        return typeof w === "string" ? w.replace("{{teil}}", String(o?.teil ?? "")) : `FEHLT:${k}`;
      };
      const fehlend = [...new Set(teilDeckel)]
        .map((k) => [k, deckelText(t, `${k}_abzug+anflug_unstable_abzug`)] as const)
        .filter(([, text]) => text == null || text.includes("FEHLT:"))
        .map(([k]) => k);
      expect(fehlend).toEqual([]);
    });

    it(`${code}: jeder Teil-Deckel nennt einen Teil mit Namen`, () => {
      // Der Teil kommt aus `landing.sub.<achse>`; ohne Eintrag stünde der
      // rohe Schlüssel im Satz.
      const t = (k: string, o?: Record<string, unknown>) => {
        const [gruppe, name] = k.split(".").slice(1);
        const w = (daten as any)?.landing?.[gruppe!]?.[name!];
        return typeof w === "string" ? w.replace("{{teil}}", String(o?.teil ?? "")) : `FEHLT:${k}`;
      };
      const fehlend = [...new Set(teilDeckel)]
        .map((k) => [k, deckelText(t, k)] as const)
        .filter(([, text]) => text == null || text.includes("FEHLT:"))
        .map(([k]) => k);
      expect(fehlend).toEqual([]);
    });

    it(`${code}: jede Kategorie hat ein Wort für die Gesamtnote`, () => {
      const fehlend = ["smooth", "acceptable", "firm", "hard", "severe"].filter(
        (k) => !hatText(daten, "gesamt", k),
      );
      expect(fehlend).toEqual([]);
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
