// Jeder Text des Bilder-Tabs steht in allen drei Sprachen. Der allgemeine Paritaetstest kennt nur
// Schluessel, die in de vorkommen; dieser hier liest die Quelltexte des Tabs.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import de from "../../../../bundle/texte/de.json";
import en from "../../../../bundle/texte/en.json";
import it_ from "../../../../bundle/texte/it.json";
import { TABELLE } from "./logik/grenzen";
import { FEHLER_CODES } from "./logik/sendezustand";

const SPRACHEN: Record<string, unknown> = { de, en, it: it_ };
const hat = (o: unknown, pfad: string): boolean => {
  const w = pfad.split(".").reduce<unknown>((a, k) => (a && typeof a === "object" ? (a as Record<string, unknown>)[k] : undefined), o);
  return typeof w === "string" && w.trim() !== "";
};
const quellen = (): string[] => {
  const dir = __dirname;
  const dateien = fs.readdirSync(dir).filter((f) => /\.tsx?$/.test(f) && !/\.test\./.test(f));
  return [...dateien.map((f) => path.join(dir, f)), path.join(__dirname, "../LiveMapView.tsx")].map((f) => fs.readFileSync(f, "utf-8"));
};

describe("Texte des Bilder-Tabs", () => {
  const schluessel = new Set<string>();
  for (const q of quellen()) for (const m of q.matchAll(/\bt\(\s*["'`](bilder\.[a-z0-9_.]+)["'`]/g)) schluessel.add(m[1]);
  for (const q of fs.readdirSync(__dirname).filter((f) => f === "BilderTab.tsx")) {
    for (const m of fs.readFileSync(path.join(__dirname, q), "utf-8").matchAll(/schluessel: "(bilder\.[a-z0-9_.]+)"/g)) schluessel.add(m[1]);
  }
  for (const q of fs.readFileSync(path.join(__dirname, "logik/store.ts"), "utf-8").matchAll(/"(bilder\.meldung\.[a-z_]+)"/g)) schluessel.add(q[1]);

  it("der Scanner findet die Schluessel ueberhaupt (Gegenprobe gegen einen leeren Scan)", () => {
    expect(schluessel.size).toBeGreaterThan(60);
    expect(schluessel.has("bilder.senden.jetzt")).toBe(true);
  });
  for (const [sprache, daten] of Object.entries(SPRACHEN)) {
    it(`alle statischen Schluessel in ${sprache}`, () => {
      expect([...schluessel].filter((k) => !hat(daten, k))).toEqual([]);
    });
    it(`dynamische Schluessel in ${sprache}: Fehlercodes, Geo-Stufen, Quellen, Metadatenfelder, Phasen, Filter`, () => {
      const fehlt: string[] = [];
      const pruefe = (k: string) => { if (!hat(daten, k)) fehlt.push(k); };
      FEHLER_CODES.forEach((c) => pruefe(`bilder.fehler.${c}`));
      TABELLE.geoStufen.forEach((s) => pruefe(`bilder.senden.geo.${s}`));
      TABELLE.metadaten.forEach((m) => pruefe(`bilder.detail.feld.${m.feld}`));
      ["sim", "protokoll", "logbuch", "keine"].forEach((s) => pruefe(`bilder.detail.quelle.${s}`));
      ["preflight", "boarding", "pushback", "taxi_out", "takeoff_roll", "takeoff", "climb", "cruise", "holding", "descent", "approach", "final", "landing", "taxi_in", "blocks_on", "arrived"].forEach((p) => pruefe(`bilder.detail.phase.${p}`));
      ["standard", "aufgespuert", "hand"].forEach((a) => pruefe(`bilder.quellen.art.${a}`));
      ["beobachtet", "pausiert", "wird_geprueft", "nicht_vorhanden", "zugriff_verweigert"].forEach((z) => pruefe(`bilder.quellen.zustand.${z}`));
      ["alle", "offen", "gesendet", "favoriten"].forEach((f) => pruefe(`bilder.liste.filter.${f}`));
      ["zusatz.msl", "zusatz.ueber_grund"].forEach((z) => pruefe(`bilder.detail.${z}`));
      pruefe("tabs.bilder");
      pruefe("paket.neu_text");
      expect(fehlt).toEqual([]);
    });
  }
  it("Cockpit-Schrift: kein Zeichen, das B612 Mono nicht kennt, in den Texten des Tabs", () => {
    const verboten = /[‑→←≤≥]/;
    const flach = (o: unknown): string[] => (typeof o === "string" ? [o] : o && typeof o === "object" ? Object.values(o as object).flatMap(flach) : []);
    for (const daten of Object.values(SPRACHEN)) {
      expect(flach((daten as { bilder: unknown }).bilder).filter((s) => verboten.test(s))).toEqual([]);
    }
  });
  it("Gegenprobe: ein fehlender Schluessel faellt auf", () => {
    expect(hat(de, "bilder.gibt.es.nicht")).toBe(false);
  });
});
