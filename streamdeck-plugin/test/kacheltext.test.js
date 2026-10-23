import { test } from "node:test";
import assert from "node:assert/strict";
import { szene, KACHEL, FUGE } from "../src/szene.js";
import { PHASEN } from "../skripte/vorschau-daten.mjs";

// Thomas 23.10. am Gerät: „Drauf achten, dass der Text in Kacheln steht.“
// Zwischen den Tasten liegt die Blende — Text darüber verliert Buchstaben.
// Linien und Flächen dürfen über die Blende laufen, Text nie.

const SCHRITT = KACHEL + FUGE;
import { textBreite } from "../src/schriftmass.js";

export function texteAusserhalb(svg) {
  const fehler = [];
  // Ausnahme: die Laufschrift im Band (wandert, Thomas will sie so).
  svg = svg.replace(/<g clip-path="url\(#lauf\)">.*?<\/g>/g, "");
  for (const m of svg.matchAll(/<text x="([\d.-]+)" y="([\d.-]+)"[^>]*font-size="([\d.]+)" font-weight="(\d+)"[^>]*text-anchor="(\w+)" letter-spacing="([\d.]+)"[^>]*>([^<]*)<\/text>/g)) {
    const [, xs, ys, gs, fw, anker, ab, roh] = m;
    const txt = roh.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">");
    if (!txt.trim()) continue;
    const x = Number(xs), y = Number(ys), g = Number(gs);
    const b = textBreite(txt, g, fw === "700", Number(ab));
    const links = anker === "middle" ? x - b / 2 : anker === "end" ? x - b : x;
    const rechts = links + b;
    const c = Math.floor((links + 0.5) / SCHRITT);
    const r = Math.floor((y - g * 0.7) / SCHRITT);
    const kx = c * SCHRITT, ky = r * SCHRITT;
    const drin = links >= kx + 4 && rechts <= kx + KACHEL - 4 && y - g * 0.75 >= ky + 2 && y + 2 <= ky + KACHEL;
    if (!drin) fehler.push(`"${txt}" (x ${Math.round(links)}–${Math.round(rechts)}, Kachel ${c},${r})`);
  }
  return fehler;
}

test("kein Text läuft über eine Blende — jedes Gerät, jede Phase", () => {
  const alle = [];
  for (const g of ["xl", "15", "plus"]) for (const [phase, s] of Object.entries(PHASEN)) for (const t of [0, 7.3, 31]) {
    for (const f of texteAusserhalb(szene(g, s, t).svg)) alle.push(`${g}/${phase}: ${f}`);
  }
  assert.deepEqual([...new Set(alle)], []);
});

test("Gegenprobe: der Prüfer findet Text über der Blende und erfasst alle Texte", async () => {
  const { text } = await import("../src/glas.js");
  // Absichtlich zu breit: beginnt in Kachel 0, ragt in die Blende.
  const zuBreit = `<svg>${text(20, 80, "BOARDING-MUSIK", { groesse: 20, fett: true })}</svg>`;
  assert.equal(texteAusserhalb(zuBreit).length, 1);
  // Mit `max` eingepasst: kein Befund.
  const eingepasst = `<svg>${text(20, 80, "BOARDING-MUSIK", { groesse: 20, fett: true, max: 110 })}</svg>`;
  assert.equal(texteAusserhalb(eingepasst).length, 0);
  // Der Prüfer liest wirklich die Texte der Szene (sonst wäre er immer grün).
  const svg = szene("xl", PHASEN.reiseflug, 1).svg;
  const anzahlText = (svg.match(/<text /g) ?? []).length;
  const erfasst = [...svg.matchAll(/<text x="[\d.-]+" y="[\d.-]+"[^>]*letter-spacing="[\d.]+"[^>]*>[^<]*<\/text>/g)].length;
  assert.ok(anzahlText > 40, `nur ${anzahlText} Texte`);
  assert.equal(erfasst, anzahlText);
});
