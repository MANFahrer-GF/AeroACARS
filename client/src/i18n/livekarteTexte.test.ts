// Die Texte der Live-Karte in allen drei Sprachen — und der Hinweistext zum Umbau.
//
// Abnahmeliste Live-Karte, Zeilen G8 und G9 (docs/spec/livekarte-umbau-abnahme.md).
//
// G8: `vollstaendigkeit.test.ts` prueft, dass de/en/it dieselben Schluessel fuehren. Das sagt aber
// nichts darueber, ob die Schluessel, die die KARTE benutzt, in den Dateien stehen: Die Karte ruft
// `t("livemap.netz", "Netz")` mit einem Rueckfalltext, und ein fehlender Schluessel faellt dann
// nicht auf, er zeigt in jeder Sprache den deutschen Rueckfall. Darum liest dieser Test den
// Quelltext der Karte (hier ist das ausdruecklich der Gegenstand der Pruefung), sammelt jeden
// Schluessel eines `t(...)`-Aufrufs und prueft ihn gegen de, en und it. Zusaetzlich wird die
// Leiste in jeder Sprache wirklich gerendert.
//
// G9: Der Text, den der Pilot nach dem Neustart sieht (`paket.neu_text`), nennt je Sprache den
// Umbau der Live-Karte vollstaendig.

import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { render, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import deRaw from "../../../bundle/texte/de.json";
import enRaw from "../../../bundle/texte/en.json";
import itRaw from "../../../bundle/texte/it.json";
import { KartenLeiste, type KartenLeisteAngaben } from "../components/livekarte/KartenLeiste";

type Baum = { [k: string]: string | Baum };
const auspacken = (m: unknown): Baum => ((m as { default?: Baum }).default ?? (m as Baum));
const TEXTE: Record<"de" | "en" | "it", Baum> = { de: auspacken(deRaw), en: auspacken(enRaw), it: auspacken(itRaw) };
const SPRACHEN = ["de", "en", "it"] as const;

const wert = (baum: Baum, schluessel: string): string | undefined => {
  let knoten: string | Baum | undefined = baum;
  for (const teil of schluessel.split(".")) {
    if (typeof knoten !== "object" || knoten === null) return undefined;
    knoten = knoten[teil];
  }
  return typeof knoten === "string" ? knoten : undefined;
};
/** Ein Schluessel gilt, wenn er selbst da ist oder als Mehrzahl-Paar `_one` und `_other`. */
const texte = (baum: Baum, schluessel: string): string[] | null => {
  const einzel = wert(baum, schluessel);
  if (einzel !== undefined) return [einzel];
  const eins = wert(baum, `${schluessel}_one`);
  const andere = wert(baum, `${schluessel}_other`);
  return eins !== undefined && andere !== undefined ? [eins, andere] : null;
};
const platzhalter = (s: string) => (s.match(/\{\{\s*[A-Za-z0-9_]+\s*\}\}/g) ?? []).map((p) => p.replace(/\s+/g, "")).sort();

// ── Schluessel aus dem Quelltext ─────────────────────────────────────────────
/** Jeder Text in Anfuehrungszeichen innerhalb eines `t(...)`-Aufrufs, der wie ein Schluessel aussieht
 *  (auch ueber Bedingungsausdruecke wie `t(zu ? "a.b" : "a.c")` und mehrzeilig). Der Rueckfalltext
 *  als zweites Argument hat Leerzeichen oder Umlaute und zaehlt nicht. */
export function schluesselIn(quelle: string): string[] {
  const gefunden = new Set<string>();
  const aufruf = /(?<![A-Za-z0-9_$.])t\(/g;
  for (let m = aufruf.exec(quelle); m; m = aufruf.exec(quelle)) {
    let i = m.index + 2;
    let tiefe = 1;
    const teil: string[] = [];
    while (i < quelle.length && tiefe > 0) {
      const z = quelle[i];
      if (z === '"' || z === "'" || z === "`") {
        let j = i + 1;
        while (j < quelle.length && quelle[j] !== z) j += quelle[j] === "\\" ? 2 : 1;
        teil.push(quelle.slice(i + 1, j));
        i = j + 1;
        continue;
      }
      if (z === "(") tiefe++;
      if (z === ")") tiefe--;
      i++;
    }
    for (const text of teil) if (/^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$/.test(text)) gefunden.add(text);
  }
  return [...gefunden].sort();
}

const QUELLEN = (() => {
  const komponenten = resolve(__dirname, "../components");
  const ordner = resolve(komponenten, "livekarte");
  const dateien = [
    resolve(komponenten, "LiveMapView.tsx"),
    resolve(komponenten, "LiveMapEvents.tsx"),
    resolve(komponenten, "LiveMapEmptyState.tsx"),
    ...readdirSync(ordner)
      .filter((d) => /\.tsx?$/.test(d) && !/\.test\.|testhilfe/.test(d))
      .map((d) => resolve(ordner, d)),
  ];
  return dateien.map((pfad) => ({ pfad, quelle: readFileSync(pfad, "utf8") }));
})();
const KARTEN_SCHLUESSEL = [...new Set(QUELLEN.flatMap((q) => schluesselIn(q.quelle)))].sort();

describe("Drei Sprachen: Schluessel der Karte (G8)", () => {
  it("findet im Quelltext der Karte die Schluessel, auch ueber Bedingungsausdruecke und Mehrzahl", () => {
    expect(KARTEN_SCHLUESSEL.length).toBeGreaterThan(45);
    for (const erwartet of [
      "livemap.title", "livemap.panel_zu", "livemap.panel_auf", "livemap.header_va_count", "livemap.netz",
      "livemap.fremde_route_fehler", "livemap.taxi_loaded", "bilder.karte.flach", "bilder.karte.kugel_lang",
    ]) expect(KARTEN_SCHLUESSEL).toContain(erwartet);
  });

  it("erkennt einen fehlenden Schluessel (Gegenprobe des Suchers)", () => {
    const probe = 't("livemap.gibt_es_nicht", "Rueckfall") + t(a ? "livemap.title" : "livemap.auch_nicht") + other("x.y.z")';
    expect(schluesselIn(probe)).toEqual(["livemap.auch_nicht", "livemap.gibt_es_nicht", "livemap.title"]);
    expect(texte(TEXTE.de, "livemap.gibt_es_nicht")).toBeNull();
    expect(texte(TEXTE.de, "livemap.title")).not.toBeNull();
  });

  for (const sprache of SPRACHEN) {
    it(`fuehrt jeden Schluessel der Karte in ${sprache}, nicht leer`, () => {
      const fehlend = KARTEN_SCHLUESSEL.filter((k) => texte(TEXTE[sprache], k) === null);
      expect(fehlend, `fehlen in ${sprache}: ${fehlend.join(", ")}`).toEqual([]);
      const leer = KARTEN_SCHLUESSEL.filter((k) => (texte(TEXTE[sprache], k) ?? []).some((t) => !t.trim()));
      expect(leer).toEqual([]);
    });
  }

  it("laesst in jeder Sprache dieselben Platzhalter in den Karten-Texten stehen", () => {
    const abweichend: string[] = [];
    for (const k of KARTEN_SCHLUESSEL) {
      const de = texte(TEXTE.de, k);
      if (!de) continue;
      for (const sprache of ["en", "it"] as const) {
        const andere = texte(TEXTE[sprache], k);
        if (!andere) continue;
        de.forEach((text, i) => {
          if (JSON.stringify(platzhalter(text)) !== JSON.stringify(platzhalter(andere[i]))) abweichend.push(`${k} (${sprache})`);
        });
      }
    }
    expect(abweichend).toEqual([]);
  });

  it("uebersetzt die Mehrzahl des Zaehlers im Kopf in allen drei Sprachen mit der Zahl", () => {
    for (const sprache of SPRACHEN) {
      const [eins, mehr] = texte(TEXTE[sprache], "livemap.header_va_count")!;
      expect(platzhalter(eins), sprache).toEqual(["{{count}}"]);
      expect(platzhalter(mehr), sprache).toEqual(["{{count}}"]);
      expect(eins, sprache).not.toBe(mehr);
    }
  });

  // Die Leiste mit den echten Texten jeder Sprache: kein deutscher Rueckfall mitten im Englischen.
  const nichts = () => {};
  const angaben: KartenLeisteAngaben = {
    satellit: false, setzeSatellit: nichts, kugel: false, setzeKugel: nichts, trackUp: false, setzeTrackUp: nichts,
    showTrack: true, umschaltenTrack: nichts, showTaxi: true, umschaltenTaxi: nichts, taxiPlaetze: [],
    bilderMarken: 3, bilderAn: true, umschaltenBilder: nichts, showVa: true, umschaltenVa: nichts,
    netz: "vatsim", setzeNetz: nichts, flAuto: true, umschaltenFlAuto: nichts, flWert: 200, setzeFlWert: nichts,
    flRegelbar: true, flAnzeige: "FL200", hatFlug: true, hatPosition: true, folgen: true, umschaltenFolgen: nichts,
    zentrieren: nichts,
  };
  for (const sprache of SPRACHEN) {
    it(`zeigt die Leiste in ${sprache} mit den Texten dieser Sprache`, async () => {
      const inst = i18next.createInstance();
      await inst.use(initReactI18next).init({
        lng: sprache,
        resources: Object.fromEntries(SPRACHEN.map((s) => [s, { common: TEXTE[s] }])),
        defaultNS: "common",
        fallbackLng: false,
        interpolation: { escapeValue: false },
      });
      const { container } = render(createElement(I18nextProvider, { i18n: inst }, createElement(KartenLeiste, angaben)));
      const t = (k: string) => wert(TEXTE[sprache], k)!;
      const namen = [...container.querySelectorAll("button, input")].map((e) => e.getAttribute("aria-label") ?? e.textContent);
      expect(namen).toEqual([
        t("livemap.view_map"), t("livemap.view_sat"), t("bilder.karte.flach"), t("bilder.karte.kugel_lang"),
        t("livemap.orient_north"), t("livemap.orient_track"),
        t("livemap.layer_track"), t("livemap.layer_taxi"), t("livemap.layer_bilder"), t("livemap.layer_va"),
        t("livemap.netz_aus"), "VATSIM", "IVAO", t("livemap.fl_auto"), t("livemap.fl_regler"),
        t("livemap.center_on_flight"), t("livemap.follow"),
      ]);
      expect(container.querySelector('[role="group"]')?.getAttribute("aria-label")).toBe(t("livemap.netz"));
      cleanup();
    });
  }
});

// ── G9 ───────────────────────────────────────────────────────────────────────
// Der Hinweistext zum Umbau der Live-Karte ging mit den Generationen bis 91
// vollstaendig an die Piloten (Pruefung der Stichworte bis 3.0.0). Ab 3.0.1
// beschreibt `paket.neu_text` die naechste Generation; `hinweis.py` verlangt
// einen neuen Text, der alte darf darum nicht festgeschrieben bleiben. Es
// bleibt die Pruefung, dass die drei Sprachen beieinander bleiben.
describe("Hinweistext (G9)", () => {
  it("steht in allen drei Sprachen", () => {
    for (const s of SPRACHEN) expect(wert(TEXTE[s], "paket.neu_text")!.length).toBeGreaterThan(100);
  });

  it("haelt die drei Sprachen im Umfang beieinander, nichts nur in einer", () => {
    const laengen = SPRACHEN.map((s) => wert(TEXTE[s], "paket.neu_text")!.length);
    expect(Math.max(...laengen) / Math.min(...laengen)).toBeLessThan(1.4);
    const saetze = SPRACHEN.map((s) => wert(TEXTE[s], "paket.neu_text")!.split(/(?<=[.!?])\s+/).length);
    expect(new Set(saetze).size).toBe(1);
  });
});
