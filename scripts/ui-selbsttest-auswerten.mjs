#!/usr/bin/env node
// v3 (Etappe 5, P-8.03): wertet zwei Laeufe des UI-Selbsttests aus
// (AEROACARS_UI_SELBSTTEST, client/src-tauri/src/ui_lader.rs) und schreibt
// das Messprotokoll als Markdown-Tabelle. Exit 1, wenn ein Fall nicht passt.
//
//   node scripts/ui-selbsttest-auswerten.mjs <lauf1.json> <lauf2.json> [plattform]
//   node scripts/ui-selbsttest-auswerten.mjs --selbsttest

import { readFileSync } from "node:fs";

export function faelle(l1, l2) {
  const a = l1 ?? {};
  const b = l2 ?? {};
  return [
    ["CSP: Inline-Skript abgewiesen", a.inline_script_blockiert === true, a.inline_script_blockiert],
    ["CSP: eval abgewiesen (kein unsafe-eval)", a.eval_blockiert === true, a.eval_blockiert],
    ["CSP: fremder Host abgewiesen", a.fremder_host_blockiert === true, a.fremder_host_fehler ?? a.fremder_host_blockiert],
    ["fetch an live.kant.ovh", a.live_fetch === "ok", a.live_fetch],
    ["Schrift: MIME font/woff2", String(a.schrift_mime).startsWith("font/woff2"), a.schrift_mime],
    ["Schrift: geladen", a.schrift === "loaded", a.schrift],
    ["WASM: MIME application/wasm", a.wasm_mime === "application/wasm", a.wasm_mime],
    ["WASM: kompilieren", a.wasm_kompilieren === "ok", a.wasm_kompilieren],
    ["WASM: instantiateStreaming", a.wasm_streaming === "ok", a.wasm_streaming],
    ["IPC im Fenster ui vorhanden", a.ipc_vorhanden === true, a.ipc_vorhanden],
    [
      "alle Systembefehle aus ui abgewiesen",
      a.system_zahl > 0 && Array.isArray(a.system_nicht_abgewiesen) && a.system_nicht_abgewiesen.length === 0,
      `${a.system_zahl} geprueft, nicht abgewiesen: ${JSON.stringify(a.system_nicht_abgewiesen ?? "?")}`,
    ],
    ["lesen erlaubt (app_info)", a.lesen_erlaubt === true, JSON.stringify(a.lesen_app_info)],
    ["fachlich erlaubt (discord_rpc_clear_flight)", a.fachlich_erlaubt === true, JSON.stringify(a.fachlich_discord_rpc_clear_flight)],
    ["Fehlerseite bei fehlender Datei", a.fehlerseite?.status === 404 && a.fehlerseite?.text === true, JSON.stringify(a.fehlerseite)],
    ["danach eingebauter Stand", a.danach_eingebaut === true, a.danach_eingebaut],
    ["Ursprung gleich ueber zwei Starts", !!a.ursprung && a.ursprung === b.ursprung, `${a.ursprung} / ${b.ursprung}`],
    [
      "localStorage ueber zwei Starts",
      !!a.speicher_neu && b.speicher_vorher === a.speicher_neu,
      `Lauf 1 schrieb ${a.speicher_neu}, Lauf 2 las ${b.speicher_vorher}`,
    ],
    ["kein Abbruch", !a.abbruch && !a.fehler && !b.abbruch && !b.fehler, a.abbruch ?? a.fehler ?? b.abbruch ?? b.fehler ?? "-"],
  ];
}

function tabelle(liste, plattform) {
  const zeilen = [`### UI-Selbsttest ${plattform}`, "", "| Fall | Ergebnis | Wert |", "|---|---|---|"];
  for (const [fall, ok, wert] of liste) {
    zeilen.push(`| ${fall} | ${ok ? "ja" : "**NEIN**"} | ${String(wert).replaceAll("|", "/").slice(0, 300)} |`);
  }
  return zeilen.join("\n");
}

if (process.argv[2] === "--selbsttest") {
  // Gegenprobe: ein guter Lauf besteht, jeder einzeln verdorbene Fall faellt.
  const gut = {
    inline_script_blockiert: true, eval_blockiert: true, fremder_host_blockiert: true, live_fetch: "ok",
    schrift_mime: "font/woff2", schrift: "loaded", wasm_mime: "application/wasm", wasm_kompilieren: "ok",
    wasm_streaming: "ok", ipc_vorhanden: true, system_zahl: 37, system_nicht_abgewiesen: [],
    lesen_erlaubt: true, fachlich_erlaubt: true, fehlerseite: { status: 404, text: true },
    danach_eingebaut: true, ursprung: "aeroui://localhost", speicher_neu: "1",
  };
  const zwei = { ...gut, speicher_vorher: "1" };
  if (!faelle(gut, zwei).every(([, ok]) => ok)) throw new Error("Selbsttest: guter Lauf faellt durch");
  const verdorben = [
    { system_nicht_abgewiesen: [{ befehl: "phpvms_login" }] }, { inline_script_blockiert: false },
    { live_fetch: "fehler" }, { wasm_mime: "text/html" }, { danach_eingebaut: false },
  ];
  for (const v of verdorben) {
    if (faelle({ ...gut, ...v }, zwei).every(([, ok]) => ok)) throw new Error(`Selbsttest: ${JSON.stringify(v)} nicht erkannt`);
  }
  if (faelle(gut, { ...zwei, speicher_vorher: null }).every(([, ok]) => ok)) throw new Error("Selbsttest: Speicherverlust nicht erkannt");
  console.log("ui-selbsttest-auswerten: Selbsttest ok");
} else {
  const lesen = (p) => {
    try { return JSON.parse(readFileSync(p, "utf8")); } catch (e) { return { fehler: `${p}: ${e.message}` }; }
  };
  const liste = faelle(lesen(process.argv[2]), lesen(process.argv[3]));
  console.log(tabelle(liste, process.argv[4] ?? ""));
  if (!liste.every(([, ok]) => ok)) process.exit(1);
}
