// Gemeinsamer Zustand und Befehlsweg für alle Aktionen.

import streamDeck from "@elgato/streamdeck";
import { befehl, lesen } from "./aeroacars.js";
import { Zustand } from "./zustand.js";

export const zustand = new Zustand();

// Befehle strikt nacheinander: so kommen sie in der Reihenfolge beim Server
// an, in der sie ausgelöst wurden. Sonst könnte ein langsamer Live-Pegel vom
// Drehen nach dem Speichern eintreffen und den Klang wieder zurückstellen.
let kette = Promise.resolve();

/**
 * `args` darf eine Funktion des Kabinenzustands sein: Sie wird erst
 * ausgewertet, wenn der Befehl an der Reihe ist — dann rechnen zwei schnelle
 * Drücke auf „lauter“ vom Ergebnis des ersten aus, nicht beide vom selben Wert.
 */
export function ausfuehren(aktion, name, args) {
  const lauf = kette.then(async () => {
    try {
      const werte = typeof args === "function" ? args(zustand.s.kabine) : args;
      if (werte == null) return;
      zustand.kabineSetzen(await befehl(name, werte));
    } catch (e) {
      streamDeck.logger.warn(`${name} fehlgeschlagen: ${e.message}`);
      // Bei Zeitüberschreitung kann AeroACARS den Befehl trotzdem ausgeführt
      // haben: Kabinenstand sofort neu holen, damit der nächste Schritt (z. B.
      // „lauter“) nicht vom alten Wert rechnet (Codex 23.10.).
      await lesen("/panel/kabine").then((k) => zustand.kabineSetzen(k)).catch(() => {});
      await aktion.showAlert();
    }
  });
  kette = lauf;
  return lauf;
}

