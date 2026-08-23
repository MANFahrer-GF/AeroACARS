// Mapper LandingRecord → RunwayDiagramV2Props.
// Pilot-Client-Pfad: liest direkt aus LandingRecord (entweder lokal
// persistiert in landings.json oder live aus FlightStats). Webapp hat
// einen separaten Mapper aus TouchdownDto.payload — siehe Spec
// §Mapping aus TouchdownDto.payload.

import type {
  RunwayDiagramV2Props,
  AimClass,
  TchClass,
} from "../components/RunwayDiagramV2";
import type { LandingRecord } from "../components/LandingPanel";
// NICHT aus LandingPanel importieren — das waere ein zirkulaerer Laufzeit-
// Import (LandingPanel importiert diesen Mapper). Der Typ-Import oben ist
// unkritisch, weil er beim Kompilieren verschwindet; ein Wert-Import nicht.
import { rolloutLdaMeters } from "../lib/runwayGeometry";

const FT_TO_M = 0.3048;

export function mapLandingRecordToV2Props(
  record: LandingRecord,
): RunwayDiagramV2Props | null {
  const rw = record.runway_match;
  if (!rw) return null;

  const source = ((): "navigraph" | "ourairports_fallback" | null => {
    if (rw.source === "navigraph") return "navigraph";
    if (rw.source === "ourairports_fallback") return "ourairports_fallback";
    return null;
  })();

  const td_distance_from_threshold_m =
    record.td_distance_from_threshold_m ??
    rw.touchdown_distance_from_threshold_ft * FT_TO_M;

  // v0.20.0: `length_m` ist die LDA (Landing Distance Available), NICHT die
  // physische Bahnlaenge. Das Diagramm definiert seinen `lengthM` selbst als
  // "nutzbare LANDE-Bahn nach dem displaced threshold" und rechnet
  // `totalVisualM = lengthM + ddsM` — bekam hier aber die physische Laenge.
  // Folge bei versetzter Schwelle: die Bahn wurde um die DDS zu lang
  // gezeichnet, und die Auslastungs-Pill rechnete gegen einen zu grossen
  // Nenner (32 % statt 42 %), waehrend die Kachel daneben korrekt gegen die
  // LDA rechnete. Dieselbe Bahn, dieselbe Landung, zwei Prozentzahlen.
  // rolloutLdaMeters() ist die eine Formel — dieselbe, die die Kachel nutzt.
  const ldaM = rolloutLdaMeters(rw) ?? rw.length_ft * FT_TO_M;

  return {
    airport_ident: rw.airport_ident,
    airport_name: null,
    runway_ident: rw.runway_ident,
    length_m: ldaM,
    surface: rw.surface ?? null,
    source,
    nav_cycle: rw.nav_cycle ?? null,
    displaced_threshold_m: (rw.displaced_threshold_ft ?? 0) * FT_TO_M,
    td_distance_from_threshold_m,
    td_centerline_offset_m: rw.centerline_distance_m,
    td_in_tdz: record.td_in_tdz ?? null,
    td_third: (record.td_third ?? null) as 1 | 2 | 3 | null,
    td_tdz_length_m: record.td_tdz_length_m ?? null,
    aim_point_m: record.aim_point_m ?? null,
    aim_delta_m: record.aim_delta_m ?? null,
    aim_class: (record.aim_class ?? null) as AimClass | null,
    tch_actual_ft: record.tch_actual_ft ?? null,
    tch_expected_ft: rw.tch_expected_ft ?? null,
    tch_delta_ft: record.tch_delta_ft ?? null,
    tch_class: (record.tch_class ?? null) as TchClass | null,
    pre_displaced_threshold: record.pre_displaced_threshold ?? null,
    rollout_m: record.rollout_distance_m ?? null,
    // ── v1.7.0 Bahndisziplin ──────────────────────────────────────────
    // Alle optional: Fluege von vor v1.7.0 haben sie nicht. Die Anzeige
    // muss das ehrlich zeigen ("fuer diesen Flug nicht erfasst") statt eine
    // leere Querachse zu malen, die wie ein Messwert aussieht.
    clearance_point_m: record.clearance_point_m ?? null,
    clearance_speed_kt: record.clearance_speed_kt ?? null,
    clearance_side: (record.clearance_side ?? null) as "left" | "right" | null,
    track_width_m: record.track_width_m ?? null,
    track_width_source:
      (record.track_width_source ?? null) as "type_table" | "aircraft_file" | null,
    min_edge_clearance_m: record.min_edge_clearance_m ?? null,
    max_lateral_offset_m: record.max_lateral_offset_m ?? null,
    lateral_samples: record.lateral_samples ?? null,
    surface_paved: record.surface_paved ?? null,
    overrun_m: record.overrun_m ?? null,
    // Aircraft-Daten für die Landeeinschätzung
    aircraft_icao: record.aircraft_icao ?? null,
    aircraft_title: record.aircraft_title ?? null,
    aircraft_registration: record.aircraft_registration ?? null,
    landing_weight_kg: record.landing_weight_kg ?? null,
    planned_ldw_kg: record.planned_ldw_kg ?? null,
    landing_speed_kt: record.landing_speed_kt ?? null,
    landing_pitch_deg: record.landing_pitch_deg ?? null,
    landing_bank_deg: record.landing_bank_deg ?? null,
    landing_peak_g_force: record.landing_peak_g_force ?? null,
    // v0.12.3 (LE9): EMA-scored G into the RunwayDiagram mapping path.
    landing_scored_g_force: record.landing_scored_g_force ?? null,
    headwind_kt: record.headwind_kt ?? null,
    crosswind_kt: record.crosswind_kt ?? null,
    locale: "de",
  };
}
