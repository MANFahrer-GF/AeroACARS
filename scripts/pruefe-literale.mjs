#!/usr/bin/env node
// Selbsttest für den Literal-Scanner (scripts/literale.mjs).
//
// Jeder Fall hier ist ein Quelltextschnipsel, an dem die VORHERIGE
// Fassung — ein regulärer Ausdruck — falsch lag. Die externe Abnahme
// vom 21.09.2026 hat die ersten drei mit Mutationen am echten Bestand
// belegt: Zurückgedrehte Zeichen blieben unbemerkt, weil ihr Text in
// der Sammlung gar nicht auftauchte.
//
// Aufruf: node scripts/pruefe-literale.mjs

import { rustLiterale, tsLiterale, aufrufLiterale } from "./literale.mjs";

let fehler = 0;
function pruefe(name, ist, soll) {
  const a = JSON.stringify(ist);
  const b = JSON.stringify(soll);
  if (a === b) {
    console.log(`  ok  ${name}`);
  } else {
    console.error(`  FAIL ${name}\n       ist:  ${a}\n       soll: ${b}`);
    fehler += 1;
  }
}

// ── R1: Zeilenfortsetzung mit Backslash ───────────────────────────────
// Der Fall, an dem der Regex brach. Rust setzt lange Meldungen so fort,
// und die Fortsetzung verschluckt Umbruch UND Einrückung.
{
  const q = 'let x = "Fuel-Diff {} kg \\\n     nicht plausibel"; let y = "zweite";';
  pruefe(
    "R1: Backslash-Fortsetzung bleibt EIN Literal",
    rustLiterale(q).map((l) => l.text),
    ["Fuel-Diff {} kg nicht plausibel", "zweite"],
  );
}

// ── R2: Rohzeichenkette ───────────────────────────────────────────────
{
  const q = 'let a = r#"roh "mit" Anführung"#; let b = "danach";';
  pruefe(
    "R2: r#\"…\"# wird erkannt, die Anführung darin beendet sie nicht",
    rustLiterale(q).map((l) => l.text),
    ['roh "mit" Anführung', "danach"],
  );
}

// ── R3: Kommentare zählen nicht ───────────────────────────────────────
{
  const q = '// "im Zeilenkommentar"\n/* "im Block" */\nlet a = "echt";';
  pruefe("R3: nur das echte Literal", rustLiterale(q).map((l) => l.text), ["echt"]);
}

// ── R4: Lebenszeit ist kein Zeichenliteral ────────────────────────────
// `&'a str` sah für einen naiven Scanner wie ein offenes Hochkomma aus.
{
  const q = "fn f<'a>(s: &'a str) -> &'a str { s }\nlet a = \"danach\";";
  pruefe("R4: Lebenszeiten verwirren den Scanner nicht", rustLiterale(q).map((l) => l.text), [
    "danach",
  ]);
}

// ── R5: Zeilennummern stimmen auch nach einer Fortsetzung ─────────────
{
  const q = 'let a = "eins \\\n  zwei";\nlet b = "drei";';
  pruefe("R5: Zeilennummern", rustLiterale(q).map((l) => l.zeile), [1, 3]);
}

// ── A1: Klammer IN einer Zeichenkette ─────────────────────────────────
// Die Klammerzählung schloss den Block zu früh, und der ganze Aufruf
// fiel lautlos aus der Prüfung — nicht nur das eine Literal.
{
  const q = 'log(&s, "Ende: 3) fertig", Some("Sicht ≥ 10 km"));';
  pruefe(
    "A1: Klammer im Literal schliesst den Aufruf nicht",
    aufrufLiterale(q, "log", rustLiterale).map((l) => l.text),
    ["Ende: 3) fertig", "Sicht ≥ 10 km"],
  );
}

// ── A2: Erwähnung im Kommentar ist kein Aufruf ────────────────────────
{
  const q = '// log_x (which reaches into State)\nlet a = "frei";\nlog_x(&s, "echt");';
  pruefe(
    "A2: Kommentar-Erwaehnung zaehlt nicht als Aufruf",
    aufrufLiterale(q, "log_x", rustLiterale).map((l) => l.text),
    ["echt"],
  );
}

// ── A3: Die Definition ist kein Aufruf ────────────────────────────────
{
  const q = 'fn log_x(t: &str) { }\nlog_x("aufruf");';
  pruefe(
    "A3: fn-Definition zaehlt nicht",
    aufrufLiterale(q, "log_x", rustLiterale).map((l) => l.text),
    ["aufruf"],
  );
}

// ── A4: Verschachtelter Aufruf bleibt drin ────────────────────────────
{
  const q = 'log_x(&s, format!("außen {}", inner("innen")));';
  pruefe(
    "A4: Literale aus verschachtelten Aufrufen gehoeren dazu",
    aufrufLiterale(q, "log_x", rustLiterale).map((l) => l.text),
    ["außen {}", "innen"],
  );
}

// ── A5: Klammer in einem Kommentar mitten im Aufruf ────────────────
{
  const q = 'log_x(&s,\n  // Hinweis 1) hier\n  "Sicht ≥ 10 km");';
  pruefe(
    "A5: Kommentar-Klammer schliesst den Aufruf nicht",
    aufrufLiterale(q, "log_x", rustLiterale).map((l) => l.text),
    ["Sicht ≥ 10 km"],
  );
}

// ── T1: Template-Literal, feste Teile bleiben ─────────────────────────
{
  const q = "const a = `Sicht ${wert} ≥ 10 km`; const b = 'einfach';";
  pruefe(
    "T1: Template-Literal ohne die ${…}-Teile",
    tsLiterale(q).map((l) => l.text),
    ["Sicht  ≥ 10 km", "einfach"],
  );
}

// ── T2: Kommentar hinter Code am Zeilenende ───────────────────────────
// Die alte Erkennung sah nur auf den Zeilenanfang und hat solche
// Kommentare als Code gelesen — falsch rot.
{
  const q = 'const a = "echt"; // "nur ein Kommentar ≥"\nconst b = "zwei";';
  pruefe("T2: Kommentar hinter Code", tsLiterale(q).map((l) => l.text), ["echt", "zwei"]);
}

// ── T3: Blockkommentar, dessen Folgezeilen nicht mit * beginnen ───────
{
  const q = '/*\n   "≥ im Block"\n   noch im Block\n*/\nconst a = "echt";';
  pruefe("T3: Blockkommentar ohne Sternchen-Rand", tsLiterale(q).map((l) => l.text), ["echt"]);
}

if (fehler > 0) {
  console.error(`\n${fehler} Prüfung(en) fehlgeschlagen`);
  process.exit(1);
}
console.log("\nliteral-scanner in Ordnung — 13 Fälle, alle aus echten Fehlern");
