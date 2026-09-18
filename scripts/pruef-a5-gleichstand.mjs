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

import { readFileSync, existsSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { resolve, dirname, join } from "node:path";
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

/** Der Vergleich aus Schritt 1 — als Funktion, damit die Gegenprobe GENAU ihn prüft. */
const dateienGleich = (a, b) => hash(a) === hash(b);

/** Kommentare entfernen — ein Kommentar über `payload.sprit` ist kein Zugriff. */
function ohneKommentare(q) {
  return q.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/(^|[^:"'`])\/\/[^\n]*/g, "$1");
}

/**
 * Die Regel für die Hülle: Sie reicht `sprit` weiter und liest davon
 * höchstens `badge`.
 *
 * Geprüft wird jedes Vorkommen des Bezeichners `sprit`, nicht nur die Form
 * `sprit.feld`. Die erste Fassung erkannte nur die — ein Alias
 * (`const s = pl.sprit; s.leiter`), eine Zerlegung (`const { leiter } =
 * pl.sprit`) oder `pl["sprit"]` gingen durch (QS-Vorschlag V-b, 18.09.2026).
 */
function huellenVerstoesse(quelltext) {
  const q = ohneKommentare(quelltext);
  const v = [];
  if (/\[\s*["'`]sprit["'`]\s*\]/.test(q)) v.push('Zugriff über ["sprit"]');
  for (const m of q.matchAll(/\bsprit\b/g)) {
    const vor = q.slice(Math.max(0, m.index - 12), m.index);
    const nach = q.slice(m.index + 5, m.index + 40);
    // Übersetzungsschlüssel `landing.sprit.*` und `data-testid="sprit-…"`.
    if (/landing\.$/.test(vor) || /^-/.test(nach)) continue;
    // Import-Pfad `../lib/sprit`.
    if (/\/$/.test(vor)) continue;
    if (/\.\.\.\s*(\w+\??\.)?$/.test(vor)) {
      v.push("sprit wird ausgebreitet (...)");
      continue;
    }
    // Zuweisung an einen Alias oder eine Zerlegung — aber nicht die
    // JSX-Weitergabe `sprit={…}` und kein Vergleich.
    if (/[^=!<>]=\s*(\w+\??\.)?$/.test(vor) && !/=\{\s*(\w+\??\.)?$/.test(vor)) {
      v.push(`sprit wird zugewiesen (…${vor.trim()}sprit)`);
      continue;
    }
    const feld = nach.match(/^\s*\??\.\s*([A-Za-z_$][\w$]*)/);
    if (feld && feld[1] !== "badge") v.push(`liest selbst sprit.${feld[1]}`);
  }
  return v;
}

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
    if (!dateienGleich(a, b)) fehler.push(`${rel}: Client und Webapp verschieden`);
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

  // 3. Die Hülle rendert die geteilte Sektion und rechnet NICHTS selbst.
  //
  // Eine Sperrliste alter Funktionsnamen fängt nur die Vergangenheit — jede
  // neue eigene Rechnung käme durch (QS-Befund F5). Die Regel ist deshalb
  // positiv: Die Hülle reicht `sprit` weiter und liest von allen seinen
  // Feldern höchstens `badge` (für die Farbe des Kopf-Chips). Wer ein
  // Zahlenfeld anfasst, rechnet oder formatiert selbst — und genau damit
  // fing die Drift bis v1.7.35 an.
  const huelle = readFileSync(resolve(WEBAPP, "components", "LandingAnalysis.tsx"), "utf-8");
  if (!/<SpritSektion\s+sprit=/.test(huelle)) {
    fehler.push("LandingAnalysis.tsx rendert die geteilte SpritSektion nicht");
  }
  for (const v of huellenVerstoesse(huelle)) {
    fehler.push(`LandingAnalysis.tsx: ${v} — die Hülle darf nur weiterreichen`);
  }
  for (const v of VERBOTEN_IN_HUELLE) {
    if (huelle.includes(v)) fehler.push(`LandingAnalysis.tsx enthält noch „${v}"`);
  }
  if (existsSync(resolve(WEBAPP, "components", "sprit.ts"))) {
    fehler.push("webapp/src/components/sprit.ts existiert noch — die alte eigene Fassung");
  }

  // Gegenproben: Würden die Prüfungen eine Abweichung überhaupt bemerken?
  // (Die erste Fassung verglich einen Datei-Hash mit hash("x") — das schlug
  // nie an, QS-Befund F5.)
  // Gegenprobe 1 prüft DENSELBEN Vergleich wie Schritt 1, an zwei echten
  // Dateien, die sich in einem Byte unterscheiden. Die Vorfassung verglich
  // nur zwei Hash-Werte miteinander und konnte nicht rot werden
  // (QS-Vorschlag V-c, 18.09.2026).
  const probe = mkdtempSync(join(tmpdir(), "a5-probe-"));
  try {
    const inhalt = readFileSync(resolve(CLIENT, GETEILT[0]));
    const echt = join(probe, "echt");
    const kopie = join(probe, "kopie");
    const falsch = join(probe, "falsch");
    writeFileSync(echt, inhalt);
    writeFileSync(kopie, inhalt);
    writeFileSync(falsch, Buffer.concat([inhalt, Buffer.from(" ")]));
    if (dateienGleich(echt, falsch)) fehler.push("Gegenprobe 1: ein veraendertes Byte faellt dem Vergleich nicht auf");
    if (!dateienGleich(echt, kopie)) fehler.push("Gegenprobe 1: zwei gleiche Dateien gelten als verschieden");
  } finally {
    rmSync(probe, { recursive: true, force: true });
  }
  if (kanon({ a: "x", b: "y" }) !== kanon({ b: "y", a: "x" }) || kanon({ a: "x" }) === kanon({ a: "z" })) {
    fehler.push("Gegenprobe 2: der Beschriftungsvergleich ist blind oder reihenfolgeabhaengig");
  }
  // Gegenprobe 3: Jede Umgehung muss auffallen — und die erlaubten Formen
  // der echten Hülle dürfen es nicht.
  const umgehungen = [
    "const n = pl.sprit.leiter;",
    "const n = pl.sprit?.leiter;",
    "const s = pl.sprit; return s.leiter;",
    "const { leiter } = pl.sprit;",
    'const n = pl["sprit"].leiter;',
    "const x = { ...pl.sprit };",
    "function K({ sprit }) { return sprit.anflug; }",
  ];
  for (const u of umgehungen) {
    if (huellenVerstoesse(u).length === 0) fehler.push(`Gegenprobe 3: „${u}" fällt der Hüllen-Regel nicht auf`);
  }
  const erlaubt = [
    'import type { SpritAuswertung } from "../lib/sprit";',
    "sprit?: SpritAuswertung | null;",
    "{pl.sprit && <SpritCard sprit={pl.sprit} />}",
    'data-testid="sprit-badge" data-ton={pl.sprit.badge}',
    'color: pl.sprit.badge === "gruen" ? "a" : "b"',
    "export function SpritCard({ sprit }: { sprit: SpritAuswertung }) { return <SpritSektion sprit={sprit} />; }",
    't("landing.sprit.title")',
    "// payload.sprit.leiter im Kommentar",
  ];
  for (const e of erlaubt) {
    const v = huellenVerstoesse(e);
    if (v.length) fehler.push(`Gegenprobe 3: erlaubte Form „${e}" gilt als Verstoß (${v.join(", ")})`);
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
