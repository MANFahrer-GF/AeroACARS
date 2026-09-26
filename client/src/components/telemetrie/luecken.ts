/**
 * Luecken im Telemetrie-Strom schliessen (v1.8.2).
 *
 * Auf dem Tablet meldet die Oberflaeche ihr Abo erst, wenn die Verbindung
 * steht; nach einer Unterbrechung ebenso. Frames dazwischen kommen nie an
 * (Codex-Befund, Nachpruefung v1.8.1). Statt eine Bestaetigung ins Protokoll
 * zu bauen, erkennt die Oberflaeche die Luecke am Zeitstempel und holt den
 * Verlauf des Sim-PCs nach — der deckt die letzten fuenf Minuten ab.
 */

import type { Frame } from "./typen";

/** Ab diesem Abstand zweier Frames (ms) gilt der Strom als unterbrochen.
 *  Der Strom liefert 10–20 Frames/s, der Verlauf 10/s. */
export const LUECKE_MS = 1000;
/** Hoechstens so oft nachladen (ms) — auch wenn der Simulator pausiert und
 *  der Verlauf die Luecke selbst enthaelt. */
export const NACHLADEN_ABSTAND_MS = 5000;

export function hatLuecke(vorher: Frame | null, neu: Frame): boolean {
  return !!vorher && neu.t - vorher.t > LUECKE_MS;
}

/** Zwei nach Zeit sortierte Listen zusammenfuehren. Jeder Zeitstempel nur
 *  einmal — auch wenn eine Liste ihn in sich mehrfach hat (stehender
 *  Simulator); bei Gleichstand bleibt die vorhandene Fassung. Danach auf die
 *  letzten `fensterMs` vor dem neuesten Frame gekuerzt. */
export function zusammenfuehren(
  vorhanden: Frame[],
  nachgeladen: Frame[],
  fensterMs = Infinity,
): Frame[] {
  const aus: Frame[] = [];
  const dazu = (f: Frame) => {
    const letzter = aus[aus.length - 1];
    if (!letzter || f.t > letzter.t) aus.push(f);
  };
  let i = 0;
  let j = 0;
  while (i < vorhanden.length || j < nachgeladen.length) {
    const a = vorhanden[i];
    const b = nachgeladen[j];
    if (b === undefined || (a !== undefined && a.t <= b.t)) {
      dazu(a);
      i++;
    } else {
      dazu(b);
      j++;
    }
  }
  if (aus.length && Number.isFinite(fensterMs)) {
    const grenze = aus[aus.length - 1].t - fensterMs;
    let weg = 0;
    while (weg < aus.length && aus[weg].t < grenze) weg++;
    if (weg) aus.splice(0, weg);
  }
  return aus;
}
