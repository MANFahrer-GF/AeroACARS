#!/usr/bin/env node
// Prüft Release-Notes gegen das, was der Code tatsächlich tut.
//
// Anlass: Codex fand am 20.09.2026, dass die Notes zu v1.7.44 sich selbst
// widersprachen — der erste Punkt sagte, die VDGS-Platte verschwinde ohne
// Eintrag ganz, zwei Zeilen später stand die schmale Zeile beschrieben.
// Beides war irgendwann wahr; der erste Satz ist beim Umbau stehen
// geblieben.
//
// Das Skript sucht nicht nach Rechtschreibung, sondern nach genau den
// Aussagen, die wir schon einmal falsch stehen hatten, und nach der
// Vollständigkeit beider Sprachblöcke.

import { readFileSync } from "node:fs";

const datei = process.argv[2];
if (!datei) {
  console.error("Aufruf: pruefe-release-notes.mjs <datei.md>");
  process.exit(2);
}

const text = readFileSync(datei, "utf8");
const fehler = [];

// 1. Widerlegte Aussagen. Jede stand einmal drin und war falsch.
const verboten = [
  {
    muster: /bleibt sie weg, statt ein leeres Gerät zu zeigen/,
    warum:
      "Die Platte verschwindet NICHT mehr ganz — ohne Eintrag bleibt eine schmale Zeile.",
  },
  {
    muster: /stays away rather than showing an empty gauge/,
    warum: "Englisches Gegenstück derselben überholten Aussage.",
  },
  {
    muster: /übergibt nichts dorthin und speichert nichts davon/,
    warum:
      "Das VDGS-Band übergibt das Rufzeichen an den fremden Dienst — diese Zusage war überholt.",
  },
  {
    muster: /Die Ampel steht ab dem ersten Wegpunkt\./,
    warum:
      "Die Abflugzeile trägt bewusst keine Ampel; richtig ist 'ab dem ersten Überflug'.",
  },
];
for (const { muster, warum } of verboten) {
  if (muster.test(text)) fehler.push(`überholte Aussage: ${warum}`);
}

// 2. Beide Sprachblöcke müssen da sein und etwa gleich viel sagen.
const [, de = "", en = ""] = text.split(/^## 🇩🇪 Deutsch$|^## 🇬🇧 English$/m);
const punkte = (s) => (s.match(/^- \*\*/gm) ?? []).length;
if (punkte(de) === 0) fehler.push("kein deutscher Block gefunden");
if (punkte(en) === 0) fehler.push("kein englischer Block gefunden");
if (punkte(de) !== punkte(en)) {
  fehler.push(
    `unterschiedlich viele Punkte: DE ${punkte(de)}, EN ${punkte(en)} — ` +
      "eine Sprache ist beim Nachziehen vergessen worden",
  );
}

if (fehler.length > 0) {
  for (const f of fehler) console.error(`FEHLER: ${f}`);
  process.exit(1);
}
console.log(`release-notes stimmig (${punkte(de)} Punkte je Sprache)`);
