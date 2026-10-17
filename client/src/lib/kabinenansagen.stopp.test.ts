// Kabinenansagen: Aus- und gleich wieder Einschalten darf keine neue
// Ansage liegen lassen (Codex-QS Runde 3).
import { describe, expect, it, vi } from "vitest";

const gespielt: string[] = [];
vi.mock("./ipc", () => ({
  isTauri: false,
  listen: vi.fn(),
  invoke: vi.fn(async (_cmd: string, args?: { datei?: string }) => {
    gespielt.push(args?.datei ?? "?");
    await new Promise((r) => setTimeout(r, 20));
    return new ArrayBuffer(8);
  }),
}));

class FakeQuelle {
  buffer: unknown = null;
  onended: (() => void) | null = null;
  connect() { return { connect: () => ({ connect: () => undefined }) }; }
  start() { setTimeout(() => this.onended?.(), 10); }
  stop() { this.onended?.(); }
}
class FakeKontext {
  state = "running";
  currentTime = 0;
  destination = {};
  async resume() {}
  async decodeAudioData() { return {}; }
  createBufferSource() { return new FakeQuelle(); }
  createGain() { return { gain: { value: 1 }, connect: (x: unknown) => x }; }
  createAnalyser() { return { fftSize: 0, connect: (x: unknown) => x, getFloatTimeDomainData: () => undefined }; }
}
vi.stubGlobal("AudioContext", FakeKontext);
vi.stubGlobal("requestAnimationFrame", (f: () => void) => setTimeout(f, 5));

const { abspielen, freigeben, stoppen, aktuell } = await import("./kabinenansagen");

const ansage = (datei: string) => ({
  slot: "boarding", rolle: "kabine" as const, zeit: 0, lautstaerke: 0.8,
  teile: [{ sprache: "de-DE", text: "x", datei, dauer_s: 1 }],
});

describe("Kabinen-Wiedergabe", () => {
  it("spielt nach Stopp und sofortigem Wiedereinschalten die neue Ansage", async () => {
    abspielen(ansage("alt.wav"));
    await new Promise((r) => setTimeout(r, 5)); // alt lädt gerade
    stoppen();
    freigeben();
    abspielen(ansage("neu.wav"));
    await new Promise((r) => setTimeout(r, 1500));
    expect(gespielt).toContain("neu.wav");
    expect(aktuell().ansage).toBeNull();
  });
});
