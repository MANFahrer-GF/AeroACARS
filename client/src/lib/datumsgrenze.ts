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
    // Ein kaputter Punkt faellt raus — er darf nicht zum Bezug fuer alle
    // folgenden werden, sonst waere der ganze Rest der Linie NaN.
    if (!Number.isFinite(lon) || !Number.isFinite(lat)) continue;
    const vorher = out[out.length - 1];
    // Der erste Punkt kommt in den Bereich ±180, damit die Linie nicht
    // jenseits der gezeichneten Weltkopien beginnt.
    out.push([
      vorher
        ? laengeNebenVorgaenger(lon, vorher[0])
        : laengeNebenVorgaenger(lon, 0),
      lat,
    ]);
  }
  return out;
}

/** Fuer eine UNGEORDNETE Punktmenge (etwa alle Kollegen auf der Karte):
 *  die Punkte auf den kleinsten Laengenbogen legen, der sie alle umfasst.
 *  Zwei Flieger bei 179° und −179° liegen danach bei 179° und 181° — ein
 *  Ausschnitt von 2° statt 358°. Die Linienfunktion oben haengt von der
 *  Reihenfolge ab und taugt dafuer nicht (Codex-QS 25.09.2026). */
export function punkteAufKleinstemBogen(
  pts: [number, number][],
): [number, number][] {
  if (pts.length < 2) return pts;
  // Werte im Bereich bleiben bitgenau, wie sie sind — Rechnen mit % runden.
  const norm = (lon: number) =>
    lon >= -180 && lon < 180 ? lon : ((((lon + 180) % 360) + 360) % 360) - 180;
  const laengen = pts.map(([lon]) => norm(lon)).sort((a, b) => a - b);
  // Die groesste Luecke zwischen zwei benachbarten Laengen (ringsum) ist der
  // Teil der Welt, den der Ausschnitt NICHT braucht. Der Bogen beginnt dahinter.
  let luecke = 360 - (laengen[laengen.length - 1] - laengen[0]);
  let start = laengen[0];
  for (let i = 1; i < laengen.length; i++) {
    const l = laengen[i] - laengen[i - 1];
    if (l > luecke) {
      luecke = l;
      start = laengen[i];
    }
  }
  return pts.map(([lon, lat]) => {
    const n = norm(lon);
    return [n < start ? n + 360 : n, lat];
  });
}

/** Liegt die Kante genau auf der Datumsgrenze (beide Enden bei ±180°)? */
function kanteAufDatumsgrenze(
  a: GeoJSON.Position,
  b: GeoJSON.Position,
): boolean {
  const auf = (lon: number) => Math.abs(Math.abs(lon) - 180) < 1e-9;
  // Auch +180 → −180 direkt: diese Kante liefe sonst einmal um die Welt.
  return auf(a[0]) && auf(b[0]);
}

/** Umriss von Flaechen OHNE die Schnittkante an der Datumsgrenze.
 *
 *  Die VATSpy-Grenzen teilen Pazifik-FIRs (Anchorage, Oakland Oceanic,
 *  Magadan, Nadi, Auckland) sauber bei ±180° in zwei Stuecke. Die Flaeche
 *  sieht dadurch richtig aus, der Rand aber zeichnet an 180° eine senkrechte
 *  Naht mitten durch den Sektor (Kartenbild 25.09.2026). Hier fallen genau
 *  diese Kanten weg; alle anderen bleiben, die Eigenschaften (Farbe usw.)
 *  wandern mit. Nur fuer die LINIEN-Ebene — Flaeche und Klicks nutzen
 *  weiter die Polygone. */
export function umrissOhneNaht(
  fc: GeoJSON.FeatureCollection,
): GeoJSON.FeatureCollection<GeoJSON.MultiLineString> {
  const features: GeoJSON.Feature<GeoJSON.MultiLineString>[] = [];
  for (const f of fc.features) {
    const g = f.geometry;
    if (!g || (g.type !== "Polygon" && g.type !== "MultiPolygon")) continue;
    const ringe = g.type === "Polygon" ? g.coordinates : g.coordinates.flat();
    const linien: GeoJSON.Position[][] = [];
    for (const ring of ringe) {
      let zug: GeoJSON.Position[] = [];
      for (let i = 0; i < ring.length; i++) {
        const p = ring[i];
        if (i > 0 && kanteAufDatumsgrenze(ring[i - 1], p)) {
          if (zug.length >= 2) linien.push(zug);
          zug = [p];
        } else {
          zug.push(p);
        }
      }
      if (zug.length >= 2) linien.push(zug);
    }
    if (linien.length > 0) {
      features.push({
        type: "Feature",
        properties: f.properties,
        geometry: { type: "MultiLineString", coordinates: linien },
      });
    }
  }
  return { type: "FeatureCollection", features };
}
