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
import { readdirSync } from "node:fs";
import { rustLiterale, tsLiterale, aufrufLiterale } from "./literale.mjs";

/** Alle .rs-Dateien unter einem Verzeichnis, rekursiv. */
function rustDateien(wurzel) {
  const raus = [];
  for (const eintrag of readdirSync(wurzel, { withFileTypes: true })) {
    const url = new URL(
      eintrag.name + (eintrag.isDirectory() ? "/" : ""),
      wurzel,
    );
    if (eintrag.isDirectory()) raus.push(...rustDateien(url));
    else if (eintrag.name.endsWith(".rs")) raus.push(url);
  }
  return raus;
}

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
 * Seit dem 21.09.2026 deckt er deshalb DREI Quellen ab: i18n-Zweige,
 * Literale in benannten Quelldateien und die Meldungen, die das
 * Backend ins Aktivitätsprotokoll schreibt.
 *
 * # Was er grundsätzlich NICHT sieht
 *
 * Ein statischer Prüfer liest Quelltext, nicht den laufenden Client.
 * Offen bleiben deshalb:
 *
 *   * Texte, die ERST IN EINER VARIABLE gebaut und dann übergeben
 *     werden (`let msg = …; log_activity_handle(app, …, msg, …)`). Die
 *     externe Abnahme fand am 21.09.2026 drei solche Meldungen mit
 *     fehlenden Zeichen (▶ beim Fortsetzen, Δ in der Loadsheet-Zeile,
 *     → in einem X-Plane-Menüpfad) — keiner davon im Aufruf selbst.
 *     Alle drei sind behoben; gefunden hat sie nur die Abnahme, nicht
 *     dieser Wächter.
 *     Eine vierte (⚠ vor „Touchdown im Pre-Threshold-Bereich") wurde
 *     nur durch eine zu breite Ersetzung erwischt, also durch Glück.
 *   * Texte, die in einem CRATE entstehen (client/src-tauri/crates/) und
 *     erst in der App ins Protokoll gehen. Beispiel, am 21.09.2026 von
 *     Hand behoben: die FMA-Zeile „HDG (→LOC)" aus sim-msfs, jetzt
 *     „HDG (arm LOC)".
 *   * Texte vom Server und aus Fehlermeldungen fremder Bibliotheken.
 *   * Einträge, die vor einem Update gespeichert wurden und beim Start
 *     wieder geladen werden, bis die Kapazitätsgrenze sie verdrängt.
 *   * In TS-Dateien: JSX-Textknoten zwischen Tags, und Literale hinter
 *     einem Regex-Literal, das selbst ein Anführungszeichen enthält.
 *
 * Grün heißt also: Kein DIREKT erkennbarer Text enthält ein Zeichen,
 * das der Schrift fehlt. Nicht: Das Cockpit ist sauber. Wer eine
 * neue Protokollmeldung über eine Variable baut, prüft sie selbst.
 */
const ZWEIGE = [["cdm", "band"]];

/**
 * Quelldateien, deren sichtbare Literale in B612 Mono landen.
 *
 * Jeder Eintrag braucht einen Grund — sonst ist diese Liste nur ein
 * Weg, die Prüfung auszuweiten, bis sie jemand abschaltet.
 *
 * Geprüft werden nur Literale mit einem Zeichen ausserhalb von ASCII.
 * Das ist der Filter, der die Prüfung brauchbar macht: Klassennamen,
 * i18n-Schlüssel und Attribute sind ASCII und fallen nie auf, ein
 * sichtbarer Text mit Sonderzeichen dagegen sofort.
 */
const QUELLDATEIEN = [
  {
    pfad: "client/src/components/WeatherBriefing.tsx",
    grund: "`.wx__cell` und `.wx__raw` tragen --font-acars (App.css). " +
      "Hier stand bis 21.09.2026 die Sichtweite mit U+2265, das der " +
      "Schrift fehlt.",
  },
  {
    pfad: "client/src/components/LoadsheetMonitor.tsx",
    grund: "`.ls td` traegt --font-acars (App.css). Hier stand bis " +
      "21.09.2026 ein Warnzeichen U+26A0 bei Uebergewicht — genau in der " +
      "Warnzeile verrutschte die Ziffernspalte.",
  },
  {
    pfad: "client/src/components/ActivityLogPanel.tsx",
    grund: "Die Protokollliste erbt --font-acars (App.css, `.log`).",
  },
];

/**
 * Die Meldungen, die das Backend ins Aktivitätsprotokoll schreibt.
 *
 * Sie entstehen in Rust und gehen durch keine Sprachdatei — über die
 * Locales waren sie also unerreichbar. Bis 21.09.2026 stand in acht
 * dieser Meldungen ein „→" (U+2192), das der Schrift fehlt; sichtbar
 * bei jedem Flugstart.
 *
 * Gelesen werden die Zeichenketten-Literale innerhalb eines
 * `log_activity(...)`-Aufrufs — nicht alle Literale von lib.rs, denn
 * der Pfeil steht dort an 55 Stellen, und die übrigen gehen in
 * PIREP-Texte an phpVMS, die niemand im Cockpit sieht.
 */
const BACKEND = {
  wurzel: "client/src-tauri/src/",
  // ALLE DREI Schreibfunktionen. Die erste Fassung kannte nur
  // `log_activity` — und weil der Name ein Präfix der beiden anderen
  // ist, sah sie so vollständig aus, wie sie es nicht war: Hinter
  // `log_activity_handle` standen 103 Aufrufe, davon 13 mit einem
  // Pfeil, hinter `log_activity_and_record` weitere zwei mit „Δ".
  // Gefunden bei der QS zur eigenen Korrektur (21.09.2026).
  aufrufe: [
    "log_activity",
    "log_activity_handle",
    "log_activity_and_record",
    // Der vierte Weg: Zeilen, die erst gepuffert und spaeter ins
    // Protokoll gespiegelt werden (lib.rs, `log_activity_handle(line…)`),
    // und gleichlautend als ACARS-Log an phpVMS gehen. Vorher ungeprueft
    // (externe Abnahme, 21.09.2026, Mutation M7).
    "pending_acars_logs.push",
  ],
};

/**
 * Literale mit mindestens einem Zeichen ausserhalb von ASCII.
 *
 * Gelesen wird mit dem Scanner aus literale.mjs, nicht mit einem
 * regulären Ausdruck: Der kann Kommentare nicht von Code
 * unterscheiden, sobald der Kommentar hinter dem Code steht oder ein
 * Blockkommentar keinen Sternchen-Rand hat.
 */
/**
 * Literale, die zwar in einer der Quelldateien stehen, aber NICHT in
 * einem Element mit der Cockpit-Schrift landen.
 *
 * Das ist die bekannte Schwäche des Datei-Ansatzes: Geprüft wird die
 * ganze Datei, aber nur einzelne Elemente darin tragen B612 Mono. Jeder
 * Eintrag nennt deshalb die Stelle und den Grund — und der Grund muss
 * am CSS nachprüfbar sein, sonst ist diese Liste nur ein Weg, den
 * Wächter ruhigzustellen.
 *
 * Verglichen wird der GANZE Text, nicht ein Teilstück. Mit
 * `includes()` hätte ein Literal wie „⟳Δ→ alles auf einmal" die
 * Ausnahme geerbt und Δ und → gleich mit durchgeschmuggelt (externe
 * Abnahme, 21.09.2026).
 */
const NICHT_IN_DER_SCHRIFT_GERENDERT = [
  {
    datei: "client/src/components/WeatherBriefing.tsx",
    texte: ["🌫", "⛈", "🌨", "🌦", "🌧", "❄", "🌁", "☀", "☁", "🌥", "⛅", "🌤"],
    grund:
      "die Wettersymbole der Kopfzeile. Sie landen in `.wx__phen` " +
      "(App.css 1496) innerhalb von `.wx__head` — beide setzen keine " +
      "font-family, und auch `.card` darüber nicht. Die Reihe erbt also " +
      "die Fliesstextschrift. B612 Mono tragen nur `.wx__cell` und " +
      "`.wx__raw` weiter unten, und dort steht kein Symbol.",
  },
  {
    datei: "client/src/components/WeatherBriefing.tsx",
    text: "⟳",
    grund:
      "steht auf `.weather-briefing__refresh` im Kopf des Briefings. " +
      "Die Regel setzt keine font-family (App.css 3397), und kein " +
      "Vorfahr setzt --font-acars — der Knopf erbt die Fliesstextschrift. " +
      "B612 Mono tragen nur `.wx__cell` und `.wx__raw` darunter.",
  },
];

/**
 * Piktogramme sind kein Befund.
 *
 * Ein Zeichen mit Emoji-Darstellung kommt IMMER aus einer Bilderschrift,
 * in jeder Anwendung, und sieht genau so aus, wie es soll. Es hier zu
 * beanstanden hiesse, den Wächter eine Woche zu haben und dann
 * abgeschaltet.
 *
 * Der Befund sind TEXTzeichen, die still aus einer fremden Textschrift
 * kommen: Ihre Laufweite passt nicht zur Monospace-Zeile daneben, und
 * niemand rechnet damit.
 *
 * Die Grenze lief zuerst an `\p{Extended_Pictographic}` — und die ist
 * VIEL weiter als „wird als Bild gezeigt". Gemessen (externe Abnahme,
 * 21.09.2026): ⚠ U+26A0, ⏸ U+23F8, ☀ U+2600, ✈, ❄, ✔ sind alle
 * Extended_Pictographic, haben aber TEXT-Darstellung und fehlen B612
 * Mono — also genau der Fehler, den dieser Wächter finden soll, per
 * Ausnahme für unauffällig erklärt. Acht reale Protokollmeldungen waren
 * betroffen.
 *
 * `\p{Emoji_Presentation}` ist die richtige Grenze: Diese Zeichen zeigt
 * jede Umgebung von sich aus als farbiges Bild (🔴, 🛬), und dafür ist
 * eine Bilderschrift gewollt. Alles andere ist Text.
 */
const PIKTOGRAMM = /\p{Emoji_Presentation}|\uFE0F|\p{Emoji_Modifier}/u;

/** Die Blattwerte eines i18n-Zweigs, mit vollem Schlüsselpfad. */
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

/** Fehlende Zeichen lesbar auflisten, mit Codepoint zum Nachschlagen. */
function zeichenListe(zeichen) {
  return zeichen
    .map(
      (z) =>
        `\u201e${z}" (U+${z.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")})`,
    )
    .join(", ");
}

/**
 * Zeichen, die der Vorrat nicht kennt — als Codepoints, nicht als
 * UTF-16-Einheiten, sonst zerfällt jedes Zeichen ausserhalb der BMP.
 */
function unbekannte(text, vorrat) {
  const raus = new Set();
  // Nach GRAPHEMEN gehen, nicht nach Codepoints: „❤️" ist U+2764 plus
  // das Variantenzeichen FE0F, das die Emoji-Darstellung erzwingt; „👨‍✈️"
  // haengt Teile mit U+200D zusammen; „1️⃣" endet auf U+20E3. Einzeln
  // betrachtet saehe U+2764 wie ein Textzeichen aus, das der Schrift
  // fehlt — als Folge ist es ein Bild und gewollt (externe Abnahme,
  // 21.09.2026, Mutationen M9–M11). FE0E dagegen erzwingt TEXT und wird
  // weiter gemeldet.
  const segmente = new Intl.Segmenter(undefined, { granularity: "grapheme" });
  for (const { segment } of segmente.segment(text)) {
    if (/[\uFE0F\u200D\u20E3]/u.test(segment) && !segment.includes("\uFE0E")) continue;
    for (const z of segment) {
      const cp = z.codePointAt(0);
      // Zeilenumbruch und Tabulator stehen nie im Bild.
      if (cp === 0x0a || cp === 0x09) continue;
      if (PIKTOGRAMM.test(z)) continue;
      if (!vorrat.has(cp)) raus.add(z);
    }
  }
  return [...raus];
}

function auffaelligeLiterale(quelle, datei, scanner) {
  return scanner(quelle)
    .filter((l) => [...l.text].some((z) => z.codePointAt(0) > 0x7f))
    .map((l) => ({ wo: `${datei}:${l.zeile}`, text: l.text }));
}

function pruefe(sprachdateien, vorratDaten, schriftSummen, quellen = null) {
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
          fehler.push(
            `${sprache}/${schluessel}: ${zeichenListe(fehlend)} fehlt in B612 Mono — ` +
              "im Cockpit erschiene dort eine Ersatzglyphe oder eine fremde Schrift",
          );
        }
      }
    }
  }
  // Die beiden Quellen ausserhalb der Sprachdateien. Ohne `quellen`
  // übersprungen — dann sagt der Prüfer das auch, statt Vollständigkeit
  // vorzutäuschen.
  let ausQuellen = 0;
  if (quellen) {
    for (const [herkunft, texte] of Object.entries(quellen)) {
      // Eine Quelldatei ohne auffälliges Literal ist der Normalfall und
      // sogar das Ziel. Beim BACKEND ist es umgekehrt: Findet das
      // Suchmuster dort nichts, wurde `log_activity` umbenannt, und der
      // Prüfer sähe stillschweigend weg.
      if (texte.length === 0) {
        if (BACKEND.aufrufe.some((a) => herkunft.endsWith(`(${a})`))) {
          fehler.push(
            `${herkunft}: kein einziger Aufruf gefunden — umbenannt? ` +
              "Ohne Treffer prüft dieser Teil nichts",
          );
        }
        continue;
      }
      for (const [wo, text] of texte) {
        const ausgenommen = NICHT_IN_DER_SCHRIFT_GERENDERT.find(
          (a) =>
            wo.startsWith(a.datei) &&
            (a.text === text || a.texte?.includes(text)),
        );
        if (ausgenommen) continue;
        ausQuellen += 1;
        const fehlend = unbekannte(text, vorrat);
        if (fehlend.length > 0) {
          fehler.push(
            `${wo}: ${zeichenListe(fehlend)} fehlt in B612 Mono — ` +
              "im Cockpit erschiene dort eine Ersatzglyphe oder eine fremde Schrift",
          );
        }
      }
    }
  }

  if (geprueft === 0) {
    fehler.push("kein einziger Text geprüft — die Zweige stimmen nicht mehr");
  }
  return { fehler, geprueft, ausQuellen };
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
  // Die drei Pfade der Quellen-Prüfung.
  const gut = faelle[0][1];
  const quellenFaelle = [
    [
      "Quelldatei mit Textzeichen, das fehlt",
      { "a.tsx": [["a.tsx:1", "Sicht \u2265 10 km"]] },
      false,
    ],
    // Ein Zeichen mit EMOJI-Darstellung kommt immer aus einer
    // Bilderschrift — kein Befund.
    ["Quelldatei mit echtem Emoji", { "a.tsx": [["a.tsx:1", "Landung \u{1F6EC}"]] }, true],
    // ☀ sieht aus wie ein Emoji, hat aber TEXT-Darstellung und fehlt
    // der Schrift. Der alte Selbsttest erwartete hier ausdruecklich
    // „stimmig" und zementierte damit den Fehler, den die externe
    // Abnahme am 21.09.2026 als sperrend gemeldet hat.
    ["Quelldatei mit Textsymbol, das nur wie ein Emoji aussieht",
     { "a.tsx": [["a.tsx:1", "Wetter \u2600"]] }, false],
    ["Quelldatei mit Warnzeichen", { "a.tsx": [["a.tsx:1", "\u26A0 Achtung"]] }, false],
    // Emoji-FOLGEN sind Bilder, auch wenn ein Teil davon allein ein
    // Textzeichen waere.
    ["Herz mit Emoji-Variante", { "a.tsx": [["a.tsx:1", "Made with \u2764\uFE0F"]] }, true],
    ["Pilot als ZWJ-Folge", { "a.tsx": [["a.tsx:1", "\u{1F468}\u200D\u2708\uFE0F"]] }, true],
    ["Tastenkappe", { "a.tsx": [["a.tsx:1", "1\uFE0F\u20E3 Schritt"]] }, true],
    // Und die Gegenrichtung: FE0E erzwingt TEXT-Darstellung.
    ["Emoji mit Text-Variante", { "a.tsx": [["a.tsx:1", "\u{1F534}\uFE0E"]] }, false],
    ["Quelldatei ohne auffaelliges Literal", { "a.tsx": [] }, true],
    [
      "Backend-Suchmuster laeuft ins Leere",
      { "lib.rs (log_activity)": [] },
      false,
    ],
    [
      "Backend-Text mit Pfeil",
      { "lib.rs (log_activity)": [["log_activity:1", "A \u2192 B"]] },
      false,
    ],
  ];
  for (const [name, quellen, erwartetOk] of quellenFaelle) {
    const { fehler } = pruefe(gut, vorratDaten, summen, quellen);
    const ok = fehler.length === 0;
    if (ok !== erwartetOk) {
      console.error(
        `SELBSTTEST FEHLGESCHLAGEN (${name}): erwartet ${erwartetOk ? "stimmig" : "abgelehnt"}, ` +
          `bekam ${ok ? "stimmig" : `abgelehnt: ${fehler.join("; ")}`}`,
      );
      schlecht += 1;
    }
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
  console.log(
    `selbsttest bestanden: ${faelle.length + quellenFaelle.length + 2} Fälle richtig beurteilt`,
  );
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

// Die beiden Quellen ausserhalb der Sprachdateien einsammeln.
const quellen = {};
for (const { pfad } of QUELLDATEIEN) {
  const quelle = readFileSync(new URL(pfad, WURZEL), "utf8");
  quellen[pfad] = auffaelligeLiterale(quelle, pfad, tsLiterale).map((l) => [
    l.wo,
    l.text,
  ]);
}
// Alle Rust-Quellen, nicht nur lib.rs: `log_activity_handle` ist
// `pub(crate)` und wird auch aus hoppie/ heraus gerufen — dieselbe
// Protokollliste, vorher ungeprüft (externe Abnahme, 21.09.2026).
for (const datei of rustDateien(new URL(BACKEND.wurzel, WURZEL))) {
  const quelle = readFileSync(datei, "utf8");
  const kurz = decodeURIComponent(datei.pathname).split("/src-tauri/")[1] ?? "";
  for (const aufruf of BACKEND.aufrufe) {
    const treffer = aufrufLiterale(quelle, aufruf, rustLiterale);
    if (treffer.length === 0) continue;
    const schluessel = `src-tauri/${kurz} (${aufruf})`;
    // `l.wo` traegt den Aufrufnamen schon; nur die Datei davorsetzen.
    quellen[schluessel] = treffer.map((l) => [
      `src-tauri/${kurz}:${l.wo}`,
      l.text,
    ]);
  }
}
// Und die Gegenprobe, dass ueberhaupt etwas gefunden wurde: Wird eine
// der drei Funktionen umbenannt, faende der Prüfer im ganzen Baum
// nichts mehr und saehe stillschweigend weg.
for (const aufruf of BACKEND.aufrufe) {
  const gefunden = Object.keys(quellen).some((k) => k.endsWith(`(${aufruf})`));
  if (!gefunden) quellen[`src-tauri (${aufruf})`] = [];
}

const { fehler, geprueft, ausQuellen } = pruefe(
  sprachdateien,
  vorratDaten,
  schriftSummen,
  quellen,
);
if (fehler.length > 0) {
  for (const f of fehler) console.error(`FEHLER: ${f}`);
  process.exit(1);
}
console.log(
  `schriftzeichen in Ordnung: ${geprueft} Bandtexte und ${ausQuellen} Texte ` +
    "aus Quelldateien und Backend, alle in B612 Mono vorhanden",
);
