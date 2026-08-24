// Was der Pilot ausdruckt, muss stimmen — und druckbar sein.
//
// # Warum diese Datei existiert
//
// Der PDF-Bericht ist die einzige Ansicht, die das Haus verlässt: Jemand
// legt ihn zur Seite, schickt ihn weiter, sieht Wochen später hinein.
// Genau deshalb fällt hier nichts auf — niemand vergleicht einen Ausdruck
// mit dem Bildschirm daneben.
//
// Vier Befunde vom 24.08.2026, alle über Monate unbemerkt:
//
//   * Die Fußzeile behauptete Version 0.12.8, die App stand bei 1.7.0.
//     Eine getippte Konstante, fünf Versionen alt.
//   * Datum und Uhrzeit kamen aus der Sprache des Betriebssystems, nicht
//     der App: „5/13/2026, 7:42:00 PM" mitten im deutschen Bericht. Bei
//     Tagen unter 13 nicht einmal als falsch erkennbar.
//   * Zoom-Hinweis, die Knöpfe − und + und der Glossar-Knopf wurden
//     mitgedruckt. Sie hatten keinen Klassennamen und waren für jede
//     Druckregel unerreichbar.
//   * Zwei verschiedene Bahnlängen auf einer Seite — 3250 m in der
//     Kachel (baulich), 2952 m in der Grafik (nach versetzter Schwelle),
//     beide ohne Angabe, welche welche ist.

import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { LandingReport } from "./LandingPanel";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import type { LandingRecord } from "./LandingPanel";

function bericht(key: string): string {
  const o = MOCK_LANDING_OPTIONS.find((x) => x.key === key);
  if (!o) throw new Error(`Variante ${key} fehlt`);
  return renderToStaticMarkup(
    <LandingReport record={o.build() as unknown as LandingRecord} />,
  );
}

function texte(markup: string): string[] {
  return [...markup.matchAll(/>([^<>]{1,})</g)].map((m) => m[1].trim()).filter(Boolean);
}

describe("PDF-Bericht der Landeanalyse", () => {
  it("nennt die Version, die wirklich läuft", () => {
    const pkg = JSON.parse(
      readFileSync(resolve(__dirname, "..", "..", "package.json"), "utf-8"),
    ) as { version: string };
    const quelle = readFileSync(resolve(__dirname, "LandingPanel.tsx"), "utf-8");
    expect(
      /const REPORT_APP_VERSION =\s*\n?\s*typeof __APP_VERSION__/.test(quelle),
      "Die Version steht als getippte Konstante im Quelltext. Sie driftet " +
        `— das Paket ist bei ${pkg.version}. Aus __APP_VERSION__ lesen.`,
    ).toBe(true);
    // Und keine getippte Fassungsnummer daneben.
    expect(
      /REPORT_APP_VERSION = "\d+\.\d+/.test(quelle),
      "getippte Versionsnummer gefunden",
    ).toBe(false);
  });

  it("formatiert Datum und Uhrzeit in der Sprache der App", () => {
    const quelle = readFileSync(resolve(__dirname, "LandingPanel.tsx"), "utf-8");
    const nackt = [
      ...quelle.matchAll(/\.toLocale(?:Date|Time)?String\(\s*\)/g),
    ].map((m) => m[0]);
    expect(
      nackt,
      "`toLocaleString()` ohne Angabe nimmt die Sprache des Betriebssystems, " +
        "nicht die der App — ein amerikanisches Datum im deutschen Bericht.",
    ).toEqual([]);
  });

  it("druckt keine Bedienelemente aufs Papier", () => {
    const markup = bericht("d_kante");
    const zeilen = texte(markup);

    // Was auf Papier nichts zu suchen hat, erkennt man am Text.
    const bedienung = zeilen.filter(
      (z) =>
        /Mausrad|Ziehen verschiebt|Begriffe erklärt|^[−+]$/.test(z) &&
        !/^\s*$/.test(z),
    );
    // Sie DÜRFEN im Markup stehen — sie werden per CSS ausgeblendet.
    // Also prüfen wir, dass jede von ihnen unter der Druckregel liegt.
    const traeger = [
      ...markup.matchAll(/class="([^"]*bahn-nur-bildschirm[^"]*)"/g),
    ];
    expect(
      traeger.length,
      "Kein Element trägt `bahn-nur-bildschirm` — die Bedienelemente sind " +
        "inline gestylt und für das Druck-CSS unerreichbar.",
    ).toBeGreaterThanOrEqual(2);

    // Und die Regel muss es auch geben.
    const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf-8");
    const druckblock = css.slice(css.indexOf("@media print"));
    expect(
      druckblock.includes(".bahn-nur-bildschirm"),
      "Die Klasse ist gesetzt, aber @media print blendet sie nicht aus.",
    ).toBe(true);

    // Gegenprobe der Erkennung selbst: Findet der Test die Texte überhaupt?
    expect(
      bedienung.length,
      "Der Test findet gar keine Bedienelemente — dann prüft er nichts.",
    ).toBeGreaterThan(0);
  });

  /**
   * Die Beschriftung der Grafik erreicht die Untergrenze — nachgerechnet.
   *
   * Die Zahl 11 ist keine Meinung, sie folgt aus der Seitengeometrie.
   * Ändert jemand den Seitenrand, das Kartenpolster oder den viewBox,
   * stimmt sie nicht mehr — und niemand würde es merken, weil ein
   * schlecht lesbarer Ausdruck kein Fehler ist, den ein Testlauf sieht.
   */
  it("hebt die Schrift der Bahn-Grafik über die Druck-Untergrenze", () => {
    const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf-8");
    const seite = /@page\s*\{[^}]*margin:\s*([\d.]+)mm\s+([\d.]+)mm/.exec(css);
    expect(seite, "@page-Rand nicht gefunden — die Herleitung hängt daran").not.toBeNull();
    const randSeitlich = Number(seite![2]);
    const POLSTER_MM = 5; // .report-chart-card__panel padding
    const spalteMm = 210 - 2 * randSeitlich - 2 * POLSTER_MM;

    const markup = bericht("d_kante");
    const svgs = markup.match(/<svg[\s\S]*?<\/svg>/g) ?? [];
    expect(svgs.length, "keine Grafik im Bericht").toBeGreaterThan(0);

    const befunde: string[] = [];
    for (const svg of svgs) {
      const vb = /viewBox="([\d.\s-]+)"/.exec(svg);
      if (!vb) continue;
      const breite = Number(vb[1].trim().split(/\s+/)[2]);
      const ptJeEinheit = (spalteMm / breite) * (72 / 25.4);
      const groessen = [
        ...svg.matchAll(/font-size="([\d.]+)"/g),
      ].map((m) => Number(m[1]));
      if (groessen.length === 0) continue;
      const kleinste = Math.min(...groessen);
      // Die Untergrenze muss durchgeschlagen sein.
      if (kleinste < 11) {
        befunde.push(
          `kleinste Schrift ${kleinste} Einheiten — die Untergrenze aus ` +
            "`BERICHT_SCHRIFT_MINDEST` erreicht die Grafik nicht",
        );
      }
      // Und sie muss über dem Stand von vorher liegen (3,6 pt).
      const pt = kleinste * ptJeEinheit;
      if (pt < 4.3) {
        befunde.push(
          `kleinste Schrift ${pt.toFixed(1)} pt bei ${spalteMm.toFixed(0)} mm ` +
            `Spalte und viewBox ${breite} — vor der Korrektur waren es 3,6 pt`,
        );
      }
    }
    expect(befunde).toEqual([]);
  });

  it("stellt zwei Bahnlängen nicht unbeschriftet nebeneinander", () => {
    const zeilen = texte(bericht("d_kante"));
    // Alle „NNNN m"-Angaben im Bericht, die eine Bahnlänge sein könnten.
    const laengen = new Set(
      zeilen
        .map((z) => /^(\d{3,5}) m$/.exec(z)?.[1])
        .filter(Boolean)
        .map(Number)
        .filter((m) => m >= 500),
    );
    // Stehen mehrere da, muss für jede eine Beschriftung existieren.
    if (laengen.size > 1) {
      const hatLda = zeilen.some((z) => /Davon landbar|Landable|atterrabile/.test(z));
      expect(
        hatLda,
        `Der Bericht zeigt ${[...laengen].join(" m und ")} m — zwei ` +
          "verschiedene Bahnlängen ohne Angabe, welche welche ist.",
      ).toBe(true);
    }
  });
});
