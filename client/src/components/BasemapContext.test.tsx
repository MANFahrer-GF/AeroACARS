import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { ausAntwort, EINGEBAUTE_GRUNDLAGE, kartenAnfrage } from "./BasemapContext";

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

/**
 * Der Schlüssel muss JEDE Anfrage erreichen, nicht nur die Stil-Adresse.
 *
 * # Der Befund
 *
 * Mein erster Bau hängte den Schlüssel an die `style.json`. Das reicht
 * nicht, und zwar nachweislich — die Datei verweist selbst weiter:
 *
 * ```text
 * glyphs:  tiles.basemaps.cartocdn.com/fonts/{fontstack}/{range}.pbf
 * sprite:  tiles.basemaps.cartocdn.com/gl/dark-matter-gl-style/sprite
 * source:  tiles.basemaps.cartocdn.com/vector/carto.streets/v1/tiles.json
 * ```
 *
 * Und die `tiles.json` verweist wiederum auf die Kachel-Adressen.
 * Ausgerechnet die Kacheln, die CARTO zählt, wären ohne Schlüssel
 * gelaufen — und der Fehler wäre erst aufgefallen, wenn CARTO abstellt.
 *
 * Thomas hat es gefunden, indem er das Beispiel aus der CARTO-Anleitung
 * danebengelegt hat: Dort steht der Schlüssel an der KACHEL-Adresse.
 */
describe("Schlüssel an jeder Anfrage", () => {
  const ADRESSEN = [
    "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json",
    "https://tiles.basemaps.cartocdn.com/fonts/Noto%20Sans%20Regular/0-255.pbf",
    "https://tiles.basemaps.cartocdn.com/gl/dark-matter-gl-style/sprite.json",
    "https://tiles.basemaps.cartocdn.com/vector/carto.streets/v1/tiles.json",
    "https://a.basemaps.cartocdn.com/vector/carto.streets/v1/8/134/86.mvt",
  ];

  it("hängt ihn an alles, was zu CARTO geht", () => {
    const t = kartenAnfrage("GEHEIM123");
    expect(t, "ohne Umschreiber wäre der Schlüssel wirkungslos").toBeTruthy();
    for (const url of ADRESSEN) {
      const r = t!(url);
      expect(r?.url, `${url} bekommt keinen Schlüssel`).toContain("key=GEHEIM123");
    }
  });

  it("lässt fremde Adressen unangetastet", () => {
    // Esri-Kacheln des Satellitenstils, unsere eigenen Endpunkte, alles
    // andere: Dort hat der Schlüssel nichts zu suchen.
    const t = kartenAnfrage("GEHEIM123")!;
    for (const url of [
      "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/5/6/7",
      "https://live.kant.ovh/api/basemap",
      "https://german-sky-group.eu/etwas",
    ]) {
      expect(t(url), `${url} wurde angefasst`).toBeUndefined();
    }
  });

  it("hängt ihn nicht doppelt an", () => {
    // MapLibre reicht Adressen aus Antworten erneut durch diesen Weg.
    // Zweimal `?key=` wäre eine kaputte Anfrage.
    const t = kartenAnfrage("GEHEIM123")!;
    const einmal = t(ADRESSEN[0]!)!.url;
    const zweimal = t(einmal)!.url;
    expect(zweimal).toBe(einmal);
    expect(zweimal.match(/key=/g)?.length).toBe(1);
  });

  it("achtet auf vorhandene Fragezeichen", () => {
    const t = kartenAnfrage("A B&C")!;
    const r = t("https://tiles.basemaps.cartocdn.com/x?v=2")!.url;
    expect(r).toContain("?v=2&key=");
    // Und kodiert, sonst zerlegt ein Sonderzeichen die Adresse.
    expect(r).toContain("key=A%20B%26C");
  });

  it("ohne Schlüssel wird gar nichts umgeschrieben", () => {
    // Der heutige Zustand: kein Schlüssel hinterlegt, alles läuft wie
    // bisher. Ein Umschreiber, der leere Schlüssel anhängt, würde jede
    // Anfrage kaputtmachen.
    for (const leer of ["", "   "]) {
      expect(kartenAnfrage(leer)).toBeUndefined();
    }
  });
});

/**
 * Beide Karten müssen den Umschreiber auch WIRKLICH benutzen.
 *
 * Die Funktion zu bauen und sie nicht anzuhängen wäre genau die Klasse,
 * die uns beim Messfenster viermal getroffen hat: berechnet, getestet,
 * und auf dem Weg zur Anzeige fallengelassen. Ein Test der Funktion
 * allein bliebe dabei grün.
 */
describe("Verdrahtung der Karten", () => {
  const KARTEN = ["LiveMapView.tsx", "LogbookView.tsx"];

  for (const datei of KARTEN) {
    it(`${datei} hängt den Umschreiber an`, async () => {
      const { readFileSync } = await import("node:fs");
      const { resolve } = await import("node:path");
      const quelle = readFileSync(resolve(__dirname, datei), "utf-8");
      expect(
        /transformRequest:\s*kartenAnfrage\(/.test(quelle),
        `${datei} baut eine Karte ohne \`transformRequest\` — der ` +
          `Schlüssel erreicht dort weder Kacheln noch Schriften.`,
      ).toBe(true);
      // Und die Grundlage muss aus dem Zusammenhang kommen, nicht aus
      // einer eigenen Konstante.
      expect(
        /useKartengrundlage\(\)/.test(quelle),
        `${datei} holt die Grundlage nicht vom Server`,
      ).toBe(true);
    });
  }

  it("jede erzeugte Karte bekommt einen Umschreiber", async () => {
    // Nicht nur „irgendwo im File", sondern: So viele `new maplibregl.Map`
    // wie `transformRequest`. Eine zweite Karte ohne Umschreiber wäre
    // sonst unsichtbar.
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const befunde: string[] = [];
    for (const datei of KARTEN) {
      const q = readFileSync(resolve(__dirname, datei), "utf-8");
      const karten = (q.match(/new maplibregl\.Map\(/g) ?? []).length;
      const umschreiber = (q.match(/transformRequest:/g) ?? []).length;
      if (karten !== umschreiber) {
        befunde.push(`${datei}: ${karten} Karte(n), ${umschreiber} Umschreiber`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});
