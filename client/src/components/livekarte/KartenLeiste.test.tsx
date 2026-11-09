// Die Schalterleiste der Live-Karte: Bedienbarkeit (G6) und feste Knopfmasse (G1).
//
// Abnahmeliste Live-Karte, Zeilen G1 und G6 (docs/spec/livekarte-umbau-abnahme.md).
// G6 geht ueber die gerenderte Leiste. G1 ist teils eine Frage des Stils: jsdom rechnet kein CSS
// und misst keine Breite. Was die Masse betrifft, liest der Test darum die Regeln aus App.css
// (Hoehe, Breite, Umbruch) und prueft, dass die Aktiv-Regeln kein Mass anfassen. Alles andere
// (Wort in beiden Zustaenden, Zahl der Gruppen) wird am gerenderten Element gemessen.

import { describe, it, expect, beforeAll, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";
import { KartenLeiste, type KartenLeisteAngaben } from "./KartenLeiste";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});
afterEach(() => cleanup());

const nichts = () => {};
function angaben(mehr: Partial<KartenLeisteAngaben> = {}): KartenLeisteAngaben {
  return {
    satellit: false, setzeSatellit: nichts,
    kugel: false, setzeKugel: nichts,
    trackUp: false, setzeTrackUp: nichts,
    showTrack: true, umschaltenTrack: nichts,
    showTaxi: true, umschaltenTaxi: nichts, taxiPlaetze: [],
    bilderMarken: 3, bilderAn: true, umschaltenBilder: nichts,
    showVa: true, umschaltenVa: nichts,
    netz: "vatsim", setzeNetz: nichts,
    flAuto: true, umschaltenFlAuto: nichts, flWert: 200, setzeFlWert: nichts, flRegelbar: true, flAnzeige: "FL200",
    hatFlug: true, hatPosition: true, folgen: true, umschaltenFolgen: nichts, zentrieren: nichts,
    ...mehr,
  };
}
/** Alles, was die Leiste zur Bedienung anbietet, wenn jede Bedingung erfuellt ist. */
const bedienelemente = (c: HTMLElement) => [...c.querySelectorAll<HTMLElement>("button, input, select, [role=button]")];
const vollst = () => render(<KartenLeiste {...angaben()} />).container;

const ERWARTET = [
  "Karte", "Satellit", "Flache Karte", "Karte als Weltkugel", "Norden oben", "Kurs oben",
  "Track", "Taxiweg", "Bilder", "VA-Verkehr",
  "Aus", "VATSIM", "IVAO", "eigene Höhe", "Flugfläche für die Sektoransicht",
  "Auf Flug zentrieren", "Folgen",
];

describe("Tooltips, Tastatur, Screenreader (G6)", () => {
  it("bietet genau die siebzehn Bedienelemente an, jedes mit seinem Namen", () => {
    const c = vollst();
    expect(bedienelemente(c).map((e) => e.getAttribute("aria-label") ?? e.textContent)).toEqual(ERWARTET);
  });

  it("traegt an JEDEM Bedienelement ein title und ein aria-label, beide nicht leer", () => {
    const c = vollst();
    const ohneTitle = bedienelemente(c).filter((e) => !(e.getAttribute("title") ?? "").trim()).map((e) => e.getAttribute("aria-label") ?? e.textContent);
    const ohneName = bedienelemente(c).filter((e) => !(e.getAttribute("aria-label") ?? "").trim()).map((e) => e.textContent || e.tagName);
    expect(ohneTitle, "ohne title").toEqual([]);
    expect(ohneName, "ohne aria-label").toEqual([]);
  });

  it("nennt im aria-label das, was der Knopf zeigt (Netz-Segmente, Regler, Folgen, Zentrieren)", () => {
    vollst();
    for (const name of ["Aus", "VATSIM", "IVAO", "eigene Höhe", "Folgen"]) {
      const knopf = screen.getByRole("button", { name });
      expect(knopf.getAttribute("aria-label"), name).toBe(name);
      expect(knopf.textContent, name).toBe(name);
    }
    const regler = screen.getByRole("slider");
    expect(regler.getAttribute("aria-label")).toBe("Flugfläche für die Sektoransicht");
    expect(regler.getAttribute("title")).toBe("Flugfläche für die Sektoransicht");
    const mitte = screen.getByRole("button", { name: "Auf Flug zentrieren" });
    expect(mitte.getAttribute("title")).toBe("Auf Flug zentrieren");
  });

  it("zeigt den Zustand jedes Schalters als aria-pressed, nur das Zentrieren ist ein Taster", () => {
    const c = vollst();
    const ohne = bedienelemente(c)
      .filter((e) => e.tagName === "BUTTON" && !e.hasAttribute("aria-pressed"))
      .map((e) => e.getAttribute("aria-label"));
    expect(ohne).toEqual(["Auf Flug zentrieren"]);
  });

  it("spiegelt aria-pressed den Zustand wider: an und aus", () => {
    const { container, rerender } = render(<KartenLeiste {...angaben()} />);
    const druck = () => Object.fromEntries(
      ["Track", "Taxiweg", "VA-Verkehr", "Folgen", "eigene Höhe"].map((n) => [n, screen.getByRole("button", { name: n }).getAttribute("aria-pressed")]),
    );
    expect(druck()).toEqual({ Track: "true", Taxiweg: "true", "VA-Verkehr": "true", Folgen: "true", "eigene Höhe": "true" });
    rerender(<KartenLeiste {...angaben({ showTrack: false, showTaxi: false, showVa: false, folgen: false, flAuto: false })} />);
    expect(druck()).toEqual({ Track: "false", Taxiweg: "false", "VA-Verkehr": "false", Folgen: "false", "eigene Höhe": "false" });
    expect(container).toBeTruthy();
  });

  it("besteht aus Tastern und einem Schieberegler, die per Tab erreichbar sind", () => {
    const c = vollst();
    for (const e of bedienelemente(c)) {
      expect(["BUTTON", "INPUT"], e.outerHTML.slice(0, 80)).toContain(e.tagName);
      expect(e.getAttribute("tabindex") === null || Number(e.getAttribute("tabindex")) >= 0).toBe(true);
    }
    expect(c.querySelectorAll("button:not([type=button])")).toHaveLength(0);
  });

  it("laesst die Netz-Gruppe als Gruppe mit Namen auslesen", () => {
    vollst();
    expect(screen.getByRole("group", { name: "Netz" })).toBeTruthy();
  });
});

// ── CSS lesen (nur fuer das, was jsdom nicht sieht) ──────────────────────────────
const css = readFileSync(resolve(__dirname, "../../App.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
/** Alle Regeln, in deren Waehlerliste genau dieser Waehler steht. */
function regeln(waehler: string): Record<string, string>[] {
  const treffer: Record<string, string>[] = [];
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const liste = m[1].split(",").map((s) => s.trim());
    if (!liste.includes(waehler)) continue;
    treffer.push(Object.fromEntries(
      m[2].split(";").map((z) => z.split(/:(.*)/s)).filter((p) => p.length > 1).map(([k, v]) => [k.trim(), v.trim()]),
    ));
  }
  return treffer;
}
const wert = (waehler: string, eigenschaft: string) => regeln(waehler).map((r) => r[eigenschaft]).find((v) => v !== undefined);
const MASS = ["width", "min-width", "max-width", "height", "min-height", "padding", "padding-left", "padding-right", "border", "border-width", "font-size", "font-weight", "letter-spacing", "gap", "margin"];

describe("Leiste: feste Knopfmasse (G1)", () => {
  it("zeigt das Wort an jedem Ein/Aus-Schalter in beiden Zustaenden, sonst springt die Breite", () => {
    const { rerender } = render(<KartenLeiste {...angaben({ showTrack: true, showTaxi: true, showVa: true, bilderAn: true })} />);
    const woerter = () => [...document.querySelectorAll(".aa-livemap-toggle__wort")].map((e) => e.textContent);
    const an = woerter();
    expect(an).toEqual(["Track", "Taxiweg", "Bilder", "VA-Verkehr"]);
    rerender(<KartenLeiste {...angaben({ showTrack: false, showTaxi: false, showVa: false, bilderAn: false })} />);
    expect(woerter()).toEqual(an);
    // Und derselbe Aufbau (vier Schalter mit Symbol, „eigene Hoehe“ und „Folgen“): nur die Aktiv-Klasse unterscheidet.
    const klassen = [...document.querySelectorAll(".aa-livemap-toggle")].map((e) => e.className.replace("aa-livemap-toggle--active", "").trim());
    expect(klassen).toEqual(Array(6).fill("aa-livemap-toggle"));
  });

  it("fasst die Leiste in wenige Gruppen: drei ohne Flug, vier mit Flug", () => {
    const ohne = render(<KartenLeiste {...angaben({ hatFlug: false, hatPosition: false })} />);
    expect(ohne.container.querySelectorAll(".aa-livemap-controls__group")).toHaveLength(3);
    cleanup();
    const mit = render(<KartenLeiste {...angaben()} />);
    expect(mit.container.querySelectorAll(".aa-livemap-controls__group")).toHaveLength(4);
  });

  it("gibt jedem Knopf eine feste Hoehe und Mindestbreite, ohne Umbruch im Knopf", () => {
    expect(wert(".aa-livemap-toggle", "height")).toBe("30px");
    expect(wert(".aa-livemap-toggle", "white-space")).toBe("nowrap");
    expect(wert(".aa-livemap-seg__btn", "height")).toBe("28px");
    expect(wert(".aa-livemap-seg__btn", "min-width")).toBe("32px");
    expect(wert(".aa-livemap-seg__btn", "white-space")).toBe("nowrap");
    expect(wert(".aa-livemap-seg__solo", "height")).toBe("30px");
    expect(wert(".aa-livemap-seg__solo", "width")).toBe("32px");
  });

  it("faerbt nur um: Aktiv- und Hover-Regeln fassen kein Mass an", () => {
    for (const waehler of [
      ".aa-livemap-toggle--active", ".aa-livemap-toggle--active:hover:not(:disabled)",
      ".aa-livemap-seg__btn--active", ".aa-livemap-seg__btn--active:hover:not(:disabled)",
    ]) {
      const alle = regeln(waehler);
      expect(alle.length, waehler).toBeGreaterThan(0);
      for (const r of alle) {
        expect(Object.keys(r).filter((k) => MASS.includes(k)), waehler).toEqual([]);
        expect(Object.keys(r).filter((k) => !["background", "color", "border-color"].includes(k)), waehler).toEqual([]);
      }
    }
  });
});
