#!/usr/bin/env node
// Rust-Freigabe: übersetzt sauber, ohne Warnung, und alle Tests laufen.
//
// Anlass: Codex fand am 20.09.2026, dass mein Gate dafür einen kaputten
// Build grün melden konnte. Es lautete:
//
//     cargo check --message-format short 2>&1 | grep "^warning" && exit 1;
//     echo "cargo check ohne Warnung"
//
// Scheitert `cargo check` OHNE eine Zeile, die mit `warning` beginnt, dann
// findet `grep` nichts, `&& exit 1` greift nicht — und das `echo` liefert
// Exit 0. Mit simuliertem Cargo-Exit 42 erschien tatsächlich „cargo check
// ohne Warnung". Ein Gate, das genau das durchlässt, wofür es da ist.
//
// Hier wird jeder Rückgabewert einzeln geprüft und benannt. Der
// Erfolgssatz steht am Ende und nur dann, wenn wirklich alles stimmt.
//
// `--selbsttest` prüft das Skript gegen sich selbst: Es lässt einen
// Befehl absichtlich scheitern und erwartet, dass das erkannt wird.

import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const selbsttest = process.argv.includes("--selbsttest");

/**
 * Der Arbeitsbereich liegt unter `client/src-tauri`, nicht im Wurzelordner.
 * Ohne diese Festlegung hing das Ergebnis davon ab, aus welchem Verzeichnis
 * jemand das Skript startet: aus der Wurzel aufgerufen meldete Cargo 101
 * („could not find Cargo.toml") — ein Fehler, der wie ein echter Befund
 * aussah und einmal Minuten kostete. Der Pfad hängt jetzt am Skript selbst.
 */
const ARBEITSBEREICH = fileURLToPath(new URL("../client/src-tauri", import.meta.url));

/** Einen Befehl ausführen und Rückgabewert plus Ausgabe zurückgeben. */
function lauf(befehl, argumente) {
  const r = spawnSync(befehl, argumente, {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    cwd: befehl === "cargo" ? ARBEITSBEREICH : undefined,
  });
  if (r.error) {
    return { code: 127, ausgabe: String(r.error.message) };
  }
  return { code: r.status ?? 1, ausgabe: `${r.stdout ?? ""}${r.stderr ?? ""}` };
}

const fehler = [];

if (selbsttest) {
  // Positiv-Kontrolle: Erkennt das Skript einen Fehlschlag OHNE Warnung?
  // Genau der Fall, an dem das alte Gate scheiterte.
  const k = lauf("node", ["-e", "process.exit(42)"]);
  if (k.code === 0) {
    console.error("SELBSTTEST FEHLGESCHLAGEN: Exit 42 wurde als Erfolg gelesen");
    process.exit(1);
  }
  if (/warning/i.test(k.ausgabe)) {
    console.error("SELBSTTEST FEHLGESCHLAGEN: unerwartete Warnung im Kontrollfall");
    process.exit(1);
  }
  console.log("selbsttest bestanden: Fehlschlag ohne Warnung wird erkannt");
  process.exit(0);
}

// 1. Übersetzen — ALLE Ziele, nicht nur die Bibliotheken. Ein Fehler in
//    einem Binär- oder Testziel blieb sonst unbemerkt.
const check = lauf("cargo", ["check", "--workspace", "--all-targets"]);
if (check.code !== 0) {
  fehler.push(`cargo check endete mit Rückgabewert ${check.code}`);
}
const warnzeilen = check.ausgabe
  .split("\n")
  .filter((z) => /^warning(:|\[)/.test(z.trim()));
if (warnzeilen.length > 0) {
  fehler.push(
    `cargo check meldet ${warnzeilen.length} Warnung(en): ` +
      warnzeilen.slice(0, 3).join(" | "),
  );
}

// 2. Testen — ebenfalls der ganze Arbeitsbereich.
const test = lauf("cargo", ["test", "--workspace"]);
if (test.code !== 0) {
  fehler.push(`cargo test endete mit Rückgabewert ${test.code}`);
}
if (/test result: FAILED/.test(test.ausgabe)) {
  fehler.push("mindestens eine Testsuite meldet FAILED");
}
// Und es muss überhaupt etwas gelaufen sein — ein Lauf ohne Tests ist
// kein bestandener Lauf.
const ergebnisse = (test.ausgabe.match(/test result: ok\./g) ?? []).length;
if (ergebnisse === 0) {
  fehler.push("kein einziges 'test result: ok.' — es lief nichts");
}

if (fehler.length > 0) {
  for (const f of fehler) console.error(`FEHLER: ${f}`);
  process.exit(1);
}
console.log(`rust sauber: ${ergebnisse} Testsuiten ok, keine Warnung`);
