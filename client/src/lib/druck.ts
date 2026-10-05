// Druck-Schalter für die Landungsabschnitte (05.10.2026).
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN).
//
// Der PDF-Bericht druckt dieselben Abschnitte wie der Bildschirm. Auf
// Papier lässt sich nichts aufklappen — wer im Bericht `true` setzt,
// bekommt Aufklapper (Forensik-Details, Sprit je Wegpunkt, Rohdaten)
// geöffnet. Auf dem Bildschirm bleibt alles wie bisher (`false`).
import { createContext, useContext } from "react";

export const DruckKontext = createContext(false);

/** Wird gerade für den Druck gerendert? */
export function useDruck(): boolean {
  return useContext(DruckKontext);
}
