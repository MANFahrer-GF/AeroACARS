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

function cssBlock(klasse: string): string {
  const start = css.indexOf(`${klasse} {`);
  expect(start, `${klasse} nicht in App.css gefunden`).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

describe("Hinweis auf der Live-Karte", () => {
  it("steht im Raster unter der Leiste statt mit festen Zahlen darueber", () => {
    // Mit `top: 92` lag der Hinweis bei umbrochener Leiste auf ihr (Thomas, 20.09.2026).
    expect(cssBlock(".aa-livemap-controls")).toMatch(/grid-row:\s*1/);
    expect(cssBlock(".aa-livemap-hinweis")).toMatch(/grid-row:\s*2/);
    expect(tsx).toMatch(/className="aa-livemap-hinweis aa-livemap-overlay"/);
  });

  it("nimmt die Karte nicht als Klickflaeche in Beschlag", () => {
    expect(cssBlock(".aa-livemap-ui")).toMatch(/pointer-events:\s*none/);
    expect(cssBlock(".aa-livemap-ui > *")).toMatch(/pointer-events:\s*auto/);
  });

  it("kommt ohne gemessene Oberkante aus", () => {
    expect(tsx).not.toContain("--aa-panel-oben");
    expect(css).not.toContain("--aa-panel-oben");
  });
});
