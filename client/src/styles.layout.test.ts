// Wächter gegen Layout-Regeln in wiederverwendeten Stil-Varianten.
//
// Feldbefund 25.07.2026 (Thomas): In den Hoppie-Einstellungen saß "Speichern"
// höher als "Entfernen", im CPDLC-Reiter "Anmelden" versetzt zum Center-Feld.
//
// Ursache war EINE Zeile:
//
//     .button--primary { align-self: flex-start; }
//
// Gedacht war sie für das Login-Formular — dort steht der Knopf in einer
// Spalte und soll nicht auf volle Breite gezogen werden. In einer ZEILE
// bedeutet dieselbe Eigenschaft aber "nach oben rücken", und `align-self`
// überstimmt das `align-items` des Containers. Jede Knopfleiste der App war
// betroffen, weil `button--primary` an 27 Stellen verwendet wird.
//
// Die Lehre ist allgemein: Eine Variante wie `--primary` beschreibt AUSSEHEN.
// Wo ein Element sitzt, entscheidet sein Container. Sobald eine Variante
// Ausrichtung mitbringt, bricht sie an jeder Stelle, deren Container es
// anders vorsieht — und man sieht es nur dort, wo man zufällig hinschaut.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(__dirname, "App.css"), "utf8");

/** Eigenschaften, die bestimmen, WO ein Element sitzt — nicht wie es aussieht. */
const LAYOUT_PROPS = ["align-self", "justify-self", "float", "position: absolute", "position: fixed"];

/**
 * Regelblöcke der Form `.foo--bar { … }` einsammeln.
 *
 * Bewusst nur einfache Selektoren: `.login__form .button--primary` ist in
 * Ordnung, weil dort ein Container die Ausrichtung für seinen eigenen Fall
 * festlegt. Verboten ist die Variante ALLEIN, weil sie überall gilt.
 */
function bareVariantRules(): Array<{ selector: string; body: string }> {
  const out: Array<{ selector: string; body: string }> = [];
  const re = /(^|\})\s*(\.[a-z0-9-]+--[a-z0-9-]+)\s*\{([^}]*)\}/gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(css)) !== null) {
    out.push({ selector: m[2], body: m[3] });
  }
  return out;
}

describe("Stil-Varianten enthalten kein Layout", () => {
  it("keine `--variante` bringt eigene Ausrichtung mit", () => {
    const offenders = bareVariantRules()
      .map(({ selector, body }) => {
        const found = LAYOUT_PROPS.filter((p) => body.includes(p));
        return found.length ? `${selector} → ${found.join(", ")}` : null;
      })
      .filter(Boolean);

    expect(
      offenders,
      "Ausrichtung gehört in den Container, nicht in eine Variante — sonst " +
        "sitzt der Knopf an einer von zwanzig Stellen falsch und niemand merkt es",
    ).toEqual([]);
  });

  it("`button--primary` beschreibt nur noch Aussehen", () => {
    const rule = bareVariantRules().find((r) => r.selector === ".button--primary");
    expect(rule, "Regel muss es weiterhin geben").toBeTruthy();
    for (const prop of LAYOUT_PROPS) {
      expect(rule!.body, `${prop} gehört hier nicht hin`).not.toContain(prop);
    }
  });

  it("die Ausnahme fürs Login-Formular bleibt erhalten", () => {
    // Dort ist sie richtig: eine Spalte, in der der Knopf nicht auf volle
    // Breite gezogen werden soll. Der Test hält fest, dass der Fix die
    // Absicht nicht mit weggeworfen hat.
    expect(css).toContain(".login__form .button--primary");
  });
});

describe("CPDLC-Feld bleibt so hoch wie seine Eingabe", () => {
  // Zweite Ursache desselben Befunds: Die Statusanzeige hing UNTER dem
  // Eingabefeld im Textfluss. Damit wurde das Feld höher als die Knöpfe
  // daneben — und weil die Leiste an der Unterkante ausrichtet, saßen die
  // Knöpfe gegenüber der Eingabe versetzt.
  it("die Statusanzeige zählt nicht zur Feldhöhe", () => {
    const rule = /\.cpdlc-station-badge\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(
      rule![1],
      "aus dem Textfluss genommen, sonst wächst das Feld über die Eingabe hinaus",
    ).toContain("position: absolute");
  });

  it("das Feld ist Bezugspunkt für die Anzeige", () => {
    const rule = /\.cpdlc-field\s*\{([^}]*)\}/.exec(css);
    expect(rule![1], "ohne relative Positionierung bezieht sich absolut auf die Seite").toContain(
      "position: relative",
    );
  });
});
