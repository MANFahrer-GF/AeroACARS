// v1.7.29 (Pilotenwunsch 15.09.2026): Das Cockpit ist höher als die
// Startgröße des Fensters — die unteren Felder lagen unter der Kante.
//
// Beim Betreten der Seite misst dieser Hook den Inhalt für kurze Zeit nach
// (Flugdaten laden nach) und bittet das Backend, das Fenster auf die
// benötigte Höhe zu VERGRÖSSERN. Danach hört er auf: wer das Fenster später
// kleiner zieht, wird nicht wieder überstimmt. Verkleinern, Bildschirmgrenze
// und maximiertes Fenster regelt `fenster_an_inhalt_anpassen` (fenster.rs).
//
// Nur in der Desktop-App — auf dem LAN-Tablet gibt es kein natives Fenster.

import { useEffect } from "react";
import { invoke, isTauri } from "./ipc";

/** Wie lange nach dem Betreten nachgemessen wird. */
export const BEOBACHTUNG_MS = 4000;

/** Messtakt während der Beobachtung. Codex-QS P2: ein ResizeObserver allein
 *  sieht später eingefügte Kinder und reines scrollHeight-Wachstum nicht —
 *  `.app` behält wegen min-height/overflow seine Boxgröße. */
export const MESSTAKT_MS = 250;

/** Höhe in CSS-Pixeln, die der Inhaltsbereich braucht, damit nichts scrollt. */
export function benoetigteHoehe(
  fensterInnenHoehe: number,
  seite: { scrollHeight: number; clientHeight: number } | null,
  dokument: { scrollHeight: number },
): number {
  const ueberstandSeite = seite ? Math.max(0, seite.scrollHeight - seite.clientHeight) : 0;
  const ueberstandDokument = Math.max(0, dokument.scrollHeight - fensterInnenHoehe);
  return Math.ceil(fensterInnenHoehe + Math.max(ueberstandSeite, ueberstandDokument));
}

/**
 * @param aktiv         Seite ist gerade offen
 * @param neuMessenBei  ändert sich das (z. B. ein Flug beginnt), wird erneut
 *                      kurz beobachtet — der Inhalt wächst dann sprunghaft
 */
export function useFensterAnInhalt(aktiv: boolean, neuMessenBei?: unknown, selektor = "main.app"): void {
  useEffect(() => {
    if (!aktiv || !isTauri) return;
    const seite = document.querySelector<HTMLElement>(selektor);
    if (!seite) return;

    let zuletztAngefragt = 0;
    let beendet = false;
    const pruefen = () => {
      if (beendet) return;
      const noetig = benoetigteHoehe(window.innerHeight, seite, document.documentElement);
      if (noetig <= window.innerHeight + 4 || noetig <= zuletztAngefragt) return;
      zuletztAngefragt = noetig;
      invoke("fenster_an_inhalt_anpassen", { benoetigtHoehe: noetig }).catch(() => {
        /* Fenstergröße ist Komfort — ein Fehler darf die Seite nicht stören */
      });
    };

    pruefen();
    const takt = window.setInterval(pruefen, MESSTAKT_MS);
    const ende = window.setTimeout(() => {
      beendet = true;
      window.clearInterval(takt);
    }, BEOBACHTUNG_MS);

    return () => {
      beendet = true;
      window.clearInterval(takt);
      window.clearTimeout(ende);
    };
  }, [aktiv, neuMessenBei, selektor]);
}
