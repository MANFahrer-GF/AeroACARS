// v1.8.2: Abo-Ereignisse ueber die LAN-Bruecke. `listen("telemetrie-frame")`
// meldet das Abo ueber die Ereignisverbindung und kehrt erst zurueck, wenn der
// Sim-PC es bestaetigt — sonst holte die Oberflaeche ihren Verlauf, waehrend
// der Server die Frames noch verwirft (Codex-Befund).
import { describe, it, expect, vi, afterEach } from "vitest";

class FakeWS {
  static OPEN = 1;
  static CONNECTING = 0;
  static letzte: FakeWS | null = null;
  readyState = 0;
  gesendet: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public url: string) {
    FakeWS.letzte = this;
  }
  send(s: string) {
    this.gesendet.push(s);
  }
  close() {}
  oeffnen() {
    this.readyState = 1;
    this.onopen?.();
  }
  empfangen(event: string, payload: unknown) {
    this.onmessage?.({ data: JSON.stringify({ event, payload }) });
  }
}
(globalThis as unknown as { WebSocket: unknown }).WebSocket = FakeWS;
localStorage.setItem("aa-remote-token", "test-token");

const { listen } = await import("./ipc");

afterEach(() => vi.useRealTimers());

describe("Abo-Ereignisse ueber die LAN-Bruecke", () => {
  it("meldet das Abo nach dem Oeffnen und wartet auf die Bestaetigung", async () => {
    let fertig = false;
    const p = listen("telemetrie-frame", () => undefined).then((ab) => {
      fertig = true;
      return ab;
    });
    const ws = FakeWS.letzte!;
    // Socket noch nicht offen: nichts gesendet, noch nicht fertig.
    await Promise.resolve();
    expect(ws.gesendet).toEqual([]);
    expect(fertig).toBe(false);
    ws.oeffnen();
    expect(ws.gesendet.map((s) => JSON.parse(s))).toEqual([{ abo: "telemetrie-frame", an: true }]);
    await Promise.resolve();
    expect(fertig).toBe(false);
    ws.empfangen("telemetrie-abo", { an: true });
    const ab = await p;
    expect(fertig).toBe(true);
    // Abmelden meldet das Abo ab.
    ab();
    expect(JSON.parse(ws.gesendet.at(-1)!)).toEqual({ abo: "telemetrie-frame", an: false });
  });

  it("ohne Bestaetigung geht es nach der Wartezeit trotzdem weiter", async () => {
    vi.useFakeTimers();
    const p = listen("telemetrie-frame", () => undefined);
    await vi.advanceTimersByTimeAsync(3000);
    await expect(p).resolves.toBeTypeOf("function");
  });

  it("gewoehnliche Ereignisse melden kein Abo", async () => {
    const ws = FakeWS.letzte!;
    const vorher = ws.gesendet.length;
    const ab = await listen("integrity-flag", () => undefined);
    expect(ws.gesendet.length).toBe(vorher);
    ab();
  });
});
