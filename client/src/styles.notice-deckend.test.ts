// Ein schwebender Hinweis muss eine DECKENDE Fläche haben.
//
// Feldbefund Thomas, 23.09.2026 (GAF 9655): Im hellen Design stand die
// rote Integritäts-Meldung über der Buchungsliste — und weil die
// Ton-Hintergründe mit 9–12 % Deckkraft gebaut sind (richtig für einen
// Hinweis IM Textfluss), las man Meldung und Seite übereinander. Der
// Screenshot zeigt es: „Gebuchte Flüge" scheint mitten durch den Kasten.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const css = readFileSync(resolve(__dirname, "components/ui/ui.css"), "utf-8");

function regel(name: string): string {
  const start = css.indexOf(name + " {");
  expect(start, `${name} nicht gefunden — Test anpassen, nicht löschen`).toBeGreaterThan(-1);
  const ende = css.indexOf("}", start);
  return css.slice(start, ende);
}

describe("Schwebender Hinweis", () => {
  it("legt den Ton auf eine deckende Fläche, nicht auf die Seite darunter", () => {
    const block = regel(".ui-notice--floating");
    expect(block).toContain("background-color: var(--surface)");
    expect(block).toContain("background-image: linear-gradient(");
  });

  it("nimmt dafür den Ton der jeweiligen Stufe, nicht eine feste Farbe", () => {
    // Ohne die Variable wäre jeder schwebende Hinweis gleich getönt —
    // die rote Meldung sähe aus wie die blaue.
    expect(regel(".ui-notice--floating")).toContain("var(--notice-ton-bg)");
    for (const stufe of [".ui-notice--warn", ".ui-notice--error", ".ui-notice--success"]) {
      expect(regel(stufe), stufe).toContain("--notice-ton-bg:");
    }
  });
});
