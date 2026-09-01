// Jede Begründung, die der Bewerter ausgeben kann, braucht einen Text.
//
// **Warum es diesen Test gibt.** Am 01.09.2026 stand in der Oberfläche
// wörtlich `landing.tip.in_tdz` — der rohe Schlüssel, dort wo ein Rat
// stehen sollte. Die Nachmessung zeigte: Es fehlte nicht ein Schlüssel,
// sondern **zwei ganze Achsen** — Aufsetzpunkt und Bahndisziplin, zehn
// Begründungen, dazu drei Skip-Gründe.
//
// ⚠ Der vorhandene `locales.parity.test.ts` hat das NICHT gefunden, und
// zwar aus einem lehrreichen Grund: Er prüft, ob die drei Sprachen
// **untereinander** gleich sind. Alle drei fehlten gleichermaßen, also
// war er grün. Ein Paritätstest sichert Gleichheit, nicht
// Vollständigkeit.
//
// Dieser Test schließt die Lücke von der anderen Seite: Er liest die
// **Rust-Quellen des Bewerters** und verlangt für jede Begründung, die
// dort entstehen kann, einen Text in allen drei Sprachen. Damit kann
// eine neue Achse nicht mehr textlos ausgeliefert werden — die
// Handarbeit, an die es bisher gebunden war, entfällt.

import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import de from "./de/common.json";
import en from "./en/common.json";
import italienisch from "./it/common.json";

const BEWERTER = resolve(
  process.cwd(),
  "src-tauri/crates/landing-scoring/src",
);

function rustQuellen(): string {
  return readdirSync(BEWERTER)
    .filter((f) => f.endsWith(".rs"))
    .map((f) => readFileSync(resolve(BEWERTER, f), "utf-8"))
    .join("\n");
}

/**
 * Die Begründungen, die `SubScoreEntry::scored(...)` erreichen können.
 *
 * ⚠ Die Band-Zuordnung (`Band::Good => "good"`) sieht genauso aus und
 * ist KEINE Begründung. Sie wird über das `=>` ausgeschlossen — ohne das
 * verlangt der Test Texte für „good", „ok" und „bad" und schickt jeden
 * auf eine falsche Fährte.
 */
function begruendungen(quelle: string): string[] {
  const gefunden = new Set<string>();
  // (100u8, Band::Good, "on_aim")
  for (const m of quelle.matchAll(
    /\(\s*\d+u?8?\s*,\s*Band::\w+\s*,\s*"([a-z][a-z0-9_]*)"\s*\)/g,
  )) {
    gefunden.add(m[1]);
  }
  // scored(KEY, LABEL, 0, wert, "overrun", Band::Bad)
  for (const m of quelle.matchAll(
    /"([a-z][a-z0-9_]*)"\s*,\s*Band::\w+\s*,?\s*\)/g,
  )) {
    gefunden.add(m[1]);
  }
  return [...gefunden].sort();
}

/** Die Gründe, mit denen eine Achse übersprungen wird. */
function skipGruende(quelle: string): string[] {
  const gefunden = new Set<string>();
  for (const m of quelle.matchAll(
    /skipped\s*\(\s*KEY\s*,\s*LABEL\s*,\s*"([a-z][a-z0-9_]*)"/g,
  )) {
    gefunden.add(m[1]);
  }
  // Belag::skip_grund() — die Zuordnung Belagsart → Grund
  for (const m of quelle.matchAll(
    /Belag::\w+\s*=>\s*"([a-z][a-z0-9_]+)"/g,
  )) {
    gefunden.add(m[1]);
  }
  return [...gefunden].sort();
}

const SPRACHEN = [
  ["Deutsch", de],
  ["Englisch", en],
  ["Italienisch", italienisch],
] as const;

function text(daten: unknown, pfad: string[]): unknown {
  return pfad.reduce<unknown>(
    (o, k) =>
      o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined,
    daten,
  );
}

describe("Begründungen des Bewerters", () => {
  const quelle = rustQuellen();

  it("die Quellen werden ueberhaupt gefunden", () => {
    // ⚠ Ohne diesen Riegel wäre der Test bei einem falschen Pfad still
    // grün: keine Quellen, keine Begründungen, nichts zu prüfen.
    expect(quelle.length, "keine Rust-Quellen gelesen").toBeGreaterThan(10_000);
    expect(
      begruendungen(quelle).length,
      "keine einzige Begründung erkannt — das Muster passt nicht mehr",
    ).toBeGreaterThan(20);
  });

  it("jede Begruendung hat einen Grund-Text in allen drei Sprachen", () => {
    const alle = begruendungen(quelle);
    for (const [name, daten] of SPRACHEN) {
      const fehlend = alle.filter((k) => !text(daten, ["landing", "rat", k]));
      expect(
        fehlend,
        `${name}: ohne landing.rat.* → der Pilot sieht den rohen ` +
          `Schluessel: ${fehlend.join(", ")}`,
      ).toEqual([]);
    }
  });

  it("jede Begruendung hat einen Rat in allen drei Sprachen", () => {
    const alle = begruendungen(quelle);
    for (const [name, daten] of SPRACHEN) {
      const fehlend = alle.filter((k) => !text(daten, ["landing", "tip", k]));
      expect(
        fehlend,
        `${name}: ohne landing.tip.* — genau der Fall vom 01.09.2026 ` +
          `(landing.tip.in_tdz stand roh in der Kachel): ${fehlend.join(", ")}`,
      ).toEqual([]);
    }
  });

  it("jeder Skip-Grund hat einen Text in allen drei Sprachen", () => {
    const alle = skipGruende(quelle);
    expect(alle.length, "keine Skip-Gruende erkannt").toBeGreaterThan(5);
    for (const [name, daten] of SPRACHEN) {
      const fehlend = alle.filter(
        (k) => !text(daten, ["landing", "skipped_reason", k]),
      );
      expect(
        fehlend,
        `${name}: ohne landing.skipped_reason.*: ${fehlend.join(", ")}`,
      ).toEqual([]);
    }
  });
});
