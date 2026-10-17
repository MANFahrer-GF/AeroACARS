// Kabinenansagen: Aus- und gleich wieder Einschalten darf keine neue
// Ansage liegen lassen (Codex-QS Runde 3). Seit beta.2 spielt Rust den Ton
// (`kabinen_ton_spielen`), das Fenster steuert nur die Reihenfolge.
import { beforeEach, describe, expect, it, vi } from "vitest";

const aufrufe: { cmd: string; args?: Record<string, unknown> }[] = [];
let stoppZaehler = 0;
vi.mock("./ipc", () => ({
  isTauri: true,
  formatIpcError: (e: unknown) => String(e),
  listen: vi.fn(),
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    aufrufe.push({ cmd, args });
    if (cmd === "kabinen_ton_huellkurve") {
      if (args?.datei === "langsam.wav") await new Promise((r) => setTimeout(r, 40));
      return [0.2, 0.4];
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

const { abspielen, freigeben, stoppen, aktuell } = await import("./kabinenansagen");

const ansage = (datei: string, rolle: "kabine" | "chime" = "kabine") => ({
  slot: "boarding", rolle, zeit: 0, lautstaerke: 0.8,
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
    freigeben();
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
    freigeben();
    await warten(1500);
  });

  it("startet nach Stopp während der Hüllkurve nicht mehr", async () => {
    abspielen(ansage("langsam.wav"));
    await warten(10); // Hüllkurve lädt noch
    stoppen();
    await warten(200);
    expect(gespielt()).not.toContain("langsam.wav");
    freigeben();
  });

  it("nennt einen Wiedergabefehler, bis eine Ansage wieder klappt", async () => {
    abspielen(ansage("kaputt.wav"));
    await warten(1500);
    expect(aktuell().fehler).toContain("Gerät belegt");
    abspielen(ansage("gut.wav"));
    await warten(1500);
    expect(aktuell().fehler).toBeNull();
  });

  it("Chime geht ohne Datei an Rust, mit der Lautstärke der Ansage", async () => {
    abspielen(ansage("", "chime"));
    await warten(1500);
    const c = aufrufe.find((a) => a.cmd === "kabinen_ton_spielen");
    expect(c?.args).toEqual({ datei: null, lautstaerke: 0.8 });
  });
});
