#!/usr/bin/env node
// v3 (Etappe 5, P-8.02/P-8.10): Capabilities der App-Befehle aus befehle.json
// erzeugen — nicht von Hand pflegen. Der Waechtertest
// `client/src-tauri/tests/befehlsliste.rs` (rechte_dateien_passen_zur_liste)
// meldet jede Abweichung zwischen befehle.json und den erzeugten Dateien.
//
//   node scripts/rechte-erzeugen.mjs
//
// ui     = Stufen lesen + fachlich (nachgeladene Oberflaeche)
// huelle = alle Befehle (eingebaute Huelle)
// Plugin-Rechte stehen weiter in capabilities/default.json.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const tauri = join(dirname(fileURLToPath(import.meta.url)), "..", "client", "src-tauri");
const { befehle } = JSON.parse(readFileSync(join(tauri, "befehle.json"), "utf8"));

// Welche Fenster welche Rechte-Datei bekommen. Uebergang (Schritt 1): main und
// telemetrie stehen in beiden, duerfen also wie bisher alles. `ui` ist das
// Fenster des UI-Selbsttests (ui_lader.rs, P-8.03) — nur Rechte aus ui.json.
const FENSTER = {
  ui: ["main", "telemetrie", "ui"],
  huelle: ["huelle", "main", "telemetrie"],
};

const recht = (b) => `allow-${b.replaceAll("_", "-")}`;
const namen = Object.keys(befehle).sort();

const datei = (name, beschreibung, liste) => ({
  $schema: "../gen/schemas/desktop-schema.json",
  identifier: name,
  description: `${beschreibung} ERZEUGT von scripts/rechte-erzeugen.mjs aus befehle.json — nicht von Hand aendern.`,
  windows: FENSTER[name],
  permissions: liste.map(recht),
});

const ausgabe = {
  ui: datei("ui", "Nachgeladene Oberflaeche: Stufen lesen und fachlich (P-8.02).", namen.filter((b) => befehle[b] !== "system")),
  huelle: datei("huelle", "Eingebaute Huelle: alle Befehle (P-8.01).", namen),
};

for (const [name, inhalt] of Object.entries(ausgabe)) {
  writeFileSync(join(tauri, "capabilities", `${name}.json`), JSON.stringify(inhalt, null, 2) + "\n");
}
console.log(`ui: ${ausgabe.ui.permissions.length}, huelle: ${ausgabe.huelle.permissions.length} Rechte`);
