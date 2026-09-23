// Ein schwebender Hinweis muss eine DECKENDE Fläche haben.
//
// Feldbefund Thomas, 23.09.2026 (GAF 9655): Im hellen Design stand die
// rote Integritäts-Meldung über der Buchungsliste — und weil die
// Ton-Hintergründe mit 9–12 % Deckkraft gebaut sind (richtig für einen
// Hinweis IM Textfluss), las man Meldung und Seite übereinander.
//
// ⚠ Dieser Test prüfte zuerst nur, dass die zwei Zeilen im Block STEHEN.
// Die Cloud-QS hat im Browser gezeigt, dass das nicht reicht: Eine
// Stufen-Regel mit der Kurzform `background:` NACH `--floating` gewinnt
// (gleiche Spezifität, spätere Regel) und setzt `background-image` wieder
// auf none — der Fehler wäre zurück, der Test grün. Deshalb prüft er
// jetzt auch die Reihenfolge und verbietet die Kurzform danach.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const css = readFileSync(resolve(__dirname, "components/ui/ui.css"), "utf-8");

function index(name: string): number {
  const i = css.indexOf(name + " {");
  expect(i, `${name} nicht gefunden — Test anpassen, nicht löschen`).toBeGreaterThan(-1);
  return i;
}
function regel(name: string): string {
  const start = index(name);
  return css.slice(start, css.indexOf("}", start));
}

/** Jede Stufe mit dem Ton, den sie tragen soll. */
const STUFEN: [string, string][] = [
  [".ui-notice", "var(--sem-info-bg)"],
  [".ui-notice--warn", "var(--sem-warn-bg)"],
  [".ui-notice--error", "var(--sem-danger-bg)"],
  [".ui-notice--success", "var(--sem-ok-bg)"],
];

describe("Schwebender Hinweis", () => {
  it("legt den Ton auf eine deckende Fläche, nicht auf die Seite darunter", () => {
    const block = regel(".ui-notice--floating");
    expect(block).toContain("background-color: var(--surface)");
    expect(block).toContain("background-image: linear-gradient(var(--notice-ton-bg)");
  });

  it("nimmt den Ton der jeweiligen Stufe — mit dem richtigen Wert", () => {
    // Existenz allein genügt nicht: `--error { --notice-ton-bg: var(--sem-ok-bg) }`
    // wäre eine rote Meldung in Grün.
    for (const [stufe, ton] of STUFEN) {
      expect(regel(stufe), stufe).toContain(`--notice-ton-bg: ${ton}`);
    }
  });

  it("steht hinter allen Stufen — sonst gewinnt die Stufe", () => {
    const floating = index(".ui-notice--floating");
    for (const [stufe] of STUFEN) {
      expect(index(stufe), stufe).toBeLessThan(floating);
    }
  });

  it("lässt keine spätere Hinweis-Regel die Fläche zurücksetzen", () => {
    // Die Kurzform `background:` setzt background-image auf none. Nach
    // `--floating` darf sie in keiner `.ui-notice`-Regel mehr vorkommen.
    const danach = css.slice(index(".ui-notice--floating") + 1);
    // Nicht nur die Kurzform: auch ein nachträgliches `background-color`
    // setzte die deckende Fläche zurück (Cloud-QS 23.09.2026, dritte Runde).
    const treffer = [...danach.matchAll(/\.ui-notice[^{}]*\{[^}]*\}/g)].filter((m) =>
      /\n\s*background(-color)?:\s/.test(m[0]),
    );
    expect(treffer.map((t) => t[0].slice(0, 60))).toEqual([]);
  });
});
