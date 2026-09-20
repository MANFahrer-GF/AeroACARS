#!/usr/bin/env node
// Prüft Release-Notes gegen das, was der Code tatsächlich tut.
//
// Anlass: Codex fand am 20.09.2026, dass die Notes zu v1.7.44 sich selbst
// widersprachen — der erste Punkt sagte, die VDGS-Platte verschwinde ohne
// Eintrag ganz, zwei Zeilen später stand die schmale Zeile beschrieben.
//
// Die ERSTE Fassung dieses Prüfers hatte dann selbst zwei Fehlgrün-Pfade
// (Codex, vierte Runde):
//   * `text.split(/^## DE$|^## EN$/m)` verwarf, WELCHE Überschrift traf.
//     Eine Datei mit zweimal Englisch und ohne Deutsch galt als stimmig.
//   * Die Verbotsmuster trafen nur den Wortlaut von heute. „Ohne Eintrag
//     wird die Platte überhaupt nicht angezeigt" kam durch.
//
// Deshalb jetzt: Blöcke werden namentlich gesucht und müssen genau einmal
// vorkommen, und die Widerspruchsprüfung fragt nach der AUSSAGE statt nach
// dem Satz.
//
// `--selbsttest` prüft den Prüfer gegen erfundene Notes, die er ablehnen
// muss — eine Absenz-Prüfung ohne Positiv-Kontrolle ist wertlos.

import { readFileSync } from "node:fs";

const UEBERSCHRIFT_DE = "## 🇩🇪 Deutsch";
const UEBERSCHRIFT_EN = "## 🇬🇧 English";

/** Alle Fundstellen einer Überschrift, damit Dubletten auffallen. */
function stellen(text, marke) {
  const gefunden = [];
  let i = text.indexOf(marke);
  while (i !== -1) {
    gefunden.push(i);
    i = text.indexOf(marke, i + 1);
  }
  return gefunden;
}

/**
 * Sätze, die einmal drinstanden und falsch waren — WÖRTLICH.
 *
 * Bewusst eng: Ein erster Versuch prüfte die AUSSAGE über mehrere
 * Regex-Bausteine in einer Zeile. Das ging nach hinten los — `bleibt.*weg`
 * griff quer über einen ganzen Absatz hinweg, und „verschwindet" traf
 * ausgerechnet den Satz, der beschreibt, was NICHT passiert. Semantik per
 * Muster erkennt man nicht; was man erkennt, ist ein Satz, den man schon
 * einmal korrigiert hat.
 *
 * Gegen Umformulierungen hilft nicht diese Liste, sondern die
 * Widerspruchsprüfung darunter.
 */
const VERBOTEN = [
  {
    satz: "bleibt sie weg, statt ein leeres Gerät zu zeigen",
    warum: "Ohne Eintrag bleibt eine schmale Zeile — die Platte verschwindet nicht.",
  },
  {
    satz: "stays away rather than showing an empty gauge",
    warum: "Englisches Gegenstück derselben überholten Aussage.",
  },
  {
    satz: "übergibt nichts dorthin und speichert nichts davon",
    warum: "Das VDGS-Band übergibt das Rufzeichen an den fremden Dienst.",
  },
  {
    satz: "Die Ampel steht ab dem ersten Wegpunkt.",
    warum: "Die Abflugzeile trägt bewusst keine Ampel; richtig ist 'ab dem ersten Überflug'.",
  },
];

/**
 * Aussagenpaare, die einander ausschließen.
 *
 * DAS war der Befund vom 20.09.2026: Ein Punkt sagte, die Platte
 * verschwinde ohne Eintrag ganz, zwei Zeilen später stand die schmale
 * Zeile beschrieben. Beides war einmal wahr; einer der Sätze ist beim
 * Umbau stehen geblieben. Eine Umformulierung fällt hier auf, weil sie
 * dem anderen Satz weiterhin widerspricht.
 */
const WIDERSPRUECHE = [
  {
    name: "Platte weg / schmale Zeile",
    a: /(Platte|panel|gauge)[^.]{0,120}(ganz nicht|überhaupt nicht angezeigt|gar nicht angezeigt|bleibt sie weg|stays away|is not shown at all)/i,
    b: /(schmale Zeile|slim line)/i,
    warum:
      "Die Notes behaupten an einer Stelle, die Platte verschwinde ganz, " +
      "und beschreiben an anderer die schmale Zeile. Beides zugleich geht nicht.",
  },
];

/**
 * Aussagen, die dem CODE widersprechen.
 *
 * Eine Datei, die durchgehend dasselbe Falsche sagt, ist in sich stimmig —
 * auffallen kann sie nur gegen den Code. Deshalb liest der Prüfer die
 * betroffene Stelle mit: Gibt es den Zweig für die schmale Zeile
 * (`vdgs-band-leer`), dann darf in den Notes nicht stehen, die Platte
 * erscheine ohne Eintrag gar nicht.
 */
const GEGEN_CODE = [
  {
    name: "schmale Zeile im Code, Notes sagen 'gar nicht'",
    codeDatei: "client/src/components/VdgsBand.tsx",
    codeMuster: /vdgs-band-leer/,
    notesMuster:
      /(Platte|panel|gauge)[^.]{0,140}(überhaupt nicht|gar nicht|ganz nicht|nicht angezeigt|not (shown|displayed) at all|does not appear)/i,
    warum:
      "Der Code zeigt ohne Eintrag eine schmale Zeile (`vdgs-band-leer`). " +
      "Die Notes behaupten, es erscheine gar nichts.",
  },
];

function pruefe(text, codeLeser = null) {
  const fehler = [];

  // 1. Beide Blöcke genau einmal.
  const de = stellen(text, UEBERSCHRIFT_DE);
  const en = stellen(text, UEBERSCHRIFT_EN);
  if (de.length !== 1) {
    fehler.push(`deutscher Block ${de.length}× gefunden, erwartet genau 1×`);
  }
  if (en.length !== 1) {
    fehler.push(`englischer Block ${en.length}× gefunden, erwartet genau 1×`);
  }

  // 2. Wörtlich verbotene Sätze.
  for (const { satz, warum } of VERBOTEN) {
    if (text.includes(satz)) {
      fehler.push(`überholter Satz „${satz}": ${warum}`);
    }
  }

  // 3. Aussagen, die einander ausschließen.
  for (const { name, a, b, warum } of WIDERSPRUECHE) {
    if (a.test(text) && b.test(text)) {
      fehler.push(`Widerspruch (${name}): ${warum}`);
    }
  }

  // 4. Aussagen gegen den Code. Ohne Codeleser übersprungen — dann sagt
  //    der Prüfer das auch, statt Vollständigkeit vorzutäuschen.
  if (codeLeser) {
    for (const eintrag of GEGEN_CODE) {
      const quelle = codeLeser(eintrag.codeDatei);
      if (quelle === null) {
        fehler.push(
          `Codestelle ${eintrag.codeDatei} nicht lesbar — Abgleich unmöglich`,
        );
        continue;
      }
      if (eintrag.codeMuster.test(quelle) && eintrag.notesMuster.test(text)) {
        fehler.push(`widerspricht dem Code (${eintrag.name}): ${eintrag.warum}`);
      }
    }
  }

  // 5. Gleich viele Punkte je Sprache — nur sinnvoll, wenn beide da sind.
  let punkteDe = 0;
  let punkteEn = 0;
  if (de.length === 1 && en.length === 1) {
    const [erst, zweit] = de[0] < en[0] ? [de[0], en[0]] : [en[0], de[0]];
    const zaehle = (s) => (s.match(/^- \*\*/gm) ?? []).length;
    const ersterBlock = zaehle(text.slice(erst, zweit));
    const zweiterBlock = zaehle(text.slice(zweit));
    [punkteDe, punkteEn] = de[0] < en[0]
      ? [ersterBlock, zweiterBlock]
      : [zweiterBlock, ersterBlock];
    if (punkteDe === 0) fehler.push("deutscher Block enthält keine Punkte");
    if (punkteEn === 0) fehler.push("englischer Block enthält keine Punkte");
    if (punkteDe !== punkteEn) {
      fehler.push(
        `unterschiedlich viele Punkte: DE ${punkteDe}, EN ${punkteEn} — ` +
          "eine Sprache ist beim Nachziehen vergessen worden",
      );
    }
  }

  return { fehler, punkteDe };
}

// ── Selbsttest: der Prüfer gegen Fälle, die er ablehnen MUSS ──────────
if (process.argv.includes("--selbsttest")) {
  const gut = [
    "## 🇩🇪 Deutsch",
    "",
    "- **Eins.** Text.",
    "",
    "## 🇬🇧 English",
    "",
    "- **One.** Text.",
    "",
  ].join("\n");

  const faelle = [
    ["gute Datei", gut, true],
    [
      "zweimal Englisch, kein Deutsch",
      gut.replace("## 🇩🇪 Deutsch", "## 🇬🇧 English"),
      false,
    ],
    [
      "umformulierte Falschaussage",
      gut.replace(
        "- **Eins.** Text.",
        "- **Eins.** Ohne Eintrag wird die Platte überhaupt nicht angezeigt.",
      ),
      false,
    ],
    [
      "wörtliche Falschaussage",
      gut.replace(
        "- **Eins.** Text.",
        "- **Eins.** Sonst bleibt sie weg, statt ein leeres Gerät zu zeigen.",
      ),
      false,
    ],
    ["ungleiche Punktzahl", gut.replace("- **One.** Text.", ""), false],
    ["gar keine Überschriften", "nur Fließtext", false],
  ];

  // Ein Codeleser, der so tut, als gaebe es die schmale Zeile — damit
  // die Prüfung gegen den Code im Selbsttest wirklich läuft.
  const alsGaebeEsDieZeile = () => 'data-testid="vdgs-band-leer"';

  let schlecht = 0;
  for (const [name, inhalt, erwartetOk] of faelle) {
    const { fehler } = pruefe(inhalt, alsGaebeEsDieZeile);
    const ok = fehler.length === 0;
    if (ok !== erwartetOk) {
      console.error(
        `SELBSTTEST FEHLGESCHLAGEN (${name}): erwartet ${
          erwartetOk ? "stimmig" : "abgelehnt"
        }, bekam ${ok ? "stimmig" : `abgelehnt: ${fehler.join("; ")}`}`,
      );
      schlecht += 1;
    }
  }
  if (schlecht > 0) process.exit(1);
  console.log(`selbsttest bestanden: ${faelle.length} Fälle richtig beurteilt`);
  process.exit(0);
}

const datei = process.argv[2];
if (!datei) {
  console.error("Aufruf: pruefe-release-notes.mjs <datei.md> | --selbsttest");
  process.exit(2);
}
const leseCode = (pfad) => {
  try {
    return readFileSync(new URL(`../${pfad}`, import.meta.url), "utf8");
  } catch {
    return null;
  }
};
const { fehler, punkteDe } = pruefe(readFileSync(datei, "utf8"), leseCode);
if (fehler.length > 0) {
  for (const f of fehler) console.error(`FEHLER: ${f}`);
  process.exit(1);
}
console.log(`release-notes stimmig (${punkteDe} Punkte je Sprache)`);
