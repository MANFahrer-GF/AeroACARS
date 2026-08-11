#!/usr/bin/env node
/*
 * Flow-Kompatibilitaets-QS fuer das AeroACARS-Widget.
 *
 * Anlass (Thomas, 11.08.2026): der Klick-Umschalter der Zeitzelle
 * funktionierte in jeder Testumgebung — nur nicht in Flow, wo die
 * Drag-Erkennung den Klick frisst. Der Fehler war KONSTRUKTIV nicht
 * auffindbar, bevor ein Pilot ihn fand, weil kein Test die Flow-
 * Eigenheiten pruefte. Dieses Skript macht die bekannten Eigenheiten
 * zu harten Regeln. Es laeuft in Sekunden und gehoert vor JEDEN
 * Flow-Export (siehe README).
 *
 * Regeln (jede aus einem echten Feldbefund):
 *  R1  Kein Feature darf NUR per Maus-Interaktion erreichbar sein.
 *      Jeder click/mousedown-Listener in code.js braucht im Umfeld die
 *      woertliche Markierung "Bonus" — das Zeichen, dass die Funktion
 *      auch ohne den Klick erreicht wird (Automatik, Anzeige, Server).
 *  R2  Keine moderne JS-Syntax, die der alte Coherent-Renderer nicht
 *      sicher kann: Arrow-Functions, const/let, Template-Literals,
 *      optional chaining, nullish coalescing, async/await, class.
 *  R3  Keine APIs, die in Coherent GT fehlen oder truegen:
 *      PointerEvent/setPointerCapture, AbortController, WebSocket.
 *  R4  Kein flex-`gap` in code.css (wirkt in Flow nicht — Feldfoto).
 *  R5  Kein Nicht-ASCII im ausfuehrbaren Code (Kaestchen-Zeichen).
 *  R6  fetch() nur innerhalb der eigenen hole()-Zeitschranke —
 *      naemlich genau 1x als Aufrufstelle (haengende Anfragen).
 */
'use strict';
var fs = require('fs');
var path = require('path');
var dir = __dirname;
var js = fs.readFileSync(path.join(dir, 'code.js'), 'utf8');
var css = fs.readFileSync(path.join(dir, 'code.css'), 'utf8');
var fehler = [];

function ohneKommentare(src) {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    // Ganze UND Inline-Zeilenkommentare — naiv genug fuer diesen Code-
    // stil (kein '//' in Strings ausser URLs, die matchen ':\/\/' nicht).
    .replace(/(^|[^:])\/\/.*$/gm, '$1');
}
var code = ohneKommentare(js);
var zeilen = js.split('\n');

// R1 — Interaktions-Inventar
var interaktiv = /addEventListener\(\s*['"](click|mousedown|mouseup|dblclick|contextmenu)['"]/g;
var m;
while ((m = interaktiv.exec(js)) !== null) {
  var zeile = js.slice(0, m.index).split('\n').length;
  var umfeld = zeilen.slice(Math.max(0, zeile - 26), zeile + 5).join('\n');
  if (!/bonus/i.test(umfeld)) {
    fehler.push('R1 Zeile ' + zeile + ": '" + m[1] + "'-Listener ohne Bonus-Markierung — " +
      'Funktion muss auch OHNE Klick erreichbar sein (Flow frisst Klicks), und das Umfeld muss das mit "Bonus" dokumentieren.');
  }
}

// R2 — Syntax
[
  [/=>/, 'Arrow-Function'],
  [/\b(const|let)\s/, 'const/let'],
  [/`/, 'Template-Literal'],
  [/\?\./, 'Optional Chaining'],
  [/\?\?/, 'Nullish Coalescing'],
  [/\basync\s+function|\bawait\s/, 'async/await'],
  [/\bclass\s+[A-Z]/, 'class'],
].forEach(function (r) {
  if (r[0].test(code)) fehler.push('R2: ' + r[1] + ' im Code — der Flow-Renderer ist aelter als das.');
});

// R3 — APIs
[
  [/PointerEvent|pointerdown|setPointerCapture/, 'Pointer Events'],
  [/AbortController/, 'AbortController'],
  [/WebSocket/, 'WebSocket'],
].forEach(function (r) {
  if (r[0].test(code)) fehler.push('R3: ' + r[1] + ' — in Coherent GT nicht verlaesslich (Feldbefund).');
});

// R4 — CSS gap
css.split('\n').forEach(function (z, i) {
  if (/^\s*gap\s*:/.test(z)) fehler.push('R4 code.css Zeile ' + (i + 1) + ': flex-gap wirkt in Flow nicht.');
});

// R5 — ASCII. EINE bewusste Ausnahme: die ERSATZ-Tabelle in nurAscii()
// MUSS die Nicht-ASCII-Zeichen kennen, die sie ersetzt — alles zwischen
// 'var ERSATZ' und dem schliessenden '];' ist deshalb frei.
var ersatzStart = code.indexOf('var ERSATZ');
var ersatzEnde = ersatzStart >= 0 ? code.indexOf('];', ersatzStart) : -1;
var pos = 0;
code.split('\n').forEach(function (z, i) {
  var inErsatz = ersatzStart >= 0 && pos >= ersatzStart && pos <= ersatzEnde;
  if (!inErsatz && /[^\x20-\x7E\t\r]/.test(z)) {
    fehler.push('R5 Zeile ' + (i + 1) + ': Nicht-ASCII im ausfuehrbaren Code.');
  }
  pos += z.length + 1;
});

// R6 — fetch-Aufrufstellen
var fetches = (code.match(/\bfetch\s*\(/g) || []).length;
if (fetches !== 1) fehler.push('R6: ' + fetches + ' fetch()-Aufrufstellen — erlaubt ist genau 1 (in hole(), mit Zeitschranke).');

if (fehler.length) {
  console.error('FLOW-KOMPAT: ' + fehler.length + ' Verstoss/Verstoesse:');
  fehler.forEach(function (f) { console.error('  ' + f); });
  process.exit(1);
}
console.log('FLOW-KOMPAT: alle 6 Regeln eingehalten (' +
  (js.match(interaktiv) || []).length + ' markierte Bonus-Interaktionen).');
