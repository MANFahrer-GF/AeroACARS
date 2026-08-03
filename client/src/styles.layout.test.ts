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
  // v1.3.5 (#Datalink-3a) rebuilt the PDC/CPDLC tab from the ground up —
  // `.cpdlc-station-badge` (the "is anyone registered under this
  // callsign?" ping feature) and `.cpdlc-field__badge-row` (the grid hack
  // that made room for it) are both gone outright: the new spec is
  // explicit that the app must not fake a station list or do network
  // lookups for one (handoff_datalink_3a/README.md §3.2). Nothing takes
  // their place, so the two regression guards that pinned their CSS are
  // retired along with the feature rather than rewritten.

  // CPDLC: fields/status and action buttons still must not be able to
  // jump around based on content width — now guaranteed structurally
  // instead of via flex-direction: the status row is a fixed 66px,
  // non-wrapping line (README §2/AC #8), so there is no wrap point left
  // for a button to land on "below the field" at all.
  it("die Statuszeile ist fix und bricht nicht um — Buttons können nicht springen", () => {
    const rule = /\.datalink-status\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toMatch(/flex:\s*0 0 66px/);
    expect(rule![1], "eine wrappende Zeile lässt Inhalte je nach Breite umbrechen").not.toContain("flex-wrap: wrap");
  });

  // Ohne `min-width: 0` setzt der ungebrochene Inhalt eines Feldes (Label +
  // Lücke + langer Status-Text) eine Mindestbreite, bevor der Browser
  // überhaupt ans Umbrechen denkt — bei Rastern kann das dazu führen, dass
  // gar nicht mehr mehrere Spalten nebeneinanderpassen und das ganze
  // Formular auf eine Spalte kollabiert (derselbe Grundbefund, jetzt an
  // `.datalink-field`, dem Nachfolger von `.cpdlc-field`).
  it("Felder dürfen unter ihre Inhaltsbreite schrumpfen (kein Grid-Kollaps)", () => {
    const rule = /\.datalink-field\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toContain("min-width: 0");
  });

  // Feldbefund 03.08.2026: "Ändern" wurde über "BTI4TK" gemalt, dann
  // "Anmelden" über "Letzte Abfrage" — das GEGENTEIL des Grid-Kollaps-Falls
  // oben. `.datalink-field` (das Composer-Formularraster) darf schrumpfen,
  // weil ein Label dort umbricht statt sich zu überlagern. `.datalink-block`
  // (Statuszeile: CALLSIGN/CPDLC-LOGON) enthält einzeiligen, nicht
  // umbrechenden Text — lässt man den Block trotzdem unter dessen
  // Textbreite schrumpfen, malt der Text einfach über das nächste Element,
  // weil er nirgendwo umbrechen kann. Diese drei Regeln müssen NIE wieder
  // erlauben, dass irgendein Glied der Kette unter seine Inhaltsbreite
  // schrumpft.
  it("die Statuszeilen-Blöcke dürfen NICHT unter ihre Textbreite schrumpfen (kein Text-Überlappen)", () => {
    const block = /\.datalink-block\s*\{([^}]*)\}/.exec(css);
    expect(block, "Regel muss existieren").toBeTruthy();
    expect(block![1], "min-width: 0 hier lässt nowrap-Text ins nächste Element malen").not.toContain(
      "min-width: 0",
    );

    const left = /\.datalink-status__left\s*\{([^}]*)\}/.exec(css);
    expect(left, "Regel muss existieren").toBeTruthy();
    expect(left![1], "flex-shrink darf hier nicht 1 sein (Default oder explizit)").toMatch(/flex:\s*0 0 auto/);

    const input = /\.datalink-block__input\s*\{([^}]*)\}/.exec(css);
    expect(input, "Regel muss existieren").toBeTruthy();
    expect(input![1], "eine feste width ohne flex-shrink:0 ist trotzdem ein Schrumpf-Ziel").toContain(
      "flex-shrink: 0",
    );
  });

  // Wenn's trotzdem nicht reicht (Fenster am dokumentierten Minimum von
  // 900px, README §1): lieber diese eine Zeile scrollt intern, als dass
  // irgendwas überlappt oder die feste 66px-Höhe bricht.
  it("die Statuszeile scrollt bei echtem Platzmangel, statt zu überlappen", () => {
    const rule = /\.datalink-status\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toContain("overflow-x: auto");
  });

  // Feldbefund 03.08.2026 (zweite Runde): nach einer Anmeldung wird der
  // linke Teil breiter (Wechseln+Abmelden statt Anmelden, "LOGON LÄUFT"
  // länger als "KEIN LOGON") — mit `flex: none` auf BEIDEN Seiten hatte
  // "Letzte Abfrage" keinen Platz mehr und wurde von space-between hinter
  // den rechten Rand geschoben: unsichtbar, ohne erkennbaren Scrollbalken
  // ("rutscht wieder alles nach außerhalb"). Die Uhr ist das am ehesten
  // entbehrliche Element hier — sie muss selbst schrumpfen/kürzen, damit
  // die Buttons links davon nie deswegen aus dem Fenster wandern.
  it("Letzte Abfrage schrumpft/kürzt selbst, statt von space-between aus dem Fenster geschoben zu werden", () => {
    const rule = /\.datalink-status__poll\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1], "flex: none hier lässt space-between es hinter den Rand schieben").not.toMatch(
      /flex:\s*none/,
    );
    expect(rule![1]).toContain("min-width: 0");
    expect(rule![1]).toContain("text-overflow: ellipsis");
  });

  // Feldbefund 03.08.2026 (dritte Runde): "CPDLC-LOGON" brach in "CPDLC-"
  // / "LOGON" um, obwohl reichlich Fensterbreite frei war — der Bindestrich
  // ist für Browser ein normaler Umbruchpunkt, und diese eine Textregel im
  // ganzen Block hatte als einzige kein `white-space: nowrap`. Der dadurch
  // zu hohe Block wurde vom `overflow-y: hidden` der Zeile teilweise
  // abgeschnitten — daher wirkten Eingabefeld und Button kollabiert/schwebend.
  it("das Block-Label bricht nicht um (Bindestrich in „CPDLC-LOGON“ ist sonst ein Umbruchpunkt)", () => {
    const rule = /\.datalink-block__label\s*\{([^}]*)\}/.exec(css);
    expect(rule, "Regel muss existieren").toBeTruthy();
    expect(rule![1]).toContain("white-space: nowrap");
  });
});
