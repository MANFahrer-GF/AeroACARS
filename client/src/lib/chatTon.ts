// Der Benachrichtigungston des Pilotenchats.
//
// Eigenes Modul, weil er NICHT in der Chat-Ansicht wohnen darf: die wird nur
// gerendert, solange der Reiter offen ist. Genau dann braucht man aber keinen
// Ton — man sieht ja hin. Gebraucht wird er, wenn man woanders ist, und dort
// war er bis zum QS-Befund vom 12.08.2026 stumm. Also lebt er hier und wird
// vom dauerhaft laufenden Empfänger in App.tsx gerufen.
//
// Erzeugt statt abgespielt: keine Datei im Paket, keine Ladezeit, und die
// Lautstärke lässt sich phasenabhängig steuern.

/** Phasen, in denen der Ton leiser wird — es wird eng, aber noch nicht kritisch. */
const LEISE = new Set(["DESCENT", "APPROACH"]);
/** Phasen, in denen gar nichts klingelt. Da hat niemand eine Hand frei. */
const STILL = new Set(["FINAL", "LANDING", "TAKEOFF_ROLL", "TAKEOFF"]);

export type Tonart = "normal" | "direkt" | "ops";

/** Wie laut darf es in dieser Flugphase sein? */
export function lautstaerkeFuerPhase(phase: string | null | undefined): "voll" | "leise" | "still" {
  if (!phase) return "voll";
  if (STILL.has(phase)) return "still";
  if (LEISE.has(phase)) return "leise";
  return "voll";
}

/**
 * Zwei kurze Sinustöne, eine Quarte auseinander — nah am echten ACARS-Ruf und
 * deutlich leiser als eine Discord-Benachrichtigung. Bei einer
 * Direktnachricht drei Töne, damit man den Unterschied hört, ohne hinzusehen.
 */
export function spieleChatTon(art: Tonart, phase: string | null | undefined): void {
  const staerke = lautstaerkeFuerPhase(phase);
  if (staerke === "still") return;
  try {
    const Ctx =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    const lautheit = staerke === "leise" ? 0.06 : 0.14;
    // Die Flugleitung klingt anders als ein Kollege: absteigend statt
    // aufsteigend, damit man den Unterschied ohne Hinsehen hoert.
    const folge =
      art === "ops" ? [1174.7, 880, 1174.7]
        : art === "direkt" ? [880, 1174.7, 1567.9]
          : [880, 1174.7];
    folge.forEach((hz, n) => {
      const start = ctx.currentTime + n * 0.13;
      const osz = ctx.createOscillator();
      const gain = ctx.createGain();
      osz.type = "sine";
      osz.frequency.value = hz;
      // Kurzer Ein- und Ausklang, sonst knackt es hörbar.
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(lautheit, start + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.19);
      osz.connect(gain).connect(ctx.destination);
      osz.start(start);
      osz.stop(start + 0.21);
    });
    // Nach dem Ausklingen schliessen — Browser deckeln die Zahl offener
    // Audio-Kontexte (typisch ~6). Ohne das faellt der Ton auf einem langen
    // Flug irgendwann still aus, und niemand wuesste warum.
    setTimeout(() => { void ctx.close(); }, 900);
  } catch {
    /* Kein Ton verfügbar — der Chat funktioniert trotzdem. */
  }
}
