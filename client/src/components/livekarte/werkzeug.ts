// Kleine Helfer, die mehrere Bausteine der Live-Karte teilen.
import type maplibregl from "maplibre-gl";

/** Eine Kartenebene anlegen, ohne die uebrigen zu gefaehrden.
 *
 *  MapLibre wirft bei einem ungueltigen Paint- oder Layout-Ausdruck. Da
 *  die Ebenen nacheinander angelegt werden, riss ein einziger Fehler
 *  alle nachfolgenden mit — zuletzt ein `case` bei `line-dasharray`
 *  (nimmt keine Ausdruecke), wodurch Lotsen, Tuerme und Sektoren
 *  verschwanden und nur der Verkehr blieb. Eine kaputte Ebene darf
 *  fehlen; sie darf nicht die Karte leeren.
 */
export function ebeneAnlegen(
  m: maplibregl.Map,
  spec: maplibregl.LayerSpecification,
  davor?: string,
): void {
  if (m.getLayer(spec.id)) return;
  try {
    m.addLayer(spec, davor);
  } catch (e) {
    console.warn(`[karte] Ebene "${spec.id}" nicht angelegt:`, e);
  }
}

export function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}
/** Great-circle-Distanz in nautischen Meilen zwischen [lon,lat]-Paaren. */
export function distNm(a: [number, number], b: [number, number]): number {
  const R = 3440.065; // Erdradius in nm
  const toRad = (d: number) => (d * Math.PI) / 180;
  const dLat = toRad(b[1] - a[1]);
  const dLon = toRad(b[0] - a[0]);
  const la1 = toRad(a[1]);
  const la2 = toRad(b[1]);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(la1) * Math.cos(la2) * Math.sin(dLon / 2) ** 2;
  return 2 * R * Math.asin(Math.min(1, Math.sqrt(h)));
}

