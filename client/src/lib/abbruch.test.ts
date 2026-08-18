import { describe, it, expect } from "vitest";
import { mitZeitgrenze } from "./abbruch";

/**
 * QS 18.08.2026 — Thomas: „brauchen wir den Fix nicht auch im Client?"
 *
 * Auf dem Live-Server hing ein toter Fremddienst ohne Zeitgrenze im Anfrageweg
 * und machte aus 1,2 s Kartenabruf 20,8 s. Im Client war dieselbe Lücke: die
 * beiden Karten-Abrufe warteten unbegrenzt.
 *
 * Geprüft wird mit ECHTEN, aber winzigen Zeiten. `AbortSignal.timeout` hört
 * nicht auf die gestellte Uhr eines Testlaufs — ein Test mit gefälschten
 * Timern lief schlicht in die Zeitüberschreitung und hätte nichts bewiesen.
 */

const gleich = () => new Promise((r) => setTimeout(r, 40));

describe("mitZeitgrenze", () => {
  it("bricht ab, wenn die Zeit ablaeuft", async () => {
    const s = mitZeitgrenze(undefined, 10)!;
    expect(s.aborted).toBe(false);
    await gleich();
    expect(s.aborted, "die Zeitgrenze hat nicht ausgeloest").toBe(true);
  });

  it("bricht ab, wenn der Aufrufer abbricht — auch lange vor der Zeit", async () => {
    const c = new AbortController();
    const s = mitZeitgrenze(c.signal, 60_000)!;
    expect(s.aborted).toBe(false);
    c.abort();
    await gleich();
    expect(s.aborted, "der Abbruch des Aufrufers kommt nicht durch").toBe(true);
  });

  it("laesst beides in Ruhe, solange nichts eintritt", async () => {
    const c = new AbortController();
    const s = mitZeitgrenze(c.signal, 60_000)!;
    await gleich();
    expect(s.aborted).toBe(false);
  });

  it("gibt ohne Aufrufer-Signal die reine Zeitgrenze zurueck", () => {
    const s = mitZeitgrenze(undefined, 60_000);
    expect(s).toBeInstanceOf(AbortSignal);
    expect(s!.aborted).toBe(false);
  });

  it("kommt ohne AbortSignal.timeout klaglos aus", () => {
    // Aeltere Webansicht: dann gilt nur das Signal des Aufrufers. Der Abruf
    // ist so ungeschuetzt wie vorher — aber nichts stuerzt ab.
    const echt = AbortSignal.timeout;
    try {
      // @ts-expect-error — absichtlich entfernt
      AbortSignal.timeout = undefined;
      const c = new AbortController();
      expect(mitZeitgrenze(c.signal, 10)).toBe(c.signal);
      expect(mitZeitgrenze(undefined, 10)).toBeUndefined();
    } finally {
      AbortSignal.timeout = echt;
    }
  });
});
