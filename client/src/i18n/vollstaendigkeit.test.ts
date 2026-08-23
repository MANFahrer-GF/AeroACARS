// Jede Beschriftung muss in allen drei Sprachen stehen.
//
// # Warum das ein Test ist
//
// `t("runway_v2.foo", { defaultValue: "Bar" })` funktioniert auch dann, wenn
// der Schlüssel in keiner Sprachdatei steht — es kommt „Bar" heraus, auf
// Deutsch, in jeder Sprache, und niemandem fällt es auf. Genau das ist beim
// Bau der Bahndisziplin-Anzeige passiert: achtzehn neue Schlüssel, keiner
// davon in `de/en/it`, alle über `defaultValue` bedient.
//
// Der Fehler zeigt sich erst bei einem Nutzer, der Englisch oder Italienisch
// eingestellt hat — und dort als deutscher Text mitten in der Oberfläche.
//
// Dieselbe Fehlerklasse hat dieses Projekt schon einmal getroffen: Ein
// `??`-Rückfall behauptete einen Zustand, den es nicht gab, weil der neue
// Zustand keinen Beschriftungs-Eintrag hatte.

import { describe, expect, it } from "vitest";
// `with { type: "json" }` waere sauberer, aber die Vitest-Version hier
// liefert JSON-Importe als Modulobjekt mit `default`. Beides abfangen.
import deRaw from "../locales/de/common.json";
import enRaw from "../locales/en/common.json";
import itRaw from "../locales/it/common.json";

const auspacken = (m: unknown): Record<string, unknown> =>
  (m as { default?: Record<string, unknown> })?.default ??
  (m as Record<string, unknown>);
// NICHT `it` nennen -- das ist der Name der Testfunktion von vitest, und
// eine Konstante gleichen Namens im Modulbereich schattet sie: Die Datei
// scheitert dann mit „it is not a function", noch bevor ein Test laeuft.
const de = auspacken(deRaw);
const en = auspacken(enRaw);
const itIT = auspacken(itRaw);

/** Alle Schlüsselpfade eines verschachtelten Objekts, punktgetrennt. */
function pfade(o: unknown, praefix = ""): string[] {
  if (typeof o !== "object" || o === null) return [praefix];
  return Object.entries(o as Record<string, unknown>).flatMap(([k, v]) =>
    pfade(v, praefix ? `${praefix}.${k}` : k),
  );
}

describe("Sprachdateien", () => {
  it("führt jeden deutschen Schlüssel auch in en und it", () => {
    const deP = new Set(pfade(de));
    for (const [name, andere] of [
      ["en", new Set(pfade(en))],
      ["it", new Set(pfade(itIT))],
    ] as const) {
      const fehlend = [...deP].filter((k) => !andere.has(k)).sort();
      expect(
        fehlend,
        `${fehlend.length} Schlüssel fehlen in ${name}: ${fehlend.slice(0, 12).join(", ")}`,
      ).toEqual([]);
    }
  });

  it("führt jeden englischen Schlüssel auch in de", () => {
    // Beide Richtungen: Ein Schlüssel, der nur in en steht, fehlt auf
    // Deutsch — und Deutsch ist die Sprache der meisten Piloten hier.
    const enP = new Set(pfade(en));
    const deP = new Set(pfade(de));
    const fehlend = [...enP].filter((k) => !deP.has(k)).sort();
    expect(
      fehlend,
      `${fehlend.length} Schlüssel fehlen in de: ${fehlend.slice(0, 12).join(", ")}`,
    ).toEqual([]);
  });

  it("lässt keine Beschriftung nur in EINER Sprache leer", () => {
    // Ein leerer String ist manchmal Absicht: Bei „AIRAC" ist die Abkürzung
    // der Begriff, eine Langform gibt es nicht — und das gilt in allen drei
    // Sprachen gleichermassen.
    //
    // Ein Fehler ist es erst, wenn eine Sprache etwas hat und die andere
    // nicht. Dann steht in der einen Oberfläche ein Text und in der anderen
    // eine Lücke, ohne dass es jemandem auffällt, der nur eine davon nutzt.
    const lies = (datei: Record<string, unknown>, pfad: string): unknown => {
      let v: unknown = datei;
      for (const teil of pfad.split(".")) v = (v as Record<string, unknown>)?.[teil];
      return v;
    };
    const befunde: string[] = [];
    for (const pfad of pfade(de)) {
      const werte = ([["de", de], ["en", en], ["it", itIT]] as const)
        .map(([name, d]) => [name, lies(d, pfad)] as const)
        .filter(([, v]) => typeof v === "string") as Array<readonly [string, string]>;
      if (werte.length < 2) continue;
      const gefuellt = werte.filter(([, v]) => v.trim() !== "").map(([n]) => n);
      const leer = werte.filter(([, v]) => v.trim() === "").map(([n]) => n);
      if (gefuellt.length > 0 && leer.length > 0) {
        befunde.push(`${pfad}: gefüllt in ${gefuellt.join("/")}, leer in ${leer.join("/")}`);
      }
    }
    expect(befunde, befunde.slice(0, 10).join("\n")).toEqual([]);
  });

  it("hält die Platzhalter über alle Sprachen gleich", () => {
    // `{{m}}` in de, aber `{{meters}}` in en — dann steht in der englischen
    // Oberfläche die geschweifte Klammer statt der Zahl.
    const platzhalter = (s: string) =>
      [...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]!).sort();
    const lies = (datei: unknown, pfad: string): unknown => {
      let v: unknown = datei;
      for (const teil of pfad.split(".")) v = (v as Record<string, unknown>)?.[teil];
      return v;
    };
    for (const pfad of pfade(de)) {
      const dv = lies(de, pfad);
      if (typeof dv !== "string") continue;
      for (const [name, datei] of [
        ["en", en],
        ["it", itIT],
      ] as const) {
        const av = lies(datei, pfad);
        if (typeof av !== "string") continue;
        expect(
          platzhalter(av),
          `${pfad}: Platzhalter in ${name} weichen von de ab`,
        ).toEqual(platzhalter(dv));
      }
    }
  });
});
