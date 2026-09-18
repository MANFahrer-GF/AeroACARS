#!/usr/bin/env node
// A0 — sieht der Beschriftungs-Wächter WIRKLICH alle Sprit-Schlüssel?
//
// # Warum es dieses Skript gibt
//
// Der Wächter in `anzeige-sync.mjs` liest den Quelltext und sammelt
// `t("…")`-Aufrufe. Zwei Schreibweisen entziehen sich ihm:
//
//   1. Ein Schlüssel aus einer Konstantentabelle — `t(p.key)`. Die drei
//      Badge-Texte standen so da und waren ungeprüft.
//   2. Ein zusammengesetzter Name mit Unterstrich — `reserve_grund_${…}`.
//      Der Vorspann-Erkenner verlangt einen PUNKT vor `${`; beim
//      Bahn-Diagramm hieß es `discipline_skip.${…}` und griff, hier nicht.
//
// Beides ist im Code behoben (literale Aufrufe, Punktschreibweise). Dieses
// Skript hält es fest — und zwar nicht, indem es den Quelltext nach den
// Schreibweisen durchsucht (das wäre ein Wächter auf einen Namen, und
// genau daran ist dieses Projekt schon zweimal gescheitert), sondern indem
// es den Wächter AUSFÜHRT und ihm für jede Bauart einen Schlüssel wegnimmt.
//
// Meldet er die Lücke nicht, ist er an dieser Bauart blind.
//
// Aufruf:  node scripts/pruef-a0-waechter.mjs

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { benoetigteSchluessel } from "./anzeige-sync.mjs";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client", "src");

/** Ein Schlüssel je Bauart, die der Wächter sehen muss. */
const PROBEN = [
  {
    bauart: "fester Aufruf t(landing.sprit.title)",
    schluessel: "landing.sprit.title",
  },
  {
    bauart: "Badge aus der Fallunterscheidung — war eine Tabelle",
    schluessel: "landing.sprit.badge_intakt",
  },
  {
    bauart: "zusammengesetzter Name — reserve_grund.${grund}",
    schluessel: "landing.sprit.reserve_grund.kein_ofp",
  },
];

function main() {
  let gesehen;
  try {
    gesehen = new Set(benoetigteSchluessel());
  } catch (e) {
    console.error(`FEHLER: Der Wächter ließ sich nicht ausführen — ${e.message}`);
    process.exit(1);
  }

  // Grundgesamtheit: Ohne sie wäre ein leeres Ergebnis „alles gesehen".
  const sprit = [...gesehen].filter((k) => k.startsWith("landing.sprit."));
  if (sprit.length < 20) {
    console.error(
      `FEHLER: Der Wächter kennt nur ${sprit.length} Sprit-Schlüssel. ` +
        "Das sind zu wenige - stehen die Dateien in DATEIEN?",
    );
    process.exit(1);
  }

  // Gegenprobe je Bauart: Kennt der Wächter den Schlüssel überhaupt?
  const blind = [];
  for (const p of PROBEN) {
    if (!gesehen.has(p.schluessel)) blind.push(p);
  }

  // Und die Sprachdateien müssen ihn tragen — sonst ist „gesehen" wertlos.
  const fehlend = [];
  for (const sprache of ["de", "en", "it"]) {
    const datei = resolve(CLIENT, "locales", sprache, "common.json");
    const baum = JSON.parse(readFileSync(datei, "utf-8"));
    for (const p of PROBEN) {
      let k = baum;
      for (const teil of p.schluessel.split(".")) {
        if (k == null || typeof k !== "object" || !(teil in k)) {
          k = null;
          break;
        }
        k = k[teil];
      }
      if (typeof k !== "string" || k.length === 0) {
        fehlend.push(`${sprache}: ${p.schluessel}`);
      }
    }
  }

  if (blind.length > 0) {
    console.error("FEHLER: Der Wächter ist an diesen Bauarten blind:");
    for (const p of blind) console.error(`  · ${p.bauart}  (${p.schluessel})`);
    console.error(
      "\nEr wuerde Vollstaendigkeit melden, waehrend die Texte in der Live-Uebersicht fehlen.",
    );
    process.exit(1);
  }
  if (fehlend.length > 0) {
    console.error("FEHLER: Diese Schlüssel fehlen in den Sprachdateien:");
    for (const f of fehlend) console.error(`  · ${f}`);
    process.exit(1);
  }

  console.log(`Der Wächter kennt ${sprit.length} Sprit-Schlüssel.`);
  for (const p of PROBEN) console.log(`  ✓ ${p.bauart}`);
  console.log("A0 WAECHTER SIEHT ALLE DREI");
}

main();
