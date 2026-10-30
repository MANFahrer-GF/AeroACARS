// LAN-Browser (iPad über die LAN-Brücke): iPadOS schiebt beim Fokus eines
// Eingabefelds das GANZE Dokument nach oben, damit das Feld über der
// Tastatur liegt. Der Anwendungsrahmen rollt aber nur in `.app`, nie als
// Dokument — den Versatz holte deshalb nichts zurück. In der Home-Bildschirm-
// App mit Hardware-Tastatur blieb er stehen: Kopfzeile, Pilotenleiste und
// Eingabefeld oben aus dem Bild, darunter nur noch schwarzer Hintergrund
// (Michel, Pilotenchat, 30.10.2026).
//
// Abhilfe: Solange der sichtbare Bereich für die Oberfläche reicht (keine
// Tastatur oder nur die schmale Leiste der Hardware-Tastatur), nimmt der
// Rahmen dessen Höhe an (`--sicht-h`) und das Dokument bleibt bei 0 — das
// Eingabefeld sitzt dann am unteren Rand, ohne dass etwas verschoben wird.
// Bei voller Bildschirmtastatur bleibt zu wenig Höhe (Querformat ~266 pt,
// Chat-Eingabe abgeschnitten); dort darf iPadOS das Feld wie gewohnt über die
// Tastatur schieben, und beim Schließen holt `anpassen` die Seite zurück.
// Im Desktop-Client (Tauri) gibt es diesen Versatz nicht — dort bleibt alles
// beim Alten.

/** Unter dieser Höhe (CSS-px) passt der Chat nicht mehr samt Eingabe hinein. */
const MIN_HOEHE = 450;

export function sichtbereichHalten(): void {
  const vv = window.visualViewport;
  const wurzel = document.documentElement;
  wurzel.classList.add("im-browser");
  const anpassen = () => {
    const hoehe = Math.round(vv?.height ?? window.innerHeight);
    if (hoehe < MIN_HOEHE) {
      wurzel.style.removeProperty("--sicht-h");
      return;
    }
    wurzel.style.setProperty("--sicht-h", `${hoehe}px`);
    if (window.scrollY !== 0 || window.scrollX !== 0) window.scrollTo(0, 0);
  };
  vv?.addEventListener("resize", anpassen);
  vv?.addEventListener("scroll", anpassen);
  window.addEventListener("scroll", anpassen);
  anpassen();
}
