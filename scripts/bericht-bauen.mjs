#!/usr/bin/env node
// Baut die Druckvorschau des Landungs-Berichts in EINE HTML-Datei.
//
//   node scripts/bericht-bauen.mjs [ziel.html] [variante]
//
// Zusammen mit `bericht-drucken.mjs` macht das aus „ist der Ausdruck
// lesbar?" eine Messung statt einer Schätzung: Chrome druckt die Datei
// kopflos, und das PDF wird auf Schriftgrössen in Punkt vermessen.
//
// Einzelne Datei, weil sie verschickt und geöffnet wird — ein `assets/`
// daneben kommt beim Weitergeben nicht mit, und die Seite bliebe wortlos
// leer.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client");
const DIST = join(CLIENT, "bericht-dist");
const ZIEL = process.argv[2] ?? join(DIST, "bericht.html");

execFileSync(
  "npx",
  ["vite", "build", "--config", "vite.bericht.config.mjs", "--logLevel", "warn"],
  { cwd: CLIENT, stdio: "inherit" },
);

const html = readFileSync(join(DIST, "bericht.html"), "utf-8");
let eine = html;
let ersetzt = 0;

eine = eine.replace(
  /<script[^>]*src="([^"]+)"[^>]*><\/script>/g,
  (_t, pfad) => {
    const p = join(DIST, pfad.replace(/^\.?\//, ""));
    if (!existsSync(p)) return _t;
    ersetzt++;
    return `<script type="module">\n${readFileSync(p, "utf-8")}\n</script>`;
  },
);
eine = eine.replace(
  /<link[^>]*rel="stylesheet"[^>]*href="([^"]+)"[^>]*>/g,
  (_t, pfad) => {
    const p = join(DIST, pfad.replace(/^\.?\//, ""));
    if (!existsSync(p)) return _t;
    ersetzt++;
    return `<style>\n${readFileSync(p, "utf-8")}\n</style>`;
  },
);

writeFileSync(ZIEL, eine, "utf-8");
const kb = Math.round(Buffer.byteLength(eine) / 1024);
console.log(`${ZIEL}  (${kb} KB, ${ersetzt} Datei(en) eingebettet)`);
