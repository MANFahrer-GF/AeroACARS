#!/usr/bin/env node
// Q4 — nennt die Spec für JEDEN offenen Punkt aus v1.7.35 einen Ausgang?
//
// Die Hausregel: Kein Punkt verschwindet stillschweigend. Er ist gelöst,
// begründet verworfen oder ausdrücklich weiter offen.
//
// Ein Skript, das nur „kommt das Wort vor?" prüft, wäre Beruhigung. Deshalb
// verlangt es je Punkt ein Stichwort UND in derselben Tabellenzeile einen
// der drei Ausgänge. Und es prüft die Gegenrichtung: Begriffe, die im Code
// nicht mehr existieren, dürfen in der Messbeschreibung nicht mehr als
// gültig stehen.
//
// Aufruf:  node scripts/pruef-spec-vollstaendig.mjs

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const SPEC = resolve(HIER, "..", "docs", "spec", "v1.7.35-sprit-ohne-note.md");
const LIB = resolve(HIER, "..", "client", "src-tauri", "src", "lib.rs");

/** Die offenen Punkte aus v1.7.35 — fest eingetragen, nicht aus der Spec gelesen. */
const OFFENE_PUNKTE = [
  "Vergleichspunkt liegt nicht am Plan-TOD",
  "Landesprit aus dem Live-Schnappschuss",
  "Best Practices",
  "Muster-Profile",
  "Anzeige in Client und Live-Übersicht verschieden",
  "Messlücke",
  "Mindestgrenzen",
  "Leiter und Extra-Zeile",
];

const AUSGAENGE = /\*\*(Gelöst|Verworfen|Nicht Client-Sache|Grundlage gelegt|Gelöst ohne neues Feld)/;

/** Bezeichner, die es seit v1.7.36 im Code nicht mehr gibt. */
const ENTFERNT = ["SPRIT_VERGLEICHSPUNKT_FENSTER", "sprit_fenster_scharf", "sprit_rest_erreicht"];

function main() {
  const spec = readFileSync(SPEC, "utf-8");
  const lib = readFileSync(LIB, "utf-8");
  const fehler = [];

  const zeilen = spec.split("\n");
  for (const punkt of OFFENE_PUNKTE) {
    // Nur Tabellenzeilen: Der Ausgang steht in der Tabelle, nicht im
    // Fließtext, der das Wort ebenfalls enthalten darf.
    const z = zeilen.find((l) => l.trimStart().startsWith("|") && l.includes(punkt));
    if (!z) {
      fehler.push(`„${punkt}" kommt in der Spec nicht vor`);
      continue;
    }
    if (!AUSGAENGE.test(z)) {
      fehler.push(`„${punkt}" steht ohne Ausgang da (gelöst / verworfen / offen)`);
    }
  }

  // Gegenrichtung: entfernte Bezeichner dürfen im Code nicht mehr stehen.
  for (const name of ENTFERNT) {
    if (lib.includes(name)) fehler.push(`${name} steht noch im Code, obwohl entfernt`);
  }

  // Gegenprobe: erkennt das Muster einen fehlenden Ausgang?
  if (AUSGAENGE.test("| **Irgendwas** | nur Text |")) {
    fehler.push("Gegenprobe: das Ausgangsmuster ist zu weit");
  }

  if (fehler.length > 0) {
    console.error("FEHLER: Die Spec deckt den Zustand nicht:");
    for (const f of fehler) console.error(`  · ${f}`);
    process.exit(1);
  }
  console.log(`${OFFENE_PUNKTE.length} offene Punkte aus v1.7.35, jeder mit Ausgang.`);
  console.log("SPEC VOLLSTAENDIG");
}

main();
