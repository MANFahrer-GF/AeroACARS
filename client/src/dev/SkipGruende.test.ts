// Kennt die Anzeige jeden Grund, aus dem eine Achse übersprungen wird?
//
// # Warum das eine eigene Prüfung braucht
//
// Ein Skip-Grund ist eine Zeichenkette, die im Rust-Client entsteht und in
// zwei TypeScript-Tabellen übersetzt wird. Fehlt der Eintrag, fällt die
// Anzeige auf „nicht bewertet" zurück — sie zeigt also etwas, nur eben
// nicht den Grund. Kein Fehler, keine Warnung, kein roter Test.
//
// Das ist zweimal passiert:
//
// * v1.6.8: Die sechs Gründe der Bahn-Auslastungs-Achse fehlten komplett.
// * v1.7.0: `implausible_lateral_track` fehlte in **beiden** Tabellen. Er
//   markiert eine Landung, deren seitlicher Versatz nicht sein kann — im
//   Korpus 52,6 m auf einer 45-m-Bahn, einmal sogar 513 m. Genau der Fall,
//   bei dem jemand nach der Ursache suchen sollte, stand als „nicht
//   bewertet" da.
//
// Beide Male wurde es beim Durchsehen gefunden, nicht von einem Test.

import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { resolve } from "node:path";

const SCORING = resolve(__dirname, "..", "..", "src-tauri", "crates", "landing-scoring", "src");
const LIVE = resolve(__dirname, "..", "..", "..", "..", "aeroacars-live");

/** Jeden `skipped(..., "grund")`-Aufruf im Bewertungs-Crate einsammeln. */
function gruendeAusRust(): string[] {
  const gefunden = new Set<string>();
  for (const datei of readdirSync(SCORING).filter((d) => d.endsWith(".rs"))) {
    const text = readFileSync(resolve(SCORING, datei), "utf-8");
    // `skipped(KEY, LABEL, "grund")` — auch über Zeilenumbrüche.
    for (const m of text.matchAll(/skipped\(([\s\S]{0,120}?)"([a-z_]+)"/g)) {
      gefunden.add(m[2]);
    }
  }
  return [...gefunden].sort();
}

describe("Skip-Gründe der Landebewertung", () => {
  it("findet überhaupt Gründe im Rust-Code", () => {
    // Ohne diese Zusicherung wäre die Prüfung unten grün, sobald sich das
    // Muster im Rust-Code ändert — und niemand würde es merken.
    expect(gruendeAusRust().length).toBeGreaterThan(10);
  });

  it("kennt jeden Grund in Webapp und Monitor", () => {
    if (!existsSync(LIVE)) {
      console.warn(`[Skip-Gründe] aeroacars-live nicht gefunden (${LIVE}) — nicht geprüft.`);
      return;
    }
    const tabellen = [
      { name: "Webapp", pfad: "webapp/src/components/landingScoring.ts" },
      { name: "Monitor", pfad: "monitor/src/tabs/PirepFeed.tsx" },
    ].map((t) => ({ ...t, text: readFileSync(resolve(LIVE, t.pfad), "utf-8") }));

    const luecken: string[] = [];
    for (const grund of gruendeAusRust()) {
      for (const t of tabellen) {
        // Als Schlüssel einer Tabelle, nicht irgendwo im Text: Ein Grund,
        // der nur in einem Kommentar vorkommt, übersetzt nichts.
        if (!new RegExp(`^\\s*${grund}:`, "m").test(t.text)) {
          luecken.push(`${grund} fehlt in ${t.name}`);
        }
      }
    }
    expect(
      luecken,
      "Diese Gründe erscheinen dem Piloten als „nicht bewertet“ — ohne den Grund.",
    ).toEqual([]);
  });
  /**
   * Und die ANZEIGE muss jeden Grund der Bahn-Achse ausschreiben können.
   *
   * Die Bewertung kennt sieben Gründe, aus denen die seitliche Lage nicht
   * gewertet wird. Die Grafik kannte fünf. Bei `untrusted_geometry` und
   * `implausible_lateral_track` wertete die Achse nicht — und die
   * Queransicht daneben zeichnete seelenruhig ein Band mit Randabstand,
   * auf einer Geometrie, der die Bewertung nicht traut, oder aus einem
   * Versatz, den sie als Messfehler verworfen hat.
   *
   * Das ist schlimmer als eine fehlende Anzeige: Die Zahl steht neben
   * echten Messwerten und ist von ihnen nicht zu unterscheiden.
   */
  it("schreibt jeden Grund der Bahn-Achse aus", () => {
    const panel = readFileSync(
      resolve(__dirname, "..", "components", "RunwayDisciplinePanel.tsx"),
      "utf-8",
    );
    const achse = readFileSync(
      resolve(SCORING, "sub_bahndisziplin.rs"),
      "utf-8",
    );
    const gruende = new Set<string>();
    for (const m of achse.matchAll(/skipped\(KEY, LABEL, "([a-z_]+)"\)/g)) {
      gruende.add(m[1]);
    }
    // Über den Belag entscheidet `Belag::seitlich_bewertbar`; die Gründe
    // stehen dort als eigene Zeichenketten.
    for (const g of ["unpaved_runway", "surface_unknown", "water_runway"]) {
      if (achse.includes(g)) gruende.add(g);
    }
    expect(gruende.size, "keine Gründe gefunden — Muster geändert?").toBeGreaterThan(5);

    const fehlend = [...gruende]
      .filter((g) => !new RegExp(`case "${g}":`).test(panel))
      .sort();
    expect(
      fehlend,
      "Für diese Gründe zeigt die Grafik einen Rückfalltext statt der Ursache — " +
        "oder schlimmer: sie zeichnet weiter, als wäre nichts.",
    ).toEqual([]);
  });
});
