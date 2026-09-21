// Zeichenketten-Literale aus Rust- und TypeScript-Quelltext lesen.
//
// Warum ein eigener Scanner und kein regulärer Ausdruck:
//
// Die erste Fassung von `pruefe-schriftzeichen.mjs` hat Literale mit
// `/"((?:[^"\\]|\\.)*)"/g` gesucht. Das sieht robust aus und ist es
// nicht — `.` trifft ohne `s`-Flag keinen Zeilenumbruch, und Rust
// setzt lange Meldungen mit einem `\` am Zeilenende fort:
//
//     "Fuel-Diff {:+.0} kg · Höhen-Diff {:+.0} ft — \
//      physikalisch nicht plausibel"
//
// An dieser Fortsetzung brach die Erkennung ab, die Anführungszeichen
// verrutschten, und der Rest der Datei wurde falsch gepaart. Die
// externe Abnahme vom 21.09.2026 hat das mit einer Mutation belegt:
// Genau die beiden Texte zurückgedreht, die eine Korrektur eben erst
// repariert hatte — der Prüfer blieb grün und meldete unverändert
// „238 Texte".
//
// Ein Prüfer, der seine eigene Korrektur nicht verteidigen kann, ist
// keiner. Deshalb hier ein Scanner, der den Quelltext einmal wirklich
// durchgeht: Er weiß, wo ein Kommentar anfängt, wo eine Zeichenkette
// anfängt und wie sie endet.

/**
 * Eine Escape-Folge ab `i` (das `\\` selbst) auflösen.
 *
 * Ohne diese Auflösung stand im Text `u{2192}` statt „→", und der
 * Wächter sah ein harmloses ASCII-Stück: Ein Pfeil als `"\\u{2192}"`
 * im Quelltext wäre im Cockpit als Fremdglyphe erschienen und im
 * Prüfer grün geblieben (externe Abnahme, 21.09.2026, Mutation M6).
 * Die Codebasis benutzt solche Escapes tatsächlich, etwa `"\\u{2212}"`.
 *
 * Liefert [aufgelöster Text, Länge der Folge im Quelltext].
 */
function escapeAufloesen(quelle, i) {
  const c = quelle[i + 1];
  const einfach = { n: "\n", t: "\t", r: "\r", "0": "\0" };
  if (c in einfach) return [einfach[c], 2];
  if (c === "x") {
    const hex = quelle.slice(i + 2, i + 4);
    return [String.fromCodePoint(parseInt(hex, 16) || 0), 4];
  }
  if (c === "u" && quelle[i + 2] === "{") {
    const zu = quelle.indexOf("}", i + 3);
    const hex = quelle.slice(i + 3, zu);
    return [String.fromCodePoint(parseInt(hex, 16) || 0), zu - i + 1];
  }
  if (c === "u") {
    const hex = quelle.slice(i + 2, i + 6);
    return [String.fromCodePoint(parseInt(hex, 16) || 0), 6];
  }
  return [c ?? "", 2];
}

/**
 * Alle Zeichenketten-Literale einer Rust-Datei, mit Zeilennummer.
 *
 * Erkannt werden:
 *   * `"…"` samt `\"`-Maskierung und `\`-Zeilenfortsetzung,
 *   * Rohzeichenketten `r"…"` und `r#"…"#` mit beliebig vielen Rauten,
 *   * Zeichenliterale `'x'` (übersprungen, nicht als Text gemeldet),
 *   * `//`- und `/* … *\/`-Kommentare (übersprungen).
 *
 * Lebenszeit-Angaben wie `&'a str` sehen wie ein offenes
 * Zeichenliteral aus; sie werden daran erkannt, dass kein schließendes
 * `'` in Reichweite steht.
 */
export function rustLiterale(quelle, kommentare = null) {
  const raus = [];
  let i = 0;
  let zeile = 1;
  const n = quelle.length;

  while (i < n) {
    const z = quelle[i];

    if (z === "\n") {
      zeile += 1;
      i += 1;
      continue;
    }

    // Kommentare.
    if (z === "/" && quelle[i + 1] === "/") {
      const start = i;
      while (i < n && quelle[i] !== "\n") i += 1;
      kommentare?.push({ start, ende: i });
      continue;
    }
    if (z === "/" && quelle[i + 1] === "*") {
      const start = i;
      i += 2;
      let tiefe = 1; // Rust erlaubt verschachtelte Blockkommentare.
      while (i < n && tiefe > 0) {
        if (quelle[i] === "\n") zeile += 1;
        else if (quelle[i] === "/" && quelle[i + 1] === "*") {
          tiefe += 1;
          i += 1;
        } else if (quelle[i] === "*" && quelle[i + 1] === "/") {
          tiefe -= 1;
          i += 1;
        }
        i += 1;
      }
      kommentare?.push({ start, ende: i });
      continue;
    }

    // Rohzeichenkette: r"…" / r#"…"# / br#"…"#
    const roh = /^b?r(#*)"/.exec(quelle.slice(i, i + 16));
    if (roh && (i === 0 || !/[A-Za-z0-9_]/.test(quelle[i - 1]))) {
      const rauten = roh[1];
      const start = i + roh[0].length;
      const ende = quelle.indexOf(`"${rauten}`, start);
      if (ende === -1) break;
      const text = quelle.slice(start, ende);
      const zeileVorher = zeile;
      zeile += text.split("\n").length - 1;
      const schluss = ende + 1 + rauten.length;
      raus.push({ zeile: zeileVorher, text, start: i, ende: schluss });
      i = schluss;
      continue;
    }

    // Gewöhnliche Zeichenkette.
    if (z === '"') {
      const startZeile = zeile;
      const start = i;
      let text = "";
      i += 1;
      while (i < n) {
        const c = quelle[i];
        if (c === "\\") {
          // Zeilenfortsetzung: Backslash, Umbruch, führende Leerzeichen.
          if (quelle[i + 1] === "\n") {
            zeile += 1;
            i += 2;
            while (i < n && (quelle[i] === " " || quelle[i] === "\t")) i += 1;
            continue;
          }
          const [zeichen, laenge] = escapeAufloesen(quelle, i);
          text += zeichen;
          i += laenge;
          continue;
        }
        if (c === '"') {
          i += 1;
          break;
        }
        if (c === "\n") zeile += 1;
        text += c;
        i += 1;
      }
      raus.push({ zeile: startZeile, text, start, ende: i });
      continue;
    }

    // Zeichenliteral oder Lebenszeit.
    if (z === "'") {
      const rest = quelle.slice(i, i + 6);
      if (/^'(\\.|[^\\'])'/.test(rest)) {
        i += rest.startsWith("'\\") ? 4 : 3;
        continue;
      }
      i += 1; // Lebenszeit — nur das Hochkomma überspringen.
      continue;
    }

    i += 1;
  }
  return raus;
}

/**
 * Alle Zeichenketten-Literale einer TypeScript-/TSX-Datei.
 *
 * Erkannt werden `"…"`, `'…'` und Template-Literale `` `…` `` (deren
 * `${…}`-Teile werden übersprungen, der feste Text bleibt), dazu `//`-
 * und `/* … *\/`-Kommentare.
 *
 * Reguläre Ausdrücke werden NICHT eigens behandelt: Ein `/` nach einem
 * Bezeichner ist eine Division, nach `(` oder `=` ein Ausdruck — diese
 * Unterscheidung braucht einen halben Parser. Der Preis dafür ist
 * überschaubar, weil hier nur Literale mit Sonderzeichen zählen.
 */
export function tsLiterale(quelle, kommentare = null) {
  const raus = [];
  scanTs(quelle, 0, false, raus, kommentare, { zeile: 1 });
  return raus;
}

/**
 * Der eigentliche TS-Durchlauf ab `i`.
 *
 * Mit `bisKlammer` endet er an der `}`, die einen `${…}`-Ausdruck
 * schliesst, und liefert deren Position. So werden Zeichenketten
 * INNERHALB eines Template-Ausdrucks mitgelesen.
 *
 * Genau die fehlten vorher: `${warn ? "⚠ " : ""}` — der Ausdruck wurde
 * als Ganzes uebersprungen, und das Warnzeichen in der Loadsheet-Tabelle
 * blieb unsichtbar. Aufgefallen bei der Gegenprobe zur eigenen
 * Korrektur (21.09.2026): zurueckgedreht, und der Waechter blieb gruen.
 */
function scanTs(quelle, i, bisKlammer, raus, kommentare, pos) {
  const n = quelle.length;
  let tiefe = 0;
  while (i < n) {
    const z = quelle[i];

    if (z === "\n") {
      pos.zeile += 1;
      i += 1;
      continue;
    }
    if (z === "/" && quelle[i + 1] === "/") {
      const start = i;
      while (i < n && quelle[i] !== "\n") i += 1;
      kommentare?.push({ start, ende: i });
      continue;
    }
    if (z === "/" && quelle[i + 1] === "*") {
      const start = i;
      i += 2;
      while (i < n && !(quelle[i] === "*" && quelle[i + 1] === "/")) {
        if (quelle[i] === "\n") pos.zeile += 1;
        i += 1;
      }
      i += 2;
      kommentare?.push({ start, ende: i });
      continue;
    }
    if (bisKlammer) {
      if (z === "{") tiefe += 1;
      else if (z === "}") {
        if (tiefe === 0) return i;
        tiefe -= 1;
      }
    }

    if (z === '"' || z === "'" || z === "`") {
      const anfang = z;
      const startZeile = pos.zeile;
      const start = i;
      let text = "";
      i += 1;
      while (i < n) {
        const c = quelle[i];
        if (c === "\\") {
          if (quelle[i + 1] === "\n") {
            // Zeilenfortsetzung im String — gehoert nicht zum Text.
            pos.zeile += 1;
            i += 2;
            continue;
          }
          const [zeichen, laenge] = escapeAufloesen(quelle, i);
          text += zeichen;
          i += laenge;
          continue;
        }
        // `${…}` im Template-Literal: der feste Text laeuft weiter, der
        // Ausdruck wird REKURSIV gelesen — seine Zeichenketten zaehlen.
        if (anfang === "`" && c === "$" && quelle[i + 1] === "{") {
          i = scanTs(quelle, i + 2, true, raus, kommentare, pos) + 1;
          continue;
        }
        if (c === anfang) {
          i += 1;
          break;
        }
        if (c === "\n") pos.zeile += 1;
        text += c;
        i += 1;
      }
      raus.push({ zeile: startZeile, text, start, ende: i });
      continue;
    }

    i += 1;
  }
  return i;
}

/**
 * Die Literale innerhalb jedes `<name>(…)`-Aufrufs.
 *
 * Die Klammern werden über die Literale hinweg gezählt: Eine Klammer
 * IN einer Zeichenkette (`"Ende: 3) fertig"`) hätte den Block sonst
 * vorzeitig geschlossen, und der ganze Aufruf wäre lautlos aus der
 * Prüfung gefallen — nicht nur das eine Literal (externe Abnahme,
 * 21.09.2026, isoliert nachgestellt).
 *
 * Erwähnungen des Namens in einem Kommentar zählen nicht als Aufruf;
 * der Scanner hat sie ohnehin schon verworfen.
 */
export function aufrufLiterale(quelle, name, scanner) {
  // Die Kommentarbereiche kommen aus DEMSELBEN Durchlauf wie die
  // Literale. Eine getrennte Schaetzung („steht vor der Stelle ein
  // unabgeschlossenes /*?") hat quer durch die Datei gegriffen und
  // ganze Aufrufe verschluckt — `log_activity_and_record` fand am
  // 21.09.2026 dadurch null Treffer statt fuenf.
  const kommentare = [];
  const alle = scanner(quelle, kommentare);
  const imLiteral = (pos) => alle.find((l) => l.start <= pos && pos < l.ende);
  const imKommentar = (pos) => kommentare.some((k) => k.start <= pos && pos < k.ende);
  const raus = [];

  // Punkte usw. maskieren: `pending_acars_logs.push` ist ein Name mit
  // Punkt, und ein unmaskierter Punkt passte auf jedes Zeichen.
  const maskiert = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (const m of quelle.matchAll(new RegExp(`\\b${maskiert}\\s*\\(`, "g"))) {
    // Die Definition selbst ist kein Aufruf.
    if (quelle.slice(0, m.index).trimEnd().endsWith("fn")) continue;
    // Eine Erwaehnung in einem Kommentar oder in einer Zeichenkette
    // auch nicht. Vorher wurde eine Kommentarzeile wie
    // „log_activity_handle (which reaches into AppState…)" als Aufruf
    // gezaehlt, und die Klammerzaehlung lief quer durch den Folgecode
    // (externe Abnahme, 21.09.2026).
    if (imLiteral(m.index) || imKommentar(m.index)) continue;

    let i = m.index + m[0].length;
    let tiefe = 1;
    while (i < quelle.length && tiefe > 0) {
      // Literale UND Kommentare am Stueck ueberspringen — ihre
      // Klammern zaehlen nicht. Ein `// Hinweis 1) hier` mitten im
      // Aufruf schloss den Block sonst zu frueh, und der ganze Aufruf
      // fiel still aus der Pruefung (externe Abnahme, 21.09.2026).
      const hier = imLiteral(i) ?? kommentare.find((k) => k.start <= i && i < k.ende);
      if (hier) {
        i = hier.ende;
        continue;
      }
      const c = quelle[i];
      if (c === "(") tiefe += 1;
      else if (c === ")") tiefe -= 1;
      i += 1;
    }
    for (const l of alle) {
      if (l.start >= m.index && l.ende <= i && l.text.length > 0) {
        raus.push({ wo: `${name}:${l.zeile}`, text: l.text });
      }
    }
  }
  return raus;
}
