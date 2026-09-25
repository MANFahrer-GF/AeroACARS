import { describe, expect, it } from "vitest";
import de from "../../locales/de/common.json";
import en from "../../locales/en/common.json";
import it_ from "../../locales/it/common.json";
import katalogJson from "./vorschauKatalog.json";
import { ereignisseAus } from "./ereignisse";
import { csvText } from "./csv";
import type { Frame, Katalog } from "./typen";
import type { Telemetrie } from "./useTelemetrie";

const katalog = katalogJson as Katalog;

describe("Telemetrie-Katalog und Texte", () => {
  const sprachen = { de, en, it: it_ } as Record<string, { telemetrie: { kanal: Record<string, string> } }>;
  for (const [sp, datei] of Object.entries(sprachen)) {
    it(`jeder Kanal hat einen Text auf ${sp}`, () => {
      const fehlend = [...katalog.zahlen, ...katalog.texte]
        .map((k) => k.id)
        .filter((id) => !datei.telemetrie.kanal[id]);
      expect(fehlend).toEqual([]);
    });
  }
});

function index(ids: string[]): Map<string, number> {
  return new Map(ids.map((id, i) => [id, i]));
}
function f(t: number, z: (number | null)[]): Frame {
  return { t, z, s: [] };
}

describe("ereignisseAus", () => {
  const idx = index(["am_boden", "vs", "g", "fahrwerk", "klappen_stufe", "ap", "ias"]);

  it("meldet das Aufsetzen mit der V/S vor dem Bodenkontakt", () => {
    const a = f(1, [0, -180, 1.0, 100, 4, 0, 130]);
    const b = f(2, [1, -20, 1.24, 100, 4, 0, 129]);
    const e = ereignisseAus(a, b, idx);
    expect(e).toHaveLength(1);
    expect(e[0]).toMatchObject({ art: "aufsetzen", wichtig: true, werte: { vs: -180, g: "1.24" } });
  });

  it("Fahrwerk, Klappen und Autopilot", () => {
    const a = f(1, [0, -700, 1, 60, 3, 1, 140]);
    const b = f(2, [0, -700, 1, 100, 4, 0, 140]);
    const arten = ereignisseAus(a, b, idx).map((e) => e.art);
    expect(arten).toEqual(["fahrwerk_unten", "klappen", "ap_aus"]);
  });

  it("fehlende Werte erzeugen keine Ereignisse", () => {
    const a = f(1, [null, null, null, null, null, null, null]);
    const b = f(2, [1, -100, 1.2, 100, 4, 1, 130]);
    expect(ereignisseAus(a, b, idx)).toEqual([]);
    expect(ereignisseAus(null, b, idx)).toEqual([]);
  });
});

describe("csvText", () => {
  it("schreibt Kopf mit Einheit und eine Zeile je Frame", () => {
    const tm = {
      zahlIndex: index(["ias", "vs"]),
      textIndex: index(["fma_lateral"]),
      kanal: (id: string) =>
        ({
          ias: { id: "ias", einheit: "kt" },
          vs: { id: "vs", einheit: "fpm" },
          fma_lateral: { id: "fma_lateral", einheit: "" },
        })[id],
      frames: [
        { t: Date.UTC(2026, 8, 25, 12, 0, 0), z: [137.5, -700], s: ['L"OC'] },
        { t: Date.UTC(2026, 8, 25, 12, 0, 1), z: [null, -690], s: [null] },
      ],
    } as unknown as Telemetrie;
    const csv = csvText(tm, ["ias", "fma_lateral", "gibt_es_nicht"]).trim().split("\n");
    expect(csv[0]).toBe("zeit_utc;ias [kt];fma_lateral");
    expect(csv[1]).toBe('2026-09-25T12:00:00.000Z;137.5;"L""OC"');
    expect(csv[2]).toBe("2026-09-25T12:00:01.000Z;;");
  });
});
