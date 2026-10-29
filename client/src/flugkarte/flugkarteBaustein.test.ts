// @vitest-environment jsdom
/**
 * 28.10.2026 — Grenzfaelle des Bausteins flugkarte.ts selbst (Codex-Abnahme Etappe 2).
 * v3 (Etappe 5, P-8.21): aus aeroacars-live hierher umgezogen — der Baustein lebt nur
 * noch im Paket (Client); die Admin-Webapp laedt ihn von dort (ui/flugkarte/).
 * MapLibre/deck.gl brauchen WebGL; hier ersetzt durch Attrappen, die jeden Aufruf
 * schlucken. Geprueft wird, was der Baustein selbst falsch machen kann: Texte aus der
 * Akte landen nie als HTML, ohne gemessene Landung keine Landeausschnitte, leere
 * Akte, und nach dem Schliessen laeuft kein Bild mehr nach.
 */
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

const leer = (): unknown => new Proxy(function () {}, { get: (_z, k) => (k === "then" ? undefined : leer()), apply: () => leer() });
vi.mock("maplibre-gl", () => {
  class Karte {
    constructor() { return new Proxy(this, { get: (z, k) => (k in z ? (z as any)[k] : leer()) }); }
    getZoom() { return 5; }
    getPitch() { return 0; }
    getBearing() { return 0; }
    getCanvas() { return { clientWidth: 1200, clientHeight: 800, getBoundingClientRect: () => ({ left: 0, top: 0, right: 1200, bottom: 800, width: 1200, height: 800 }) }; }
    getCenter() { return { lng: 0, lat: 0 }; }
    on() { return this; }
    once() { return this; }
    remove = vi.fn();
  }
  return { default: { Map: Karte, NavigationControl: class {}, AttributionControl: class {} } };
});
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));
vi.mock("@deck.gl/mapbox", () => ({ MapboxOverlay: class { setProps() {} } }));
vi.mock("@deck.gl/layers", () => {
  const Ebene = class { constructor(public props: unknown) {} };
  return { IconLayer: Ebene, LineLayer: Ebene, PathLayer: Ebene, ScatterplotLayer: Ebene, SolidPolygonLayer: Ebene, TextLayer: Ebene };
});
// echte Projektion: deck.gl reicht WebMercatorViewport aus @math.gl/web-mercator durch (reine Rechnung, laedt in
// jsdom — deck.gl selbst nicht); die Drehung des Flugzeugsymbols haengt davon ab
vi.mock("@deck.gl/core", async () => ({ WebMercatorViewport: (await vi.importActual<any>("@math.gl/web-mercator")).WebMercatorViewport }));

import { flugkarteAnzeigen, hoeheUeberGrund, type TextWerte } from "./flugkarte";
import de from "../../../bundle/texte/de.json";
import en from "../../../bundle/texte/en.json";
import itL from "../../../bundle/texte/it.json"; // nicht `it` — das ist die Testfunktion
import { readFileSync } from "node:fs";
import { KATEGORIEN } from "./flugakteTypen";

/** Uebersetzer wie i18next ({{x}}), aus der deutschen Sprachdatei; null bei fehlendem Schluessel */
function uebersetzer(daten: unknown) {
  return (k: string, w?: TextWerte) => {
    const v = k.split(".").reduce<unknown>((o, t) => (o && typeof o === "object" ? (o as Record<string, unknown>)[t] : undefined), daten);
    return typeof v === "string" ? v.replace(/\{\{(\w+)\}\}/g, (_m, n: string) => String(w?.[n] ?? "")) : null;
  };
}
const OPT = { stilUrl: "/x.json", t: uebersetzer(de), sprache: "de" };
import type { Flugakte } from "./flugakteTypen";

beforeAll(() => {
  // jsdom kann kein Canvas — der Datenstreifen zeichnet ins Leere
  HTMLCanvasElement.prototype.getContext = (() => leer()) as any;
  // jsdom kennt keinen ResizeObserver
  (globalThis as any).ResizeObserver ??= class { observe() {} disconnect() {} };
  // jsdom kennt kein matchMedia (reduzierte Bewegung, Handy-Breite)
  (window as any).matchMedia ??= () => ({ matches: false });
  Element.prototype.scrollIntoView ??= () => {};
});
afterEach(() => { document.body.innerHTML = ""; vi.restoreAllMocks(); });

const zeile = (t: string, lat: number, alt: number) => [lat, 8.5, alt, alt - 300, 250, 240, 0, 90, 2, 0, 5000, t, 0, 1, 1, 0, 1, 1, 1];
function akte(mehr: Partial<Flugakte> = {}): Flugakte {
  return {
    version: 1, kopf: { start: "2026-10-26T11:00:00Z", dpt_airport: "EDDF", arr_airport: "EDDM" }, flugzeug: "A320",
    dep_gate: null, arr_gate: null, route: null, klappenArt: "hebel", messbar: {}, abdeckung: {}, land: null, filed: null,
    track: [zeile("11:00:00", 50.0, 400), zeile("11:10:00", 50.1, 9000), zeile("11:20:00", 50.2, 1000)],
    events: [{ t: "2026-10-26T11:10:00Z", i: 1, k: "ap", txt: '<img src=x onerror="window.__xss=1">' }],
    ...mehr,
  } as unknown as Flugakte;
}
const flaeche = () => { const d = document.createElement("div"); document.body.appendChild(d); return d; };

describe("flugkarteAnzeigen", () => {
  it("Text aus der Akte wird nie als HTML eingesetzt", () => {
    const d = flaeche();
    flugkarteAnzeigen(d, akte(), OPT);
    expect(d.querySelector("img[src='x']")).toBeNull();
    expect(d.textContent).toContain('<img src=x onerror="window.__xss=1">');
  });

  it("ohne gemessene Landung sind Anflug und Landung & Rollen gesperrt", () => {
    const d = flaeche();
    flugkarteAnzeigen(d, akte(), OPT);
    const knopf = (k: string) => d.querySelector<HTMLButtonElement>(`[data-fit="${k}"]`)!;
    expect(knopf("arr").disabled).toBe(true);
    expect(knopf("gnd").disabled).toBe(true);
    expect(knopf("all").disabled).toBe(false);
  });

  it("mit gemessener Landung bleiben die Landeausschnitte frei", () => {
    const d = flaeche();
    flugkarteAnzeigen(d, akte({ land: { t: "2026-10-26T11:19:00Z", lat: 50.2, lon: 8.5 } }), OPT);
    expect(d.querySelector<HTMLButtonElement>('[data-fit="arr"]')!.disabled).toBe(false);
  });

  it("leere Akte: Hinweis statt Absturz", () => {
    const d = flaeche();
    flugkarteAnzeigen(d, akte({ track: [], events: [] }), OPT);
    expect(d.textContent).toContain("keine Positionen");
  });

  it("Pause→Abspielen im selben Bild: eine Wiedergabeschleife; nach dem Schliessen nichts mehr eingeplant", () => {
    const geplant = new Map<number, FrameRequestCallback>();
    let n = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.set(++n, cb); return n; });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => { geplant.delete(id); });
    const d = flaeche();
    const weg = flugkarteAnzeigen(d, akte(), OPT);
    const abarbeiten = () => { const jetzt = [...geplant]; geplant.clear(); jetzt.forEach(([, cb]) => cb(performance.now())); };
    abarbeiten();
    const play = d.querySelector<HTMLButtonElement>('[data-fk="play"]')!;
    play.click();
    const mitEiner = geplant.size;
    play.click(); play.click(); // Pause und gleich wieder Abspielen
    expect(geplant.size).toBe(mitEiner);
    abarbeiten();
    expect(play.textContent).toContain("Pause");
    expect(geplant.size).toBeGreaterThan(0); // Schleife laeuft weiter
    weg();
    expect([...geplant.keys()]).toEqual([]);
    expect(d.innerHTML).toBe("");
  });

  // Thomas 28.10.: „Ganzer Flug“ begann in der Mitte, „Anflug“ spielte nur die letzten Sekunden und hielt an —
  // der Zeitregler stand kurz vor dem Aufsetzen und blieb dort, weil die Stelle in beiden Ausschnitten lag
  it("Zeitregler steht anfangs am Start und springt beim Ausschnitt-Wechsel an dessen Anfang", () => {
    const geplant: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
    const abarbeiten = () => { while (geplant.length) geplant.shift()!(performance.now()); };
    const d = flaeche();
    flugkarteAnzeigen(d, akte({
      land: { t: "2026-10-26T11:19:00Z", lat: 50.2, lon: 8.5 },
      events: [{ t: "2026-10-26T11:12:00Z", i: 1, k: "phase", txt: "Approach" }],
    }), OPT);
    const jetzt = () => d.querySelector('[data-fk="jetzt"]')!.textContent!.slice(0, 8);
    abarbeiten();
    expect(jetzt()).toBe("11:00:00");
    d.querySelector<HTMLButtonElement>('[data-fit="arr"]')!.click(); abarbeiten();
    expect(jetzt()).toBe("11:12:00");
    d.querySelector<HTMLButtonElement>('[data-fit="all"]')!.click(); abarbeiten();
    expect(jetzt()).toBe("11:00:00");
  });

  // Thomas 28.10.: nach dem Wechsel auf „Ganzer Flug“ blieb das Popup des alten Ereignisses in der Ecke stehen
  it("Ausschnitt-Wechsel hebt die Auswahl auf (Popup zu)", () => {
    const geplant: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
    const d = flaeche();
    flugkarteAnzeigen(d, akte(), OPT);
    d.querySelector<HTMLButtonElement>(".fk-liste button[data-n]")!.click();
    while (geplant.length) geplant.shift()!(performance.now());
    expect(d.querySelector<HTMLElement>('[data-fk="pop"]')!.hidden).toBe(false);
    d.querySelector<HTMLButtonElement>('[data-fit="all"]')!.click();
    expect(d.querySelector<HTMLElement>('[data-fk="pop"]')!.hidden).toBe(true);
  });

  // Thomas 28.10.: beim Pushback zeigte das Symbol nach hinten (es folgte der Bewegung, nicht der Nase)
  it("Flugzeugsymbol zeigt in den gemessenen Kurs, auch wenn es rueckwaerts rollt", () => {
    // Kurs 0° (Nase nach Norden), das Flugzeug wird nach Sueden geschoben — weit genug, dass die Bewegung bei
    // Zoom 5 auf dem Bildschirm sichtbar ist (sonst wuerde auch die alte Bewegungsregel zufaellig 0° liefern)
    const p = (t: string, lat: number) => [lat, 8.5, 400, 0, 2, 0, 0, 0, 0, 0, 5000, t, 1, 0, 1, 0, 0, 0, 0];
    const geplant: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
    const d = flaeche();
    flugkarteAnzeigen(d, akte({ track: [p("11:00:00", 51.0), p("11:00:30", 50.5), p("11:01:00", 50.0)], events: [] }), OPT);
    const drehung = () => Number(/rotate\((-?[\d.]+)deg\)/.exec(d.querySelector<HTMLElement>('[data-fk="flugzeug"]')!.style.transform)![1]);
    d.querySelector<HTMLButtonElement>('[data-fit="all"]')!.click();
    while (geplant.length) geplant.shift()!(performance.now());
    expect(Math.abs(drehung())).toBeLessThan(1); // nach oben (Norden), nicht 180°
  });

  it("ohne gemessenen Kurs: Bewegungsrichtung, auch am Spurende (nicht stumpf Norden)", () => {
    // nach Osten, Kurs nicht geliefert; Zeitregler ans Ende (nach der Landung)
    const p = (t: string, lon: number) => [50, lon, 400, 0, 2, 0, 0, null, 0, 0, 5000, t, 1, 0, 1, 0, 0, 0, 0];
    const geplant: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
    const d = flaeche();
    flugkarteAnzeigen(d, akte({ track: [p("11:00:00", 8.0), p("11:00:30", 8.5), p("11:01:00", 9.0)], events: [{ t: "2026-10-26T11:01:00Z", i: 2, k: "ap", txt: "am Ende" }] }), OPT);
    const drehung = () => Number(/rotate\((-?[\d.]+)deg\)/.exec(d.querySelector<HTMLElement>('[data-fk="flugzeug"]')!.style.transform)![1]);
    d.querySelector<HTMLButtonElement>('.fk-liste button[data-n]')!.click(); // Ereignis am letzten Messpunkt: Zeitregler ans Spurende
    while (geplant.length) geplant.shift()!(performance.now());
    expect(Math.round(drehung())).toBe(90);
  });
});

// ---------- 3D-Hoehe ueber Grund (Thomas 28.10.: beim Rollen schwebte alles ueber der Karte) ----------
describe("hoeheUeberGrund", () => {
  const p = (alt: number | null, agl: number | null, amBoden = 0) => [50, 8, alt, agl, 0, 0, 0, 0, 0, 0, 0, "12:00:00", amBoden];
  it("am Boden eines hochgelegenen Platzes 0 (Vilnius 636 ft), im Flug die Hoehe ueber Grund", () => {
    expect(hoeheUeberGrund([p(636, 0, 1), p(5636, 5000), p(636, 0, 1)])).toEqual([0, 5000, 0]);
  });
  it("fehlt AGL zwischendurch, gilt das zuletzt gemessene Gelaende", () => {
    expect(hoeheUeberGrund([p(636, 0, 1), p(3636, null), p(5636, 5000)])).toEqual([0, 3000, 5000]);
  });
  // Codex 28.10.: ein fortgesetzter Flug, der in 5 000 ft ohne AGL beginnt, lag erst am Boden und sprang dann hoch
  it("fehlt AGL am Anfang, gilt das erste gemessene Gelaende — kein Sprung", () => {
    expect(hoeheUeberGrund([p(5000, null), p(5100, null), p(5200, 4600)])).toEqual([4400, 4500, 4600]);
  });
  it("ganz ohne AGL: Gelaende aus dem ersten Punkt am Boden", () => {
    expect(hoeheUeberGrund([p(636, null, 1), p(2636, null), p(-21, null)])).toEqual([0, 2000, 0]);
  });
});

// Codex 28.10.: die Kuerzel (KL, FW …) kommen aus der Sprachdatei und gingen ungeescapt ins HTML
it("Kuerzel aus der Sprachdatei landen nie als HTML (Liste, letztes Ereignis)", () => {
  const geplant: FrameRequestCallback[] = [];
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
  const d = flaeche();
  const t = (k: string, w?: TextWerte) => (k === "flugkarte.kat.ap.c" ? '<img src=x onerror="window.__kuerzel=1">' : uebersetzer(de)(k, w));
  flugkarteAnzeigen(d, akte(), { stilUrl: "/x.json", t, sprache: "de" });
  d.querySelector<HTMLButtonElement>(".fk-liste button[data-n]")!.click();
  while (geplant.length) geplant.shift()!(performance.now());
  expect(d.querySelector("img[src='x']")).toBeNull();
  expect(d.textContent).toContain('<img src=x onerror="window.__kuerzel=1">');
});

// ---------- Sprachen (AeroACARS: de/en/it; derselbe Baustein, dieselben Texte) ----------
const flach = (o: unknown, p = ""): Record<string, string> => Object.assign({}, ...Object.entries(o as Record<string, unknown>).map(([k, v]) =>
  typeof v === "string" ? { [p + k]: v } : flach(v, p + k + ".")));
const platzhalter = (t: string) => [...t.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort().join(",");
const FK = { de: flach(de.flugkarte), en: flach(en.flugkarte), it: flach(itL.flugkarte) };
/** v als ganzes Wort/ganze Wendung in t („Satellit“ steckt in „Satellite“ und zaehlt nicht) */
const ganz = (t: string, v: string) => new RegExp(`(^|[^\\p{L}])${v.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}($|[^\\p{L}])`, "u").test(t);

describe("Flugkarte in drei Sprachen", () => {
  it("en und it haben genau die deutschen Schluessel, nicht leer, mit denselben Platzhaltern", () => {
    for (const l of ["en", "it"] as const) {
      expect(Object.keys(FK[l]).sort(), l).toEqual(Object.keys(FK.de).sort());
      for (const [k, v] of Object.entries(FK.de)) {
        expect(FK[l][k].trim(), `${l}: ${k}`).not.toBe("");
        expect(platzhalter(FK[l][k]), `${l}: ${k}`).toBe(platzhalter(v));
      }
    }
  });

  it("jeder Schluessel, den Baustein und Fenster benutzen, steht in der Sprachdatei", () => {
    const quelle = readFileSync("src/flugkarte/flugkarte.ts", "utf8"), fenster = readFileSync("src/flugkarte/FlugkarteFenster.tsx", "utf8");
    const benutzt = [
      ...[...quelle.matchAll(/\b[Uu]\("([a-z_]+)"/g)].map((m) => "ui." + m[1]),
      ...[...quelle.matchAll(/"((?:off|on)_block|tip_(?:zoom|auf)|band_klappen(?:_prozent)?)"/g)].map((m) => "ui." + m[1]),
      ...Object.keys(KATEGORIEN).flatMap((k) => ["c", "n", "how"].map((f) => `kat.${k}.${f}`)), "kat.flaps.how_stellung",
      ...[...fenster.matchAll(/t\("flugkarte\.([a-z_.]+)"/g)].map((m) => m[1]),
    ];
    expect(benutzt.length).toBeGreaterThan(80);
    expect(benutzt.filter((k) => !(k in FK.de))).toEqual([]);
  });

  // Gerendert pruefen statt gezaehlt: bleibt ein deutscher Text sichtbar stehen, obwohl die Uebersetzung anders lautet?
  for (const [name, daten, sprache] of [["Englisch", en, "en"], ["Italienisch", itL, "it"]] as const) {
    it(`${name}: kein deutscher Text bleibt sichtbar (Geruest, Filter, Liste, HUD, Popup, Ereignisse)`, () => {
      const geplant: FrameRequestCallback[] = [];
      vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { geplant.push(cb); return geplant.length; });
      const d = flaeche();
      const t0 = "2026-10-26T11:0";
      flugkarteAnzeigen(d, akte({
        messbar: { light: false },
        events: [
          { t: t0 + "1:00Z", i: 0, k: "phase", code: "phase", werte: { phase: "Pushback" }, txt: "Pushback" },
          { t: t0 + "2:00Z", i: 0, k: "pa", code: "ansage", werte: { slot: "kapitaen_begruessung" }, txt: "Begrüßung des Kapitäns", dauer: 31, sprachen: ["de"] },
          { t: t0 + "3:00Z", i: 1, k: "gear", code: "fahrwerk_rein", txt: "Fahrwerk eingefahren" },
          { t: t0 + "4:00Z", i: 1, k: "eng", code: "wiederholt", werte: { regel: "antiice_tw", n: 8, von: "11:00", bis: "11:04" }, txt: "Triebwerks-Anti-Ice schaltet wiederholt (8×, 11:00–11:04)", anzahl: 8, bis: t0 + "4:00Z" },
          { t: t0 + "5:00Z", i: 1, k: "fcu", code: "fcu_hoehe", werte: { a: 5000, b: 36000 }, txt: "Gewählte Höhe 5.000 ft → 36.000 ft" },
        ],
      }), { stilUrl: "/x.json", t: uebersetzer(daten), sprache });
      d.querySelector<HTMLButtonElement>('[data-fk="alle"]')!.click();
      d.querySelector<HTMLButtonElement>(".fk-liste button[data-n]")!.click(); // Popup
      while (geplant.length) geplant.shift()!(performance.now());
      // je Textknoten eine Zeile — textContent klebte Knoepfe aneinander („Ganzer FlugPartenza“) und die Wortgrenze griff nie
      const knoten: string[] = []; const gang = document.createTreeWalker(d, NodeFilter.SHOW_TEXT);
      while (gang.nextNode()) knoten.push(gang.currentNode.textContent ?? "");
      const sichtbar = [...knoten, ...Array.from(d.querySelectorAll("[title],[aria-label]")).flatMap((e) => [e.getAttribute("title") ?? "", e.getAttribute("aria-label") ?? ""])].join("\n");
      // deutsche Texte ohne Platzhalter, die sich von der Uebersetzung unterscheiden (mind. 5 Zeichen, sonst Zufallstreffer)
      const durch = Object.entries(FK.de).filter(([k, v]) => !v.includes("{{") && v.length >= 5 && v !== FK[sprache][k] && ganz(sichtbar, v)).map(([k, v]) => `${k}: ${v}`);
      expect(durch).toEqual([]);
      // die Ereignistexte selbst: uebersetzt, Zahlen in der Sprache
      expect(sichtbar).toContain(FK[sprache]["ev.fahrwerk_rein"]);
      expect(sichtbar).toContain(FK[sprache]["slot.kapitaen_begruessung"]);
      expect(sichtbar).toContain(sprache === "en" ? "Selected altitude 5,000 ft → 36,000 ft" : "Quota selezionata 5000 ft → 36.000 ft"); // it-IT gruppiert erst ab 5 Stellen (CLDR)
      expect(sichtbar).toContain(FK[sprache]["regel.antiice_tw"] + (sprache === "en" ? " switches repeatedly" : " commuta ripetutamente"));
    });
  }
});
