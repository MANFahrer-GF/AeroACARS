#!/usr/bin/env node
// Druckt die Bericht-Vorschau kopflos in ein PDF und vermisst es.
//
//   node scripts/bericht-drucken.mjs [quelle.html] [ziel.pdf]
//
// # Warum das nötig ist
//
// Wie gross eine Schrift auf dem Papier wird, ergibt sich erst beim
// Drucken: Seitenrand, Spaltenbreite und der viewBox der Grafik greifen
// ineinander. Am 24.08.2026 landeten die Beschriftungen der Bahn-Grafik
// bei 3,6 pt — errechnet, weil es keinen Weg gab, es zu sehen. Diesen Weg
// gibt es jetzt.
//
// Chrome druckt mit `--print-to-pdf`; `bericht-messen.py` liest die
// tatsächlichen Schriftgrössen aus dem PDF.

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client");
const QUELLE = process.argv[2] ?? join(CLIENT, "bericht-dist", "bericht.html");
const ZIEL = process.argv[3] ?? join(CLIENT, "bericht-dist", "bericht.pdf");

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
if (!existsSync(CHROME)) {
  console.error(`Chrome nicht gefunden: ${CHROME}`);
  process.exit(1);
}
if (!existsSync(QUELLE)) {
  console.error(`Quelle fehlt: ${QUELLE}\n  node scripts/bericht-bauen.mjs`);
  process.exit(1);
}

execFileSync(CHROME, [
  "--headless=new",
  "--disable-gpu",
  "--no-pdf-header-footer",
  // Die Seite rendert React; ohne Wartezeit druckt Chrome ein leeres Blatt.
  "--virtual-time-budget=8000",
  `--print-to-pdf=${ZIEL}`,
  `file://${QUELLE}`,
], { stdio: ["ignore", "ignore", "pipe"] });

console.log(ZIEL);
