// Der Hinweis auf der Karte muss SICHTBAR sein.
//
// Thomas, 20.09.2026: „die Meldung liegt hinter dem Band Track Vatsim und
// so weiter". Der Hinweis „Für diesen Flug liegt keine Route vor" stand
// auf `top: 12` mit `zIndex: 5` — die Kartenschalter-Leiste steht auf
// `top: 16` mit `z-index: 500` (App.css). Der Hinweis lag also sowohl
// örtlich als auch in der Stapelung darunter und war nie zu sehen.
//
// Fuer den Piloten sah das aus, als täte der Klick auf einen Kollegen
// nichts — dabei stand der Grund verdeckt darunter.
//
// Dieser Test vergleicht die ZAHLEN aus beiden Dateien miteinander,
// statt nur nachzusehen, ob irgendwo ein z-index steht. Zieht jemand die
// Leiste hoch, wird er rot.

import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const tsx = readFileSync(
  resolve(__dirname, "LiveMapView.tsx"),
  "utf8",
);
const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf8");

/** Groesster z-index, den ein Element der Kartenschicht traegt. */
function hoechsteEbeneDerKarte(): number {
  const werte: number[] = [];
  for (const klasse of [".aa-livemap-controls", ".aa-livemap-panel"]) {
    const start = css.indexOf(`${klasse} {`);
    expect(start, `${klasse} nicht in App.css gefunden`).toBeGreaterThan(-1);
    const block = css.slice(start, css.indexOf("}", start));
    const treffer = block.match(/z-index:\s*(\d+)/);
    expect(treffer, `${klasse} ohne z-index`).not.toBeNull();
    werte.push(Number(treffer![1]));
  }
  return Math.max(...werte);
}

/** Der Inline-Stil des Hinweis-Bandes. */
function hinweisStil(): { top: number; zIndex: number } {
  const anker = tsx.indexOf("{fremdeRouteHinweis && (");
  expect(anker, "Hinweis-Band nicht gefunden").toBeGreaterThan(-1);
  // Kommentare RAUS, bevor gesucht wird: Der Block erklaert den alten
  // Zustand („Bei `top: 12` + `zIndex: 5` lag der Hinweis hinter der
  // Leiste") — ein Regex ueber den Rohtext liest genau diese Zahlen und
  // meldet einen Fehler, den es nicht gibt. Beim ersten Lauf ist mir das
  // passiert.
  const block = tsx
    .slice(anker, anker + 1900)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
  const top = block.match(/top:\s*(\d+)/);
  const z = block.match(/zIndex:\s*(\d+)/);
  expect(top, "kein top im Hinweis-Band").not.toBeNull();
  expect(z, "kein zIndex im Hinweis-Band").not.toBeNull();
  return { top: Number(top![1]), zIndex: Number(z![1]) };
}

describe("Hinweis auf der Live-Karte", () => {
  it("liegt ueber der Kartenschalter-Leiste, nicht darunter", () => {
    const { zIndex } = hinweisStil();
    expect(zIndex).toBeGreaterThan(hoechsteEbeneDerKarte());
  });

  it("beginnt unterhalb der Leiste, statt sie zu ueberdecken", () => {
    // Die Leiste sitzt auf top:16 und ist rund 60px hoch (zwei Zeilen
    // Schalter mit Rubrik). Der Hinweis darf erst danach anfangen —
    // sonst sitzt er auf ihr, auch wenn er obenauf liegt.
    const { top } = hinweisStil();
    const leisteStart = Number(
      css
        .slice(css.indexOf(".aa-livemap-controls {"))
        .match(/top:\s*(\d+)/)![1],
    );
    expect(top).toBeGreaterThan(leisteStart + 60);
  });
});
