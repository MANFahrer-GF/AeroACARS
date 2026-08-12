// Die beiden Schalter des Pilotenchats — und ihr Grundzustand.
//
// Beide stehen beim ersten Start auf AN: der Chat ist da, wenn man ihn
// braucht, und wer ihn nicht will, schaltet ihn in den Einstellungen ab
// (Extras → Pilotenchat). Deshalb gilt nur ein ausdrücklich gespeichertes
// "0" als Aus — ein leerer Speicher, ein neuer Rechner oder ein
// zurückgesetztes Profil landen wieder beim Grundzustand.
//
// Die Umkehrung wäre die stille Falle: ein Standard von "aus" hieße, dass
// nach jeder Neuinstallation niemand erreichbar ist, ohne dass es jemand
// merkt.

export const CHAT_AN_STORAGE_KEY = "aeroacars.chat.an";
export const CHAT_TON_STORAGE_KEY = "aeroacars.chat.ton";

function anAusserAusdruecklichAus(key: string): boolean {
  try {
    return localStorage.getItem(key) !== "0";
  } catch {
    // Kein Speicher (privater Modus, gesperrtes Profil): der Grundzustand
    // gilt trotzdem.
    return true;
  }
}

/** Ist der Pilotenchat eingeschaltet? Standard: ja. */
export function chatAnGeladen(): boolean {
  return anAusserAusdruecklichAus(CHAT_AN_STORAGE_KEY);
}

/** Klingelt es bei neuen Nachrichten? Standard: ja. */
export function chatTonGeladen(): boolean {
  return anAusserAusdruecklichAus(CHAT_TON_STORAGE_KEY);
}

export function chatAnSpeichern(an: boolean): void {
  try { localStorage.setItem(CHAT_AN_STORAGE_KEY, an ? "1" : "0"); } catch { /* egal */ }
}

export function chatTonSpeichern(an: boolean): void {
  try { localStorage.setItem(CHAT_TON_STORAGE_KEY, an ? "1" : "0"); } catch { /* egal */ }
}
