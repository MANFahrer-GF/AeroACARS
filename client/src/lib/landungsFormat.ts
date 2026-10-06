// Kleine gemeinsame Helfer der Landungsanzeige: Zahlformate, der bewertete
// G-Wert, Gültigkeit des Aufsetzfensters.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 in LandingPanel.tsx; die Webapp formatierte selbst (Querlage
// ohne Vorzeichen, Wind mit anderer Rundung).

import type { LandingRecord } from "./landungsDatensatz";

/// Ist diese Landung überhaupt bewertbar?
///
/// Landungen ohne ausreichende Aufzeichnung haben keine Sinkrate. Sie
/// dürfen deshalb in keine Statistik einfliessen — ein Mittelwert über
/// "keine Zahl" ist keine Zahl, und als weichste Landung dürfte eine
/// gelten, die niemand gemessen hat.
export function istBewertbar(record: { landung_nicht_bewertbar?: unknown }): boolean {
  return record.landung_nicht_bewertbar == null;
}

/// Stammen die Fensterwerte (G-Kraft, Hopser, Flare) aus einer ausreichenden
/// Aufzeichnung? Das ist nicht dasselbe wie "bewertbar": Hat MSFS die
/// Sinkrate selbst gemeldet, gibt es eine Note, aber die Werte aus unserem
/// dünnen Fenster bleiben ausgeblendet (Codex, zweite Abnahme 13.09.2026).
export function fensterWerteGueltig(r: LandingRecord): boolean {
  return istBewertbar(r) && !r.fenster_unzureichend;
}

/** `toFixed` ohne „-0": kleine negative Werte, die auf 0 runden, zeigen „0"
 *  (Abnahme 06.10.2026: „Δ -0 m"). */
function ohneMinusNull(v: number, digits: number): string {
  const s = v.toFixed(digits);
  return Number(s) === 0 ? s.replace(/^-/, "") : s;
}

export function fmtNumber(
  v: number | null | undefined,
  digits = 0,
  unit = "",
): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${ohneMinusNull(v, digits)}${unit ? ` ${unit}` : ""}`;
}

export function fmtSigned(v: number | null | undefined, digits = 0, unit = ""): string {
  if (v == null || !Number.isFinite(v)) return "—";
  const s = ohneMinusNull(v, digits);
  const sign = Number(s) > 0 ? "+" : "";
  return `${sign}${s}${unit ? ` ${unit}` : ""}`;
}

/** v0.12.3 (LE9): the G value the client scores / flags / colours on —
 *  the EMA-smoothed scored G when present, else the raw 50 Hz peak.
 *  The raw `landing_peak_g_force` is forensic-only after v0.12.3. */
export function scoreG(
  r: Pick<LandingRecord, "landing_scored_g_force" | "landing_peak_g_force">,
): number | null {
  return r.landing_scored_g_force ?? r.landing_peak_g_force ?? null;
}

/** Distanz hinter der Schwelle: der korrigierte Wert. `null` heißt „kein
 *  Bahnbezug" — dann kein Rückfall auf den rohen Float-Wert (anderer
 *  Nullpunkt). Nur ein FEHLENDES Feld (vor v1.7.15) nimmt den Rohwert. */
export function distanzHinterSchwelle(
  r: Pick<LandingRecord, "td_distance_from_threshold_m" | "landing_float_distance_m">,
): number | null {
  if (r.td_distance_from_threshold_m !== undefined) return r.td_distance_from_threshold_m ?? null;
  return r.landing_float_distance_m ?? null;
}
