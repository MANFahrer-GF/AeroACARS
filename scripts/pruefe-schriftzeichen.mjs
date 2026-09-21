#!/usr/bin/env node
// Jedes Zeichen im VDGS-Band muss die Cockpit-Schrift auch kennen.
//
// Anlass, 21.09.2026: Ich hatte einen Zeilenumbruch mitten im Wort mit
// einem geschützten Bindestrich (U+2011) behoben — und B612 Mono, die
// Schrift des Bandes, kennt dieses Zeichen nicht. Im Cockpit wäre an
// der Stelle eine Ersatzglyphe erschienen oder der Browser auf eine
// andere Schrift gesprungen, mitten im Wort, in einer Monospace-Zeile.
// Das ist schlechter als der Umbruch, den es beheben sollte.
//
// Aufgefallen ist es NUR, weil ich die Schriftdatei aufgemacht habe.
// Die Vorschau im Browser sah richtig aus: dort sprang klaglos eine
// Ersatzschrift ein, die das Zeichen hat. Ein Blick auf das Bild hätte
// den Fehler also bestätigt statt gefunden.
//
// Deshalb dieser Wächter. Er prüft die Bandtexte aller drei Sprachen
// gegen den echten Zeichenvorrat der Schrift.
//
// `--selbsttest` prüft ihn gegen einen Text, den er ablehnen MUSS.

import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

const WURZEL = new URL("..", import.meta.url);
const SCHRIFTEN = new URL("client/src/assets/fonts/", WURZEL);
const VORRAT = new URL("b612mono-zeichen.json", SCHRIFTEN);
const SPRACHEN = ["de", "en", "it"];

/**
 * Welche Texte in der Cockpit-Schrift stehen.
 *
 * Das VDGS-Band trägt `font-family: var(--font-acars)` auf seinem
 * Wurzelelement (client/src/components/vdgs.css), also erbt jeder Text
 * darin die Schrift. Die Beschriftungen des Bandes liegen alle unter
 * `cdm.band.*`.
 *
 * Bewusst NICHT der ganze Sprachbaum: Der größte Teil der Oberfläche
 * läuft in der Fließtextschrift, und die kann mehr. Ein Wächter, der
 * alles verbietet, was B612 Mono fehlt, würde Texte beanstanden, die
 * völlig in Ordnung sind — und wäre nach einer Woche abgeschaltet.
 *
 * # Was dieser Wächter NICHT sieht
 *
 * Die erste Fassung dieses Kommentars behauptete, außerhalb des Bandes
 * laufe alles in der Fließtextschrift. Das stimmt nicht, und die
 * externe Abnahme hat es am 21.09.2026 mit zwei Gegenbeispielen
 * widerlegt, die der Pilot heute schon sieht:
 *
 *   * Das Wetter (`.wx__cell`, App.css) zeigt bei Sicht ab 9,5 km
 *     „≥ 10 km" — U+2265 fehlt in B612 Mono. Der Text ist ein Literal
 *     in WeatherBriefing.tsx, kein i18n-Schlüssel, also kann ein
 *     Prüfer über die Sprachdateien ihn gar nicht finden.
 *   * Das Aktivitätsprotokoll (`.log`, App.css) erbt B612 Mono und
 *     zeigt bei jedem Flugstart „A → B" — U+2192 fehlt ebenfalls. Der
 *     Text entsteht im Rust-Backend (lib.rs), nicht in den Locales.
 *
 * Beides ist Bestand und älter als dieser Wächter; beides wurde
 * bewusst nicht im Vorbeigehen geändert, weil der Pfeil an 55 Stellen
 * steht und ein Teil davon in PIREP-Texte geht, die Betreiber greppen.
 *
 * Wer diesen Wächter erweitert, muss also wissen: Er deckt i18n-Texte
 * ab, keine Literale im Quelltext und keine Texte aus dem Backend.
 */
const ZWEIGE = [["cdm", "band"]];

function texteSammeln(baum, pfad) {
  let knoten = baum;
  for (const teil of pfad) {
    knoten = knoten?.[teil];
    if (knoten === undefined) return [];
  }
  const raus = [];
  const gehe = (k, name) => {
    if (typeof k === "string") raus.push([name, k]);
    else if (k && typeof k === "object") {
      for (const [s, v] of Object.entries(k)) gehe(v, `${name}.${s}`);
    }
  };
  gehe(knoten, pfad.join("."));
  return raus;
}

/** Zeichen, die der Vorrat nicht kennt — als Codepoints, nicht als
 *  UTF-16-Einheiten, sonst zerfällt jedes Zeichen außerhalb der BMP. */
function unbekannte(text, vorrat) {
  const raus = new Set();
  for (const z of text) {
    const cp = z.codePointAt(0);
    // Zeilenumbruch und Tabulator stehen nie im Bild.
    if (cp === 0x0a || cp === 0x09) continue;
    if (!vorrat.has(cp)) raus.add(z);
  }
  return [...raus];
}

function pruefe(sprachdateien, vorratDaten, schriftSummen) {
  const fehler = [];
  const vorrat = new Set(vorratDaten.zeichen);

  // Zuerst: Passt der Vorrat überhaupt noch zur Schrift? Sonst prüft
  // der Wächter gegen eine Liste von gestern und meldet grün, während
  // die neue Schrift ein Zeichen verloren hat.
  const schriftNamen = Object.keys(vorratDaten.schriften ?? {});
  if (schriftNamen.length === 0) {
    // Sonst prüft der Wächter gegen eine Liste ohne jede Herkunft: Wer
    // `schriften` leert, schaltet die Prüfsummen-Wache ab, ohne dass
    // etwas rot wird (externe Abnahme, gemessen: fehler=[], geprueft=1).
    fehler.push(
      "der Zeichenvorrat nennt keine Schrift — ohne Herkunft ist er " +
        "nicht überprüfbar; mit scripts/erzeuge-schriftzeichen.py erneuern",
    );
  }
  for (const [name, summe] of Object.entries(vorratDaten.schriften ?? {})) {
    const ist = schriftSummen[name];
    if (ist === undefined) {
      fehler.push(`Schrift ${name} fehlt — Vorrat nicht überprüfbar`);
    } else if (ist !== summe) {
      fehler.push(
        `Schrift ${name} hat sich geändert (${summe} → ${ist}) — ` +
          "Vorrat mit scripts/erzeuge-schriftzeichen.py erneuern",
      );
    }
  }

  // Und: Trägt jede Sprache jeden Zweig?
  //
  // Ohne diese Wache zählte eine Sprache, der `cdm.band` fehlt, einfach
  // nicht mit — `geprueft` stieg durch die anderen beiden, und der
  // Lauf meldete grün. Genau dann zeigt i18next aber den deutschen
  // Ersatztext, und ein Zeichen, das nur dort steht, wäre nie geprüft
  // worden (externe Abnahme, 21.09.2026, gemessen: fehler=[], geprueft=2).
  let geprueft = 0;
  for (const [sprache, baum] of Object.entries(sprachdateien)) {
    for (const zweig of ZWEIGE) {
      const texte = texteSammeln(baum, zweig);
      if (texte.length === 0) {
        fehler.push(
          `${sprache}: Zweig ${zweig.join(".")} fehlt oder ist leer — ` +
            "diese Sprache wurde nicht geprüft",
        );
        continue;
      }
      for (const [schluessel, text] of texte) {
        geprueft += 1;
        const fehlend = unbekannte(text, vorrat);
        if (fehlend.length > 0) {
          const liste = fehlend
            .map((z) => `„${z}" (U+${z.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")})`)
            .join(", ");
          fehler.push(
            `${sprache}/${schluessel}: ${liste} fehlt in B612 Mono — ` +
              "im Cockpit erschiene dort eine Ersatzglyphe oder eine fremde Schrift",
          );
        }
      }
    }
  }
  if (geprueft === 0) {
    fehler.push("kein einziger Text geprüft — die Zweige stimmen nicht mehr");
  }
  return { fehler, geprueft };
}

// ── Selbsttest ────────────────────────────────────────────────────────
if (process.argv.includes("--selbsttest")) {
  const vorratDaten = { schriften: { "X.woff2": "aaaa" }, zeichen: [...Array(95)].map((_, i) => 0x20 + i) };
  const summen = { "X.woff2": "aaaa" };
  const faelle = [
    ["reines ASCII", { de: { cdm: { band: { a: "kein CDM-Eintrag" } } } }, true],
    // Genau der Fehler vom 21.09.2026.
    ["U+2011", { de: { cdm: { band: { a: "PDC/CPDLC‑Tab" } } } }, false],
    ["Gedankenstrich", { de: { cdm: { band: { a: "a — b" } } } }, false],
    ["leerer Zweig", { de: { cdm: {} } }, false],
    // Die beiden Fehlgrün-Pfade, die die externe Abnahme gemessen hat.
    [
      "eine von drei Sprachen ohne Zweig",
      {
        de: { cdm: { band: { a: "ok" } } },
        en: { cdm: {} },
        it: { cdm: { band: { a: "ok" } } },
      },
      false,
    ],
  ];
  let schlecht = 0;
  for (const [name, dateien, erwartetOk] of faelle) {
    const { fehler } = pruefe(dateien, vorratDaten, summen);
    const ok = fehler.length === 0;
    if (ok !== erwartetOk) {
      console.error(
        `SELBSTTEST FEHLGESCHLAGEN (${name}): erwartet ${erwartetOk ? "stimmig" : "abgelehnt"}, ` +
          `bekam ${ok ? "stimmig" : `abgelehnt: ${fehler.join("; ")}`}`,
      );
      schlecht += 1;
    }
  }
  // Und die Gegenproben zur Prüfsummen-Wache.
  const { fehler: f2 } = pruefe(faelle[0][1], vorratDaten, { "X.woff2": "bbbb" });
  if (!f2.some((f) => f.includes("geändert"))) {
    console.error("SELBSTTEST FEHLGESCHLAGEN: geänderte Schrift wurde nicht bemerkt");
    schlecht += 1;
  }
  // Ein Vorrat ohne Herkunft schaltete die Wache lautlos ab.
  const { fehler: f3 } = pruefe(
    faelle[0][1],
    { schriften: {}, zeichen: vorratDaten.zeichen },
    {},
  );
  if (!f3.some((f) => f.includes("keine Schrift"))) {
    console.error("SELBSTTEST FEHLGESCHLAGEN: leere Schriftliste wurde durchgelassen");
    schlecht += 1;
  }
  if (schlecht > 0) process.exit(1);
  console.log(`selbsttest bestanden: ${faelle.length + 2} Fälle richtig beurteilt`);
  process.exit(0);
}

const vorratDaten = JSON.parse(readFileSync(VORRAT, "utf8"));
const schriftSummen = {};
for (const name of Object.keys(vorratDaten.schriften)) {
  try {
    schriftSummen[name] = createHash("sha256")
      .update(readFileSync(new URL(name, SCHRIFTEN)))
      .digest("hex")
      .slice(0, 16);
  } catch {
    /* fehlt → der Prüfer meldet es oben */
  }
}
const sprachdateien = {};
for (const s of SPRACHEN) {
  sprachdateien[s] = JSON.parse(
    readFileSync(new URL(`client/src/locales/${s}/common.json`, WURZEL), "utf8"),
  );
}

const { fehler, geprueft } = pruefe(sprachdateien, vorratDaten, schriftSummen);
if (fehler.length > 0) {
  for (const f of fehler) console.error(`FEHLER: ${f}`);
  process.exit(1);
}
console.log(`schriftzeichen in Ordnung: ${geprueft} Bandtexte, alle in B612 Mono vorhanden`);
