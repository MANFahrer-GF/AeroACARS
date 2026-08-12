// Der Benachrichtigungston des Pilotenchats.
//
// Warum das eigene Tests hat: Er saß zunächst in der Chat-Ansicht — und die
// wird nur gerendert, solange der Reiter offen ist. Er klingelte also
// ausgerechnet dann nicht, wenn man woanders war, also im einzigen Fall, für
// den er gedacht ist. Das hat erst eine unabhängige Prüfung gefunden.
//
// Deshalb prüfen diese Tests, WAS gespielt wird und WANN geschwiegen wird —
// nicht, ob eine Funktion existiert.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { lautstaerkeFuerPhase, spieleChatTon } from "./chatTon";

/** Ein Aufnahmegerät statt echter Audio-Ausgabe. */
function hoerrohr() {
  const toene: number[] = [];
  const gains: number[] = [];
  const geschlossen = { n: 0 };
  class FakeCtx {
    currentTime = 0;
    destination = {};
    createOscillator() {
      const o = {
        type: "", frequency: { value: 0 },
        connect: () => o, start: () => {}, stop: () => {},
      };
      // Frequenz wird nach dem Anlegen gesetzt — beim Start einsammeln.
      const echterStart = o.start;
      o.start = () => { toene.push(o.frequency.value); echterStart(); };
      return o;
    }
    createGain() {
      const g = {
        gain: {
          setValueAtTime: () => {},
          exponentialRampToValueAtTime: (v: number) => { if (v > 0.001) gains.push(v); },
        },
        connect: () => ({ connect: () => {} }),
      };
      return g;
    }
    close() { geschlossen.n++; return Promise.resolve(); }
  }
  (window as unknown as { AudioContext: unknown }).AudioContext = FakeCtx;
  return { toene, gains, geschlossen };
}

describe("Chat-Ton", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("ein normaler Zuruf sind zwei Töne", () => {
    const h = hoerrohr();
    spieleChatTon("normal", "CRUISE");
    expect(h.toene.length).toBe(2);
  });

  it("eine Direktnachricht sind drei — hörbar anders, ohne hinzusehen", () => {
    const h = hoerrohr();
    spieleChatTon("direkt", "CRUISE");
    expect(h.toene.length).toBe(3);
  });

  it("im Sinkflug leiser, im Reiseflug voll", () => {
    const voll = hoerrohr();
    spieleChatTon("normal", "CRUISE");
    const leise = hoerrohr();
    spieleChatTon("normal", "DESCENT");
    expect(Math.max(...leise.gains)).toBeLessThan(Math.max(...voll.gains));
  });

  it("SICHERHEIT: im Endanflug bleibt es still", () => {
    for (const phase of ["FINAL", "LANDING", "TAKEOFF_ROLL", "TAKEOFF"]) {
      const h = hoerrohr();
      spieleChatTon("direkt", phase);
      expect(h.toene.length, `${phase} muss still bleiben`).toBe(0);
    }
  });

  it("ohne bekannte Phase klingelt es normal — am Boden vor dem Flug", () => {
    const h = hoerrohr();
    spieleChatTon("normal", null);
    expect(h.toene.length).toBe(2);
  });

  it("schliesst den Audio-Kontext wieder", () => {
    // Browser deckeln die Zahl offener Kontexte (typisch ~6). Ohne das
    // faellt der Ton auf einem langen Flug irgendwann still aus.
    const h = hoerrohr();
    spieleChatTon("normal", "CRUISE");
    expect(h.geschlossen.n).toBe(0);
    vi.advanceTimersByTime(1000);
    expect(h.geschlossen.n).toBe(1);
  });

  it("ohne Audio-Unterstützung wirft es nicht", () => {
    (window as unknown as { AudioContext: unknown }).AudioContext = undefined;
    expect(() => spieleChatTon("normal", "CRUISE")).not.toThrow();
  });

  it("die Lautstärke-Regel ist für sich prüfbar", () => {
    expect(lautstaerkeFuerPhase("CRUISE")).toBe("voll");
    expect(lautstaerkeFuerPhase("CLIMB")).toBe("voll");
    expect(lautstaerkeFuerPhase("DESCENT")).toBe("leise");
    expect(lautstaerkeFuerPhase("APPROACH")).toBe("leise");
    expect(lautstaerkeFuerPhase("FINAL")).toBe("still");
    expect(lautstaerkeFuerPhase(null)).toBe("voll");
  });
});
