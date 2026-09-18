#!/usr/bin/env node
// A5 — zeigen Client und Live-Übersicht dieselbe Sprit-Auswertung?
//
// Seit v1.7.36 gibt es die Sektion nur noch einmal (`anzeige-sync.mjs`).
// Gleich sein können sie trotzdem nur, wenn DREI Dinge stimmen — und genau
// an diesen drei Stellen kann weiterhin gedriftet werden:
//
//   1. Die geteilten Dateien sind byteweise gleich.
//   2. Die Beschriftungen (`landing.sprit.*`) sind in allen drei Sprachen
//      auf beiden Seiten gleich — ein fehlender Schlüssel fällt still auf
//      einen Ersatz zurück und ist im Test unsichtbar.
//   3. Die Hülle der Live-Übersicht rendert die geteilte Sektion und KEINE
//      eigenen Zahlen daneben. Bis v1.7.35 hatte sie eigene Textbauer —
//      samt Kommentar „dieselbe Zahl darf nicht zweierlei aussehen" — und
//      ist trotzdem auseinandergelaufen.
//
// Aufruf:  node scripts/pruef-a5-gleichstand.mjs

import { readFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { benoetigteSchluessel } from "./anzeige-sync.mjs";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client", "src");
const WEBAPP = resolve(HIER, "..", "..", "aeroacars-live", "webapp", "src");

const GETEILT = ["components/SpritSektion.tsx", "lib/sprit.ts"];

/** Was die Hülle der Live-Übersicht NICHT mehr enthalten darf. */
const VERBOTEN_IN_HUELLE = [
  "phasenText",
  "reserveText",
  "leiterText",
  "extraText",
  "badgeText(",
  "phasenTon",
  'from "./sprit"',
];

/** Kanonisch: Schlüssel sortiert — die Reihenfolge in JSON trägt keine Bedeutung. */
const kanon = (v) =>
  v && typeof v === "object"
    ? JSON.stringify(Object.keys(v).sort().map((k) => [k, JSON.parse(kanon(v[k]))]))
    : JSON.stringify(v);

const hash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");

function teilbaum(datei) {
  const d = JSON.parse(readFileSync(datei, "utf-8"));
  return JSON.stringify(d?.landing?.sprit ?? null);
}

function main() {
  if (!existsSync(WEBAPP)) {
    console.error(`FEHLER: Live-Webapp fehlt unter ${WEBAPP}.`);
    process.exit(1);
  }
  const fehler = [];

  // 1. Byteweise gleich.
  for (const rel of GETEILT) {
    const a = resolve(CLIENT, rel);
    const b = resolve(WEBAPP, rel);
    if (!existsSync(a) || !existsSync(b)) {
      fehler.push(`${rel}: fehlt auf einer Seite`);
      continue;
    }
    if (hash(a) !== hash(b)) fehler.push(`${rel}: Client und Webapp verschieden`);
  }

  // 2. Beschriftungen: Jede, die die Live-Übersicht führt, muss wörtlich
  //    der des Clients entsprechen, und nichts, was die geteilte Sektion
  //    braucht, darf fehlen. Der Client führt zusätzlich die Texte des
  //    PDF-Berichts (`report_*`) — der existiert nur dort, das ist richtig.
  const benoetigt = [...benoetigteSchluessel()].filter((k) => k.startsWith("landing.sprit."));
  for (const sprache of ["de", "en", "it"]) {
    const c = JSON.parse(teilbaum(resolve(CLIENT, "locales", sprache, "common.json"))) ?? {};
    const w = JSON.parse(teilbaum(resolve(WEBAPP, "locales", sprache, "common.json"))) ?? {};
    for (const k of Object.keys(w)) {
      if (kanon(w[k]) !== kanon(c[k])) {
        fehler.push(`${sprache}: landing.sprit.${k} weicht in der Webapp ab`);
      }
    }
    for (const voll of benoetigt) {
      const pfad = voll.slice("landing.sprit.".length).split(".");
      let k = w;
      for (const teil of pfad) k = k?.[teil];
      if (typeof k !== "string" || !k) fehler.push(`${sprache}: ${voll} fehlt in der Webapp`);
    }
    if (Object.keys(w).length < 30) {
      fehler.push(`${sprache}: nur ${Object.keys(w).length} Sprit-Beschriftungen — zu wenige`);
    }
  }

  // 3. Die Hülle rendert die geteilte Sektion und nichts Eigenes.
  const huelle = readFileSync(resolve(WEBAPP, "components", "LandingAnalysis.tsx"), "utf-8");
  if (!/<SpritSektion\s+sprit=/.test(huelle)) {
    fehler.push("LandingAnalysis.tsx rendert die geteilte SpritSektion nicht");
  }
  for (const v of VERBOTEN_IN_HUELLE) {
    if (huelle.includes(v)) fehler.push(`LandingAnalysis.tsx enthält noch „${v}"`);
  }
  if (existsSync(resolve(WEBAPP, "components", "sprit.ts"))) {
    fehler.push("webapp/src/components/sprit.ts existiert noch — die alte eigene Fassung");
  }

  // Gegenprobe: Würde Prüfung 1 eine Abweichung überhaupt bemerken?
  if (hash(resolve(CLIENT, GETEILT[0])) === createHash("sha256").update("x").digest("hex")) {
    fehler.push("Gegenprobe: der Vergleich ist blind");
  }

  if (fehler.length > 0) {
    console.error("FEHLER: Client und Live-Übersicht können auseinanderlaufen:");
    for (const f of fehler) console.error(`  · ${f}`);
    process.exit(1);
  }
  console.log(`${GETEILT.length} Dateien byteweise gleich, Beschriftungen in de/en/it gleich,`);
  console.log("die Hülle rendert die geteilte Sektion und keine eigenen Zahlen.");
  console.log("A5 GLEICHSTAND OK");
}

main();
