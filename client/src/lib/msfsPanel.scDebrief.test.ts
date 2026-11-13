// MSFS-Panel / Flow-Widget (panel.js): Landeprotokoll beim Shared-Cockpit-
// Partner. Laedt das echte panel.js (eine Quelle fuer Panel, Flow-Widget und
// Browser-HUD) mit Schein-fetch und falscher Uhr und laesst den Sekundentakt
// laufen, also die ganze Kette statusTakt -> neuerStatus -> holeDebrief ->
// zeichne.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// vitest laeuft im Ordner client/.
const QUELLE = resolve(
  process.cwd(),
  "../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.js",
);

type Status = Record<string, unknown> | null;

let status: Status = null;
let debrief: unknown = null;
let abrufe: Record<string, number> = {};

function antwort(body: unknown) {
  return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) });
}

function lade() {
  document.body.innerHTML = '<div class="aa2-strip"></div>';
  // Frisch je Test: das Skript legt die Vorgaengerin (letzter Test) selbst still.
  new Function(readFileSync(QUELLE, "utf8"))();
}

async function takte(n: number) {
  for (let i = 0; i < n; i++) await vi.advanceTimersByTimeAsync(1000);
}

const text = (sel: string) => (document.querySelector(sel) as HTMLElement | null)?.textContent ?? "";
const sichtbar = (sel: string) => {
  const e = document.querySelector(sel) as HTMLElement | null;
  return !!e && e.style.display !== "none";
};

// Sichtbare Zellen als "Beschriftung Wert".
const zellen = () =>
  Array.from(document.querySelectorAll<HTMLElement>(".aa2-cell"))
    .filter((z) => z.style.display !== "none")
    .map((z) => `${z.querySelector(".aa2-lbl")?.textContent} ${z.querySelector(".aa2-val")?.textContent}`);

const basis = {
  pirep_id: "P1",
  callsign: "GSG 1",
  landing_at: "2026-11-13T10:00:00Z",
  landing_rate_fpm: null,
  landing_g_force: null,
};

const gastgeber = {
  score_numeric: 91,
  score_label: "SC - Landung von Anna",
  landing_rate_fpm: -180.4,
  landing_scored_g_force: 1.12,
  bounce_count: 0,
  runway_match: null,
  sc_kennzeichen: "SC - Landung von Anna",
};

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-13T10:05:00Z"));
  status = null;
  debrief = null;
  abrufe = {};
  vi.stubGlobal("fetch", (url: string) => {
    const pfad = url.replace(/^http:\/\/[^/]+/, "").replace(/\?.*$/, "");
    abrufe[pfad] = (abrufe[pfad] || 0) + 1;
    if (pfad === "/panel/status") return antwort(status);
    if (pfad === "/panel/debrief") return antwort(debrief);
    return antwort([]);
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("panel.js: Landung des Gastgebers beim Partner", () => {
  it("Note kommt erst in blocks_on: Panel holt einmal neu und zeigt die Gastgeber-Werte", async () => {
    lade();
    // Direkt nach dem Aufsetzen: Server haelt, noch nichts zu holen.
    status = { ...basis, phase: "landing", landing_score_finalized: false, sc_landung_stand: null };
    await takte(3);
    status = { ...basis, phase: "taxi_in", landing_score_finalized: false, sc_landung_stand: null };
    await takte(3);
    expect(abrufe["/panel/debrief"] ?? 0).toBe(0);
    expect(text(".aa2-state")).toBe("Landung wird ausgewertet");

    // Am Stand ohne Note: normale Lage, kein Abruf.
    status = { ...basis, phase: "blocks_on", landing_score_finalized: true, sc_landung_stand: null };
    await takte(5);
    expect(abrufe["/panel/debrief"] ?? 0).toBe(0);

    // Note des Gastgebers ist da.
    debrief = gastgeber;
    status = { ...basis, phase: "blocks_on", landing_score_finalized: true, sc_landung_stand: "1|a" };
    await takte(2);
    expect(abrufe["/panel/debrief"]).toBe(1);
    expect(text(".aa2-state")).toBe("Am Stand");
    expect(zellen()).toEqual(expect.arrayContaining(["Note 91", "Rate -180"]));
    expect(text(".aa2-tail")).toBe("SC - Landung von Anna");

    // Gleicher Stand: keine Dauerabfrage.
    await takte(10);
    expect(abrufe["/panel/debrief"]).toBe(1);

    // Neuere Antwort (z. B. angeglichene Sinkrate): genau ein Abruf mehr.
    debrief = { ...gastgeber, landing_rate_fpm: -150 };
    status = { ...basis, phase: "pirep_submitted", landing_score_finalized: true, sc_landung_stand: "1|b" };
    await takte(10);
    expect(abrufe["/panel/debrief"]).toBe(2);
    expect(text(".aa2-state")).toBe("PIREP eingereicht");
    expect(zellen()).toEqual(["Gesendet OK", "Note 91", "Rate -150"]);
    expect(text(".aa2-tail")).toBe("SC - Landung von Anna");
  });

  it("Partner ohne Note in 'ergebnis': kein Abruf je Sekunde", async () => {
    lade();
    status = { ...basis, phase: "landing", landing_score_finalized: true, sc_landung_stand: null };
    await takte(10);
    expect(abrufe["/panel/debrief"] ?? 0).toBe(0);
  });

  it("Solo unveraendert: genau ein Abruf auf der Kante, Am Stand ohne Note", async () => {
    lade();
    const solo = { ...basis, landing_rate_fpm: -200, landing_g_force: 1.2 };
    status = { ...solo, phase: "landing", landing_score_finalized: false };
    await takte(3);
    expect(abrufe["/panel/debrief"] ?? 0).toBe(0);
    debrief = { score_numeric: 80, score_label: "smooth", landing_rate_fpm: -200, bounce_count: 0 };
    status = { ...solo, phase: "landing", landing_score_finalized: true };
    await takte(10);
    expect(abrufe["/panel/debrief"]).toBe(1);
    expect(text(".aa2-score .aa2-xl")).toBe("80");
    status = { ...solo, phase: "blocks_on", landing_score_finalized: true };
    await takte(10);
    expect(abrufe["/panel/debrief"]).toBe(1);
    expect(text(".aa2-state")).toBe("Am Stand");
    expect(zellen().some((z) => z.startsWith("Note") || z.startsWith("Rate"))).toBe(false);
    expect(sichtbar(".aa2-tail")).toBe(false);
  });
});
