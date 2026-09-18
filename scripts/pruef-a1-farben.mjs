#!/usr/bin/env node
// A1 — jede Farbe der geteilten Anzeige muss in BEIDEN Repos ankommen.
//
// # Der Befund
//
// Die Sprit-Sektion benutzte 19 CSS-Farbnamen. In der Live-Webapp heissen
// sie anders: dort gibt es `--fg`, `--fg-dim`, `--surf-2`, nicht `--text`,
// `--text-muted`, `--surface-2`. **18 von 19 waren dort undefiniert.**
//
// Das ist keine Kleinigkeit und auch kein „grau auf grau":
//
//   · `var(--text-muted)` ohne Wert faellt auf „nicht gesetzt" zurueck —
//     die Schrift erbt die Elternfarbe.
//   · `color-mix(in srgb, var(--text) 8%, transparent)` mit undefiniertem
//     `--text` ist eine UNGUELTIGE Deklaration. Das Element verliert
//     Hintergrund und Rahmen ganz.
//   · Die SVG-Beschriftungen der Sprit-Leiter werden schwarz — auf einem
//     Grund von #07090e also unsichtbar.
//
// Und **kein Test haette es gemeldet**: CSS-Variablen werden erst im
// Browser aufgeloest, `renderToStaticMarkup` sieht davon nichts.
//
// # Die Regel
//
// Jeder Farbname in einer geteilten Datei traegt einen Rueckfallwert:
// `var(--text-muted, #9aa4b2)`. Dann sieht die Anzeige ueberall richtig
// aus — auch in einem Repo, das den Namen gar nicht kennt. Wo das Repo ihn
// kennt, folgt sie weiter dessen Thema.
//
// Das ist mehr als ein Alias-Block in der Webapp-CSS: Der repariert genau
// diese eine Sektion und laesst die naechste wieder durchrutschen.
//
// Aufruf:  node scripts/pruef-a1-farben.mjs

import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { DATEIEN } from "./anzeige-sync.mjs";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client", "src");
const WEBAPP = resolve(HIER, "..", "..", "aeroacars-live", "webapp", "src");

/** Farbnamen OHNE Rueckfallwert: `var(--x)` statt `var(--x, #abc)`. */
const OHNE_RUECKFALL = /var\(\s*(--[\w-]+)\s*\)/g;
/** Alle Farbnamen, mit und ohne. */
const ALLE = /var\(\s*(--[\w-]+)\s*[,)]/g;

function main() {
  const fehler = [];
  let geprueft = 0;
  let mitRueckfall = 0;

  for (const rel of DATEIEN) {
    const p = resolve(CLIENT, rel);
    if (!existsSync(p)) continue;
    const text = readFileSync(p, "utf-8");
    geprueft += 1;

    for (const m of text.matchAll(OHNE_RUECKFALL)) {
      fehler.push(`${rel}: var(${m[1]}) ohne Rueckfallwert`);
    }
    for (const _ of text.matchAll(ALLE)) mitRueckfall += 1;
  }

  if (geprueft === 0) {
    console.error("FEHLER: keine einzige Datei geprueft - stimmt der Pfad?");
    process.exit(1);
  }

  // Gegenprobe: Das Muster muss ueberhaupt anschlagen koennen.
  const probe = 'color: var(--text);';
  if ([...probe.matchAll(OHNE_RUECKFALL)].length !== 1) {
    console.error("FEHLER: Das Suchmuster erkennt einen fehlenden Rueckfall nicht.");
    process.exit(1);
  }

  if (fehler.length > 0) {
    console.error("FEHLER: Farbnamen ohne Rueckfallwert in geteilten Dateien:");
    for (const f of fehler) console.error(`  · ${f}`);
    console.error(
      "\nIn einem Repo, das den Namen nicht kennt, faellt die Farbe ersatzlos aus.",
    );
    console.error("Schreibweise: var(--text-muted, #9aa4b2)");
    process.exit(1);
  }

  // Zusatz, kein Muss: Kennt die Webapp die Namen auch selbst? Dann folgt
  // die Anzeige dort dem eigenen Thema statt dem Rueckfall.
  const css = resolve(WEBAPP, "styles.css");
  const eigene = [];
  if (existsSync(css)) {
    const t = readFileSync(css, "utf-8");
    for (const name of ["--text", "--text-muted", "--surface-2", "--border"]) {
      if (!new RegExp(`${name}\\s*:`).test(t)) eigene.push(name);
    }
  }

  console.log(`${geprueft} geteilte Dateien, ${mitRueckfall} Farbnamen - alle mit Rueckfall.`);
  if (eigene.length > 0) {
    console.log(
      `Hinweis: Die Webapp kennt ${eigene.join(", ")} nicht selbst - dort greift der Rueckfall.`,
    );
  }
  console.log("A1 KEINE FARBNAMEN OHNE RUECKFALL");
}

main();
