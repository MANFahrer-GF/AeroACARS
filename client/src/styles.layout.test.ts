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

describe("Erreichbarkeits-Anzeige verschiebt oder verdeckt nichts (v1.3.3)", () => {
  // Dritte Runde desselben Grundproblems (Feldbefund 31.07.2026, PDC + CPDLC):
  // die vorherige Lösung (siehe git history dieser Datei) nahm die Anzeige
  // per `position: absolute` aus dem Textfluss, damit sie das Feld nicht
  // höher macht. Das tauschte einen Bug gegen einen schlimmeren: aus dem
  // Fluss genommen beanspruchte sie GAR keinen Platz mehr und rendert dann
  // einfach UNTER dem, was zufällig als Nächstes im normalen Fluss kam — im
  // CPDLC-Tab je nach Anmeldestatus mal über, mal unter den Buttons, im
  // PDC-Formular über der nächsten Feldzeile. `position: absolute` auf
  // `.cpdlc-station-badge` ist daher jetzt selbst das Regressionssignal,
  // nicht mehr die Lösung.
  it("die Statusanzeige ist NICHT aus dem Textfluss genommen", () => {
    const rule = /\.cpdlc-station-badge\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(
      rule![1],
      "absolute Positionierung reserviert keinen Platz mehr — die Anzeige kann " +
        "dann unter Buttons oder der nächsten Formularzeile landen (Befund 31.07.2026)",
    ).not.toContain("position: absolute");
  });

  // CPDLC: Felder (Link-Status + Center-Eingabe) und Aktions-Buttons stehen
  // in getrennten Zeilen. Vorher standen sie in EINER `flex-wrap`-Zeile mit
  // `align-items: flex-end` — je nachdem wie lang der Anmelde-Status-Text
  // war (bzw. ob die Statusanzeige Platz brauchte), brach die Zeile an
  // anderer Stelle um und die Buttons sprangen zwischen "neben dem Feld"
  // und "darunter". Getrennte Zeilen heißt: das Feld kann wachsen oder
  // schrumpfen, ohne dass es die Buttons je bewegt.
  it("CPDLC: Felder und Buttons stehen in unabhängigen Zeilen", () => {
    const rule = /\.cpdlc-section__bar\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(
      rule![1],
      "eine wrappende Ein-Zeilen-Leiste lässt die Buttons je nach Feld-/Status-Breite springen",
    ).toContain("flex-direction: column");
  });

  // PDC: die Anzeige lebt als eigene, volle Raster-Zeile — nicht mehr
  // INNERHALB des Delivery-Feldes. Sonst wird nur Delivery höher als
  // Flugzeugtyp daneben, und die beiden Eingaben liegen nicht mehr auf
  // gleicher Höhe (Befund 31.07.2026: "DELIVERY wandert nach unten").
  it("PDC: die Anzeige spannt die volle Rasterbreite, statt in einem Feld zu stecken", () => {
    const rule = /\.cpdlc-field__badge-row\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toContain("grid-column: 1 / -1");
  });

  // Ohne `min-width: 0` setzt der ungebrochene Inhalt eines Feldes (Label +
  // Lücke + langer Status-Text) eine Mindestbreite, bevor der Browser
  // überhaupt ans Umbrechen denkt — bei `auto-fit`-Rastern kann das dazu
  // führen, dass gar nicht mehr zwei Spalten nebeneinanderpassen und das
  // ganze Formular auf eine Spalte kollabiert (derselbe Befund).
  it("Felder dürfen unter ihre Inhaltsbreite schrumpfen (kein Grid-Kollaps)", () => {
    const rule = /\.cpdlc-field\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toContain("min-width: 0");
  });
});
