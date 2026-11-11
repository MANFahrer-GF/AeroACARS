// Jede Aktivitätszeile mit Schlüssel braucht ihren Text in allen drei Sprachen.
//
// **Warum es diesen Test gibt.** Seit 3.0.1 schreibt der Kern zu einer
// Aktivitätszeile Schlüssel und Werte (`akt(...)`, `AktivitaetText`), die
// Oberfläche zeigt die Zeile in der Sprache des Piloten. Fehlt ein Text in
// einer Sprache, fällt `aktivitaetAnzeige` still auf den deutschen Wortlaut
// zurück, und niemand merkt es. Dieser Test liest deshalb die Rust-Quellen
// und verlangt:
//
// 1. jeden `activity.*`-Schlüssel in de, en und it, mit denselben Platzhaltern,
//    und genau die Platzhalter, deren Werte der Kern mitgibt;
// 2. jeden Satz einer Zuordnungstabelle (`teil_aus_text`) wortgleich im
//    Quelltext und im deutschen Paket, sonst greift die Tabelle nach einer
//    Umformulierung nicht mehr und die Zeile bleibt deutsch;
// 3. keinen neuen `log_activity*`-Aufruf ohne Schlüssel. Ohne Schlüssel bleiben
//    nur englische Cockpit- und Statuszeilen („Gear DOWN“, „PIREP filed“),
//    die in jeder Sprache gleich lauten (Entscheidung Thomas 11.11.2026: nur
//    Pilotenhinweise übersetzen). Ihre Zahl je Datei steht unten fest.

import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import de from "../../../bundle/texte/de.json";
import en from "../../../bundle/texte/en.json";
import italienisch from "../../../bundle/texte/it.json";

const KERN = resolve(process.cwd(), "src-tauri/src");

function rustDateien(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const pfad = join(dir, name);
    if (statSync(pfad).isDirectory()) return name === "tests_lib" ? [] : rustDateien(pfad);
    return name.endsWith(".rs") ? [pfad] : [];
  });
}

const quellen = new Map(
  rustDateien(KERN).map((f) => [relative(KERN, f).split("\\").join("/"), readFileSync(f, "utf-8")]),
);
const alles = [...quellen.values()].join("\n");

const SPRACHEN: Record<string, unknown> = { de, en, it: italienisch };

function text(sprache: string, schluessel: string): unknown {
  return schluessel
    .split(".")
    .reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), SPRACHEN[sprache]);
}

function platzhalter(s: string): string[] {
  return [...new Set([...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]))].sort();
}

function schluesselIm(quelle: string): string[] {
  return [...new Set([...quelle.matchAll(/"(activity\.[a-z0-9_.]+)"/g)].map((m) => m[1]))].sort();
}

/** `"activity.x", &[("a", …), ("b", …)]` → Platzhalter, die der Kern füllt. */
function werteImKern(quelle: string): Map<string, string[][]> {
  const gefunden = new Map<string, string[][]>();
  for (const m of quelle.matchAll(/"(activity\.[a-z0-9_.]+)"\s*,\s*&\[([^\]]*)\]/g)) {
    const namen = [...new Set([...m[2].matchAll(/\(\s*"(\w+)"\s*,/g)].map((n) => n[1]))].sort();
    gefunden.set(m[1], [...(gefunden.get(m[1]) ?? []), namen]);
  }
  return gefunden;
}

/** `("deutscher Satz", "activity.x")` aus den Tabellen für `teil_aus_text`. */
function tabellen(quelle: string): [string, string][] {
  return [...quelle.matchAll(/\(\s*"((?:[^"\\]|\\.)+)"\s*,\s*"(activity\.[a-z0-9_.]+)"\s*\)/g)].map(
    (m) => [m[1], m[2]] as [string, string],
  );
}

/** Aufrufe ohne Schlüssel: die alten Funktionen und `aufgeschoben.handle`. */
function ohneSchluessel(quelle: string): number {
  const ohneKommentare = quelle.replace(/\/\/[^\n]*/g, "");
  const frei = ohneKommentare.match(/(?<![\w.])(?<!fn )(?:log_activity|log_activity_handle|log_activity_and_record)\(/g) ?? [];
  const aufgeschoben = ohneKommentare.match(/\.(?:handle|ohne_meldung)\(\s*_?app\s*,/g) ?? [];
  return frei.length + aufgeschoben.length;
}

// Englische Cockpit- und Statuszeilen, die bewusst ohne Schlüssel bleiben,
// dazu die Weiterleitungen in `lib_aktivitaet.rs` und `hoppie/poller.rs`.
// Wer hier eine Zahl erhöht, schreibt eine neue Zeile ohne Übersetzung:
// lieber `akt(...)` mit Texten in `bundle/texte/{de,en,it}.json`.
const OHNE_SCHLUESSEL: Record<string, number> = {
  "hoppie/poller.rs": 1, // PollEffects-Weiterleitung
  "lib_aktivitaet.rs": 1, // „Fuel & Weight @ …“
  "lib_anmeldung_mqtt.rs": 2, // „Logged in as …“, „Session restored — …“
  "lib_auto_start.rs": 1, // „Auto-Start: DLH 41 EDDF – KJFK“
  "lib_flug_abbrechen_fortsetzen.rs": 2, // „Manual PIREP filed“, „Flight cancelled“
  "lib_flug_beenden.rs": 2, // „PIREP filed“, „PIREP file failed“
  "lib_flugstart.rs": 4, // „Adopted …“, „Flight started“, 2 × „Phase: Boarding“
  "lib_metar_und_staende.rs": 1, // „Departure METAR: EDDF“
  "lib_pause_und_uploads.rs": 1, // „Flight log uploaded …“
  "lib_positions_streamer.rs": 1, // „Phase: Cruise“
  "lib_telemetrie_meldungen.rs": 44, // Cockpit: Gear, Flaps, FMA, MCP, Lichter …
};

describe("Aktivitätszeilen: Schlüssel und Texte", () => {
  const schluessel = schluesselIm(alles);

  it("findet die Schlüssel im Kern (Gegenprobe des Suchmusters)", () => {
    expect(schluessel.length).toBeGreaterThan(150);
    expect(schluessel).toContain("activity.refuel.line");
    expect(schluesselIm('akt("activity.probe.zeile", &[])')).toEqual(["activity.probe.zeile"]);
  });

  it("jeder Schlüssel hat Text in de, en und it, mit denselben Platzhaltern", () => {
    const fehler: string[] = [];
    for (const k of schluessel) {
      const texte = Object.keys(SPRACHEN).map((l) => [l, text(l, k)] as const);
      for (const [l, t] of texte) if (typeof t !== "string" || !t.trim()) fehler.push(`${k}: ${l} fehlt`);
      const ph = texte.filter(([, t]) => typeof t === "string").map(([, t]) => platzhalter(t as string).join(","));
      if (new Set(ph).size > 1) fehler.push(`${k}: Platzhalter ungleich (${ph.join(" | ")})`);
    }
    expect(fehler).toEqual([]);
  });

  it("der Kern füllt genau die Platzhalter des Textes", () => {
    const fehler: string[] = [];
    for (const [k, aufrufe] of werteImKern(alles)) {
      const erwartet = platzhalter(String(text("de", k) ?? "")).join(",");
      for (const namen of aufrufe) if (namen.join(",") !== erwartet) fehler.push(`${k}: Kern [${namen}] / Text [${erwartet}]`);
    }
    expect(fehler).toEqual([]);
    expect(werteImKern('akt("activity.x", &[("kg", &a), ("nm", &b)])').get("activity.x")).toEqual([["kg", "nm"]]);
  });

  it("kein Text im Paket ohne Verwendung im Kern", () => {
    const imPaket: string[] = [];
    const sammeln = (o: unknown, pfad: string) => {
      if (typeof o === "string") imPaket.push(pfad);
      else if (o && typeof o === "object") for (const [k, v] of Object.entries(o)) sammeln(v, `${pfad}.${k}`);
    };
    sammeln((de as Record<string, unknown>).activity, "activity");
    expect(imPaket.filter((k) => !schluessel.includes(k))).toEqual([]);
  });

  it("Tabellensätze stehen wortgleich im Quelltext und im deutschen Paket", () => {
    const paare = tabellen(alles);
    expect(paare.length).toBeGreaterThan(15);
    const fehler: string[] = [];
    for (const [satz, k] of paare) {
      if (text("de", k) !== satz) fehler.push(`${k}: de weicht vom Satz ab`);
      if (alles.split(`"${satz}"`).length - 1 < 2) fehler.push(`${k}: Satz kommt außerhalb der Tabelle nicht mehr vor: ${satz}`);
    }
    expect(fehler).toEqual([]);
  });

  it("kein neuer log_activity-Aufruf ohne Schlüssel", () => {
    const ist: Record<string, number> = {};
    for (const [datei, quelle] of quellen) {
      const n = ohneSchluessel(quelle);
      if (n) ist[datei] = n;
    }
    expect(ist).toEqual(OHNE_SCHLUESSEL);
  });

  it("Gegenprobe: der Zähler erkennt einen Aufruf ohne Schlüssel", () => {
    expect(ohneSchluessel('log_activity_handle(&app, ActivityLevel::Info, "neu", None);')).toBe(1);
    expect(ohneSchluessel('log_activity_handle_t(&app, ActivityLevel::Info, akt("activity.x", &[]), "neu", None);')).toBe(0);
    expect(ohneSchluessel('aufgeschoben.handle(\n app,\n ActivityLevel::Info, "x".into(), None);')).toBe(1);
    expect(ohneSchluessel("fn log_activity(\n")).toBe(0);
  });
});
