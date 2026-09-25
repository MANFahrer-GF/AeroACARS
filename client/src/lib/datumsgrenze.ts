// Linien ueber die Datumsgrenze.
//
// MapLibre verbindet zwei Punkte immer auf dem kurzen Weg in LAENGENGRADEN.
// Von 179° O nach 179° W sind das 358° — die Linie lief quer ueber die ganze
// Karte statt 2° ueber die Beringsee (Screenshot 25.09.2026, Route ueber die
// Aleuten). Abhilfe: die Laengen fortlaufend weiterzaehlen (181°, 182° …);
// MapLibre zeichnet Werte ausserhalb ±180 nahtlos in der Nachbarkopie der
// Welt weiter. Dieselbe Regel steht in aeroacars-live
// (`webapp/src/data/geo.ts`) — beide Karten sollen gleich zeichnen.

/** `lon` um volle 360° verschoben, so dass es am naechsten an `vorher` liegt. */
export function laengeNebenVorgaenger(lon: number, vorher: number): number {
  return lon + 360 * Math.round((vorher - lon) / 360);
}

/** Linie ohne Sprung ueber die Datumsgrenze: jeder Punkt hoechstens 180°
 *  Laenge vom vorigen entfernt. Die Breite bleibt unveraendert. */
export function ohneDatumsgrenzenSprung(
  coords: [number, number][],
): [number, number][] {
  const out: [number, number][] = [];
  for (const [lon, lat] of coords) {
    const vorher = out[out.length - 1];
    out.push([vorher ? laengeNebenVorgaenger(lon, vorher[0]) : lon, lat]);
  }
  return out;
}
