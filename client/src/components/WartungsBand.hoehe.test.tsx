// Feldbefund Thomas (02.11.2026): „Wartungsfenster zu klein, mal passt es
// mal nicht.“ Ein langer Wartungstext lief oben und unten aus dem Kasten.
//
// Ursache: `.app` ist eine Flex-Spalte mit fester Höhe (Grid in `.shell`).
// Ist der Seiteninhalt länger als das Fenster, schrumpfen die Kinder — und
// `.ui-notice { min-height: 34px }` hebt die Inhalts-Untergrenze auf. Im
// Browser gemessen (1490×720 … 420 breit): Kasten immer 34 px, Text 39–956 px.
//
// jsdom rechnet kein Layout. Geprüft wird deshalb der berechnete Stil mit dem
// ECHTEN CSS (App.css samt ui.css) am echten Band in `main.app` — fällt die
// Regel weg oder verliert sie gegen eine spätere, wird der Test rot.
import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { WartungsBand } from "./WartungsBand";

const SRC = resolve(__dirname, "..");
const CSS =
  readFileSync(resolve(SRC, "components/ui/ui.css"), "utf-8") +
  "\n" +
  readFileSync(resolve(SRC, "App.css"), "utf-8").replace(/@import "[^"]+";/g, "");

const LANG =
  "Der Anbieter wartet den Sprachserver (Speicher-Aufrüstung). Zwischen 16 und 24 Uhr ist er " +
  "rund 20–30 Minuten nicht erreichbar, dazu kurze Netzabbrüche.";

afterEach(() => {
  vi.unstubAllGlobals();
  document.head.querySelectorAll("style[data-test]").forEach((s) => s.remove());
});

describe("Wartungsband in der App-Spalte", () => {
  it("schrumpft nicht unter seinen Text", async () => {
    const stil = document.createElement("style");
    stil.dataset.test = "";
    stil.textContent = CSS;
    document.head.appendChild(stil);
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          version: 1, erzeugt: new Date().toISOString(), gesamt: "ok", dienste: [], stoerungen: [], verlauf: {},
          wartung: [{
            id: 1, dienste: ["tts"], notfall: false, aktiv: false,
            von: new Date(Date.now() + 864e5).toISOString(),
            bis: new Date(Date.now() + 2 * 864e5).toISOString(),
            text: { de: LANG, en: LANG, it: LANG },
          }],
        }),
      }),
    );
    render(
      <main className="app">
        <WartungsBand />
      </main>,
    );
    const band = await screen.findByRole("status");
    expect(band.className).toContain("ui-notice");
    // Gegenprobe: ohne die Regel ist es "1" (Vorgabe für Flex-Kinder).
    expect(getComputedStyle(band).flexShrink).toBe("0");
  });
});
