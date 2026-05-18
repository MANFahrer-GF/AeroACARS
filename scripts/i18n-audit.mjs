#!/usr/bin/env node
// i18n-Audit für AeroACARS — drei Checks, klare Berichte.
//
// 1. PARITY-Check: alle Keys aus EN müssen auch in DE und IT existieren
//    (und umgekehrt). Fehlende Keys werden pro Sprache aufgelistet.
//
// 2. CODE-Check: alle in src/**/*.{ts,tsx} referenzierten t("foo.bar")-
//    Keys müssen mind. im EN-File existieren — sonst rendert die UI den
//    Key-String roh ("landing.foo.bar" statt der übersetzten Phrase).
//    Das war der Bug von vorhin (Approach-Stability-Card-Klone).
//
// 3. DEAD-Check: Keys die im EN existieren aber nirgendwo im Code
//    referenziert werden — Indikator für Altlasten / Tippfehler.
//    Nur Warnung, nicht fatal (manche Keys werden dynamisch aufgebaut:
//    `landing.rat.${rationale}`, `landing.tip.${rationale}` usw.).
//
// Aufruf: node scripts/i18n-audit.mjs [--strict]
//   --strict → Exit-Code 1 bei DEAD-Keys (sonst nur bei MISSING)
//
// Output ist farbcodiert via ANSI-Codes (Standard-Terminals).

import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..");
const LOCALES_DIR = path.join(ROOT, "client", "src", "locales");
const SRC_DIR = path.join(ROOT, "client", "src");
const LOCALES = ["en", "de", "it"];
const REFERENCE = "en"; // Master-Locale: EN ist source-of-truth

const ANSI = {
  reset: "\x1b[0m",
  red: "\x1b[31m",
  green: "\x1b[32m",
  yellow: "\x1b[33m",
  cyan: "\x1b[36m",
  gray: "\x1b[90m",
  bold: "\x1b[1m",
};

const strict = process.argv.includes("--strict");

// ─── 1. Locales laden ───────────────────────────────────────────────
function flattenKeys(obj, prefix = "") {
  const out = [];
  for (const [k, v] of Object.entries(obj)) {
    const fullKey = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      out.push(...flattenKeys(v, fullKey));
    } else {
      out.push(fullKey);
    }
  }
  return out;
}

const localeFiles = {};
const localeKeys = {};
for (const loc of LOCALES) {
  const filePath = path.join(LOCALES_DIR, loc, "common.json");
  if (!fs.existsSync(filePath)) {
    console.error(`${ANSI.red}❌ Missing locale file: ${filePath}${ANSI.reset}`);
    process.exit(1);
  }
  localeFiles[loc] = JSON.parse(fs.readFileSync(filePath, "utf8"));
  localeKeys[loc] = new Set(flattenKeys(localeFiles[loc]));
}

// ─── 2. PARITY-Check ────────────────────────────────────────────────
console.log(`\n${ANSI.bold}${ANSI.cyan}═══ 1. PARITY-Check (EN ↔ DE ↔ IT) ═══${ANSI.reset}\n`);

const refKeys = localeKeys[REFERENCE];
let parityFails = 0;

for (const loc of LOCALES) {
  if (loc === REFERENCE) continue;
  const missing = [...refKeys].filter((k) => !localeKeys[loc].has(k));
  const extra = [...localeKeys[loc]].filter((k) => !refKeys.has(k));
  if (missing.length === 0 && extra.length === 0) {
    console.log(`  ${ANSI.green}✓ ${loc.toUpperCase()}: parity with ${REFERENCE.toUpperCase()}${ANSI.reset}`);
    continue;
  }
  parityFails++;
  console.log(`  ${ANSI.red}✗ ${loc.toUpperCase()}:${ANSI.reset}`);
  if (missing.length > 0) {
    console.log(
      `    ${ANSI.red}${missing.length} keys missing (in ${REFERENCE} but not in ${loc}):${ANSI.reset}`,
    );
    missing.slice(0, 20).forEach((k) => console.log(`      ${ANSI.gray}- ${k}${ANSI.reset}`));
    if (missing.length > 20) {
      console.log(`      ${ANSI.gray}... and ${missing.length - 20} more${ANSI.reset}`);
    }
  }
  if (extra.length > 0) {
    console.log(
      `    ${ANSI.yellow}${extra.length} keys EXTRA (in ${loc} but not in ${REFERENCE} — likely orphans):${ANSI.reset}`,
    );
    extra.slice(0, 20).forEach((k) => console.log(`      ${ANSI.gray}- ${k}${ANSI.reset}`));
    if (extra.length > 20) {
      console.log(`      ${ANSI.gray}... and ${extra.length - 20} more${ANSI.reset}`);
    }
  }
}

// ─── 3. CODE-Check ──────────────────────────────────────────────────
console.log(`\n${ANSI.bold}${ANSI.cyan}═══ 2. CODE-Check (alle t("…")-Keys existieren in ${REFERENCE.toUpperCase()}) ═══${ANSI.reset}\n`);

// Collect all .ts/.tsx files
function walkDir(dir, files = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist" || entry.name === ".git") continue;
      walkDir(full, files);
    } else if (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx")) {
      files.push(full);
    }
  }
  return files;
}
const sourceFiles = walkDir(SRC_DIR);

// Match t("foo.bar"), t('foo.bar'), t(`foo.bar`) — only LITERAL keys.
// Dynamic keys like t(`landing.rat.${x}`) sind nicht prüfbar — die
// muessen über die Code-Logik validiert werden (siehe DEAD-Check
// für unbenutzte Keys, die dort dann auftauchen wenn ein dynamischer
// Lookup fehlschlägt).
const STATIC_T_REGEX = /\bt\(\s*["'`]([a-zA-Z][a-zA-Z0-9_.]+)["'`]/g;

const referencedKeys = new Set();
const dynamicPrefixes = new Set(); // z.B. "landing.rat" aus t(`landing.rat.${x}`)
const usedByFile = {};

// v0.11.0-dev Audit-Pass 2: zusätzlich zu direkt-in-t()-Templates auch
// ALLE Template-Strings im Code erfassen die wie i18n-Keys aussehen.
// Grund: Hilfs-Funktionen wie `coachTipKey(r) = \`landing.tip.${r}\``
// bauen den Key separat zusammen und übergeben ihn als String an t() —
// die direkte t()-Inspektion verpasst das.
// Heuristik: jeder Template-String mit `prefix.${…}` der wie ein i18n-
// Pfad aussieht (mindestens ein Punkt vor dem `${`) wird als dynamic
// prefix markiert. False-positives sind harmlos (= ein paar mehr keys
// gelten als legitim).
const ANY_TEMPLATE_REGEX = /`([a-zA-Z][a-zA-Z0-9_.]+)\.\$\{/g;

for (const file of sourceFiles) {
  const content = fs.readFileSync(file, "utf8");
  let m;
  while ((m = STATIC_T_REGEX.exec(content)) !== null) {
    referencedKeys.add(m[1]);
    usedByFile[m[1]] = usedByFile[m[1]] || [];
    usedByFile[m[1]].push(path.relative(ROOT, file));
  }
  while ((m = ANY_TEMPLATE_REGEX.exec(content)) !== null) {
    const prefix = m[1];
    // Filter: nur Prefixes die zu mind. einem existierenden EN-Key
    // passen → vermeidet zufällige Template-Strings die nichts mit i18n
    // zu tun haben (z.B. URLs wie `api.example.com/${id}`).
    for (const refKey of refKeys) {
      if (refKey.startsWith(prefix + ".")) {
        dynamicPrefixes.add(prefix);
        break;
      }
    }
  }
  // v0.11.0-dev Audit-Pass 3: implizite String-Konstanten als
  // i18n-Keys erkennen. Pattern wie:
  //   const KEY = "landing.confidence.high";
  //   return { textKey: "landing.side.left" };
  //   t(KEY);  // ← Audit-Regex sieht t(KEY) nicht als i18n-call
  // werden so abgefangen. Heuristik: jedes String-Literal das exakt
  // einem existierenden EN-Key entspricht wird als referenziert
  // markiert. False-positives unwahrscheinlich weil die Match-
  // Bedingung „ist exakter EN-Key" sehr streng ist.
  const STRING_LITERAL_REGEX = /["'`]([a-z][a-zA-Z0-9_]+(?:\.[a-zA-Z0-9_]+)+)["'`]/g;
  while ((m = STRING_LITERAL_REGEX.exec(content)) !== null) {
    if (refKeys.has(m[1])) {
      referencedKeys.add(m[1]);
      usedByFile[m[1]] = usedByFile[m[1]] || [];
      usedByFile[m[1]].push(path.relative(ROOT, file));
    }
  }
  // Underscore-Suffix-Pattern: t(`settings.simbrief.verify_err_${err}`)
  // Der vorherige Regex matcht nur `prefix.${...}` (mit Punkt). Hier
  // erfassen wir zusätzlich `prefix_${...}` und behandeln den Prefix
  // wie eine Plural-Familie (alle Keys, die mit `prefix_` anfangen,
  // gelten als referenziert).
  const UNDERSCORE_TMPL_REGEX = /`([a-zA-Z][a-zA-Z0-9_.]+)_\$\{/g;
  while ((m = UNDERSCORE_TMPL_REGEX.exec(content)) !== null) {
    const stem = m[1] + "_";
    // Markiere alle EN-Keys mit diesem Stamm als referenziert.
    for (const refKey of refKeys) {
      if (refKey.startsWith(stem)) {
        referencedKeys.add(refKey);
      }
    }
  }
}

// i18next-V4 Plural-Resolution: `t("foo", { count })` löst auf `foo_one`
// oder `foo_other` auf — der Base-Key `foo` muss NICHT existieren. Wir
// behandeln einen Key als „vorhanden" wenn entweder der exakte Key ODER
// einer der Plural-Suffixe (_zero/_one/_two/_few/_many/_other) im Locale ist.
const PLURAL_SUFFIXES = ["_zero", "_one", "_two", "_few", "_many", "_other"];
function isResolvable(key, keyset) {
  if (keyset.has(key)) return true;
  for (const s of PLURAL_SUFFIXES) {
    if (keyset.has(key + s)) return true;
  }
  return false;
}

const missingInRef = [...referencedKeys].filter((k) => !isResolvable(k, refKeys));
if (missingInRef.length === 0) {
  console.log(
    `  ${ANSI.green}✓ All ${referencedKeys.size} referenced static keys exist in ${REFERENCE.toUpperCase()}${ANSI.reset}`,
  );
} else {
  console.log(
    `  ${ANSI.red}✗ ${missingInRef.length} keys referenced in code but missing in ${REFERENCE.toUpperCase()}:${ANSI.reset}`,
  );
  for (const k of missingInRef) {
    const files = usedByFile[k] ?? [];
    console.log(`    ${ANSI.red}- ${k}${ANSI.reset} ${ANSI.gray}(${files[0]})${ANSI.reset}`);
  }
}
if (dynamicPrefixes.size > 0) {
  console.log(
    `  ${ANSI.gray}(${dynamicPrefixes.size} dynamic prefixes detected: ${[...dynamicPrefixes].join(", ")} — values not statically checked)${ANSI.reset}`,
  );
}

// ─── 4. DEAD-Check ──────────────────────────────────────────────────
console.log(`\n${ANSI.bold}${ANSI.cyan}═══ 3. DEAD-Check (Keys in ${REFERENCE.toUpperCase()} aber nicht im Code referenziert) ═══${ANSI.reset}\n`);

const deadKeys = [...refKeys].filter((k) => {
  if (referencedKeys.has(k)) return false;
  // Skip wenn ein dynamic prefix passt
  for (const prefix of dynamicPrefixes) {
    if (k.startsWith(prefix + ".")) return false;
  }
  return true;
});

if (deadKeys.length === 0) {
  console.log(`  ${ANSI.green}✓ No dead keys.${ANSI.reset}`);
} else {
  console.log(
    `  ${ANSI.yellow}⚠ ${deadKeys.length} keys defined but apparently unused:${ANSI.reset}`,
  );
  deadKeys.slice(0, 30).forEach((k) =>
    console.log(`    ${ANSI.gray}- ${k}${ANSI.reset}`),
  );
  if (deadKeys.length > 30) {
    console.log(`    ${ANSI.gray}... and ${deadKeys.length - 30} more${ANSI.reset}`);
  }
  console.log(
    `  ${ANSI.gray}(Hinweis: nicht jeder „dead\" key ist wirklich tot — manche werden via dynamische Template-Strings ohne sichtbaren Prefix gerendert. Trotzdem ein guter Indikator für Altlasten.)${ANSI.reset}`,
  );
}

// ─── 5. Summary ─────────────────────────────────────────────────────
console.log(`\n${ANSI.bold}${ANSI.cyan}═══ Zusammenfassung ═══${ANSI.reset}`);
console.log(`  EN-Master-Keys:        ${refKeys.size}`);
console.log(`  Referenziert im Code:  ${referencedKeys.size} (statisch)`);
console.log(`  Dynamic Prefixes:      ${dynamicPrefixes.size}`);
console.log(`  Parity-Failures:       ${parityFails === 0 ? ANSI.green + "0" : ANSI.red + parityFails}${ANSI.reset}`);
console.log(`  Missing-In-Code:       ${missingInRef.length === 0 ? ANSI.green + "0" : ANSI.red + missingInRef.length}${ANSI.reset}`);
console.log(`  Dead-Keys:             ${deadKeys.length === 0 ? ANSI.green + "0" : ANSI.yellow + deadKeys.length}${ANSI.reset}`);

const fatal = parityFails > 0 || missingInRef.length > 0;
const failOnDead = strict && deadKeys.length > 0;

if (fatal || failOnDead) {
  console.log(`\n${ANSI.red}❌ AUDIT FAILED${ANSI.reset}\n`);
  process.exit(1);
}
console.log(`\n${ANSI.green}✓ AUDIT PASSED${ANSI.reset}\n`);
