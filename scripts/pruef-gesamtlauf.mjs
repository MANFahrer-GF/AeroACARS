#!/usr/bin/env node
// Der Gesamtlauf — Rust, Client, Live-Webapp, alle Wächter.
//
// # Warum es dieses Skript gibt
//
// Ein Prüflauf, der bei einem Fehler trotzdem „fertig" meldet, ist
// schlimmer als keiner. Genau das ist hier schon zweimal passiert:
//
//   · Ein `cd` in einen Pfad, den es nicht gab, schlug fehl — das Skript
//     lief weiter und schrieb „ALLES DURCH", ohne die Webapp je berührt zu
//     haben.
//   · Ein Testfilter traf keinen einzigen Test. `cargo` meldete
//     „0 passed; 0 failed" und damit Erfolg.
//
// Und im Projekt nebenan hat ein nächtlicher Lauf 30 Nächte lang nichts
// getan, weil er den Rückgabewert seines eigenen Teilschritts ignorierte.
//
// Deshalb hier drei Regeln:
//   1. JEDER Schritt muss mit 0 enden — sonst bricht das Skript ab.
//   2. Es zählt, WIE VIELE Tests gelaufen sind, und bricht bei null ab.
//   3. „ALLES GRUEN" steht erst am Ende, nach allen Schritten.
//
// Aufruf:  node scripts/pruef-gesamtlauf.mjs

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const WURZEL = resolve(HIER, "..");
const WEBAPP = resolve(WURZEL, "..", "aeroacars-live", "webapp");

const SCHRITTE = [
  {
    name: "Rust-Workspace",
    cwd: resolve(WURZEL, "client", "src-tauri"),
    cmd: "cargo",
    args: ["test", "--workspace"],
    zaehlt: /(\d+) passed/g,
    mindestens: 1000,
  },
  {
    name: "Client: Typen",
    cwd: resolve(WURZEL, "client"),
    cmd: "npx",
    args: ["tsc", "--noEmit"],
  },
  {
    name: "Client: Tests",
    cwd: resolve(WURZEL, "client"),
    cmd: "npx",
    args: ["vitest", "run"],
    zaehlt: /Tests\s+(\d+) passed/g,
    mindestens: 800,
  },
  {
    name: "Live-Webapp: Typen",
    cwd: WEBAPP,
    cmd: "npx",
    args: ["tsc", "--noEmit"],
  },
  {
    // Der PROJEKTEIGENE Befehl, nicht ein nacktes vitest: Er führt
    // zusätzlich den Wächter gegen nicht eingebundene Testdateien und
    // sechs eigenständige Skripte aus.
    name: "Live-Webapp: npm test",
    cwd: WEBAPP,
    cmd: "npm",
    args: ["test"],
    zaehlt: /Tests\s+(\d+) passed/g,
    mindestens: 40,
  },
  {
    name: "Anzeige-Abgleich",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/anzeige-sync.mjs"],
  },
  {
    name: "Wächter: Beschriftungen",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/pruef-a0-waechter.mjs"],
  },
  {
    name: "Wächter: Farben",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/pruef-a1-farben.mjs"],
  },
  // Bis zur QS vom 18.09.2026 fehlten diese drei hier — der Gesamtlauf
  // meldete „alles grün", ohne sie je auszuführen (Befund F5).
  {
    name: "Wächter: Gleichstand Client/Live",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/pruef-a5-gleichstand.mjs"],
  },
  {
    name: "Wächter: Spec deckt Zustand",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/pruef-spec-vollstaendig.mjs"],
  },
  {
    // Der ausgelieferte Code gegen 43 echte Flüge.
    name: "Korpus gegen echte Flüge",
    cwd: WURZEL,
    cmd: "node",
    args: ["scripts/pruef-korpus.mjs"],
  },
];

function main() {
  if (!existsSync(WEBAPP)) {
    console.error(`FEHLER: Die Live-Webapp fehlt unter ${WEBAPP}.`);
    console.error("Ohne sie ist der Lauf unvollständig — kein stilles Überspringen.");
    process.exit(1);
  }

  let gesamtTests = 0;
  for (const s of SCHRITTE) {
    process.stdout.write(`${s.name} … `);
    const r = spawnSync(s.cmd, s.args, {
      cwd: s.cwd,
      encoding: "utf-8",
      shell: process.platform === "win32",
    });

    if (r.error) {
      console.log("FEHLER");
      console.error(`  ${s.cmd} ließ sich nicht starten: ${r.error.message}`);
      process.exit(1);
    }
    if (r.status !== 0) {
      console.log(`ROT (Rückgabewert ${r.status})`);
      const aus = `${r.stdout ?? ""}${r.stderr ?? ""}`;
      for (const z of aus.split("\n").slice(-40)) {
        if (/error|FAIL|panicked|failed/i.test(z)) console.error(`  ${z.trim()}`);
      }
      process.exit(1);
    }

    if (s.zaehlt) {
      const aus = `${r.stdout ?? ""}${r.stderr ?? ""}`;
      let summe = 0;
      for (const m of aus.matchAll(s.zaehlt)) summe += Number(m[1]);
      if (summe < (s.mindestens ?? 1)) {
        console.log("VERDÄCHTIG");
        console.error(
          `  Nur ${summe} Tests gelaufen, erwartet mindestens ${s.mindestens}.`,
        );
        console.error("  Ein Lauf, der nichts ausführt, meldet ebenfalls Erfolg.");
        process.exit(1);
      }
      gesamtTests += summe;
      console.log(`grün (${summe} Tests)`);
    } else {
      console.log("grün");
    }
  }

  console.log(`\n${SCHRITTE.length} Schritte, ${gesamtTests} Tests.`);
  console.log("ALLES GRUEN");
}

main();
