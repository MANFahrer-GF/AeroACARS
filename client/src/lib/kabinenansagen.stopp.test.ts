// Kabinenansagen: Aus- und gleich wieder Einschalten darf keine neue
// Ansage liegen lassen (Codex-QS Runde 3). Seit beta.2 spielt Rust den Ton
// (`kabinen_ton_spielen`), das Fenster steuert nur die Reihenfolge.
import { beforeEach, describe, expect, it, vi } from "vitest";

const aufrufe: { cmd: string; args?: Record<string, unknown> }[] = [];
let stoppZaehler = 0;
const hoerer: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("./ipc", () => ({
  isTauri: true,
  formatIpcError: (e: unknown) => String(e),
  listen: vi.fn(async (ereignis: string, cb: (e: { payload: unknown }) => void) => {
    hoerer[ereignis] = cb;
    return () => undefined;
  }),
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    aufrufe.push({ cmd, args });
    if (cmd === "kabinen_ton_huellkurve") {
      if (args?.datei === "langsam.wav") await new Promise((r) => setTimeout(r, 40));
      return [0.2, 0.4];
    }
    if (cmd === "kabinen_wiedergabe_start") {
      // Rust antwortet nie: Die Wiedergabe darf nicht hängen.
      if (args?.id === 31) return new Promise(() => undefined);
      // Rust antwortet verzögert: Der Ton darf erst danach beginnen.
      await new Promise((r) => setTimeout(r, 30));
      aufrufe.push({ cmd: "start_bestaetigt" });
      return undefined;
    }
    if (cmd === "kabinen_ton_stoppen") {
      stoppZaehler += 1;
      return undefined;
    }
    if (cmd === "kabinen_ton_spielen" && args?.datei === "kaputt.wav") throw new Error("Gerät belegt");
    if (cmd === "kabinen_ton_spielen") {
      // Wie Rust: endet nach der Spieldauer oder sofort bei einem Stopp.
      const stand = stoppZaehler;
      for (let i = 0; i < 4 && stoppZaehler === stand; i++) await new Promise((r) => setTimeout(r, 5));
      return undefined;
    }
    return undefined;
  }),
}));
vi.stubGlobal("requestAnimationFrame", (f: () => void) => setTimeout(f, 5));

const { abspielen, stoppen, aktuell, stummSetzen, wiedergabeStarten } = await import("./kabinenansagen");

const ansage = (datei: string, rolle: "kabine" | "chime" = "kabine", id = 0) => ({
  id, slot: "boarding", rolle, zeit: 0, vorschau: false, ergebnis: null,
  teile: rolle === "chime" ? [] : [{ sprache: "de-DE", text: "x", datei, dauer_s: 1 }],
});
const gespielt = () => aufrufe.filter((a) => a.cmd === "kabinen_ton_spielen").map((a) => a.args?.datei);
const warten = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("Kabinen-Wiedergabe", () => {
  beforeEach(() => {
    aufrufe.length = 0;
  });

  it("spielt nach Stopp und sofortigem Wiedereinschalten die neue Ansage", async () => {
    abspielen(ansage("alt.wav"));
    await warten(5); // alt lädt gerade
    stoppen();
    abspielen(ansage("neu.wav"));
    await warten(1500);
    expect(gespielt()).toContain("neu.wav");
    expect(aktuell().ansage).toBeNull();
  });

  it("Stopp beendet die laufende Wiedergabe in Rust", async () => {
    abspielen(ansage("lang.wav"));
    await warten(30);
    stoppen();
    expect(aufrufe.some((a) => a.cmd === "kabinen_ton_stoppen")).toBe(true);
    await warten(1500);
  });

  it("startet nach Stopp während der Hüllkurve nicht mehr", async () => {
    abspielen(ansage("langsam.wav"));
    await warten(10); // Hüllkurve lädt noch
    stoppen();
    await warten(200);
    expect(gespielt()).not.toContain("langsam.wav");
  });

  it("nennt einen Wiedergabefehler, bis eine Ansage wieder klappt", async () => {
    abspielen(ansage("kaputt.wav"));
    await warten(1500);
    expect(aktuell().fehler).toContain("Gerät belegt");
    abspielen(ansage("gut.wav"));
    await warten(1500);
    expect(aktuell().fehler).toBeNull();
  });

  it("Chime geht ohne Datei an Rust; die Lautstärke setzt Rust selbst", async () => {
    abspielen(ansage("", "chime"));
    await warten(1500);
    const c = aufrufe.find((a) => a.cmd === "kabinen_ton_spielen");
    expect(c?.args).toEqual({ datei: null });
  });

  it("meldet den Start vor der ersten Wiedergabe, Hörproben nicht", async () => {
    abspielen(ansage("start.wav", "kabine", 7));
    await warten(1500);
    const start = aufrufe.findIndex((a) => a.cmd === "kabinen_wiedergabe_start");
    const bestaetigt = aufrufe.findIndex((a) => a.cmd === "start_bestaetigt");
    const spiel = aufrufe.findIndex((a) => a.cmd === "kabinen_ton_spielen");
    expect(start).toBeGreaterThanOrEqual(0);
    expect(aufrufe[start].args).toEqual({ id: 7 });
    expect(bestaetigt).toBeLessThan(spiel);
    aufrufe.length = 0;
    abspielen(ansage("probe.wav"));
    await warten(1500);
    expect(aufrufe.some((a) => a.cmd === "kabinen_wiedergabe_start")).toBe(false);
  });

  it("hängt nicht, wenn die Startmeldung nie beantwortet wird", async () => {
    abspielen(ansage("haengt.wav", "kabine", 31));
    await warten(3500);
    expect(gespielt()).toContain("haengt.wav");
  });

  it("Stopp während der Startmeldung: auch ein Gong fängt nicht mehr an", async () => {
    abspielen(ansage("", "chime", 21));
    await warten(10); // Startmeldung läuft noch (30 ms)
    stoppen();
    await warten(200);
    expect(aufrufe.some((a) => a.cmd === "kabinen_ton_spielen")).toBe(false);
    expect(aufrufe.find((a) => a.cmd === "kabinen_wiedergabe_ende")?.args).toEqual({ id: 21, ergebnis: "abgebrochen" });
  });

  // 1b (Rundumschlag 22.10.2026): Ansagen des Fluges spielt das Backend.
  // Rot auf v2.1.6: der Hörer reihte sie ein und rief `kabinen_ton_spielen`.
  it("Ansage aus Rust: das Fenster spielt nicht und meldet kein Ende, es zeigt nur an", async () => {
    wiedergabeStarten();
    await warten(5);
    const a = { ...ansage("backend.wav", "kabine", 41), teile: [{ sprache: "de-DE", text: "x", datei: "backend.wav", dauer_s: 1 }, { sprache: "en-GB", text: "y", datei: "b2.wav", dauer_s: 1 }] };
    hoerer["kabine-ansage"]?.({ payload: a });
    hoerer["kabine-teil"]?.({ payload: { id: 41, teil: 1 } });
    await warten(50);
    expect(aktuell().ansage?.id).toBe(41);
    expect(aktuell().teil).toBe(1);
    expect(gespielt()).toEqual([]);
    expect(aufrufe.some((x) => x.cmd === "kabinen_wiedergabe_start" || x.cmd === "kabinen_wiedergabe_ende")).toBe(false);
    hoerer["kabine-ende"]?.({ payload: { id: 41, ergebnis: "fehler", fehler: "Gerät belegt" } });
    expect(aktuell().ansage).toBeNull();
    expect(aktuell().fehler).toBe("Gerät belegt");
    // Ein Ende einer anderen Ansage ändert die Anzeige nicht.
    hoerer["kabine-ansage"]?.({ payload: { ...a, id: 42 } });
    hoerer["kabine-teil"]?.({ payload: { id: 42, teil: 0 } });
    hoerer["kabine-ende"]?.({ payload: { id: 41, ergebnis: "gespielt", fehler: null } });
    expect(aktuell().ansage?.id).toBe(42);
    hoerer["kabine-ende"]?.({ payload: { id: 42, ergebnis: "gespielt", fehler: null } });
    expect(aktuell().fehler).toBeNull();
    await stummSetzen(true);
    expect(aufrufe.find((x) => x.cmd === "kabinen_stumm")?.args).toEqual({ stumm: true });
    await stummSetzen(false);
  });
});
