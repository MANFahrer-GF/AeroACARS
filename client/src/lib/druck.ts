// Druck-Schalter für die Landungsabschnitte (05.10.2026).
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN).
//
// Der PDF-Bericht druckt dieselben Abschnitte wie der Bildschirm. Auf
// Papier lässt sich nichts aufklappen — wer im Bericht `true` setzt,
// bekommt Aufklapper (Forensik-Details, Rohdaten) geöffnet. Die Sprit-
// Tabelle je Wegpunkt bleibt zu — sie füllte bei Langstrecken sechs Seiten. Auf dem Bildschirm bleibt alles wie bisher (`false`).
import { createContext, useContext } from "react";

export const DruckKontext = createContext(false);

/** Wird gerade für den Druck gerendert? */
export function useDruck(): boolean {
  return useContext(DruckKontext);
}
