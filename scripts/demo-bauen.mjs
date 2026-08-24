#!/usr/bin/env node
// Baut die bedienbare Bahndisziplin-Demo in EINE HTML-Datei.
//
//   node scripts/demo-bauen.mjs [ziel.html]
//
// # Warum eine einzelne Datei
//
// Die Demo wird verschickt und angesehen, nicht ausgeliefert. Ein Ordner
// mit `assets/` daneben kommt beim Weitergeben nicht mit — die Seite bliebe
// dann leer, und zwar wortlos.
//
// # Warum überhaupt gebündelt
//
// Die frühere Fassung entstand über `renderToStaticMarkup`: HTML ohne eine
// Zeile JavaScript. Für „sieht die Grafik richtig aus?" reicht das. Nur hat
// die Grafik inzwischen Bedienung — Zoom mit Strg + Mausrad, Zoomknöpfe,
// Ziehen zum Verschieben —, und davon tat in der statischen Fassung nichts
// etwas. Es sah nicht kaputt aus, sondern als wäre die Bedienung nicht
// gebaut.
//
// `renderDisciplineDemo.test.tsx` bleibt daneben bestehen: Es prüft bei
// jedem Testlauf, dass jede Variante überhaupt rendert. Das ist billig und
// braucht keinen Bündellauf.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client");
const DIST = join(CLIENT, "demo-dist");
const ZIEL = process.argv[2] ?? join(CLIENT, "demo-dist", "bahndisziplin.html");

execFileSync(
  "npx",
  ["vite", "build", "--config", "vite.demo.config.mjs", "--logLevel", "warn"],
  { cwd: CLIENT, stdio: "inherit" },
);

const html = readFileSync(join(DIST, "demo.html"), "utf-8");

// Jedes `<script src>` und `<link rel=stylesheet>` durch seinen Inhalt
// ersetzen. Vite bündelt JS trotz `assetsInlineLimit` in eine eigene Datei
// — die Grenze gilt nur für Assets, nicht für Code-Chunks.
let eine = html;
let ersetzt = 0;
eine = eine.replace(
  /<script[^>]*src="([^"]+)"[^>]*><\/script>/g,
  (_treffer, pfad) => {
    const datei = join(DIST, pfad.replace(/^\.?\//, ""));
    if (!existsSync(datei)) throw new Error(`Bündel fehlt: ${datei}`);
    ersetzt++;
    return `<script type="module">\n${readFileSync(datei, "utf-8")}\n</script>`;
  },
);
eine = eine.replace(
  /<link[^>]*rel="stylesheet"[^>]*href="([^"]+)"[^>]*>/g,
  (_treffer, pfad) => {
    const datei = join(DIST, pfad.replace(/^\.?\//, ""));
    if (!existsSync(datei)) throw new Error(`Stilblatt fehlt: ${datei}`);
    ersetzt++;
    return `<style>\n${readFileSync(datei, "utf-8")}\n</style>`;
  },
);

if (ersetzt === 0) {
  // Lieber laut scheitern als eine Datei ausliefern, die im Browser leer
  // bleibt: Genau dieser Zustand — HTML da, Bedienung tot — ist der Grund
  // für dieses Skript.
  throw new Error("kein Bündel eingebettet — die Seite wäre ohne Funktion");
}
// Auf verbliebene AUSSENVERWEISE prüfen — und zwar auf die Tags, nicht auf
// den Text. Die erste Fassung suchte schlicht nach `src="` und schlug an,
// weil der eingebettete React-Code diese Zeichenfolge selbst enthält: eine
// Prüfung, die immer rot ist, prüft nichts.
const offen = [
  ...eine.matchAll(/<script[^>]*\ssrc="([^"]+)"/g),
  ...eine.matchAll(/<link[^>]*rel="stylesheet"[^>]*\shref="([^"]+)"/g),
].map((m) => m[1]);
if (offen.length > 0) {
  throw new Error(`nicht eigenständig — zeigt noch nach draussen: ${offen.join(", ")}`);
}

writeFileSync(ZIEL, eine, "utf-8");
rmSync(join(DIST, "assets"), { recursive: true, force: true });
console.log(
  `${ZIEL}  (${(Buffer.byteLength(eine) / 1024).toFixed(0)} KB, ${ersetzt} Bündel eingebettet)`,
);
