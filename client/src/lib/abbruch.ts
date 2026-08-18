/**
 * Abbruchwunsch des Aufrufers mit einer eigenen Zeitgrenze verbinden.
 *
 * **Warum es das gibt** (18.08.2026): auf dem Live-Server hing ein toter
 * Fremddienst OHNE Zeitgrenze im Anfrageweg der Karte und machte aus 1,2 s
 * Kartenabruf 20,8 s. Serverseitig behoben — aber Thomas fragte zu Recht
 * „brauchen wir den Fix nicht auch im Client?". Dort war dieselbe Lücke: die
 * beiden Karten-Abrufe (Sektoren vom Live-Server, VATSIM-Datafeed) liefen ohne
 * Zeitgrenze und hätten unbegrenzt gewartet, wenn irgendetwas auf dem Weg
 * klemmt.
 *
 * Eigener Baustein, weil beide Stellen ihn brauchen — und weil sich die
 * Verknüpfung so überhaupt prüfen lässt. Inline in einer `fetch`-Zeile ist sie
 * nur über echte Wartezeiten testbar, und `AbortSignal.timeout` hört nicht auf
 * die gestellte Uhr eines Testlaufs.
 */

/**
 * Gibt ein Signal zurück, das abbricht, sobald **eines von beidem** eintritt:
 * der Aufrufer bricht ab, oder die Zeitgrenze läuft ab.
 *
 * Fehlt `AbortSignal.timeout` oder `AbortSignal.any` in der Webansicht wider
 * Erwarten, gilt weiter nur das Signal des Aufrufers — dann ist der Abruf so
 * ungeschützt wie vorher, aber nichts ist kaputt. Beide gibt es in WebView2
 * und WKWebView seit 2024.
 */
export function mitZeitgrenze(
  signal: AbortSignal | undefined,
  ms: number,
): AbortSignal | undefined {
  const zeit = AbortSignal.timeout?.(ms);
  if (!zeit) return signal;
  if (!signal) return zeit;
  return AbortSignal.any?.([signal, zeit]) ?? signal;
}
