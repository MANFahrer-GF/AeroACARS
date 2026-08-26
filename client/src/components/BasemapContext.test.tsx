import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { ausAntwort, EINGEBAUTE_GRUNDLAGE } from "./BasemapContext";

/**
 * Der Schlüssel wirkt nur, wenn KEINE Karte an ihm vorbei zeichnet.
 *
 * # Warum das eine Prüfung braucht
 *
 * CARTO verlangt seit dem 26.08.2026 einen Schlüssel. Er liegt auf dem
 * Server, damit ein Wechsel kein Release kostet — aber das gilt nur,
 * solange jede Karte ihn auch benutzt. Bleibt irgendwo eine feste
 * `cartocdn`-Adresse stehen, hängt sie am schlüssellosen Weg und fällt
 * genau dann aus, wenn CARTO umstellt.
 *
 * Das ist dieselbe Klasse, die uns beim Messfenster viermal getroffen
 * hat: ein Feld, das an einer Stelle vergessen wird und lautlos einen
 * Ersatzwert benutzt.
 */
describe("Kartengrundlage", () => {
  afterEach(() => vi.restoreAllMocks());

  it("keine Karte trägt eine feste CARTO-Adresse", () => {
    const ordner = resolve(__dirname);
    const befunde: string[] = [];
    for (const datei of readdirSync(ordner)) {
      if (!/\.tsx?$/.test(datei)) continue;
      // Hier GEHÖREN die Adressen hin — als Rückfall ohne Netz.
      if (datei.startsWith("BasemapContext")) continue;
      const text = readFileSync(resolve(ordner, datei), "utf-8");
      for (const [i, zeile] of text.split("\n").entries()) {
        // Kommentare dürfen den Namen nennen, Code nicht.
        const roh = zeile.trim();
        if (roh.startsWith("//") || roh.startsWith("*") || roh.startsWith("/*")) continue;
        if (/cartocdn/.test(zeile)) {
          befunde.push(`${datei}:${i + 1} — ${roh.slice(0, 80)}`);
        }
      }
    }
    expect(
      befunde,
      `Diese Stellen zeichnen an der Server-Konfiguration vorbei und ` +
        `bekämen nie einen Schlüssel:\n${befunde.join("\n")}`,
    ).toEqual([]);
  });

  it("füllt jedes fehlende Feld einzeln auf", () => {
    // Eine halb gefüllte Antwort darf nicht dazu führen, dass die Karte
    // gar keinen Stil mehr hat — und sie darf auch nicht dazu führen,
    // dass ein vorhandener Wert vom Ersatz überschrieben wird.
    const teil = ausAntwort({ dunkel: "https://x/dark.json?key=abc" });
    expect(teil.dunkel).toBe("https://x/dark.json?key=abc");
    expect(teil.hell).toBe(EINGEBAUTE_GRUNDLAGE.hell);
    expect(teil.glyphen).toBe(EINGEBAUTE_GRUNDLAGE.glyphen);
    expect(teil.nennung).toBe(EINGEBAUTE_GRUNDLAGE.nennung);
  });

  it("nimmt Unsinn nicht als Adresse", () => {
    // Leere Zeichenketten, Zahlen, null — alles darf die Karte nicht
    // blind machen. Sonst reicht ein Tippfehler im Admin, um jedem
    // Piloten die Karte zu nehmen.
    for (const unsinn of [{}, null, undefined, { dunkel: "" }, { dunkel: "   " }, { dunkel: 42 }]) {
      const g = ausAntwort(unsinn);
      expect(g.dunkel).toBe(EINGEBAUTE_GRUNDLAGE.dunkel);
      expect(g.hell).toBe(EINGEBAUTE_GRUNDLAGE.hell);
    }
  });

  it("die eingebauten Adressen sind die, die heute laufen", () => {
    // Ohne hinterlegten Schlüssel muss der Client aussehen wie vorher.
    // Diese Zeile hält fest, was „vorher" heisst.
    expect(EINGEBAUTE_GRUNDLAGE.dunkel).toBe(
      "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json",
    );
    expect(EINGEBAUTE_GRUNDLAGE.hell).toBe(
      "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json",
    );
    // Und die Nennung, die CARTO als Bedingung nennt.
    expect(EINGEBAUTE_GRUNDLAGE.nennung).toMatch(/CARTO/);
    expect(EINGEBAUTE_GRUNDLAGE.nennung).toMatch(/OpenStreetMap/);
  });
});
