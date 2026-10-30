// Mit welcher Fassung ist ein Landungsdatensatz des Clients bewertet?
//
// P-14.02 / N-03: „bewertet mit 2.x“ ergibt sich aus der gespeicherten
// Fassung (`score_algorithm_version` ≤ 26). Es wird nichts neu gerechnet und
// nichts überschrieben; der Vermerk steht nur neben der gespeicherten Note.
//
// Dieselbe REGEL wie `bewertetMit2x`/`istDreiNull` der Webapp
// (webapp/src/lib/bewertungsModul.ts), nicht derselbe Code: Die Webapp liest
// Server-Nutzlast und Modul-Ausgabe, der Client seinen `LandingRecord`
// (Felder `client_version`, `score_algorithm_version`, `bewertung_status`).
import type { LandingRecord } from "./landungsDatensatz";

type Datensatz = Pick<LandingRecord, "score_numeric" | "score_algorithm_version" | "client_version" | "bewertung_status">;

/** Trägt der Datensatz die Marken eines 3.0-Clients? client_version ≥ 3,
 *  Fassung ≥ 27 oder ein `bewertung_status` (P-4.33). */
export function istDreiNull(r: Datensatz): boolean {
  const cv = typeof r.client_version === "string" ? Number.parseInt(r.client_version, 10) : NaN;
  if (Number.isFinite(cv) && cv >= 3) return true;
  if (typeof r.score_algorithm_version === "number" && r.score_algorithm_version >= 27) return true;
  return typeof r.bewertung_status === "string" && r.bewertung_status !== "";
}

/** P-14.02: gespeicherte Note, mit 2.x bewertet (Fassung ≤ 26 bzw. ohne Fassung, kein 3.0-Marker). */
export function bewertetMit2x(r: Datensatz): boolean {
  if (r.score_numeric == null) return false;
  return !istDreiNull(r);
}
