import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { useConfirm } from "./ConfirmDialog";
import { ForensicsBadge } from "./ForensicsBadge";
import { SinkrateForensik, scoreBasisVs } from "./SinkrateForensik";
import { GForceForensik } from "./GForceForensik";
import { RunwayDiagramV2 } from "./RunwayDiagramV2";
import { RunwayUtilizationHelpModal } from "./RunwayUtilizationHelpModal";
import { ApproachStabilityCard } from "./ApproachStabilityCard";
import { mapLandingRecordToV2Props } from "../dev/runwayDiagramV2Mapper";
// v0.5.47 — Score-Modul ist jetzt zentral, identisch zu webapp/src/
// components/landingScoring.ts. Dieselben Schwellen, Bands, Coach-Tipps
// für Pilot-App und Live-Monitor.
import {
  computeSubScores as libComputeSubScores,
  type SubScore as LibSubScore,
} from "../lib/landingScoring";

// ---- Types (mirror storage::LandingRecord on the Rust side) -------------

export interface LandingProfilePoint {
  t_ms: number;
  vs_fpm: number;
  g_force: number;
  agl_ft: number;
  on_ground: boolean;
  heading_true_deg: number;
  groundspeed_kt: number;
  indicated_airspeed_kt: number;
  pitch_deg: number;
  bank_deg: number;
}

export interface LandingRunwayMatch {
  airport_ident: string;
  runway_ident: string;
  surface: string;
  length_ft: number;
  centerline_distance_m: number;
  centerline_distance_abs_ft: number;
  side: string;
  touchdown_distance_from_threshold_ft: number;
  // v0.8.0 VPS-Navdata fields — alle optional weil pre-v0.8.0
  // landing_history.json-Eintraege diese Felder nicht haben.
  /** "navigraph" | "ourairports_fallback" */
  source?: string | null;
  /** AIRAC-Cycle wenn source = "navigraph" */
  nav_cycle?: string | null;
  /** Geographic true-course in deg (Threshold → End bearing). */
  true_course_deg?: number | null;
  /** Displaced-Threshold in ft. 0 = keine Displacement. */
  displaced_threshold_ft?: number | null;
  /** Erwartete Threshold-Crossing-Height in ft (typisch 49-55). */
  tch_expected_ft?: number | null;
  /** Glideslope-Winkel in deg (typisch 3.0). */
  glideslope_angle_deg?: number | null;
}

export interface LandingRecord {
  pirep_id: string;
  touchdown_at: string;
  recorded_at: string;
  flight_number: string;
  airline_icao: string;
  dpt_airport: string;
  arr_airport: string;
  /** v0.7.18 (B-012): aufgelöster Touchdown-Airport (real, nicht geplant).
   *  - Wenn `runway_match` zur Runway korreliert: dessen ICAO.
   *  - Sonst nächster Airport innerhalb 25 nmi.
   *  - Sonst fallback auf `arr_airport`. */
  touchdown_airport: string | null;
  /** Resolution-Source: "runway_match" / "nearest_25nm" / "planned_fallback". */
  touchdown_airport_source: string | null;
  /** Distanz vom TD-Punkt zur geplanten Destination (nmi). */
  touchdown_distance_to_destination_nm: number | null;
  /** Distanz vom TD-Punkt zum nearest Airport (nmi), nur bei nearest_25nm-Source. */
  touchdown_nearest_distance_nm: number | null;
  aircraft_registration: string | null;
  aircraft_icao: string | null;
  aircraft_title: string | null;
  sim_kind: string | null;

  score_numeric: number;
  score_label: string;
  grade_letter: string;

  landing_rate_fpm: number;
  landing_peak_vs_fpm: number | null;
  landing_g_force: number | null;
  landing_peak_g_force: number | null;
  landing_pitch_deg: number | null;
  landing_bank_deg: number | null;
  landing_speed_kt: number | null;
  landing_heading_deg: number | null;
  landing_weight_kg: number | null;
  touchdown_sideslip_deg: number | null;
  bounce_count: number;

  headwind_kt: number | null;
  crosswind_kt: number | null;

  approach_vs_stddev_fpm: number | null;
  approach_bank_stddev_deg: number | null;
  rollout_distance_m: number | null;

  planned_block_fuel_kg: number | null;
  planned_burn_kg: number | null;
  planned_tow_kg: number | null;
  planned_ldw_kg: number | null;
  planned_zfw_kg: number | null;
  actual_trip_burn_kg: number | null;
  fuel_efficiency_kg_diff: number | null;
  fuel_efficiency_pct: number | null;
  takeoff_weight_kg: number | null;
  takeoff_fuel_kg: number | null;
  landing_fuel_kg: number | null;
  block_fuel_kg: number | null;

  runway_match: LandingRunwayMatch | null;
  touchdown_profile: LandingProfilePoint[];
  approach_samples: ApproachSample[];

  // v0.5.43 — 50-Hz-TouchdownWindow Forensik. Optional weil pre-v0.5.39
  // landing_history.json-Eintraege sie nicht haben.
  vs_at_edge_fpm?: number | null;
  vs_smoothed_250ms_fpm?: number | null;
  vs_smoothed_500ms_fpm?: number | null;
  vs_smoothed_1000ms_fpm?: number | null;
  vs_smoothed_1500ms_fpm?: number | null;
  peak_g_post_500ms?: number | null;
  peak_g_post_1000ms?: number | null;
  /** v0.12.3 (LE4/LE7): EMA-geglätteter gescorter G-Wert (FOQA-Methode).
   *  Der Wert, auf dem die Landung gescort wird + den die G-Force-Card
   *  als Headline zeigt. `peak_g_post_*` bleibt der rohe Forensik-Peak. */
  landing_scored_g_force?: number | null;
  /** v0.12.3 (LE8): "ema_max" | "raw_fallback". */
  scored_g_method?: string | null;
  // v0.7.17 (B-009): G-Force-Forensik (analog vs_smoothed_*)
  g_at_edge?: number | null;
  g_smoothed_250ms_post?: number | null;
  g_median_post_500ms?: number | null;
  g_p95_post_500ms?: number | null;
  max_gear_force_n?: number | null;
  peak_vs_pre_flare_fpm?: number | null;
  vs_at_flare_end_fpm?: number | null;
  flare_reduction_fpm?: number | null;
  flare_dvs_dt_fpm_per_sec?: number | null;
  flare_quality_score?: number | null;
  flare_detected?: boolean | null;
  forensic_sample_count?: number | null;

  // v0.8.3 (#8): Forensische Bounce-Counts — surface fuer den Pilot,
  // damit „kleine" Hopser (5-14 ft, per Spec score-frei) trotzdem
  // sichtbar werden statt im UI als „0 Bounces" verloren zu gehen.
  // Quelle: touchdown_v2::compute_landing_rate Forensik-Pipeline.
  /// Hoechster gemessener AGL-Wert in Post-TD-Hopsern, ft.
  /// >= 5 ft = sichtbar (forensic), >= 15 ft = scored.
  bounce_max_agl_ft?: number | null;
  /// Anzahl Hopser >= 5 ft. Subset: forensic_bounce_count >= scored.
  /// Wenn > 0 aber bounce_count = 0 → rein score-freie Hopser.
  forensic_bounce_count?: number | null;
  /// Anzahl Hopser >= 15 ft (= was im Score bestraft wird,
  /// identisch mit bounce_count nach Override-Pfad).
  scored_bounce_count?: number | null;

  // ─── v0.7.1 Felder (Spec docs/spec/v0.7.1-landing-ux-fairness.md §5) ──
  // Phase 1: nur Felder durchreichen, keine UI-Aenderung. Phase 3
  // konsumiert sie (ForensicsBadge + StabilityDetailPanel + Sub-Score-
  // Breakdown via §3.5 getSubScores Legacy-Schutz).

  /// UX-Cutoff. 0/fehlt = pre-v0.7.1, 1+ = v0.7.1 Sub-Scores aktiv.
  ux_version?: number;
  /// Touchdown-Forensik-Version (P2.4-Fix: sauber im Record statt
  /// UI zwingt den Wert zu raten). 1 = legacy, 2 = touchdown_v2.
  forensics_version?: number;
  /// Confidence-Tagging vom Touchdown-v2-Cascade.
  /// "High" | "Medium" | "Low" | "VeryLow"
  landing_confidence?: string | null;
  /// "vs_at_impact" | "smoothed_500ms" | "smoothed_1000ms" | "pre_flare_peak"
  landing_source?: string | null;
  /// F7: Stability-v2-Felder (P2.1-A — bestehende Backend-Felder
  /// exponiert, keine neue Berechnung).
  /// `approach_vs_jerk_fpm` ist mean |ΔVS| (NICHT max).
  approach_vs_jerk_fpm?: number | null;
  approach_ias_stddev_kt?: number | null;
  approach_stable_config?: boolean | null;
  /// `approach_excessive_sink` ist bool (NICHT count).
  approach_excessive_sink?: boolean | null;
  gate_window?: GateWindow | null;
  // v0.11.0-dev: 3 weitere Stability-v2-Felder. Backend rechnet sie schon
  // (lib.rs::compute_approach_stability_v2), persistiert sie seit dieser
  // Version auch ins LandingRecord. Alte PIREPs (vor v0.11) haben die
  // Werte nicht — ApproachStabilityCard zeigt dann "—" pro Kachel.
  /// Mean |V/S − Target_V/S(3°-ILS, GS)|, fpm, über Stability-Gate.
  approach_vs_deviation_fpm?: number | null;
  /// Max |V/S − Target_V/S(3°-ILS, GS)|, fpm, für Samples unter 500 ft HAT.
  approach_max_vs_deviation_below_500_fpm?: number | null;
  /// True wenn Gate auf Height-Above-Touchdown gefiltert wurde
  /// (Airport-Elevation bekannt). False = AGL-Fallback.
  approach_used_hat?: boolean | null;
  /// Sub-Score-Breakdown aus der landing-scoring Crate (Spec §3.1
  /// SSoT). UI rendert direkt aus diesen Felder, KEIN Recompute.
  /// Bei alten PIREPs (ux_version < 1) leer/fehlt → LegacyPirepNotice.
  sub_scores?: SubScoreEntry[];

  /** v0.10.0 (#runway-utilization-score) — Algorithmus-Version des
   *  sub_scores-Arrays. Spec docs/spec/v0.10.0-runway-utilization-score.md
   *  LE11. None/0/1 = pre-v0.10 (meter-only Bahn-Auslastung); 2 = v0.10
   *  (LDA-basierter Score). UI rendert die neuen extra-Lines + erweiterten
   *  Rationale-/Warning-Keys nur wenn `>= 2`. */
  score_algorithm_version?: number | null;

  // ─── v0.7.6 P1-3: Runway-Geometry-Trust ──────────────────────────────
  // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
  // Bei trusted=false werden Centerline-Offset, Past-Threshold (= Float-
  // Distance) und der RunwayDiagram ausgeblendet — Pilot soll nicht mit
  // einer kaputten Runway-Geometrie konfrontiert werden. Rollout bleibt
  // sichtbar (kommt aus GPS-Track, nicht aus Runway-DB).
  // Backward-Compat: alte v0.7.5-PIREPs ohne diese Felder werden via
  // (trusted ?? true) wie trusted behandelt.
  runway_geometry_trusted?: boolean | null;
  /// "no_runway_match" / "icao_mismatch" / "centerline_offset_too_large"
  /// / "negative_float_distance"
  runway_geometry_reason?: string | null;

  // ─── v0.7.19 GAF-707 Accident-Detection ──────────────────────────
  // Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md.
  // Alle Felder optional — pre-v0.7.19 LandingRecords haben sie nicht.
  /// True wenn Confirmed Accident. Suspected wird hier nicht als
  /// true gespeichert; die Suspected-Variante laeuft ueber
  /// `accident_confidence === "medium"` ohne `accident=true`.
  accident?: boolean;
  /// "sim_crash" | "impact" | "off_airport_impact"
  accident_kind?: string | null;
  /// "high" (Confirmed) | "medium" (Suspected)
  accident_confidence?: string | null;
  /// Begruendungs-Strings, free-form lesbar.
  accident_reasons?: string[];
  /// ISO-8601 UTC — wann der Accident detektiert wurde. Bei Sim-Event-
  /// Pfad kann das mehrere Sekunden vor `touchdown_at` liegen.
  accident_at?: string | null;

  // ─── v0.8.0 VPS-Navdata + Runway-Awareness ────────────────────────
  // Touchdown-Quality-Assessment-Felder, Spec docs/spec/v0.8.0-vps-
  // navdata-runway-awareness.md. Identisches Wire-Format zwischen
  // LandingRecord (lokal) und TouchdownPayload (live MQTT). Alle
  // optional — pre-v0.8.0-Records haben sie nicht und der
  // OurAirports-Fallback-Pfad liefert nur die quell-agnostischen Werte
  // (TDZ/Aim/td_distance) und lässt TCH/DDS leer.
  /** Signed along-track Distanz Threshold→Touchdown in Metern. */
  td_distance_from_threshold_m?: number | null;
  /** F3 TDZ-Result: Touchdown im 900-m-Marker? None bei RWY < 1200 m. */
  td_in_tdz?: boolean | null;
  /** 1-indexed Third der RWY (1/2/3) wo der Touchdown sitzt. */
  td_third?: number | null;
  /** TDZ-Marker-Länge in Metern (für RunwayDiagram). */
  td_tdz_length_m?: number | null;
  /** F4 Aim-Delta in Metern (positiv = past, negativ = short). */
  aim_delta_m?: number | null;
  /** F4 Aim-Klassifikation: perfect|short_of_aim|past_aim|long_landing|severe */
  aim_class?: string | null;
  /** F4 Aim-Distance vom Threshold in Metern (300 oder 400). */
  aim_point_m?: number | null;
  /** F5 actual TCH at threshold-crossing (AGL ft). */
  tch_actual_ft?: number | null;
  /** F5 TCH-Delta = actual - expected. */
  tch_delta_ft?: number | null;
  /** F5 TCH-Klassifikation: on_profile|slightly_low|slightly_high|high|below_profile */
  tch_class?: string | null;
  /** F6 Pilot in Pre-Threshold-Paint gelandet (= illegal DDS-Touchdown). */
  pre_displaced_threshold?: boolean | null;
}

/// v0.7.1: Stability-Gate-Window-Metadaten (Spec §5.4).
export interface GateWindow {
  start_at_ms: number;
  end_at_ms: number;
  start_height_ft: number;
  end_height_ft: number;
  sample_count: number;
}

/// v0.7.1 SubScoreEntry — voll ausgebautes Wire-Format aus der
/// landing-scoring Crate (Spec §5.4 P1.5-A). Spiegel des Rust-Typs.
/// UI rendert direkt aus diesen Felder, kein Recompute.
export interface SubScoreEntry {
  key: string;             // "landing_rate" | "g_force" | "bounces" | ...
  score: number;           // 0-100
  points: number;          // Alias fuer score (bestehende UI nutzt .points)
  band: 'good' | 'ok' | 'bad' | 'skipped';
  label_key: string;       // i18n key z.B. "landing.sub.fuel"
  value?: string;          // formatiert: "-191 fpm" — v0.10.0 rollout: "1100 m / 3657 m  ·  30 %"
  rationale_key?: string;
  tip_key?: string;
  skipped: boolean;
  reason?: string;
  warning?: string;
  /** v0.10.0 (#runway-utilization-score) — Zusatz-Display-Zeilen (LE9),
   *  z.B. „davon ~520 m Float vor Aufsetzen", „Bahn: YMML 16, LDA 3657 m".
   *  Renderer alter Versionen ignorieren das Feld schweigend. Default
   *  bei pre-v0.10-Records: leeres Array. */
  extra?: string[];
}

export interface ApproachSample {
  vs_fpm: number;
  bank_deg: number;
  // v0.7.1 (P1.1-D + P1.3-C): Zeit/Hoehe/Flags damit Approach-Chart
  // Vorlauf/Gate/Flare-Zonen rendern kann. Alle optional —
  // alte PIREPs ohne diese Felder fallen auf Index-basierten Plot zurueck.
  t_ms?: number | null;
  agl_ft?: number | null;
  /// True wenn das Sample im Stability-Gate liegt
  /// (`MIN_HEIGHT < height <= MAX_HEIGHT` UND nicht in den letzten
  /// `FLARE_CUTOFF_MS` vor TD).
  is_scored_gate?: boolean | null;
  /// True wenn das Sample in den letzten `FLARE_CUTOFF_MS` vor TD
  /// liegt (zeitbasiert).
  is_flare?: boolean | null;
}

/** v0.12.7: Flare-Score-Aufschlüsselung — der „Flare-Score" ist
 *  Endsink-Eimer + Flare-Bonus (1:1 lib.rs:13727-13745). Offengelegt,
 *  damit der Pilot nachvollziehen kann, woher die Punkte kommen
 *  (Pilot-Befund Michel/GSG: Score 40 neben „kein Flare" wirkte wirr). */
function flareSubScores(
  vsEnd?: number | null,
  reduction?: number | null,
): { endpoint: number; bonus: number; total: number } | null {
  if (vsEnd == null || reduction == null) return null;
  const endpoint =
    vsEnd > -75 ? 100 : vsEnd > -150 ? 80 : vsEnd > -300 ? 60 : vsEnd > -500 ? 40 : 20;
  const bonus =
    reduction > 400 ? 20 : reduction > 200 ? 15 : reduction > 100 ? 10 : reduction > 50 ? 5 : 0;
  return { endpoint, bonus, total: Math.max(0, Math.min(100, endpoint + bonus)) };
}

// ---- Score breakdown ---------------------------------------------------
//
// We split the overall touchdown score into 6 sub-categories so the pilot
// can see *which* aspect of the landing pulled the grade down. Each is a
// 0-100 score with a short rationale. Thresholds are calibrated against
// FOQA-style guidelines and the existing primary score table.

export interface SubScore {
  key: string;
  points: number;
  /** Pre-formatted value to show on the card ("379 fpm", "1.10 G", …). */
  value: string;
  /** "good" | "ok" | "bad" | "skipped" — drives the colour band.
   *  v0.7.1 (P1.2-Fix): "skipped" als visuell graue Variante damit
   *  "nicht bewertet" sichtbar bleibt (Loadsheet/Fuel ohne Plan). */
  band: "good" | "ok" | "bad" | "skipped";
  /** Why we awarded this score (one short sentence). */
  rationale: string;
  /** v0.7.1 (P1.2-Fix): true wenn dieser Sub-Score nicht bewertet wurde
   *  (z.B. VFR ohne ZFW → loadsheet skipped, ohne planned_burn → fuel
   *  skipped). UI rendert eine "nicht bewertet"-Karte statt der Karte
   *  ganz auszublenden. */
  skipped?: boolean;
  /** Skip-Reason fuer i18n-Key landing.skipped_reason.* */
  skipReason?: string;
  /** v0.10.0 (#runway-utilization-score) — Extra-Display-Zeilen unter
   *  der Rationale. Wird vom Card-Renderer als Bullet-Liste gezeigt.
   *  Leeres Array → nichts gerendert (= forward-compat mit pre-v0.10
   *  Records ohne `extra`-Feld). */
  extra?: string[];
  /** v0.10.0 — Warning-Wert (z.B. "pre_displaced_threshold") für die
   *  Warning-Pill. UI lookup: `landing.warn.<warning>`. */
  warning?: string;
}

// v0.5.47 — Sub-Score-Berechnung delegiert an die zentrale Lib.
// Webapp und Client nutzen jetzt dieselben Schwellen, Bands und
// Coach-Tipps. SubScore (lokal) ist strukturidentisch zu LibSubScore.

/// v0.7.1 Phase 3 (Spec §3.5 Legacy-Schutz):
///   - ux_version >= 1 → gespeicherte sub_scores aus dem Record nutzen
///     (kein Recompute), damit Werte konsistent zum PIREP-Payload sind.
///     SKIPPED Sub-Scores BLEIBEN drin und werden als "nicht bewertet"
///     mit grauem Band gerendert (P1.2-Fix).
///   - ux_version < 1 → Legacy-Pfad mit libComputeSubScores (nur fuer
///     pre-v0.7.1-PIREPs als Backward-Compat)
///   - bei v0.7.1+ ohne sub_scores (sollte nie passieren) → Legacy-Pfad
function getSubScores(r: LandingRecord): SubScore[] {
  const ux = r.ux_version ?? 0;
  if (ux >= 1 && r.sub_scores && r.sub_scores.length > 0) {
    // Phase 3 (P1.2-Fix): skipped sind sichtbar als "nicht bewertet"
    // v0.10.0 (#runway-utilization-score): extra-Lines + warning werden
    // 1:1 vom Rust-Crate durchgereicht (SSoT — kein Recompute in TS).
    return r.sub_scores.map((s) => {
      const band: SubScore["band"] =
        s.band === "good" || s.band === "ok" || s.band === "bad"
          ? s.band
          : ("skipped" as unknown as SubScore["band"]);
      return {
        key: s.key,
        points: s.points ?? s.score,
        // skipped → menschlicher String statt leer
        value: s.skipped ? "" : (s.value ?? ""),
        band,
        rationale: (s.rationale_key ?? "").replace(/^landing\.rat\./, ""),
        skipped: s.skipped,
        skipReason: s.reason,
        extra: s.extra ?? [],
        warning: s.warning,
      };
    });
  }
  // Legacy-Pfad fuer pre-v0.7.1-PIREPs (forward-compat)
  // v0.7.17 (B-015): vs_at_edge_fpm bevorzugen — der 50-Hz-Edge-Wert
  // ist der echte FAR-25.473-Engineering-Standard. Ohne diesen Fix
  // zog der Pilot-Client den Streamer-Tick-Wert (-311 fpm in
  // EIN799-Fall), waehrend die Webapp den Edge-Wert nutzte (-265).
  // Pilot konnte die Diskrepanz nicht erklaeren.
  const peakVs =
    (r.vs_at_edge_fpm != null && r.vs_at_edge_fpm < 0
      ? r.vs_at_edge_fpm
      : null) ??
    r.landing_peak_vs_fpm ??
    r.landing_rate_fpm;
  const subs: LibSubScore[] = libComputeSubScores({
    vs_fpm: peakVs,
    peak_g_load: r.landing_peak_g_force,
    // v0.12.3 (LE8): EMA-Scored-G → sub_g_force scort diesen Wert,
    // sonst Fallback auf den rohen peak_g_load.
    scored_g_load: r.landing_scored_g_force,
    bounce_count: r.bounce_count,
    approach_vs_stddev_fpm: r.approach_vs_stddev_fpm,
    approach_bank_stddev_deg: r.approach_bank_stddev_deg,
    rollout_distance_m: r.rollout_distance_m,
    fuel_efficiency_pct: r.fuel_efficiency_pct,
  });
  return subs as SubScore[];
}

// Alias fuer Backward-Compat mit bestehenden Aufruf-Stellen
function computeSubScores(r: LandingRecord): SubScore[] {
  return getSubScores(r);
}

// ─── v0.12.0 (#runway-utilization-refinement) — TS-gerenderte Extra-Zeilen ──
//
// Spec docs/spec/v0.12.0-runway-utilization-refinement.md LE5: ab
// `score_algorithm_version >= 3` lässt das Rust-Crate das `extra`-Feld
// LEER. Die drei Bahn-Auslastungs-Extra-Zeilen (Aufsetzpunkt, Ausroll-
// strecke, Bahn) baut stattdessen der TS-Renderer aus den ohnehin
// vorhandenen Record-Feldern + i18n — damit sie sprach-fähig sind statt
// hardcoded-Deutsch. Alt-v2-Records (`< 3`) behalten ihre gespeicherten
// `extra`-Strings (Legacy).

/** Minimaler t-Typ — reicht für die Extra-Zeilen-Interpolation. */
type RolloutTFn = (key: string, opts?: Record<string, string | number>) => string;

/** Erste score_algorithm_version mit TS-gerenderten Extra-Zeilen. */
const ROLLOUT_ALGO_V3 = 3;

/** True wenn der Record v3-Scoring nutzt (TS rendert die Extra-Zeilen). */
export function isRolloutV3(r: LandingRecord): boolean {
  return (r.score_algorithm_version ?? 0) >= ROLLOUT_ALGO_V3;
}

/**
 * LDA in Metern aus dem (nested) Pilot-Client `runway_match`.
 * Spec LE5: `LDA_m = (length_ft − displaced_threshold_ft) × 0.3048`.
 * Liefert null wenn die Geometrie unbrauchbar ist (length ≤ 0, LDA ≤ 0).
 */
export function rolloutLdaMeters(rm: LandingRunwayMatch): number | null {
  if (!Number.isFinite(rm.length_ft) || rm.length_ft <= 0) return null;
  const displacedFt = rm.displaced_threshold_ft ?? 0;
  const lda = (rm.length_ft - displacedFt) * 0.3048;
  return lda > 0 ? lda : null;
}

/**
 * v0.12.0 LE4 — sprach-lokalisiertes Value-Label für die rollout-Card.
 * Zeigt die ECHTE Auslastung (raw, nicht toleranzbereinigt). Liefert null
 * wenn Felder fehlen — der Caller fällt dann auf den sprachneutralen
 * `value`-String des Rust-Crates zurück.
 */
export function buildRolloutValueLabel(
  r: LandingRecord,
  t: RolloutTFn,
): string | null {
  const rm = r.runway_match;
  if (!rm) return null;
  const lda = rolloutLdaMeters(rm);
  if (lda == null) return null;
  const td = r.td_distance_from_threshold_m;
  const rollout = r.rollout_distance_m;
  if (td == null || rollout == null) return null;
  const used = Math.max(td + rollout, rollout);
  return t("landing.rollout_extra.value_label", {
    pct: Math.round((used / lda) * 100),
    used: Math.round(used),
    lda: Math.round(lda),
  });
}

/**
 * v0.12.0 LE5 — die drei TS-gerenderten Extra-Zeilen der rollout-Card.
 * Reihenfolge: Aufsetzpunkt → Ausrollstrecke → Bahn. Jede Zeile entfällt
 * einzeln wenn ihr Quell-Feld fehlt (z.B. kein `runway_match` → keine
 * Bahn-Zeile, die anderen zwei bleiben).
 *
 * R2-P2-Fix: negatives `td_distance` (Aufsetzen VOR der Schwelle) wählt
 * den `_before`-Key und übergibt den BETRAG — nie „−50 m hinter …".
 */
export function buildRolloutExtraLines(
  r: LandingRecord,
  t: RolloutTFn,
): string[] {
  const lines: string[] = [];

  const td = r.td_distance_from_threshold_m;
  if (td != null) {
    const m = Math.round(Math.abs(td));
    const key =
      td < 0
        ? "landing.rollout_extra.touchdown_point_before"
        : "landing.rollout_extra.touchdown_point";
    lines.push(t(key, { m }));
  }

  if (r.rollout_distance_m != null) {
    lines.push(
      t("landing.rollout_extra.rollout_distance", {
        m: Math.round(r.rollout_distance_m),
      }),
    );
  }

  const rm = r.runway_match;
  if (rm) {
    const lda = rolloutLdaMeters(rm);
    if (lda != null) {
      lines.push(
        t("landing.rollout_extra.runway", {
          icao: rm.airport_ident,
          ident: rm.runway_ident,
          lda: Math.round(lda),
        }),
      );
    }
  }

  return lines;
}

/** Rationale → i18n key for the coach tip. We point straight at the
 *  fully-qualified `landing.tip.*` path so a missing translation
 *  shows up as the key (easier to spot in QA) rather than as a
 *  silent fallback. */
function coachTipKey(rationale: string): string {
  return `landing.tip.${rationale}`;
}

// ---- Helpers ------------------------------------------------------------

function gradeColor(grade: string): string {
  if (grade === "A+" || grade === "A") return "#22c55e"; // green
  if (grade === "B+" || grade === "B") return "#84cc16"; // lime
  if (grade === "C") return "#eab308"; // amber
  if (grade === "D") return "#f97316"; // orange
  return "#ef4444"; // red — F
}

function fmtNumber(
  v: number | null | undefined,
  digits = 0,
  unit = "",
): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${v.toFixed(digits)}${unit ? ` ${unit}` : ""}`;
}

function fmtSigned(v: number | null | undefined, digits = 0, unit = ""): string {
  if (v == null || !Number.isFinite(v)) return "—";
  const sign = v >= 0 ? "+" : "";
  return `${sign}${v.toFixed(digits)}${unit ? ` ${unit}` : ""}`;
}

function fmtDateTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

/** Translate the runway-side enum from the backend ("RIGHT"/"LEFT"/
 *  "CENTER") into the i18n key under `landing.side.*` so the UI
 *  reads "rechts" / "links" / "Mitte" in German. */
function sideKey(side: string): string {
  const upper = side.toUpperCase();
  if (upper === "RIGHT") return "landing.side.right";
  if (upper === "LEFT") return "landing.side.left";
  return "landing.side.center";
}

// ---- VS Curve chart -----------------------------------------------------

// v0.12.8: Touchdown-Nahaufnahme — 50-Hz-Window, exakt wie auf dem VPS.
// Zeitbasierte X-Achse (−4 s … +3 s), Touchdown als senkrechte Linie,
// Auto-Zoom-Y, Gridlines, Zonen vor-Flare/Flare/nach-TD, Hover-Tooltip.
function VsCurveChart({ profile }: { profile: LandingProfilePoint[] }) {
  const { t } = useTranslation();
  const [hover, setHover] = useState<number | null>(null);

  // Auf das vereinbarte Fenster −4 s … +3 s beschneiden.
  // v0.12.8: Fenster −4 s … +10 s nach TD (zeigt, was der 50-Hz-Buffer
  // hergibt — typischerweise ~8 s post-TD).
  const data = profile.filter((p) => p.t_ms >= -4000 && p.t_ms <= 10000);
  if (data.length < 5) {
    return (
      <div className="landing-chart landing-chart--empty">
        {t("landing.no_profile")}
      </div>
    );
  }

  const w = 1120;
  const h = 320;
  const pad = { top: 20, right: 20, bottom: 52, left: 64 };
  const innerW = w - pad.left - pad.right;
  const innerH = h - pad.top - pad.bottom;

  const vss = data.map((p) => p.vs_fpm);
  let lo = Math.min(...vss);
  let hi = Math.max(...vss);
  const padv = Math.max(60, (hi - lo) * 0.12);
  lo = Math.floor((lo - padv) / 100) * 100;
  hi = Math.ceil((hi + padv) / 100) * 100;
  if (hi < 0) hi = 0;
  const range = Math.max(1, hi - lo);

  const t0 = data[0]!.t_ms;
  const t1 = data[data.length - 1]!.t_ms;
  const tRange = Math.max(1, t1 - t0);
  const x = (tMs: number) => pad.left + ((tMs - t0) / tRange) * innerW;
  const y = (vs: number) => pad.top + innerH - ((vs - lo) / range) * innerH;

  const path = data
    .map((p, i) => `${i === 0 ? "M" : "L"} ${x(p.t_ms).toFixed(1)} ${y(p.vs_fpm).toFixed(1)}`)
    .join(" ");

  const step = range > 1400 ? 400 : 200;
  const gridVals: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) gridVals.push(v);

  const tdX = x(0);
  const preEnd = x(-3000);

  return (
    <svg
      className="landing-chart"
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={t("landing.vs_curve")}
      onMouseMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const sx = (e.clientX - rect.left) * (w / rect.width);
        const tMs = t0 + ((sx - pad.left) / innerW) * tRange;
        let best = 0;
        let bd = Infinity;
        data.forEach((p, i) => {
          const d = Math.abs(p.t_ms - tMs);
          if (d < bd) { bd = d; best = i; }
        });
        setHover(best);
      }}
      onMouseLeave={() => setHover(null)}
    >
      <rect x={pad.left} y={pad.top} width={innerW} height={innerH}
            fill="rgba(255,255,255,0.02)" stroke="rgba(255,255,255,0.15)" />

      {/* Zonen: vor Flare / Flare / nach TD */}
      {[
        { x0: pad.left, x1: Math.max(pad.left, Math.min(preEnd, pad.left + innerW)), fill: "rgba(56,189,248,0.10)" },
        { x0: Math.max(pad.left, preEnd), x1: Math.min(pad.left + innerW, tdX), fill: "rgba(234,179,8,0.16)" },
        { x0: Math.max(pad.left, tdX), x1: pad.left + innerW, fill: "rgba(248,113,113,0.13)" },
      ].map((z, i) =>
        z.x1 > z.x0 ? (
          <rect key={i} x={z.x0} y={pad.top} width={z.x1 - z.x0} height={innerH} fill={z.fill} />
        ) : null,
      )}

      {/* Gridlines */}
      {gridVals.map((v) => {
        const gy = y(v);
        const zero = v === 0;
        return (
          <g key={v}>
            <line x1={pad.left} y1={gy} x2={pad.left + innerW} y2={gy}
                  stroke={zero ? "#475569" : "rgba(255,255,255,0.07)"}
                  strokeWidth={zero ? 1.6 : 1} />
            <text x={pad.left - 8} y={gy + 4} textAnchor="end" fontSize="12"
                  fill={zero ? "#94a3b8" : "#64748b"}>{v}</text>
          </g>
        );
      })}

      {/* Touchdown-Linie */}
      <line x1={tdX} y1={pad.top} x2={tdX} y2={pad.top + innerH}
            stroke="#f87171" strokeWidth="1.4" strokeDasharray="4 3" />
      <text x={tdX} y={pad.top - 6} textAnchor="middle" fontSize="11" fill="#f87171">
        {t("landing.touchdown")}
      </text>

      <path d={path} fill="none" stroke="#38bdf8" strokeWidth="2" />

      <text x={pad.left} y={h - 28} fontSize="12" fill="#94a3b8">
        {(t0 / 1000).toFixed(1)} s
      </text>
      <text x={pad.left + innerW} y={h - 28} textAnchor="end" fontSize="12" fill="#94a3b8">
        +{(t1 / 1000).toFixed(1)} s
      </text>
      <text x={16} y={pad.top + innerH / 2} fontSize="11" fill="#64748b" textAnchor="middle"
            transform={`rotate(-90 16 ${pad.top + innerH / 2})`}>
        {t("landing.vs_chart.axis")}
      </text>

      {hover != null && data[hover] && (() => {
        const p = data[hover]!;
        const hx = x(p.t_ms);
        const hy = y(p.vs_fpm);
        const tRel = p.t_ms / 1000;
        const tLabel = tRel <= 0
          ? t("landing.vs_chart.before_td", { s: Math.abs(tRel).toFixed(1) })
          : t("landing.vs_chart.after_td", { s: tRel.toFixed(1) });
        const boxW = 188;
        const boxX = Math.min(Math.max(hx + 12, pad.left), pad.left + innerW - boxW);
        const boxY = Math.max(hy - 46, pad.top + 2);
        return (
          <g pointerEvents="none">
            <line x1={hx} y1={pad.top} x2={hx} y2={pad.top + innerH}
                  stroke="#38bdf8" strokeWidth="1" strokeDasharray="3 3" />
            <circle cx={hx} cy={hy} r="4" fill="#38bdf8" stroke="#0e1420" strokeWidth="1.5" />
            <rect x={boxX} y={boxY} width={boxW} height={40} rx="5"
                  fill="#1e293b" stroke="#334155" />
            <text x={boxX + 9} y={boxY + 17} fontSize="12.5" fill="#38bdf8" fontWeight="700">
              {Math.round(p.vs_fpm)} fpm
              <tspan fill="#cbd5e1" fontWeight="400">{`  ·  ${tLabel}`}</tspan>
            </text>
            <text x={boxX + 9} y={boxY + 32} fontSize="11" fill="#94a3b8">
              AGL {Math.round(p.agl_ft)} ft  ·  {p.on_ground ? t("landing.on_ground") : t("landing.airborne")}
            </text>
          </g>
        );
      })()}
    </svg>
  );
}

// ---- v0.7.6 P1-3: Runway-Geometry-Trust Helper -------------------------
//
// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
//
// Bei untrusted geometry werden Centerline-Offset, Past-Threshold (Float-
// Distance) und das RunwayDiagram ausgeblendet — Pilot soll nicht mit
// kaputter Runway-Geometrie konfrontiert werden. "no_runway_match" wird
// SILENT behandelt (kein Alarm-Pill — bei Privatplaetzen normal).

function runwayTrustReasonLabel(reason: string | null | undefined): string | null {
  switch (reason) {
    case "icao_mismatch":
      return "Falscher Flughafen erkannt — Geometrie ausgeblendet";
    case "centerline_offset_too_large":
      return "Touchdown weit von Runway-Mitte — Geometrie ausgeblendet";
    case "negative_float_distance":
      return "Touchdown vor Threshold — Geometrie ausgeblendet";
    case "no_runway_match":
      // Privatplatz / Off-DB-Bahn ist KEIN Pilot-Fehler — kein Alarm-Pill.
      return null;
    default:
      return null;
  }
}

// ---- Runway diagram ----------------------------------------------------

export function RunwayDiagram({
  rw,
  rolloutDistanceM,
  tdzLengthM,
  tdInTdz,
  aimPointM,
  aimClass,
  preDisplacedThreshold,
}: {
  rw: LandingRunwayMatch;
  rolloutDistanceM: number | null;
  // v0.8.0 assessment-Felder. Alle optional — wenn None bzw. undefined
  // wird der entsprechende Layer nicht gerendert.
  tdzLengthM?: number | null;
  tdInTdz?: boolean | null;
  aimPointM?: number | null;
  aimClass?: string | null;
  preDisplacedThreshold?: boolean | null;
}) {
  const { t } = useTranslation();
  const w = 480;
  const h = 154;
  // Runway band — slightly taller now (height grew from 130 to 154) so
  // the new TDZ + Aim + DDS markers don't crowd the existing
  // threshold/TD labels.
  const rwLeft = 30;
  const rwRight = w - 30;
  const rwTop = h / 2 - 16;
  const rwBottom = h / 2 + 16;
  const lengthFt = rw.length_ft;
  const lengthM = lengthFt * 0.3048;
  const tdFromThresh = rw.touchdown_distance_from_threshold_ft;
  const tdFromThreshM = tdFromThresh * 0.3048;
  // Map threshold→far-end onto the rect.
  const tdFrac = Math.min(1, Math.max(0, tdFromThresh / Math.max(1, lengthFt)));
  const tdX = rwLeft + tdFrac * (rwRight - rwLeft);
  // v0.8.0: Convert meters along the runway to a pixel X coordinate.
  const mToX = (m: number) =>
    rwLeft + (Math.max(0, Math.min(lengthM, m)) / Math.max(1, lengthM)) * (rwRight - rwLeft);
  const displacedFt = rw.displaced_threshold_ft ?? 0;
  const displacedM = displacedFt * 0.3048;

  // Centerline offset → vertical Y inside the strip
  const offsetM = rw.centerline_distance_m;
  const widthM = 45; // assume ~45 m runway width if not exposed
  const offFrac = Math.max(-1, Math.min(1, offsetM / widthM));
  const tdY = (rwTop + rwBottom) / 2 + offFrac * 12;

  // Rollout band: from TD point along the runway centerline for the
  // accumulated rollout distance (in m). End-X clamps to the far-end
  // marker if the rollout extended past the runway end (rare —
  // means the pilot exited beyond the threshold).
  const rolloutEndFrac =
    rolloutDistanceM != null && lengthM > 0
      ? Math.min(1, (tdFromThreshM + rolloutDistanceM) / lengthM)
      : null;
  const rolloutEndX =
    rolloutEndFrac != null
      ? rwLeft + rolloutEndFrac * (rwRight - rwLeft)
      : null;

  return (
    <svg
      className="landing-runway"
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={t("landing.runway_diagram")}
    >
      {/* Runway tarmac */}
      <rect
        x={rwLeft}
        y={rwTop}
        width={rwRight - rwLeft}
        height={rwBottom - rwTop}
        fill="#1f2937"
        stroke="rgba(255,255,255,0.3)"
      />
      {/* v0.8.0 — TDZ-Box: ICAO Annex 14 painted touchdown-zone
          marker (= erste 900 m oder 1/3 der Bahn). Subtiles Gelb damit
          es nicht den TD-Punkt überfärbt. */}
      {tdzLengthM != null && tdzLengthM > 0 && (
        <rect
          x={rwLeft}
          y={rwTop + 2}
          width={mToX(tdzLengthM) - rwLeft}
          height={rwBottom - rwTop - 4}
          fill="rgba(253,224,138,0.18)"
          stroke="rgba(253,224,138,0.5)"
          strokeDasharray="3,3"
        >
          <title>
            {t("landing.tdz_box_tooltip", {
              defaultValue:
                "TDZ-Marker (ICAO Annex 14) — Soll-Aufsetzbereich (erste 900 m oder 1/3 der Bahn)",
            })}
          </title>
        </rect>
      )}
      {/* v0.8.0 — Displaced-Threshold-Paint (verbotene Pre-Threshold-
          Zone). Bei DDS > 0 färben wir die ersten X m rot, weil dort
          IRL nicht gelandet werden darf (Hindernisclearance-Slope). */}
      {displacedM > 0 && (
        <rect
          x={rwLeft}
          y={rwTop}
          width={mToX(displacedM) - rwLeft}
          height={rwBottom - rwTop}
          fill="rgba(124,45,18,0.35)"
          stroke="rgba(220,38,38,0.6)"
          strokeDasharray="2,2"
        >
          <title>
            {t("landing.dds_zone_tooltip", {
              defaultValue:
                "Displaced Threshold — keine Landung erlaubt. Distanz vom physischen Bahn-Anfang.",
            })}
          </title>
        </rect>
      )}
      {/* Centerline dashes */}
      <line
        x1={rwLeft + 8}
        x2={rwRight - 8}
        y1={(rwTop + rwBottom) / 2}
        y2={(rwTop + rwBottom) / 2}
        stroke="#fbbf24"
        strokeWidth="1.4"
        strokeDasharray="10,8"
      />
      {/* v0.8.0 — Aim-Point-Marker (FAA AIM 8-9-1). 300 m / 400 m past
          Threshold je nach Bahn-Länge. Tone hängt von aim_class ab. */}
      {aimPointM != null && aimPointM > 0 && (
        <g>
          {(() => {
            const aimX = mToX(aimPointM);
            const tone =
              aimClass === "perfect"
                ? "#22c55e"
                : aimClass === "short_of_aim" || aimClass === "past_aim"
                ? "#fbbf24"
                : "#ef4444";
            return (
              <>
                <line
                  x1={aimX}
                  x2={aimX}
                  y1={rwTop - 4}
                  y2={rwBottom + 4}
                  stroke={tone}
                  strokeWidth="1.6"
                  strokeDasharray="4,3"
                />
                <polygon
                  points={`${aimX - 4},${rwTop - 10} ${aimX + 4},${rwTop - 10} ${aimX},${rwTop - 3}`}
                  fill={tone}
                >
                  <title>
                    {t("landing.aim_marker_tooltip", {
                      defaultValue:
                        "Aim-Point: 300 m (kurze Bahn) oder 400 m (lange Bahn) past Threshold",
                    })}
                  </title>
                </polygon>
              </>
            );
          })()}
        </g>
      )}
      {/* Rollout band — semi-transparent green strip from TD to exit */}
      {rolloutEndX != null && (
        <>
          <rect
            x={Math.min(tdX, rolloutEndX)}
            y={rwTop + 4}
            width={Math.abs(rolloutEndX - tdX)}
            height={rwBottom - rwTop - 8}
            fill="rgba(34,197,94,0.28)"
            stroke="rgba(34,197,94,0.7)"
            strokeWidth="1"
          />
          {/* Exit marker line */}
          <line
            x1={rolloutEndX}
            x2={rolloutEndX}
            y1={rwTop - 2}
            y2={rwBottom + 2}
            stroke="#22c55e"
            strokeWidth="2"
          />
        </>
      )}
      {/* Threshold marker */}
      <line
        x1={rwLeft + 4}
        x2={rwLeft + 4}
        y1={rwTop}
        y2={rwBottom}
        stroke="#ffffff"
        strokeWidth="3"
      />
      {/* Far end */}
      <line
        x1={rwRight - 4}
        x2={rwRight - 4}
        y1={rwTop}
        y2={rwBottom}
        stroke="#ffffff"
        strokeWidth="3"
      />
      {/* Touchdown dot — Tone vom Assessment abgeleitet:
          rot = DDS-Verstoss, grün = TDZ-Treffer, cyan = sonst */}
      <circle
        cx={tdX}
        cy={tdY}
        r="6"
        fill={
          preDisplacedThreshold === true
            ? "#ef4444"
            : tdInTdz === true
            ? "#22c55e"
            : "#22d3ee"
        }
        stroke="#000"
        strokeWidth="1"
      ><title>{`TD · ${tdFromThreshM.toFixed(0)} m past threshold`}</title></circle>
      {/* Labels above the runway */}
      <text x={rwLeft} y={rwTop - 6} fontSize="11" fill="currentColor">
        {rw.runway_ident} · {rw.airport_ident}
      </text>
      <text
        x={rwRight}
        y={rwTop - 6}
        textAnchor="end"
        fontSize="11"
        fill="currentColor"
      >
        {lengthM.toFixed(0)} m
      </text>
      {/* TD label below the dot */}
      <text
        x={tdX}
        y={rwBottom + 14}
        textAnchor="middle"
        fontSize="10"
        fill="#22d3ee"
      >
        TD · {tdFromThreshM.toFixed(0)} m {t("landing.past_threshold_short")}
      </text>
      {/* Centerline-offset annotation, suppressed for CENTER (=0 m) */}
      {Math.abs(rw.centerline_distance_m) >= 1 && (
        <text
          x={tdX}
          y={rwBottom + 26}
          textAnchor="middle"
          fontSize="10"
          fill="currentColor"
        >
          {Math.abs(rw.centerline_distance_m).toFixed(1)} m {t(sideKey(rw.side))}
        </text>
      )}
      {/* Exit label — only when there's enough horizontal room */}
      {rolloutEndX != null && Math.abs(rolloutEndX - tdX) > 60 && (
        <text
          x={rolloutEndX}
          y={rwTop - 6}
          textAnchor="middle"
          fontSize="10"
          fill="#22c55e"
        >
          {t("landing.exit")} · {(rolloutDistanceM ?? 0).toFixed(0)} m
        </text>
      )}
    </svg>
  );
}

// ---- Wind compass -------------------------------------------------------

// v0.12.8-dev: Wind-Visualisierung — animiertes Stromlinien-Feld. Der Wind
// "weht" sichtbar über die Karte: Richtung = echte Anströmrichtung relativ
// zur Landerichtung (Bahn waagerecht, Nase rechts), Tempo + Dichte der
// Streaks = Windstärke, Farbe = Kritikalität (Seitenwind-/Rückenwind-Limit).
// Im Vordergrund die Kennzahlen als Hero-Zahl + Chips.
function WindCompass({
  headwindKt,
  crosswindKt,
  runwayIdent,
}: {
  headwindKt: number | null;
  crosswindKt: number | null;
  runwayIdent?: string | null;
}) {
  const { t } = useTranslation();
  if (headwindKt == null && crosswindKt == null) return null;
  const hw = headwindKt ?? 0;
  const xw = crosswindKt ?? 0;

  const totalKt = Math.sqrt(hw * hw + xw * xw);
  const isTailwind = hw < -0.5;
  const xwAbs = Math.abs(xw);
  const twAbs = Math.abs(hw);
  const xwFromRight = xw >= 0;
  const calm = totalKt < 1.5;

  // Kritikalität nach Seitenwind-/Rückenwind-Limit — färbt Streaks + Zahl.
  const critColor =
    xwAbs >= 25 || (isTailwind && twAbs >= 10)
      ? "#f87171"
      : xwAbs >= 15 || (isTailwind && twAbs >= 5)
        ? "#fbbf24"
        : "#38bdf8";

  // Anströmrichtung in Screen-Koordinaten. Wind WEHT von der Quelle weg →
  // Travel-Vektor (−hw, −xw). +x = mit der Landerichtung, +y = nach unten.
  const travelDeg = calm ? 0 : (Math.atan2(-xw, -hw) * 180) / Math.PI;

  // Mehr Wind → mehr & schnellere Streaks.
  const streakCount = calm ? 0 : Math.round(Math.min(34, 8 + totalKt * 0.95));
  const durationMs = Math.round(
    Math.min(2400, Math.max(620, 2600 - totalKt * 62)),
  );

  // Deterministisches Pseudo-Feld — kein Flackern bei Re-Render.
  const streaks = Array.from({ length: streakCount }, (_, i) => {
    const rand = ((i * 9301 + 49297) % 233280) / 233280;
    return {
      y: -110 + ((i * 81) % 440),
      len: 26 + rand * 40,
      delay: -(i / Math.max(1, streakCount)) * durationMs,
      thickness: 1.4 + rand * 1.7,
      opacity: 0.3 + rand * 0.42,
    };
  });

  const headLabel = isTailwind
    ? t("landing.wind_tailwind")
    : t("landing.wind_headwind");
  const sideLabel = xwFromRight
    ? t("landing.wind_side_right")
    : t("landing.wind_side_left");

  return (
    <div className="windflow">
      <svg
        className="windflow__field"
        viewBox="0 0 360 200"
        preserveAspectRatio="xMidYMid slice"
        aria-hidden="true"
      >
        <g transform={`rotate(${travelDeg.toFixed(1)} 180 100)`}>
          {streaks.map((s, i) => (
            <line
              key={i}
              className="windflow__streak"
              x1={-140}
              y1={s.y}
              x2={-140 + s.len}
              y2={s.y}
              stroke={critColor}
              strokeWidth={s.thickness}
              strokeLinecap="round"
              opacity={s.opacity}
              style={{
                animationDuration: `${durationMs}ms`,
                animationDelay: `${s.delay}ms`,
              }}
            />
          ))}
        </g>
      </svg>
      <div className="windflow__overlay">
        <div className="windflow__top">
          <span className="windflow__cap">{t("landing.wind")}</span>
        </div>
        {calm ? (
          <div className="windflow__hero">
            <span className="windflow__hero-label">
              {t("landing.wind_calm")}
            </span>
          </div>
        ) : (
          <div className="windflow__hero">
            <span
              className="windflow__hero-num"
              style={{ color: critColor }}
            >
              {xwAbs.toFixed(0)}
            </span>
            <div className="windflow__hero-meta">
              <span className="windflow__hero-unit">kt</span>
              <span className="windflow__hero-sub">
                {t("landing.wind_crosswind")} · {sideLabel}
              </span>
            </div>
          </div>
        )}
        {/* Landebahn — waagerecht, UNTER der Hero-Zahl. Bleibt fix, damit
            der Winkel der Streaks die Anströmrichtung relativ zur Bahn
            zeigt. */}
        {runwayIdent && (
          <div className="windflow__runway">
            <span className="windflow__rwy-keys" />
            <span className="windflow__rwy-line" />
            <span className="windflow__rwy-id">{runwayIdent}</span>
          </div>
        )}
        {!calm && (
          <div className="windflow__chips">
            <span className="windflow__chip">
              {headLabel} {twAbs.toFixed(0)} kt
            </span>
            <span className="windflow__chip">
              {t("landing.wind_total")} {totalKt.toFixed(0)} kt
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ---- Approach stability time-series chart ------------------------------

// v0.12.7: Anflug-V/S-Profil — Redesign. Auto-Zoom-Y (Kurve füllt die
// Fläche statt im festen −1500…+100-Band zu verschwinden), Gridlines +
// 0-Linie, Soll-Band −600…−900, gestrichelte Stabilitätsgrenze −1000,
// Hover-Tooltip. Spec: Pilot-Befund Michel/GSG.
// v0.13.15: Fractional sample-index for the touchdown line (t_ms = 0).
// Approach samples carry the (up to ~0.5 s late) streamer-tick timestamp,
// so the first sample with t_ms >= 0 frequently sat ~0.5 s AFTER the real
// touchdown — the old `findIndex(t_ms >= 0)` placed the red line there,
// right of the curve's end. We instead interpolate between the last pre-TD
// and the first post-TD sample by time, returning a fractional index that
// the (index-linear) x() maps to the exact t = 0 position. Falls back to
// the last sample when no post-TD sample exists. Exported for unit tests.
export function approachTdLineIndex(
  samples: { t_ms?: number | null }[],
): number {
  if (samples.length === 0) return 0;
  const firstPos = samples.findIndex((s) => s.t_ms != null && s.t_ms >= 0);
  if (firstPos < 0) return samples.length - 1; // kein Post-TD-Sample
  if (firstPos === 0) return 0;
  const tPrev = samples[firstPos - 1]!.t_ms ?? 0;
  const tCur = samples[firstPos]!.t_ms ?? 0;
  const span = tCur - tPrev;
  const frac = span > 0 ? Math.min(1, Math.max(0, (0 - tPrev) / span)) : 0;
  return firstPos - 1 + frac;
}

function ApproachChart({
  samples,
  glideslopeAngleDeg,
}: {
  samples: ApproachSample[];
  /** v0.15.18: echter Gleitpfad-Winkel (Navdaten). Skaliert Soll-Band +
   *  Stabilitätsgrenze 1:1 wie das Backend. null/3° → unverändert. */
  glideslopeAngleDeg?: number | null;
}) {
  const { t } = useTranslation();
  const [hover, setHover] = useState<number | null>(null);
  if (samples.length < 3) return null;

  // v0.15.18: Soll-Band + Stabilitätsgrenze mit dem ECHTEN Gleitwinkel
  // skalieren — identisch zum Backend (compute_approach_stability_v2:
  // gs_factor = tan(g)/tan(3°), Plausibilitäts-Clamp 2–7,5°, sonst ×1,0).
  // Bei 3°/unbekannt bleibt alles bei −600…−900 / −1000 (bit-identisch zu
  // vorher). Steilanflüge (ENTC 4°, EGLC 5,5°) bekommen die korrekte, tiefere
  // Linie — sonst „stürzt" die V/S-Spur scheinbar unter eine falsche −1000-
  // Marke, obwohl das Backend (korrekt skaliert) den Anflug gar nicht flaggt.
  const gsFactor =
    glideslopeAngleDeg != null &&
    glideslopeAngleDeg >= 2 &&
    glideslopeAngleDeg <= 7.5
      ? Math.tan((glideslopeAngleDeg * Math.PI) / 180) /
        Math.tan((3 * Math.PI) / 180)
      : 1;
  const bandHi = -600 * gsFactor;
  const bandLo = -900 * gsFactor;
  const limitFpm = -1000 * gsFactor;
  const isScaledGp = gsFactor !== 1;
  const fmtFpm = (v: number) => `−${Math.abs(Math.round(v / 10) * 10)}`;

  const w = 1120;
  const h = 320;
  const pad = { top: 20, right: 20, bottom: 52, left: 64 };
  const innerW = w - pad.left - pad.right;
  const innerH = h - pad.top - pad.bottom;

  // Auto-Zoom-Y auf den echten Wertebereich (+12 % Polster), auf 100er
  // gerundet. 0-Linie bleibt immer sichtbar.
  const vss = samples.map((s) => s.vs_fpm);
  let lo = Math.min(...vss);
  let hi = Math.max(...vss);
  const padv = Math.max(60, (hi - lo) * 0.12);
  lo = Math.floor((lo - padv) / 100) * 100;
  hi = Math.ceil((hi + padv) / 100) * 100;
  if (hi < 0) hi = 0;
  // v0.12.8: Achse IMMER mindestens 0 … −1100 — sonst klebt bei einem
  // ruhigen Anflug das Soll-Band unten und die Stabilitätslinie fällt raus.
  // v0.15.18: mit dem Gleitwinkel mit-skaliert, damit die (ggf. tiefere)
  // Grenze + Band auch bei Steilanflügen im Bild bleiben.
  lo = Math.min(lo, Math.floor((limitFpm - 100) / 100) * 100);
  const range = Math.max(1, hi - lo);

  const xStep = innerW / Math.max(1, samples.length - 1);
  const x = (i: number) => pad.left + i * xStep;
  const y = (vs: number) => pad.top + innerH - ((vs - lo) / range) * innerH;
  const clampY = (v: number) => Math.min(Math.max(y(v), pad.top), pad.top + innerH);

  const path = samples
    .map((s, i) => `${i === 0 ? "M" : "L"} ${x(i).toFixed(1)} ${y(s.vs_fpm).toFixed(1)}`)
    .join(" ");

  const zoneOf = (s: ApproachSample): "vorlauf" | "gate" | "flare" =>
    s.is_flare ? "flare" : s.is_scored_gate ? "gate" : "vorlauf";
  const hasZones = samples.some((s) => s.is_scored_gate != null);
  const zones: { start: number; end: number; kind: "vorlauf" | "gate" | "flare" }[] = [];
  if (hasZones) {
    let i = 0;
    while (i < samples.length) {
      const kind = zoneOf(samples[i]!);
      let j = i;
      while (j + 1 < samples.length && zoneOf(samples[j + 1]!) === kind) j++;
      zones.push({ start: i, end: j, kind });
      i = j + 1;
    }
  }
  const zoneFill = (k: string) =>
    k === "gate" ? "rgba(56,189,248,0.10)"
    : k === "flare" ? "rgba(234,179,8,0.16)"
    : "rgba(120,120,120,0.10)";

  const step = range > 1400 ? 400 : 200;
  const gridVals: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) gridVals.push(v);

  // v0.13.15 (Pilot-Befund ViolonC 2026-05-31): TD-Linie EXAKT bei
  // t_ms = 0 setzen. x() ist linear im Sample-Index, also liefert der
  // (ggf. gebrochene) Index aus approachTdLineIndex die korrekte X-Position.
  const tdX = x(approachTdLineIndex(samples));
  const tdNearRight = tdX > pad.left + innerW - 70;

  const bandTop = clampY(bandHi);
  const bandBottom = clampY(bandLo);
  const limitVisible = limitFpm > lo && limitFpm < hi;

  return (
    <svg
      className="landing-chart"
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={t("landing.approach_chart")}
      onMouseMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const sx = (e.clientX - rect.left) * (w / rect.width);
        let k = Math.round((sx - pad.left) / xStep);
        k = Math.max(0, Math.min(samples.length - 1, k));
        setHover(k);
      }}
      onMouseLeave={() => setHover(null)}
    >
      <rect x={pad.left} y={pad.top} width={innerW} height={innerH}
            fill="rgba(255,255,255,0.02)" stroke="rgba(255,255,255,0.15)" />

      {zones.map((z, idx) => {
        const x0 = z.start > 0 ? (x(z.start - 1) + x(z.start)) / 2 : x(z.start) - 2;
        const x1 = z.end < samples.length - 1 ? (x(z.end) + x(z.end + 1)) / 2 : x(z.end) + 2;
        return <rect key={idx} x={x0} y={pad.top} width={Math.max(0, x1 - x0)}
                     height={innerH} fill={zoneFill(z.kind)} />;
      })}

      {bandBottom > bandTop && (
        <rect x={pad.left} y={bandTop} width={innerW} height={bandBottom - bandTop}
              fill="rgba(34,197,94,0.16)" />
      )}

      {gridVals.map((v) => {
        const gy = y(v);
        const zero = v === 0;
        return (
          <g key={v}>
            <line x1={pad.left} y1={gy} x2={pad.left + innerW} y2={gy}
                  stroke={zero ? "#475569" : "rgba(255,255,255,0.07)"}
                  strokeWidth={zero ? 1.6 : 1} />
            <text x={pad.left - 8} y={gy + 4} textAnchor="end" fontSize="12"
                  fill={zero ? "#94a3b8" : "#64748b"}>{v}</text>
          </g>
        );
      })}

      {limitVisible && (
        <g>
          <line x1={pad.left} y1={y(limitFpm)} x2={pad.left + innerW} y2={y(limitFpm)}
                stroke="#f87171" strokeWidth="1.2" strokeDasharray="6 4" opacity="0.7" />
          <text x={pad.left + innerW - 6} y={y(limitFpm) - 5} textAnchor="end"
                fontSize="10" fill="#f87171" opacity="0.85">
            {isScaledGp
              ? t("landing.vs_chart.limit_custom", {
                  fpm: fmtFpm(limitFpm),
                  angle: String(glideslopeAngleDeg),
                })
              : t("landing.vs_chart.limit", { fpm: fmtFpm(limitFpm) })}
          </text>
        </g>
      )}

      <line x1={tdX} y1={pad.top} x2={tdX} y2={pad.top + innerH}
            stroke="#f87171" strokeWidth="1.4" strokeDasharray="4 3" />
      <text x={tdNearRight ? tdX - 6 : tdX} y={pad.top - 6}
            textAnchor={tdNearRight ? "end" : "middle"} fontSize="11" fill="#f87171">
        {t("landing.touchdown")}
      </text>

      <path d={path} fill="none" stroke="#38bdf8" strokeWidth="2" />

      <text x={pad.left} y={h - 28} fontSize="12" fill="#94a3b8">
        {t("landing.approach_start")}
      </text>
      <text x={pad.left + innerW} y={h - 28} textAnchor="end" fontSize="12" fill="#94a3b8">
        {t("landing.touchdown")}
      </text>
      <text x={16} y={pad.top + innerH / 2} fontSize="11" fill="#64748b" textAnchor="middle"
            transform={`rotate(-90 16 ${pad.top + innerH / 2})`}>
        {t("landing.vs_chart.axis")}
      </text>

      {hasZones && (
        <g fontSize="11" fill="currentColor">
          <rect x={pad.left} y={h - 14} width={9} height={9} fill="rgba(120,120,120,0.4)" />
          <text x={pad.left + 13} y={h - 6}>{t("landing.chart_zone.vorlauf")}</text>
          <rect x={pad.left + 78} y={h - 14} width={9} height={9} fill="rgba(56,189,248,0.4)" />
          <text x={pad.left + 91} y={h - 6}>{t("landing.chart_zone.gate")}</text>
          <rect x={pad.left + 160} y={h - 14} width={9} height={9} fill="rgba(234,179,8,0.4)" />
          <text x={pad.left + 173} y={h - 6}>{t("landing.chart_zone.flare")}</text>
          <rect x={pad.left + 230} y={h - 14} width={9} height={9} fill="rgba(34,197,94,0.4)" />
          <text x={pad.left + 243} y={h - 6}>
            {t("landing.vs_chart.band", {
              hi: fmtFpm(bandHi),
              lo: fmtFpm(bandLo),
            })}
          </text>
        </g>
      )}

      {hover != null && samples[hover] && (() => {
        const s = samples[hover]!;
        const hx = x(hover);
        const hy = y(s.vs_fpm);
        const tRel = (s.t_ms ?? 0) / 1000;
        const tLabel = tRel <= 0
          ? t("landing.vs_chart.before_td", { s: Math.abs(tRel).toFixed(1) })
          : t("landing.vs_chart.after_td", { s: tRel.toFixed(1) });
        const zoneLabel = s.is_flare
          ? t("landing.chart_zone.flare")
          : s.is_scored_gate
            ? t("landing.chart_zone.gate")
            : t("landing.chart_zone.vorlauf");
        const boxW = 188;
        const boxX = Math.min(Math.max(hx + 12, pad.left), pad.left + innerW - boxW);
        const boxY = Math.max(hy - 46, pad.top + 2);
        return (
          <g pointerEvents="none">
            <line x1={hx} y1={pad.top} x2={hx} y2={pad.top + innerH}
                  stroke="#38bdf8" strokeWidth="1" strokeDasharray="3 3" />
            <circle cx={hx} cy={hy} r="4" fill="#38bdf8" stroke="#0e1420" strokeWidth="1.5" />
            <rect x={boxX} y={boxY} width={boxW} height={40} rx="5"
                  fill="#1e293b" stroke="#334155" />
            <text x={boxX + 9} y={boxY + 17} fontSize="12.5" fill="#38bdf8" fontWeight="700">
              {Math.round(s.vs_fpm)} fpm
              <tspan fill="#cbd5e1" fontWeight="400">{`  ·  ${tLabel}`}</tspan>
            </text>
            <text x={boxX + 9} y={boxY + 32} fontSize="11" fill="#94a3b8">
              {s.agl_ft != null ? `AGL ${Math.round(s.agl_ft)} ft  ·  ` : ""}{zoneLabel}
            </text>
          </g>
        );
      })()}
    </svg>
  );
}

// ---- (i) info badge — small click-to-toggle popover --------------------
//
// Pilots may not know what "V/S σ" or "Bahn-Auslastung" means precisely.
// Each sub-score card carries a small (i) icon that, when clicked,
// reveals an explanation popover above/below the card. We use a click
// instead of hover so it's tappable on touch devices and stays open
// while the pilot reads it.

function InfoBadge({ explanation }: { explanation: string }) {
  const [open, setOpen] = useState(false);
  // Whether to flip the popover to the LEFT side of the badge to keep
  // it inside the viewport. Decided on click using getBoundingClientRect
  // — popover is ~300 px wide; if the badge's right edge sits within
  // 320 px of the viewport's right edge we flip.
  const [flipLeft, setFlipLeft] = useState(false);
  return (
    <span className="info-badge-wrap">
      <button
        type="button"
        className={`info-badge ${open ? "info-badge--open" : ""}`}
        onClick={(e) => {
          e.stopPropagation();
          if (!open) {
            const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setFlipLeft(window.innerWidth - rect.right < 320);
          }
          setOpen((v) => !v);
        }}
        aria-label="info"
      >
        i
      </button>
      {open && (
        <span
          className={`info-badge__popover ${
            flipLeft ? "info-badge__popover--flip-left" : ""
          }`}
          role="tooltip"
        >
          {explanation}
          <button
            type="button"
            className="info-badge__close"
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
            aria-label="close"
          >
            ×
          </button>
        </span>
      )}
    </span>
  );
}

// ---- Score breakdown card grid -----------------------------------------

function ScoreBreakdown({
  subs,
  record,
}: {
  subs: SubScore[];
  record: LandingRecord;
}) {
  const { t } = useTranslation();
  // v0.11.0-dev: Pilot-Hilfe-Modal für den "Bahn-Auslastung"-Sub-Score.
  // Wird über den "🛬 Wie wird das berechnet?"-Button am Boden der
  // rollout-Card geöffnet. Andere Sub-Scores behalten ihren bestehenden
  // InfoBadge-Tooltip — nur Bahn-Auslastung bekommt das tiefe Erklärungs-
  // Modal, weil sie mit Bändern + Heavy-Bonus + Pre-Displaced-Cap die
  // komplexeste Score-Logik hat.
  const [runwayUtilHelpOpen, setRunwayUtilHelpOpen] = useState(false);
  if (subs.length === 0) return null;
  return (
    <div className="landing-subscores">
      {runwayUtilHelpOpen && (
        <RunwayUtilizationHelpModal
          onClose={() => setRunwayUtilHelpOpen(false)}
        />
      )}
      {subs.map((s) => {
        // v0.7.1 P1.2-Fix: skipped wird sichtbar als "nicht bewertet"
        // (graue Variante, keine Punkte/Wert/Rationale).
        if (s.skipped) {
          const reasonKey = s.skipReason
            ? `landing.skipped_reason.${s.skipReason}`
            : "landing.skipped_reason.fallback";
          return (
            <div
              key={s.key}
              className="landing-subscore landing-subscore--skipped"
              style={{ opacity: 0.65 }}
            >
              <div className="landing-subscore__head">
                <span className="landing-subscore__label">
                  {t(`landing.sub.${s.key}`)}
                  {/* v0.11.0-dev: kein i-Tooltip für rollout — der
                      "🛬 Wie wird das berechnet?"-Button am Boden öffnet
                      bereits das ausführliche Modal. Zwei Erklärungen
                      auf der gleichen Card wären redundant. */}
                  {s.key !== "rollout" && (
                    <InfoBadge explanation={t(`landing.info.${s.key}`)} />
                  )}
                </span>
                <span
                  className="landing-subscore__points"
                  style={{ fontStyle: "italic", fontSize: "0.75rem" }}
                >
                  {t("landing.skipped_label")}
                </span>
              </div>
              <div
                className="landing-subscore__rationale"
                style={{ fontStyle: "italic" }}
              >
                {t(reasonKey)}
              </div>
            </div>
          );
        }
        // v0.10.0 (#runway-utilization-score) — Warning-Pill (z.B.
        // pre_displaced_threshold) + extra-Lines (Float-Distance,
        // Bahn-Info). Beides Vorhanden NUR wenn das Rust-Crate sie
        // gefüllt hat — pre-v0.10 SubScoreEntries kommen ohne diese
        // Felder durch (undefined) und das Rendering ist No-op.
        const hasWarning =
          typeof s.warning === "string" && s.warning.length > 0;
        // v0.12.0 (#runway-utilization-refinement, LE4/LE5): die rollout-
        // Card rendert ab score_algorithm_version >= 3 ihr Value-Label
        // und ihre Extra-Zeilen sprach-lokalisiert aus den Record-Feldern.
        // Alt-v2-Records (< 3) zeigen den sprachneutralen Rust-`value`
        // bzw. die gespeicherten `extra`-Strings unverändert (Legacy).
        const isV3Rollout = s.key === "rollout" && isRolloutV3(record);
        const extraLines = isV3Rollout
          ? buildRolloutExtraLines(record, t)
          : (s.extra ?? []);
        const valueText = isV3Rollout
          ? (buildRolloutValueLabel(record, t) ?? s.value)
          : s.value;
        return (
          <div
            key={s.key}
            className={`landing-subscore landing-subscore--${s.band}`}
          >
            <div className="landing-subscore__head">
              <span className="landing-subscore__label">
                {t(`landing.sub.${s.key}`)}
                {/* v0.11.0-dev: kein i-Tooltip für rollout — der
                    "🛬 Wie wird das berechnet?"-Button am Boden öffnet
                    bereits das ausführliche Modal. */}
                {s.key !== "rollout" && (
                  <InfoBadge explanation={t(`landing.info.${s.key}`)} />
                )}
              </span>
              <span className="landing-subscore__points">{s.points} PTS</span>
            </div>
            <div className="landing-subscore__value">{valueText}</div>
            <div className="landing-subscore__bar">
              <div
                className="landing-subscore__fill"
                style={{ width: `${s.points}%` }}
              />
            </div>
            <div className="landing-subscore__rationale">
              {t(`landing.rat.${s.rationale}`)}
            </div>
            {hasWarning && (
              <div
                className="landing-subscore__warning"
                style={{
                  marginTop: 4,
                  fontSize: "0.75rem",
                  color: "#fbbf24",
                  fontWeight: 600,
                }}
              >
                {t(`landing.warn.${s.warning}`)}
              </div>
            )}
            {extraLines.length > 0 && (
              <ul
                className="landing-subscore__extra"
                style={{
                  marginTop: 4,
                  marginBottom: 0,
                  paddingLeft: 14,
                  fontSize: "0.72rem",
                  color: "var(--text-muted)",
                  listStyle: "'▸ '",
                }}
              >
                {extraLines.map((line, idx) => (
                  <li key={idx}>{line}</li>
                ))}
              </ul>
            )}
            {/* v0.11.0-dev: Pilot-Hilfe-Button nur auf der rollout-Card.
                Öffnet RunwayUtilizationHelpModal mit Formel, allen Bändern,
                Heavy-Bonus, Pre-Displaced-Cap und Skip-Reasons. */}
            {s.key === "rollout" && (
              <button
                type="button"
                onClick={() => setRunwayUtilHelpOpen(true)}
                style={{
                  marginTop: 8,
                  padding: "4px 10px",
                  background: "rgba(34,197,94,0.10)",
                  border: "1px solid rgba(34,197,94,0.35)",
                  borderRadius: 4,
                  color: "#bbf7d0",
                  fontSize: "0.72rem",
                  cursor: "pointer",
                  alignSelf: "flex-start",
                }}
              >
                {t("landing.runway_utilization_help.open_button")}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ---- Coach tip — focuses on the worst sub-score ------------------------

function CoachTip({ subs }: { subs: SubScore[] }) {
  const { t } = useTranslation();
  if (subs.length === 0) return null;
  // v0.7.1 P1.2-Fix: skipped Sub-Scores nicht als "schwaechster"
  // Punkt im Coach-Tip nutzen (sie sind nicht bewertet, nicht schlecht).
  const scored = subs.filter((s) => !s.skipped);
  if (scored.length === 0) return null;
  // Sort ascending, the lowest sub-score is the area to improve. If
  // everything is ≥ 90, surface the genuine "good landing" message.
  const sorted = [...scored].sort((a, b) => a.points - b.points);
  const worst = sorted[0];
  const tipKey = coachTipKey(worst.rationale);
  return (
    <div
      className={`landing-coach landing-coach--${
        worst.points >= 85 ? "good" : worst.points >= 65 ? "ok" : "bad"
      }`}
    >
      <div className="landing-coach__head">
        {t("landing.coach_title")} ·{" "}
        <strong>{t(`landing.sub.${worst.key}`)}</strong>
      </div>
      <p className="landing-coach__body">{t(tipKey)}</p>
    </div>
  );
}

// ---- Off-airport banner (v0.7.18 B-012) -------------------------------
//
// Zeigt wenn der echte Touchdown nicht beim geplanten Destination-Airport
// war — Diversion, Off-airport-Crash (GAF-152-Fall), oder Crash mit
// Nearest-Airport in Reichweite.
//
// Drei sichtbare Fälle:
//   1. runway_match-Resolution, aber icao != arr_airport → Divert
//   2. nearest_25nm-Resolution → Off-airport, Nearest gefunden
//   3. planned_fallback mit Distanz > 5 nmi → Off-airport, kein Nearest
//
// Match-Resolution mit icao == arr_airport rendert keinen Banner (= normaler Flug).

/**
 * v0.7.19 GAF-707 Accident-Detection — Banner als Primary-Klassifikation.
 *
 * Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md §AeroACARS
 * Client Tab "Landung". GAF 707 darf hier NICHT als normale Hard-
 * Landing/Bone-Rattler erscheinen.
 *
 * - `accident === true` (Confirmed): roter Top-Level-Banner "ABSTURZ
 *   ERKANNT" mit Gruenden-Liste.
 * - `accident_confidence === "medium"` ohne accident=true (Suspected):
 *   gelber Review-Hinweis-Banner.
 * - Sonst: kein Banner.
 */
function AccidentBanner({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  const isConfirmed = record.accident === true;
  const isSuspected =
    !isConfirmed && record.accident_confidence === "medium";

  if (!isConfirmed && !isSuspected) {
    return null;
  }

  const kindLabel = (() => {
    switch (record.accident_kind) {
      case "sim_crash":
        return t("landing.accident.kind.sim_crash");
      case "impact":
        return t("landing.accident.kind.impact");
      case "off_airport_impact":
        return t("landing.accident.kind.off_airport_impact");
      default:
        return null;
    }
  })();

  const reasons = record.accident_reasons ?? [];

  if (isConfirmed) {
    return (
      <div className="accident-banner accident-banner--confirmed" role="alert">
        <div className="accident-banner__head">
          ⚠ {t("landing.accident.confirmed_title")}
        </div>
        <div className="accident-banner__body">
          {t("landing.accident.confirmed_body")}
        </div>
        {kindLabel && (
          <div className="accident-banner__kind">
            <strong>{t("landing.accident.kind_label")}:</strong> {kindLabel}
          </div>
        )}
        {reasons.length > 0 && (
          <ul className="accident-banner__reasons">
            {reasons.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        )}
      </div>
    );
  }

  // Suspected
  return (
    <div className="accident-banner accident-banner--suspected" role="alert">
      <div className="accident-banner__head">
        ⚠ {t("landing.accident.suspected_title")}
      </div>
      <div className="accident-banner__body">
        {t("landing.accident.suspected_body")}
      </div>
      {reasons.length > 0 && (
        <ul className="accident-banner__reasons">
          {reasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function OffAirportBanner({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  const td = record.touchdown_airport;
  const planned = record.arr_airport;
  const source = record.touchdown_airport_source;
  const distToDest = record.touchdown_distance_to_destination_nm;
  const nearestDist = record.touchdown_nearest_distance_nm;

  // Normaler Fall: gleiche ICAO → kein Banner.
  if (!td || td === planned) {
    // Selbst bei runway_match==arr_airport kann ein > 5nm-Distanz
    // auftreten (Multi-Field-Airports), aber das ist kein Off-airport-Fall.
    return null;
  }

  // Spec-konforme Varianten:
  if (source === "nearest_25nm") {
    return (
      <div className="off-airport-banner off-airport-banner--nearest" role="alert">
        <div className="off-airport-banner__head">
          ⚠ {t("landing.off_airport.title")}
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.planned")}:{" "}
          <strong>{planned}</strong>
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.actual")}:{" "}
          <strong>{td}</strong>
          {nearestDist != null && (
            <span className="off-airport-banner__hint">
              {" — "}
              {t("landing.off_airport.nearest_hint", {
                nm: nearestDist.toFixed(1),
              })}
            </span>
          )}
        </div>
        {distToDest != null && distToDest > 1 && (
          <div className="off-airport-banner__line">
            {t("landing.off_airport.distance_to_dest", {
              nm: distToDest.toFixed(1),
            })}
          </div>
        )}
      </div>
    );
  }

  if (source === "planned_fallback") {
    // Position bekannt aber kein Airport in 25 nmi. Distanz-Hinweis nur
    // sinnvoll wenn echter Off-airport-Crash (> 5 nmi).
    if (distToDest == null || distToDest <= 5) return null;
    return (
      <div className="off-airport-banner off-airport-banner--no-nearest" role="alert">
        <div className="off-airport-banner__head">
          ⚠ {t("landing.off_airport.no_nearest_title")}
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.planned")}:{" "}
          <strong>{planned}</strong>
        </div>
        <div className="off-airport-banner__line">
          {t("landing.off_airport.no_nearest_body", {
            nm: distToDest.toFixed(1),
          })}
        </div>
      </div>
    );
  }

  // source == "runway_match" aber td != planned → Diversion.
  return (
    <div className="off-airport-banner off-airport-banner--divert" role="alert">
      <div className="off-airport-banner__head">
        🛬 {t("landing.off_airport.divert_title")}
      </div>
      <div className="off-airport-banner__line">
        {t("landing.off_airport.planned")}:{" "}
        <strong>{planned}</strong>
      </div>
      <div className="off-airport-banner__line">
        {t("landing.off_airport.actual")}:{" "}
        <strong>{td}</strong>
        {distToDest != null && distToDest > 1 && (
          <span className="off-airport-banner__hint">
            {" — "}
            {t("landing.off_airport.distance_to_dest", {
              nm: distToDest.toFixed(1),
            })}
          </span>
        )}
      </div>
    </div>
  );
}

// ---- Quick-Flag chips (v0.5.47) ---------------------------------------
//
// Auf-einen-Blick-Auffälligkeiten direkt unter dem Headline-Block.
// Spiegelt die Chips aus webapp/src/components/LandingAnalysis.tsx
// (B:124-133) — Pilot sieht im Client und im Live-Monitor exakt dieselben
// Flags. Nur die wirklichen Auffälligkeiten anzeigen — keine "OK"-Chips.

/** v0.12.3 (LE9): the G value the client scores / flags / colours on —
 *  the EMA-smoothed scored G when present, else the raw 50 Hz peak.
 *  The raw `landing_peak_g_force` is forensic-only after v0.12.3. */
export function scoreG(
  r: Pick<LandingRecord, "landing_scored_g_force" | "landing_peak_g_force">,
): number | null {
  return r.landing_scored_g_force ?? r.landing_peak_g_force ?? null;
}

function QuickFlags({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const flags: { label: string; tone: "warn" | "err" }[] = [];

  // HARD LANDING — V/S oder Peak-G erreichen Hard/Severe-Schwellen
  // (gespiegelt aus landingScoring.ts T_VS_HARD_FPM / T_G_HARD).
  // v0.7.17 (B-015): vs_at_edge_fpm bevorzugen — siehe scoreBasisVs Doc.
  // v0.12.3 (LE9): G-Flag auf dem gescorten (EMA) Wert, nicht dem Roh-Peak.
  const peakVs =
    (record.vs_at_edge_fpm != null && record.vs_at_edge_fpm < 0
      ? record.vs_at_edge_fpm
      : null) ??
    record.landing_peak_vs_fpm ??
    record.landing_rate_fpm;
  const gForFlag = scoreG(record) ?? 0;
  const isHardVs = Math.abs(peakVs) >= 600;
  const isHardG = gForFlag >= 1.7;
  if (isHardVs || isHardG) {
    const severe = Math.abs(peakVs) >= 1000 || gForFlag >= 2.1;
    flags.push({
      label: severe ? t("landing.flag.severe") : t("landing.flag.hard"),
      tone: "err",
    });
  }

  // BOUNCE × n
  // v0.8.3 (#8): Auch score-freie Hopser (5-14 ft) zeigen. Vorher
  // landeten 14-ft-Hopser stumm bei bounce_count=0 — Pilot dachte
  // „nicht erkannt" (Reported 2026-05-14 Adrian, TD #167).
  //
  // Drei Faelle:
  //   bounce_count > 0                            → wie bisher, voller Flag
  //   bounce_count = 0, forensic_bounce_count > 0 → Light-bounce-Hinweis
  //   alle 0                                       → kein Flag
  if (record.bounce_count > 0) {
    flags.push({
      label: `${t("landing.flag.bounce")} × ${record.bounce_count}`,
      tone: record.bounce_count >= 2 ? "err" : "warn",
    });
  } else if ((record.forensic_bounce_count ?? 0) > 0) {
    const heightFt = record.bounce_max_agl_ft != null
      ? Math.round(record.bounce_max_agl_ft)
      : null;
    flags.push({
      label: heightFt != null
        ? t("landing.flag.bounce_light_with_height", { ft: heightFt })
        : t("landing.flag.bounce_light"),
      tone: "warn",
    });
  }

  // OFF-CENTERLINE — > 5 m vom Centerline weg ist auffällig
  if (record.runway_match && Math.abs(record.runway_match.centerline_distance_m) > 5) {
    flags.push({
      label: t("landing.flag.off_centerline"),
      tone: "warn",
    });
  }

  // UNSTABLE APPROACH — σ V/S > 400 (Score-Lib-Schwelle für "bad")
  if ((record.approach_vs_stddev_fpm ?? 0) > 400) {
    flags.push({
      label: t("landing.flag.unstable_approach"),
      tone: "warn",
    });
  }

  if (flags.length === 0) return null;
  return (
    <div className="landing-flags">
      {flags.map((f, i) => (
        <span key={i} className={`landing-flag landing-flag--${f.tone}`}>
          {f.label}
        </span>
      ))}
    </div>
  );
}

// ---- Trend sparkline (last N landings) ---------------------------------

function TrendSparkline({ records }: { records: LandingRecord[] }) {
  const { t } = useTranslation();
  if (records.length < 2) return null;
  // Use newest-first list; chart wants oldest→newest left→right.
  const latest = records.slice(0, 12).reverse();
  const w = 360;
  const h = 60;
  const pad = 6;
  const innerW = w - pad * 2;
  const innerH = h - pad * 2;
  const scores = latest.map((r) => r.score_numeric);
  const sMin = Math.min(...scores, 50);
  const sMax = Math.max(...scores, 100);
  const range = Math.max(1, sMax - sMin);
  const xStep = innerW / Math.max(1, latest.length - 1);
  const y = (s: number) => pad + innerH - ((s - sMin) / range) * innerH;
  const path = latest
    .map((r, i) =>
      `${i === 0 ? "M" : "L"} ${(pad + i * xStep).toFixed(1)} ${y(r.score_numeric).toFixed(1)}`,
    )
    .join(" ");
  const last = latest[latest.length - 1];
  return (
    <div className="landing-trend">
      <span className="landing-trend__label">{t("landing.trend")}</span>
      <svg
        className="landing-trend__svg"
        viewBox={`0 0 ${w} ${h}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={t("landing.trend")}
      >
        <path d={path} fill="none" stroke="#38bdf8" strokeWidth="2" />
        {latest.map((r, i) => (
          <circle
            key={r.pirep_id}
            cx={pad + i * xStep}
            cy={y(r.score_numeric)}
            r={i === latest.length - 1 ? 4 : 2}
            fill={i === latest.length - 1 ? gradeColor(r.grade_letter) : "#38bdf8"}
          />
        ))}
      </svg>
      <span className="landing-trend__last">
        {last.score_numeric}/100 · {last.grade_letter}
      </span>
    </div>
  );
}

// ---- Fuel comparison bar ------------------------------------------------

function FuelComparisonBar({
  plan,
  actual,
}: {
  plan: number;
  actual: number;
}) {
  const { t } = useTranslation();
  const max = Math.max(plan, actual, 1);
  const planPct = (plan / max) * 100;
  const actualPct = (actual / max) * 100;
  const diff = actual - plan;
  const sign = diff >= 0 ? "+" : "";
  const pct = (diff / Math.max(1, plan)) * 100;

  return (
    <div className="landing-fuelbar">
      <div className="landing-fuelbar__row">
        <span className="landing-fuelbar__label">{t("landing.plan_burn")}</span>
        <div className="landing-fuelbar__track">
          <div
            className="landing-fuelbar__fill landing-fuelbar__fill--plan"
            style={{ width: `${planPct}%` }}
          />
        </div>
        <span className="landing-fuelbar__value">{plan.toFixed(0)} kg</span>
      </div>
      <div className="landing-fuelbar__row">
        <span className="landing-fuelbar__label">{t("landing.actual_burn")}</span>
        <div className="landing-fuelbar__track">
          <div
            className={`landing-fuelbar__fill ${
              diff > 0
                ? "landing-fuelbar__fill--over"
                : "landing-fuelbar__fill--under"
            }`}
            style={{ width: `${actualPct}%` }}
          />
        </div>
        <span className="landing-fuelbar__value">{actual.toFixed(0)} kg</span>
      </div>
      {/* v0.11.0-dev: Delta-Pill statt versteckter Mini-Text. Farbcode:
          ±1% grün (im Rahmen) · 1–5% gelb · >5% rot — Symbolik klar
          unterscheidbar zwischen ok/warn/alert ohne nur auf Farbe zu
          setzen (auch für Color-Blind-Piloten lesbar). */}
      {(() => {
        const absPct = Math.abs(pct);
        const deltaColor =
          absPct < 1 ? "#22c55e" : absPct < 5 ? "#eab308" : "#ef4444";
        const deltaIcon =
          absPct < 1 ? "✓" : absPct < 5 ? "≈" : diff > 0 ? "▲" : "▼";
        return (
          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              marginTop: 4,
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "3px 10px",
                borderRadius: 4,
                background: `${deltaColor}1a`,
                border: `1px solid ${deltaColor}55`,
                color: deltaColor,
                fontSize: "0.82rem",
                fontWeight: 600,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              <span>{deltaIcon}</span>
              <span>
                {sign}
                {diff.toFixed(0)} kg
              </span>
              <span style={{ opacity: 0.75 }}>
                ({sign}
                {pct.toFixed(1)}%)
              </span>
            </span>
          </div>
        );
      })()}
    </div>
  );
}

// ---- Detail view --------------------------------------------------------

// ─── v0.12.8-dev: PDF-Export der Landungs-Analyse ───────────────────────
//
// Der Pilot exportiert die Landungs-Analyse als mehrseitigen A4-Report.
// Mechanik:
//   1. Button "PDF exportieren" in `.landing-detail__top` setzt
//      `printing = true`.
//   2. `<LandingReport>` wird via `createPortal` in einen an `document.body`
//      gehängten Container gerendert. Auf dem Bildschirm ist der Container
//      `display:none` — sichtbar wird er NUR im `@media print`.
//   3. Ein Effect ruft `window.print()` (nach dem ersten Render-Frame) und
//      registriert einen One-Shot `afterprint`-Listener, der `printing`
//      zurücksetzt — der Report verschwindet wieder aus dem DOM.
// Der Report nutzt ALLE lokalen Helper (gradeColor, fmtNumber, computeSub-
// Scores, ApproachChart, VsCurveChart, RunwayDiagramV2 …), darum lebt er
// hier in-file statt in einer eigenen Datei.

/** App-Version, die in der Report-Fußzeile erscheint. */
const REPORT_APP_VERSION = "0.12.8";

/** v0.12.8-dev: Fußzeile — EINMAL am Ende des fließenden Dokuments
 *  (vorher pro Seite). Generierungs-Datum + App-Version. */
function ReportFooter() {
  const { t } = useTranslation();
  const generated = t("landing.report.generated", {
    date: new Date().toLocaleDateString(),
    version: REPORT_APP_VERSION,
  });
  return (
    <div className="report-footer">
      <span>{generated}</span>
    </div>
  );
}

/** Branding-Kopfzeile (Wortmarke + Untertitel + Akzent-Linie). */
function ReportHeader() {
  const { t } = useTranslation();
  return (
    <div className="report-head">
      <div className="report-head__brand">
        <span className="report-head__mark">{t("landing.report.title")}</span>
        <span className="report-head__sub">
          {t("landing.report.subtitle")}
        </span>
      </div>
      <div className="report-head__rule" />
    </div>
  );
}

/** Section-Überschrift mit kurzem Akzent-Strich. v0.12.8-dev: jede
 *  Section bekommt eine eigene Akzentfarbe (`accent`) — wird als CSS-
 *  Variable `--report-accent` gesetzt und färbt Titel, Strich und die
 *  Kachel-Akzente. Bringt Farbe in den sonst sehr weißen Report. */
function ReportSection({
  title,
  accent = "#2563eb",
  children,
}: {
  title: string;
  accent?: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className="report-section"
      style={{ ["--report-accent" as string]: accent } as React.CSSProperties}
    >
      <h3 className="report-section__title">
        {title}
        <span className="report-section__rule" />
      </h3>
      {children}
    </section>
  );
}

/** Dunkle Karte um ein Chart — die Chart-Komponenten zeichnen helle
 *  Strokes auf transparentem Grund und wären auf weißem Papier
 *  unsichtbar. Die explizite Breite sorgt für korrektes SVG-Sizing
 *  im Print. */
function ReportChartCard({
  caption,
  children,
}: {
  caption: string;
  children: React.ReactNode;
}) {
  return (
    <div className="report-chart-card">
      <div className="report-chart-card__caption">{caption}</div>
      <div className="report-chart-card__panel">{children}</div>
    </div>
  );
}

/** Eine Touchdown-Kennwert-Kachel: Label + großer Wert. */
function ReportTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="report-tile">
      <div className="report-tile__label">{label}</div>
      <div className="report-tile__value">{value}</div>
    </div>
  );
}

/**
 * v0.12.8-dev: Der A4-Landungs-Report. Hell, luftig, EIN Akzent
 * (#2563eb). KEIN fixes A4-Box-Paradigma mehr — der Report ist EIN
 * fließendes Dokument; der Browser paginiert selbst (kein Leerseiten-
 * Bug). Ausgewählte Section-Gruppen starten via `.report-break-before`
 * auf einer frischen Seite. Nur sichtbar im `@media print`.
 */
function LandingReport({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  const callsign = record.airline_icao
    ? `${record.airline_icao}${record.flight_number}`
    : record.flight_number;

  const subs = useMemo(() => computeSubScores(record), [record]);
  const scored = subs.filter((s) => !s.skipped);
  const worst = scored.length
    ? [...scored].sort((a, b) => a.points - b.points)[0]
    : null;

  // Charts nur rendern, wenn genug Datenpunkte da sind (gleiche
  // Schwellen wie LandingDetail).
  const hasApproach = record.approach_samples.length >= 3;
  const hasCloseup = record.touchdown_profile.length >= 5;
  const v2Props =
    record.runway_match && (record.runway_geometry_trusted ?? true)
      ? mapLandingRecordToV2Props(record)
      : null;
  const hasCharts = hasApproach || hasCloseup || v2Props != null;

  // Score-Label: bei Confirmed Accident → "ABSTURZ ERKANNT".
  const heroLabel =
    record.accident === true
      ? t("landing.report.accident_label")
      : record.score_label.toUpperCase();

  // Sub-Score-Balken: Farbe nach Punkten (grün / amber / rot).
  const barColor = (pts: number) =>
    pts >= 80 ? "#22c55e" : pts >= 55 ? "#f59e0b" : "#ef4444";

  const rm = record.runway_match;

  // v0.12.8-dev: Anflug-Stabilität — sichtbar wenn mindestens eines der
  // 7 Stability-v2-Felder vorhanden ist (alte PIREPs ohne sie zeigen die
  // "no_data"-Hint). Bools zählen nur als vorhanden wenn != null.
  const hasStability =
    record.approach_vs_jerk_fpm != null ||
    record.approach_bank_stddev_deg != null ||
    record.approach_ias_stddev_kt != null ||
    record.approach_vs_deviation_fpm != null ||
    record.approach_max_vs_deviation_below_500_fpm != null ||
    record.approach_stable_config != null ||
    record.approach_excessive_sink != null;

  // v0.12.8-dev: Sprit & Gewicht — sichtbar wenn irgendein Fuel/Weight-
  // Feld da ist (gleiche Bedingung wie die Loadsheet-Section im
  // on-screen LandingDetail).
  const hasFuel =
    record.planned_burn_kg != null ||
    record.actual_trip_burn_kg != null ||
    record.block_fuel_kg != null ||
    record.takeoff_fuel_kg != null ||
    record.landing_fuel_kg != null ||
    record.takeoff_weight_kg != null ||
    record.landing_weight_kg != null ||
    record.fuel_efficiency_pct != null;

  return (
    // v0.12.8-dev: EIN fließendes Dokument — kein <ReportPage>-A4-Box-
    // Stapel mehr. EIN <ReportHeader> oben, dann fließende Sections,
    // EINE <ReportFooter> ganz unten. Saubere Seitenanfänge entstehen
    // durch `.report-break-before` auf den Section-Gruppen.
    <div className="landing-report report-page">
      <ReportHeader />

      {/* ── Identität & Score ──────────────────────────────────────── */}
      <div className="report-identity">
        <div className="report-identity__callsign">{callsign}</div>
        <div className="report-identity__route">
          <span>{record.dpt_airport}</span>
          <span className="report-identity__arrow">→</span>
          <span>{record.arr_airport}</span>
        </div>
        <div className="report-identity__meta">
          <div>
            <span className="report-identity__k">
              {t("landing.report.aircraft")}
            </span>
            <span className="report-identity__v">
              {record.aircraft_title ?? "—"}
              {record.aircraft_registration
                ? ` · ${record.aircraft_registration}`
                : ""}
              {record.aircraft_icao ? ` · ${record.aircraft_icao}` : ""}
            </span>
          </div>
          <div>
            <span className="report-identity__k">
              {t("landing.report.simulator")}
            </span>
            <span className="report-identity__v">
              {record.sim_kind ?? "—"}
            </span>
          </div>
          <div>
            <span className="report-identity__k">
              {t("landing.report.touchdown_time")}
            </span>
            <span className="report-identity__v">
              {fmtDateTime(record.touchdown_at)}
            </span>
          </div>
        </div>
      </div>

      <div
        className="report-hero"
        style={{
          borderLeft: `7px solid ${gradeColor(record.grade_letter)}`,
        }}
      >
        <div
          className="report-hero__badge"
          style={{ background: gradeColor(record.grade_letter) }}
        >
          {record.grade_letter}
        </div>
        <div className="report-hero__text">
          <div className="report-hero__score">
            {record.score_numeric}
            <span className="report-hero__of">
              {" "}
              {t("landing.report.hero_of")}
            </span>
          </div>
          <div className="report-hero__label">{heroLabel}</div>
        </div>
      </div>

      <ReportSection title={t("landing.report.breakdown")}>
        <div className="report-bars">
          {subs.map((s) => (
            <div key={s.key} className="report-bar">
              <div className="report-bar__head">
                <span className="report-bar__label">
                  {t(`landing.sub.${s.key}`)}
                </span>
                <span className="report-bar__pts">
                  {s.skipped
                    ? t("landing.skipped_label")
                    : `${s.points} PTS`}
                </span>
              </div>
              <div className="report-bar__track">
                {!s.skipped && (
                  <div
                    className="report-bar__fill"
                    style={{
                      width: `${Math.max(0, Math.min(100, s.points))}%`,
                      background: barColor(s.points),
                    }}
                  />
                )}
              </div>
            </div>
          ))}
        </div>
      </ReportSection>

      {worst && (
        <ReportSection title={t("landing.report.coach")}>
          <div className="report-coach">
            <div className="report-coach__focus">
              {t(`landing.sub.${worst.key}`)}
            </div>
            <p className="report-coach__body">
              {t(coachTipKey(worst.rationale))}
            </p>
          </div>
        </ReportSection>
      )}

      {/* ── Touchdown-Kennwerte (frische Seite) ────────────────────── */}
      <div className="report-break-before">
        <ReportSection title={t("landing.report.touchdown_section")}>
          <div className="report-tiles">
            <ReportTile
              label={t("landing.landing_rate")}
              value={fmtNumber(scoreBasisVs(record), 0, "fpm")}
            />
            <ReportTile
              label={t("landing.g_force")}
              value={fmtNumber(record.landing_g_force, 2, "G")}
            />
            <ReportTile
              label={t("landing.pitch")}
              value={fmtSigned(record.landing_pitch_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.bank")}
              value={fmtSigned(record.landing_bank_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.speed")}
              value={fmtNumber(record.landing_speed_kt, 0, "kt")}
            />
            <ReportTile
              label={t("landing.sideslip")}
              value={fmtSigned(record.touchdown_sideslip_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.bounces")}
              value={String(record.bounce_count)}
            />
            <ReportTile
              label={t("landing.heading")}
              value={fmtNumber(record.landing_heading_deg, 0, "°")}
            />
          </div>
        </ReportSection>

        {/* v0.12.8-dev: NEUE Section — Anflug-Stabilität. Felder +
            Labels gespiegelt von ApproachStabilityCard für Konsistenz. */}
        <ReportSection
          title={t("landing.report.stability_section")}
          accent="#7c3aed"
        >
          {hasStability ? (
            <div className="report-tiles">
              {record.approach_vs_jerk_fpm != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.vs_jerk.label",
                  )}
                  value={fmtNumber(record.approach_vs_jerk_fpm, 0, "fpm")}
                />
              )}
              {record.approach_bank_stddev_deg != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.bank_sigma.label",
                  )}
                  value={fmtNumber(record.approach_bank_stddev_deg, 1, "°")}
                />
              )}
              {record.approach_ias_stddev_kt != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.ias_sigma.label",
                  )}
                  value={fmtNumber(record.approach_ias_stddev_kt, 1, "kt")}
                />
              )}
              {record.approach_vs_deviation_fpm != null && (
                <ReportTile
                  label={(() => {
                    const gs = record.runway_match?.glideslope_angle_deg;
                    return gs != null &&
                      gs >= 2 &&
                      gs <= 7.5 &&
                      Math.abs(gs - 3) > 0.05
                      ? t(
                          "landing.approach_stability_card.tiles.vs_vs_ils.label_custom",
                          { angle: String(gs) },
                        )
                      : t("landing.approach_stability_card.tiles.vs_vs_ils.label");
                  })()}
                  value={fmtNumber(record.approach_vs_deviation_fpm, 0, "fpm")}
                />
              )}
              {record.approach_max_vs_deviation_below_500_fpm != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.max_vs_dev.label",
                  )}
                  value={fmtNumber(
                    record.approach_max_vs_deviation_below_500_fpm,
                    0,
                    "fpm",
                  )}
                />
              )}
              {record.approach_stable_config != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.landing_config.label",
                  )}
                  value={
                    record.approach_stable_config
                      ? t(
                          "landing.approach_stability_card.tiles.landing_config.value_ok",
                        )
                      : t(
                          "landing.approach_stability_card.tiles.landing_config.value_partial",
                        )
                  }
                />
              )}
              {record.approach_excessive_sink != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.sink_rate.label",
                  )}
                  value={
                    record.approach_excessive_sink
                      ? t(
                          "landing.approach_stability_card.tiles.sink_rate.value_excessive",
                        )
                      : t(
                          "landing.approach_stability_card.tiles.sink_rate.value_ok",
                        )
                  }
                />
              )}
            </div>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>
      </div>

      {/* ── Wind & Bahn (frische Seite) ────────────────────────────── */}
      <div className="report-break-before">
        <ReportSection
          title={t("landing.report.wind_section")}
          accent="#0891b2"
        >
          {(() => {
            const hw = record.headwind_kt;
            const xw = record.crosswind_kt;
            if (hw == null && xw == null) {
              return (
                <div className="report-empty">
                  {t("landing.report.no_data")}
                </div>
              );
            }
            const hwV = hw ?? 0;
            const xwV = xw ?? 0;
            const total = Math.sqrt(hwV * hwV + xwV * xwV);
            const hwLabel =
              hwV < -0.5
                ? t("landing.wind_tailwind")
                : t("landing.wind_headwind");
            const xwSide =
              xwV >= 0
                ? t("landing.wind_side_right")
                : t("landing.wind_side_left");
            return (
              <div className="report-tiles">
                <ReportTile
                  label={hwLabel}
                  value={fmtNumber(Math.abs(hwV), 0, "kt")}
                />
                <ReportTile
                  label={`${t("landing.wind_crosswind")} · ${xwSide}`}
                  value={fmtNumber(Math.abs(xwV), 0, "kt")}
                />
                <ReportTile
                  label={t("landing.wind_total")}
                  value={fmtNumber(total, 0, "kt")}
                />
              </div>
            );
          })()}
        </ReportSection>

        <ReportSection
          title={t("landing.report.runway_section")}
          accent="#0d9488"
        >
          {rm ? (
            <div className="report-tiles">
              <ReportTile
                label={t("landing.runway")}
                value={`${rm.airport_ident} ${rm.runway_ident}`}
              />
              <ReportTile
                label={t("landing.report.rwy_length")}
                value={fmtNumber(rm.length_ft * 0.3048, 0, "m")}
              />
              <ReportTile
                label={t("landing.report.rwy_surface")}
                value={rm.surface || "—"}
              />
              {record.td_distance_from_threshold_m != null && (
                <ReportTile
                  label={t("landing.report.td_distance")}
                  value={fmtNumber(
                    record.td_distance_from_threshold_m,
                    0,
                    "m",
                  )}
                />
              )}
              {record.td_in_tdz != null && (
                <ReportTile
                  label={t("landing.report.in_tdz")}
                  value={
                    record.td_in_tdz
                      ? t("landing.report.yes")
                      : t("landing.report.no")
                  }
                />
              )}
              {record.aim_delta_m != null && (
                <ReportTile
                  label={t("landing.report.aim_delta")}
                  value={fmtSigned(record.aim_delta_m, 0, "m")}
                />
              )}
              {Number.isFinite(rm.centerline_distance_m) && (
                <ReportTile
                  label={t("landing.report.centerline")}
                  value={fmtNumber(
                    Math.abs(rm.centerline_distance_m),
                    1,
                    "m",
                  )}
                />
              )}
              {record.rollout_distance_m != null && (
                <ReportTile
                  label={t("landing.report.rollout_dist")}
                  value={fmtNumber(record.rollout_distance_m, 0, "m")}
                />
              )}
            </div>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>

        {/* v0.12.8-dev: NEUE Section — Sprit & Gewicht (Plan vs. Ist).
            Felder + Labels gespiegelt von der Loadsheet-Section im
            on-screen LandingDetail. */}
        <ReportSection
          title={t("landing.report.fuel_section")}
          accent="#d97706"
        >
          {hasFuel ? (
            <div className="report-tiles">
              {(record.planned_burn_kg != null ||
                record.actual_trip_burn_kg != null) && (
                <ReportTile
                  label={`${t("landing.trip_burn")} · ${t(
                    "landing.actual_burn",
                  )}`}
                  value={fmtNumber(record.actual_trip_burn_kg, 0, "kg")}
                />
              )}
              {record.planned_burn_kg != null && (
                <ReportTile
                  label={`${t("landing.trip_burn")} · ${t(
                    "landing.plan_burn",
                  )}`}
                  value={fmtNumber(record.planned_burn_kg, 0, "kg")}
                />
              )}
              {record.fuel_efficiency_kg_diff != null && (
                <ReportTile
                  label={t("landing.report.fuel_diff")}
                  value={fmtSigned(record.fuel_efficiency_kg_diff, 0, "kg")}
                />
              )}
              {record.fuel_efficiency_pct != null && (
                <ReportTile
                  label={t("landing.report.fuel_efficiency")}
                  value={fmtSigned(record.fuel_efficiency_pct, 1, "%")}
                />
              )}
              {record.block_fuel_kg != null && (
                <ReportTile
                  label={t("landing.block_fuel")}
                  value={fmtNumber(record.block_fuel_kg, 0, "kg")}
                />
              )}
              {record.landing_fuel_kg != null && (
                <ReportTile
                  label={t("landing.landing_fuel")}
                  value={fmtNumber(record.landing_fuel_kg, 0, "kg")}
                />
              )}
              {record.takeoff_weight_kg != null && (
                <ReportTile
                  label={t("landing.tow")}
                  value={fmtNumber(record.takeoff_weight_kg, 0, "kg")}
                />
              )}
              {record.landing_weight_kg != null && (
                <ReportTile
                  label={t("landing.ldw")}
                  value={fmtNumber(record.landing_weight_kg, 0, "kg")}
                />
              )}
            </div>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>
      </div>

      {/* ── Profile & Verläufe (frische Seite) ─────────────────────── */}
      {hasCharts && (
        <div className="report-break-before">
          <ReportSection title={t("landing.report.charts_section")}>
            <div className="report-charts">
              {hasApproach && (
                <ReportChartCard
                  caption={t("landing.report.approach_chart")}
                >
                  <ApproachChart
                    samples={record.approach_samples}
                    glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
                  />
                </ReportChartCard>
              )}
              {hasCloseup && (
                <ReportChartCard caption={t("landing.report.vs_curve")}>
                  <VsCurveChart profile={record.touchdown_profile} />
                </ReportChartCard>
              )}
              {v2Props && (
                <ReportChartCard
                  caption={t("landing.report.runway_diagram")}
                >
                  <RunwayDiagramV2 {...v2Props} />
                </ReportChartCard>
              )}
            </div>
          </ReportSection>
        </div>
      )}

      {/* v0.12.8-dev: EINE Fußzeile am Dokumentende. */}
      <ReportFooter />
    </div>
  );
}

function LandingDetail({
  record,
  allRecords,
  onBack,
  onDelete,
  isPreview,
}: {
  record: LandingRecord;
  /** Full history — used to compute personal-best comparisons. */
  allRecords: LandingRecord[];
  onBack: () => void;
  onDelete?: () => void;
  isPreview: boolean;
}) {
  const { t } = useTranslation();

  const callsign = record.airline_icao
    ? `${record.airline_icao}${record.flight_number}`
    : record.flight_number;

  const subs = useMemo(() => computeSubScores(record), [record]);

  // Personal-best comparison — best (closest to zero) landing rate
  // across ALL filed PIREPs. None when this is the only record yet.
  const personalBest = useMemo(() => {
    const others = allRecords.filter((r) => r.pirep_id !== record.pirep_id);
    if (others.length === 0) return null;
    return others.reduce(
      (best, r) =>
        Math.abs(r.landing_rate_fpm) < Math.abs(best.landing_rate_fpm) ? r : best,
      others[0],
    );
  }, [allRecords, record.pirep_id]);

  const isNewBest =
    personalBest != null &&
    Math.abs(record.landing_rate_fpm) < Math.abs(personalBest.landing_rate_fpm);

  // v0.12.8-dev: PDF-Export-State. Sobald `printing` true wird, rendert
  // der Effect den <LandingReport> ins DOM, ruft `window.print()` und
  // setzt nach `afterprint` wieder zurück.
  const [printing, setPrinting] = useState(false);
  useEffect(() => {
    if (!printing) return;
    // Einen Frame warten, damit der Report (inkl. Charts) im DOM ist,
    // bevor der Druckdialog aufgeht.
    const raf = requestAnimationFrame(() => {
      const done = () => setPrinting(false);
      window.addEventListener("afterprint", done, { once: true });
      window.print();
    });
    return () => cancelAnimationFrame(raf);
  }, [printing]);

  return (
    <div className="landing-detail">
      <div className="landing-detail__top">
        <button type="button" className="landing-back" onClick={onBack}>
          ← {t("landing.back_to_list")}
        </button>
        <button
          type="button"
          className="landing-export"
          onClick={() => setPrinting(true)}
          title={t("landing.report.button")}
        >
          🖨 {t("landing.report.button")}
        </button>
        {!isPreview && onDelete && (
          <button
            type="button"
            className="landing-delete"
            onClick={onDelete}
            title={t("landing.delete")}
          >
            🗑 {t("landing.delete")}
          </button>
        )}
      </div>

      {/* v0.12.8-dev: Druck-Report — per Portal an document.body gehängt,
          auf dem Bildschirm display:none, nur sichtbar im @media print. */}
      {printing &&
        createPortal(
          <div className="landing-report-print">
            <LandingReport record={record} />
          </div>,
          document.body,
        )}

      <div className="landing-headline">
        <div
          className="landing-grade-big"
          style={{ background: gradeColor(record.grade_letter) }}
        >
          {record.grade_letter}
        </div>
        <div className="landing-headline__text">
          <h2>
            {callsign} · {record.dpt_airport} → {record.arr_airport}
          </h2>
          <div className="landing-headline__sub">
            {/* v0.7.19 GAF-707: bei Confirmed Accident wird die Primary-
                Klassifikation auf "ABSTURZ ERKANNT" ueberschrieben. Score
                bleibt sichtbar (0/100), bekommt aber die Bedeutung
                "Accident" statt "normale schwere Landung". Spec §AeroACARS
                Client Tab "Landung". */}
            {record.accident === true
              ? t("landing.accident.primary_label")
              : record.score_label.toUpperCase()}
            {" "}· {record.score_numeric}/100 ·{" "}
            {fmtDateTime(record.touchdown_at)}
            {isPreview && (
              <span className="landing-preview-badge">{t("landing.preview")}</span>
            )}
            {isNewBest && (
              <span className="landing-best-badge">★ {t("landing.new_best")}</span>
            )}
          </div>
          {record.aircraft_title && (
            <div className="landing-headline__aircraft">
              {record.aircraft_title}
              {record.aircraft_registration ? ` · ${record.aircraft_registration}` : ""}
              {record.aircraft_icao ? ` · ${record.aircraft_icao}` : ""}
              {record.sim_kind ? ` · ${record.sim_kind}` : ""}
            </div>
          )}
          {personalBest && !isNewBest && (
            <div className="landing-headline__pb">
              {t("landing.this_landing")}: {record.landing_rate_fpm.toFixed(0)} fpm ·{" "}
              {t("landing.personal_best")}: {personalBest.landing_rate_fpm.toFixed(0)}{" "}
              fpm ({personalBest.dpt_airport} → {personalBest.arr_airport})
            </div>
          )}
          {/* v0.7.1 Phase 3 F4 + P2.4-Fix: Forensik-v2 Badge mit
              Confidence-Pill. Bedingung im Component (P1.1-C:
              ux_version >= 1 AND forensics_version >= 2). Beide
              Werte kommen jetzt sauber aus dem LandingRecord. */}
          <div style={{ marginTop: "0.5rem" }}>
            <ForensicsBadge
              forensicsVersion={record.forensics_version}
              uxVersion={record.ux_version}
              confidence={record.landing_confidence}
              source={record.landing_source}
            />
          </div>
        </div>
      </div>

      {/* v0.7.19 GAF-707 Accident-Detection: rot/gelber Banner als
          Primary-Klassifikation OBERHALB von Off-Airport + Score-
          Breakdown. GAF 707 darf hier nicht als normale Hard-Landing
          erscheinen. Spec §AeroACARS Client Tab "Landung". */}
      <AccidentBanner record={record} />

      {/* v0.7.18 (B-012): Off-airport-Banner wenn der Touchdown nicht
          beim geplanten Destination-Airport war. Quelle ist die
          backend-resolution (runway_match / nearest_25nm / planned_fallback). */}
      <OffAirportBanner record={record} />

      {/* v0.5.47 — Quick-Flag-Chips direkt unter dem Headline-Block.
          Pilot sieht auf einen Blick was die Auffälligkeiten sind.
          Webapp hat das schon; jetzt auch im Client für visuelle Parität. */}
      <QuickFlags record={record} />

      {/* Score breakdown — most important new section */}
      <section className="landing-section">
        <h3>
          {t("landing.score_breakdown")}
          <InfoBadge explanation={t("landing.info.score_section")} />
        </h3>
        <ScoreBreakdown subs={subs} record={record} />
        <CoachTip subs={subs} />
      </section>

      {/* Runway */}
      {record.runway_match && (() => {
        // v0.7.6 P1-3: Runway-Geometry-Trust check.
        // - trusted ?? true → alte v0.7.5-PIREPs werden wie trusted
        //   behandelt (Backward-Compat).
        // - Bei untrusted: Centerline-Offset, Past-Threshold, runway_used_pct
        //   und das RunwayDiagram ausblenden. Rollout bleibt sichtbar
        //   (kommt aus GPS-Track).
        // - "no_runway_match" zeigt KEINEN Alarm-Pill (Privatplatz normal).
        const geometryTrusted = record.runway_geometry_trusted ?? true;
        const trustWarning = !geometryTrusted
          ? runwayTrustReasonLabel(record.runway_geometry_reason)
          : null;
        return (
          <section className="landing-section">
            <h3>
              {t("landing.runway")}
              <InfoBadge explanation={t("landing.info.runway_section")} />
            </h3>
            {trustWarning && (
              <div
                style={{
                  padding: "6px 10px",
                  marginBottom: 10,
                  borderRadius: 6,
                  background: "#3f2b0e",
                  border: "1px solid #b8842a",
                  color: "#f5d68b",
                  fontSize: "0.85rem",
                }}
              >
                ⚠ {trustWarning}
              </div>
            )}
            {(() => {
              // v0.8.2: alte RunwayDiagram → RunwayDiagramV2.
              //
              // v0.8.3.1 (Hotfix): die legacy <dl>-Liste die hier vorher
              // ZUSAETZLICH rendert wurde entfernt — V2 hat alle Felder
              // (bahn/laenge/hinter-schwelle/mittellinie/rollout/bahn-
              // auslastung/navdata/tdz/aim/tch/dds) als eigene Pills.
              // Vorher liefen beide parallel und zeigten WIDERSPRECHENDE
              // Werte: Bahn-Auslastung 52% (V2: (td_dist+rollout)/length)
              // vs 38% (legacy: rollout/length). Reported von Thomas
              // 2026-05-18 mit Fenix-A320 EVRA-Landung.
              //
              // Bei untrusted geometry NICHTS rendern — die trust-Warn-
              // Box oberhalb erklaert dem Piloten warum (vorher zeigten
              // einige legacy-Felder auch bei untrusted weiter, was
              // inkonsistent zur V2-Logik war).
              //
              // Bei v2Props=null trotz trusted geometry → das ist ein
              // Mapping-Bug, kein UI-Fallback. Tritt nicht auf weil
              // mapLandingRecordToV2Props bei trusted records komplett
              // ist (alle Pflichtfelder kommen aus record.runway_match,
              // das bei trusted=true garantiert vollstaendig ist).
              if (!geometryTrusted) return null;
              const v2Props = mapLandingRecordToV2Props(record);
              return v2Props ? <RunwayDiagramV2 {...v2Props} /> : null;
            })()}
          </section>
        );
      })()}

      {/* Touchdown: vitals + Wind compass. v0.12.8: die V/S-Nahaufnahme
          ist in die Anflug-Stabilität-Section gewandert (gestapelt unter
          dem Anflug-Profil, wie auf dem VPS). */}
      <section className="landing-section">
        <h3>{t("landing.touchdown")}</h3>
        <div className="landing-grid landing-grid--td">
          <dl className="landing-keyvals">
            {/* v0.7.11: Touchdown-Card auf die wichtigen Werte reduziert.
                Alle smoothed-VS-Werte (250/500/1000/1500 ms) + vs_at_edge
                + landing_peak_vs_fpm + Peak-G post-TD wurden hier
                entfernt — die gehoeren in die Sinkrate-Forensik-Sektion
                weiter unten (v0.7.8). Pilot sieht hier nur EINE Sinkrate
                (= Score-Basis nach v0.7.11 = vs_at_edge_fpm) + die
                Aufprall-Werte. Kein Werte-Dschungel mehr. */}
            <div>
              <dt>{t("landing.landing_rate")}</dt>
              {/* v0.7.17 (B-015): Edge-Wert bevorzugen — Touchdown-Card
                  zeigte bisher `landing_rate_fpm` (Streamer-Tick), was
                  meist 30-50 fpm vom echten Aufsetz-Moment abwich. */}
              <dd>
                {fmtNumber(
                  scoreBasisVs(record),
                  0,
                  "fpm",
                )}
              </dd>
            </div>
            <div>
              <dt>{t("landing.g_force")}</dt>
              <dd>{fmtNumber(record.landing_g_force, 2, "G")}</dd>
            </div>
            {/* v0.12.8-dev: ROH-PEAK G aus der Touchdown-Headline
                entfernt — war ~immer identisch mit G-Kraft (Doppel-
                Anzeige). Der rohe Peak bleibt in der G-Force-Forensik. */}
            <div>
              <dt>{t("landing.pitch")}</dt>
              <dd>{fmtSigned(record.landing_pitch_deg, 1, "°")}</dd>
            </div>
            <div>
              <dt>{t("landing.bank")}</dt>
              <dd>{fmtSigned(record.landing_bank_deg, 1, "°")}</dd>
            </div>
            <div>
              <dt>{t("landing.speed")}</dt>
              <dd>{fmtNumber(record.landing_speed_kt, 0, "kt")}</dd>
            </div>
            <div>
              <dt>{t("landing.sideslip")}</dt>
              <dd>{fmtSigned(record.touchdown_sideslip_deg, 1, "°")}</dd>
            </div>
            <div>
              <dt>{t("landing.bounces")}</dt>
              <dd>{record.bounce_count}</dd>
            </div>
            <div>
              <dt>{t("landing.heading")}</dt>
              <dd>{fmtNumber(record.landing_heading_deg, 0, "°")}</dd>
            </div>
          </dl>
          <WindCompass
            headwindKt={record.headwind_kt}
            crosswindKt={record.crosswind_kt}
            runwayIdent={record.runway_match?.runway_ident ?? null}
          />
        </div>
      </section>

      {/* Approach stability — v0.11.0-dev: 7-Kacheln-Card analog zur
          aeroacars-live-Webapp (V/S-Jerk, Bank σ, IAS σ, Sink Rate,
          Landing-Config, V/S vs. 3°-ILS, Max V/S-Dev <500ft) plus
          STABLE-GATE-Pill und Coaching. Der alte schmale Stability-
          Indicator (nur σ-V/S und σ-Bank) ist abgelöst — alle Werte
          kommen direkt aus dem Backend (compute_approach_stability_v2),
          die Card rendert nur. Der Approach-Chart darunter bleibt. */}
      <ApproachStabilityCard
        vsJerkFpm={record.approach_vs_jerk_fpm}
        bankStddevDeg={record.approach_bank_stddev_deg}
        iasStddevKt={record.approach_ias_stddev_kt}
        excessiveSink={record.approach_excessive_sink}
        stableConfig={record.approach_stable_config}
        vsDeviationFpm={record.approach_vs_deviation_fpm}
        maxVsDeviationBelow500Fpm={
          record.approach_max_vs_deviation_below_500_fpm
        }
        usedHat={record.approach_used_hat}
        sampleCount={
          record.gate_window?.sample_count ?? record.approach_samples.length
        }
        simKind={record.sim_kind}
        glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
      />
      {record.approach_samples.length >= 3 && (
        <section className="landing-section">
          <h3>{t("landing.approach_stability")}</h3>
          <div className="landing-stability-chart">
            <ApproachChart
                    samples={record.approach_samples}
                    glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
                  />
          </div>
          {/* v0.12.8: Touchdown-Nahaufnahme (50 Hz, −4…+3 s) direkt unter
              dem Anflug-Profil — gleiche Anordnung wie auf dem VPS. */}
          {record.touchdown_profile.length >= 5 && (
            <>
              <h3 style={{ marginTop: 18 }}>
                {t("landing.vs_curve_section")}
              </h3>
              <div className="landing-stability-chart">
                <VsCurveChart profile={record.touchdown_profile} />
              </div>
            </>
          )}
        </section>
      )}

      {/* v0.7.8: Sinkrate-Forensik — erklaert dem Piloten warum die
          Landerate so ist wie sie ist. Spec docs/spec/v0.7.8-landing-rate-
          explainability.md. Rendert nur wenn 50-Hz-Forensik-Felder
          vorhanden sind (hasForensics()), sonst kompakter Legacy-Hinweis. */}
      <SinkrateForensik record={record} />

      {/* v0.7.17 (B-009): G-Force-Forensik — analog zur Sinkrate-Forensik.
          Erklaert warum AeroACARS bei butterweichen Landungen manchmal hohe
          G-Werte misst (Sim-Strut-Compression statt echtem Pilot-Impact)
          und der Master-Score trotzdem als „Smooth" klassifiziert wird. */}
      <GForceForensik record={record} />

      {/* v0.5.43: Flare-Quality — als eigene Section im gleichen Stil wie
          Approach-Stability. Nur sichtbar wenn die 50-Hz-Forensik-Felder
          gefuellt sind (= v0.5.39+ Sampler hat den Buffer-Dump geschafft).
          Pre-v0.5.39 PIREPs zeigen die Section nicht. */}
      {record.flare_quality_score != null && (
        <section className="landing-section landing-section--flare">
          <h3>
            {t("landing.flare_section")}
            {record.flare_detected === true && (
              <span className="landing-flare__chip landing-flare__chip--ok">
                ✈ {t("landing.flare_detected")}
              </span>
            )}
            {record.flare_detected === false && (
              <span className="landing-flare__chip landing-flare__chip--warn">
                {t("landing.flare_not_detected")}
              </span>
            )}
          </h3>
          <div className="landing-flare">
            <div className="landing-flare__score">
              <div className="landing-flare__score-num" data-band={
                record.flare_quality_score >= 80 ? "good" :
                record.flare_quality_score >= 60 ? "ok" : "bad"
              }>
                {record.flare_quality_score}
              </div>
              <div className="landing-flare__score-label">
                {t("landing.flare_score")}
              </div>
              <div className="landing-flare__score-hint">
                {(() => {
                  const bd = flareSubScores(
                    record.vs_at_flare_end_fpm,
                    record.flare_reduction_fpm,
                  );
                  if (!bd) return t("landing.flare_score_hint");
                  // v0.12.7: Aufschlüsselung statt statischem Hinweis.
                  return t("landing.flare_breakdown", {
                    vs: Math.round(record.vs_at_flare_end_fpm ?? 0),
                    ep: bd.endpoint,
                    red: Math.round(record.flare_reduction_fpm ?? 0),
                    bonus: bd.bonus,
                  });
                })()}
              </div>
            </div>
            <dl className="landing-keyvals landing-flare__metrics">
              {record.peak_vs_pre_flare_fpm != null && (
                <div title={t("landing.flare_pre_vs_hint") ?? undefined}>
                  <dt>{t("landing.flare_pre_vs")}</dt>
                  <dd>{fmtNumber(record.peak_vs_pre_flare_fpm, 0, "fpm")}</dd>
                </div>
              )}
              {record.vs_at_flare_end_fpm != null && (
                <div title={t("landing.flare_end_vs_hint") ?? undefined}>
                  <dt>{t("landing.flare_end_vs")}</dt>
                  <dd>{fmtNumber(record.vs_at_flare_end_fpm, 0, "fpm")}</dd>
                </div>
              )}
              {record.flare_reduction_fpm != null && (
                <div title={t("landing.flare_reduction_hint") ?? undefined}>
                  <dt>{t("landing.flare_reduction")}</dt>
                  <dd>{fmtSigned(record.flare_reduction_fpm, 0, "fpm")}</dd>
                </div>
              )}
              {record.flare_dvs_dt_fpm_per_sec != null && (
                <div title={t("landing.flare_dvs_dt_hint") ?? undefined}>
                  <dt>{t("landing.flare_dvs_dt")}</dt>
                  <dd>{fmtSigned(record.flare_dvs_dt_fpm_per_sec, 0, "fpm/s")}</dd>
                </div>
              )}
            </dl>
          </div>
        </section>
      )}


      {/* Fuel + Weight — Soll/Ist-Vergleich (v0.3.0).
          Render whenever ANY fuel/weight value is present. */}
      {(record.planned_burn_kg != null ||
        record.actual_trip_burn_kg != null ||
        record.block_fuel_kg != null ||
        record.takeoff_fuel_kg != null ||
        record.landing_fuel_kg != null ||
        record.takeoff_weight_kg != null ||
        record.landing_weight_kg != null) && (
        <section className="landing-section">
          <h3>
            {t("landing.loadsheet_section_title")}
            <InfoBadge explanation={t("landing.info.fuel_section")} />
          </h3>
          {/* v0.4.2: Hinweis wenn der Flug keinen SimBrief-OFP hatte —
              dann sind alle SOLL-Spalten in den Tabellen unten leer.
              Banner erklärt klar warum, statt nur ratlose Striche. */}
          {record.planned_block_fuel_kg == null &&
            record.planned_burn_kg == null &&
            record.planned_tow_kg == null &&
            record.planned_ldw_kg == null &&
            record.planned_zfw_kg == null && (
              <div className="landing-no-plan-hint" role="note">
                ℹ️ {t("landing.no_plan_hint")}
              </div>
            )}
          {record.planned_burn_kg != null && record.actual_trip_burn_kg != null && (
            <FuelComparisonBar
              plan={record.planned_burn_kg}
              actual={record.actual_trip_burn_kg}
            />
          )}
          {/* v0.11.0-dev Pass 5: Treibstoff + Gewicht nebeneinander als
              2-Spalten-Grid auf breiten Screens (≥ 720 px Card-Breite,
              CSS minmax sorgt für auto-fit). Auf schmalen Screens
              stapeln sich die zwei Cards automatisch untereinander. So
              entsteht klarer Rhythmus: 2 kompakte Cards oben, 1 Hero-
              Score-Card (LoadsheetScore) unten — keine endlose vertikale
              Liste mehr, kein „Klotz"-Effekt. */}
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))",
              gap: 14,
              marginTop: "1rem",
            }}
          >
            <ComparisonTable
              title={t("landing.fuel_table")}
              icon="⛽"
              rows={[
                {
                  label: t("landing.block_fuel"),
                  ist: record.block_fuel_kg,
                  soll: record.planned_block_fuel_kg,
                },
                {
                  label: t("landing.takeoff_fuel"),
                  ist: record.takeoff_fuel_kg,
                  soll: null, // SimBrief OFP hat nur Block + Burn, kein TO-Fuel separat
                },
                {
                  label: t("landing.landing_fuel"),
                  ist: record.landing_fuel_kg,
                  soll:
                    record.planned_block_fuel_kg != null && record.planned_burn_kg != null
                      ? record.planned_block_fuel_kg - record.planned_burn_kg
                      : null,
                },
                {
                  label: t("landing.trip_burn"),
                  ist: record.actual_trip_burn_kg,
                  soll: record.planned_burn_kg,
                },
              ]}
            />
            <ComparisonTable
              title={t("landing.weight_table")}
              icon="⚖️"
              rows={[
                {
                  label: t("landing.tow"),
                  ist: record.takeoff_weight_kg,
                  soll: record.planned_tow_kg,
                },
                {
                  label: t("landing.ldw"),
                  ist: record.landing_weight_kg,
                  soll: record.planned_ldw_kg,
                },
                {
                  label: t("landing.zfw"),
                  ist:
                    record.takeoff_weight_kg != null && record.takeoff_fuel_kg != null
                      ? record.takeoff_weight_kg - record.takeoff_fuel_kg
                      : null,
                  soll: record.planned_zfw_kg,
                },
              ]}
            />
          </div>
          <LoadsheetScore record={record} />
        </section>
      )}
    </div>
  );
}

// ---- Loadsheet-Bewertung (v0.3.0) ----------------------------------------
//
// Numerischer Score 0-100 basierend auf Abweichungen Plan vs. IST. Nicht
// blockierend — nur Information für den Piloten ("nächstes Mal weniger
// Reserve-Sprit"). Score wird auch im PIREP-Custom-Field gepostet.
//
// Algorithmus:
// - Start bei 100
// - Pro Wert (Block-Fuel, ZFW, TOW, LDW): Δ > 5 % → -5 Punkte
// - Pro Wert: Δ > 10 % → -15 Punkte (additiv: also bei 12% sind's -20)
// - Niemals < 0
// - Wenn keine Plan-Werte vorhanden, kein Score (komplette Sektion blendet aus)

interface LoadsheetScoreInput {
  block_fuel_kg: number | null;
  takeoff_weight_kg: number | null;
  landing_weight_kg: number | null;
  takeoff_fuel_kg: number | null;
  planned_block_fuel_kg: number | null;
  planned_tow_kg: number | null;
  planned_ldw_kg: number | null;
  planned_zfw_kg: number | null;
}

function LoadsheetScore({ record }: { record: LoadsheetScoreInput }) {
  const { t } = useTranslation();

  // Berechne Δ% für jeden vergleichbaren Wert.
  const items = [
    {
      label: "Block-Fuel",
      ist: record.block_fuel_kg,
      soll: record.planned_block_fuel_kg,
    },
    {
      label: "TOW",
      ist: record.takeoff_weight_kg,
      soll: record.planned_tow_kg,
    },
    {
      label: "LDW",
      ist: record.landing_weight_kg,
      soll: record.planned_ldw_kg,
    },
    {
      label: "ZFW",
      ist:
        record.takeoff_weight_kg != null && record.takeoff_fuel_kg != null
          ? record.takeoff_weight_kg - record.takeoff_fuel_kg
          : null,
      soll: record.planned_zfw_kg,
    },
  ];

  // Nur Items mit beidem vergleichbar.
  const comparable = items.filter(
    (i) => i.ist != null && i.soll != null && i.soll > 0,
  );

  if (comparable.length === 0) return null; // Kein Plan → keine Bewertung

  // Score berechnen + Penalty-Liste sammeln für Anzeige.
  let score = 100;
  const breakdown: Array<{ label: string; pct: number; penalty: number }> = [];
  for (const item of comparable) {
    const ist = item.ist!;
    const soll = item.soll!;
    const pct = Math.abs((ist - soll) / soll) * 100;
    let penalty = 0;
    if (pct > 10) penalty += 15;
    else if (pct > 5) penalty += 5;
    score -= penalty;
    breakdown.push({ label: item.label, pct, penalty });
  }
  score = Math.max(0, score);

  // v0.11.0-dev: Score-Farbe als hex statt CSS-Klasse — wird sowohl im
  // Donut-Ring (SVG-stroke) als auch im Center-Label gebraucht.
  const scoreColor =
    score >= 90 ? "#22c55e" : score >= 70 ? "#eab308" : "#ef4444";
  const ringBg = "rgba(255,255,255,0.08)";

  // Donut-Ring-Geometrie. Radius 36 in einem 80×80-Viewport (= 8 PX margin)
  // mit 8 PX stroke-width. Circumference = 2π·r.
  const RING_R = 36;
  const RING_CIRC = 2 * Math.PI * RING_R;
  const ringFilled = (score / 100) * RING_CIRC;

  return (
    <div
      className="loadsheet-score loadsheet-score--hero"
      style={{
        marginTop: "1rem",
        // Gradient-Background statt Flat-Color, plus subtler farbiger Glow
        // im Score-Band — gibt der Hero-Card mehr „Premium"-Anmutung ohne
        // aus dem Dark-Theme zu fallen.
        background: `linear-gradient(135deg, ${scoreColor}12, ${scoreColor}04 60%, transparent), var(--surface-2)`,
        border: `1px solid ${scoreColor}3a`,
        borderLeft: `4px solid ${scoreColor}`,
        borderRadius: 12,
        padding: "16px 18px",
        display: "grid",
        gridTemplateColumns: "auto 1fr",
        gap: 18,
        alignItems: "center",
        boxShadow: `0 0 24px ${scoreColor}14, inset 0 1px 0 rgba(255,255,255,0.04)`,
      }}
    >
      {/* SVG-Donut mit Score in der Mitte. Mount-Animation via
          stroke-dashoffset: Start vom leeren Ring, animiert in 0.7 s
          auf den finalen Score-Wert — sieht modern aus, kostet nichts. */}
      <div
        style={{
          position: "relative",
          width: 92,
          height: 92,
          flexShrink: 0,
          filter: `drop-shadow(0 0 8px ${scoreColor}40)`,
        }}
      >
        <svg
          width={92}
          height={92}
          viewBox="0 0 92 92"
          style={{ transform: "rotate(-90deg)" }}
        >
          <defs>
            <linearGradient
              id={`donut-grad-${score}`}
              x1="0%"
              y1="0%"
              x2="100%"
              y2="100%"
            >
              <stop offset="0%" stopColor={scoreColor} stopOpacity={1} />
              <stop offset="100%" stopColor={scoreColor} stopOpacity={0.7} />
            </linearGradient>
          </defs>
          <circle
            cx={46}
            cy={46}
            r={RING_R}
            fill="none"
            stroke={ringBg}
            strokeWidth={8}
          />
          <circle
            cx={46}
            cy={46}
            r={RING_R}
            fill="none"
            stroke={`url(#donut-grad-${score})`}
            strokeWidth={8}
            strokeLinecap="round"
            strokeDasharray={RING_CIRC}
            strokeDashoffset={RING_CIRC - ringFilled}
            style={{
              transition:
                "stroke-dashoffset 0.7s cubic-bezier(0.22, 1, 0.36, 1)",
            }}
          />
        </svg>
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            color: scoreColor,
            fontWeight: 700,
            lineHeight: 1,
          }}
        >
          <span
            style={{
              fontSize: "1.65rem",
              fontVariantNumeric: "tabular-nums",
              letterSpacing: "-0.02em",
            }}
          >
            {score}
          </span>
          <span
            style={{
              fontSize: "0.62rem",
              fontWeight: 500,
              opacity: 0.7,
              marginTop: 3,
              letterSpacing: "0.04em",
            }}
          >
            / 100
          </span>
        </div>
      </div>

      {/* Title + Breakdown-Pills */}
      <div style={{ display: "flex", flexDirection: "column", gap: 10, minWidth: 0 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: "0.74rem",
            fontWeight: 700,
            color: "var(--text-muted)",
            textTransform: "uppercase",
            letterSpacing: "0.08em",
          }}
        >
          <span>📋</span>
          <span>{t("landing.loadsheet_score")}</span>
        </div>
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 6,
          }}
        >
          {breakdown.map((b) => {
            const pillColor =
              b.pct < 5 ? "#22c55e" : b.pct < 10 ? "#eab308" : "#ef4444";
            const pillIcon = b.pct < 5 ? "✓" : b.pct < 10 ? "⚠" : "✕";
            const pctText =
              b.pct >= 0.05 ? `${b.pct.toFixed(1)}%` : "0%";
            return (
              <span
                key={b.label}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 7,
                  padding: "4px 11px",
                  borderRadius: 999,
                  background: `${pillColor}1c`,
                  border: `1px solid ${pillColor}50`,
                  fontSize: "0.78rem",
                  fontVariantNumeric: "tabular-nums",
                  color: "var(--text)",
                  boxShadow: `0 0 0 1px ${pillColor}10`,
                }}
              >
                <span style={{ color: pillColor, fontWeight: 700, fontSize: "0.72rem" }}>
                  {pillIcon}
                </span>
                <span style={{ fontWeight: 600 }}>{b.label}</span>
                <span style={{ opacity: 0.72, fontSize: "0.74rem" }}>
                  {pctText}
                </span>
                {b.penalty > 0 && (
                  <span style={{ color: pillColor, fontWeight: 700 }}>
                    −{b.penalty}
                  </span>
                )}
              </span>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ---- Soll/Ist-Vergleichstabelle (v0.3.0) ---------------------------------
//
// Drei Spalten: IST | SOLL | Δ. Zeilen werden nur gerendert wenn IST oder
// SOLL vorhanden ist — leere Zeilen kommen NICHT in die Tabelle. Δ wird mit
// Farbcode versehen: grün <1%, gelb 1-3%, rot >3% Abweichung. Bei Weight
// gilt "drüber Plan = warnung", bei Fuel gilt "stark unter Plan = warnung
// (zu wenig getankt)".

interface ComparisonRow {
  label: string;
  ist: number | null;
  soll: number | null;
}

function ComparisonTable({
  title,
  icon,
  rows,
}: {
  title: string;
  /** Optional Emoji/Icon das vor dem Section-Titel rendert (z.B. ⛽ ⚖) */
  icon?: string;
  rows: ComparisonRow[];
}) {
  // v0.11.0-dev Polish-Pass 4: kompletter Re-Design weg von der „Card-im-
  // Card-Klotz"-Optik hin zu einer schlanken Liste. Pilot-Feedback Pass 3:
  // „wirkt nicht modern, das ist ein großer Klotz". Ursache war: doppelte
  // Borders (parent landing-section + eigene Card), redundante SOLL-Spalte,
  // viele schwere Elemente nebeneinander.
  //
  // Neuer Look:
  // - KEINE eigene Card-Hülle mehr (transparent, fließt in die parent-
  //   landing-section ein — keine doppelten Borders)
  // - SOLL-Spalte aufgelöst → wird zur dezenten Sub-Zeile unter dem IST-
  //   Wert wenn Δ != 0 ("vs 13.884 kg"); spart eine ganze Spalte
  // - Mehr vertikales Spacing pro Zeile (Werte atmen)
  // - Δ-Pill bleibt rechts als visueller Anker, sonst alles ruhig
  // - Dünne Trenn-Linie zwischen Zeilen statt Zebra-Stripe-Background
  // v0.11.0-dev (Polish-Pass 2, Pass 3 fix): modernerer Look ohne Bruch
  // mit dem dark-Theme. Änderungen ggü. Pass 1:
  // - Section-Header bekommt optionales Icon (⛽ Treibstoff, ⚖ Gewicht)
  // - Δ-Pills sind rounded-full (rounded-999) statt rechteckig
  // - IST-Wert eine Stufe größer (1 rem statt 0,95)
  // - Hover-State auf den Zeilen (subtle Brightness-Heben)
  //
  // Pass-3-Fix: die Mini-Δ-Progress-Bar am unteren Rand der Zeile war
  // ein Pass-2-Experiment — der Pilot fand sie verwirrend („grüner Balken
  // lang heißt was?"), weil die Bar das Δ-Ausmaß codierte aber farblich
  // mit der ok/warn/alert-Pill kollidierte. Pill rechts sagt schon alles —
  // Mini-Bar wieder entfernt.
  const visible = rows.filter((r) => r.ist != null || r.soll != null);
  if (visible.length === 0) return null;
  return (
    <div
      style={{
        background: "color-mix(in srgb, var(--text) 2%, transparent)",
        border: "1px solid color-mix(in srgb, var(--text) 8%, transparent)",
        borderRadius: 12,
        padding: "14px 16px 8px 16px",
      }}
    >
      {/* Section-Header — schlank, im Card-Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: "0.72rem",
          fontWeight: 700,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: "0.1em",
          paddingBottom: 8,
          marginBottom: 2,
          borderBottom: "1px solid rgba(255,255,255,0.05)",
        }}
      >
        {icon && (
          <span style={{ fontSize: "0.85rem", opacity: 0.7 }}>{icon}</span>
        )}
        <span>{title}</span>
      </div>

      {/* Datenliste — jede Zeile mit dünner Trennlinie nach oben.
          Großzügiges padding für Atemraum. */}
      <div>
        {visible.map((r, idx) => {
          const delta =
            r.ist != null && r.soll != null ? r.ist - r.soll : null;
          const deltaPct =
            delta != null && r.soll != null && r.soll !== 0
              ? Math.abs(delta / r.soll) * 100
              : null;

          const deltaColor =
            deltaPct == null
              ? "rgba(255,255,255,0.35)"
              : deltaPct < 5
                ? "#22c55e"
                : deltaPct < 10
                  ? "#eab308"
                  : "#ef4444";

          const deltaIcon =
            delta == null
              ? ""
              : deltaPct! < 1
                ? "✓"
                : deltaPct! < 5
                  ? "≈"
                  : delta > 0
                    ? "▲"
                    : "▼";

          // SOLL als Sub-Zeile nur zeigen wenn IST ≠ SOLL (= Δ exists und
          // != 0). Bei exaktem Match (oder fehlendem SOLL) keine Sub-Zeile,
          // damit die Liste ruhig bleibt.
          const showSollSubline =
            r.soll != null && delta != null && delta !== 0;

          return (
            <div
              key={r.label}
              style={{
                display: "grid",
                gridTemplateColumns: "1fr auto auto",
                columnGap: 16,
                rowGap: 2,
                alignItems: "baseline",
                padding: "12px 4px",
                borderTop:
                  idx === 0
                    ? "none"
                    : "1px solid color-mix(in srgb, var(--text) 8%, transparent)",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {/* Label */}
              <span
                style={{
                  color: "var(--text-muted)",
                  fontSize: "0.86rem",
                  fontWeight: 500,
                  gridRow: "1 / 2",
                }}
              >
                {r.label}
              </span>

              {/* IST-Wert (primary, prominent) */}
              <span
                style={{
                  textAlign: "right",
                  fontSize: "1.02rem",
                  fontWeight: 600,
                  color: "var(--text)",
                  letterSpacing: "-0.01em",
                  gridRow: "1 / 2",
                }}
              >
                {r.ist != null ? fmtNumber(r.ist, 0, "kg") : "—"}
              </span>

              {/* Δ-Pill (oder em-dash bei fehlenden Daten) */}
              <span style={{ textAlign: "right", gridRow: "1 / 2" }}>
                {delta != null ? (
                  <span
                    style={{
                      display: "inline-flex",
                      alignItems: "center",
                      gap: 5,
                      padding: "2px 9px",
                      borderRadius: 999,
                      background: `${deltaColor}18`,
                      color: deltaColor,
                      fontSize: "0.76rem",
                      fontWeight: 600,
                    }}
                  >
                    <span style={{ fontSize: "0.7rem" }}>{deltaIcon}</span>
                    <span>
                      {delta >= 0 ? "+" : ""}
                      {delta.toFixed(0)} kg
                    </span>
                  </span>
                ) : (
                  <span style={{ opacity: 0.3, fontSize: "0.86rem" }}>—</span>
                )}
              </span>

              {/* SOLL-Sub-Zeile (nur wenn signifikant) — direkt unter dem
                  IST-Wert. v0.11.0-dev Polish-Pass 6: Kontrast deutlich
                  hoch (Pilot-Feedback „vs ist schwer zu erkennen"). „vs"
                  bleibt blass als Label, der Zahlenwert ist gut lesbar. */}
              {showSollSubline && (
                <span
                  style={{
                    gridColumn: "2 / 3",
                    textAlign: "right",
                    fontSize: "0.78rem",
                    gridRow: "2 / 3",
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  <span style={{ color: "var(--text-muted)", marginRight: 4 }}>
                    Plan
                  </span>
                  <span style={{ color: "var(--text)", fontWeight: 500 }}>
                    {fmtNumber(r.soll!, 0, "kg")}
                  </span>
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---- Stats summary across all landings ----------------------------------

function HistoryStats({ records }: { records: LandingRecord[] }) {
  const { t } = useTranslation();
  const stats = useMemo(() => {
    if (records.length === 0) return null;
    const total = records.length;
    const avgScore =
      records.reduce((s, r) => s + r.score_numeric, 0) / total;
    const bestRate = records.reduce(
      (best, r) =>
        Math.abs(r.landing_rate_fpm) < Math.abs(best.landing_rate_fpm) ? r : best,
      records[0],
    );
    const aGrades = records.filter(
      (r) => r.grade_letter === "A+" || r.grade_letter === "A",
    ).length;
    const totalBounces = records.reduce((s, r) => s + r.bounce_count, 0);
    return { total, avgScore, bestRate, aGrades, totalBounces };
  }, [records]);

  if (!stats) return null;

  return (
    <div className="landing-stats">
      <div className="landing-stat">
        <div className="landing-stat__label">{t("landing.total")}</div>
        <div className="landing-stat__value">{stats.total}</div>
      </div>
      <div className="landing-stat">
        <div className="landing-stat__label">{t("landing.avg_score")}</div>
        <div className="landing-stat__value">{stats.avgScore.toFixed(1)}</div>
      </div>
      <div className="landing-stat">
        <div className="landing-stat__label">{t("landing.a_grades")}</div>
        <div className="landing-stat__value">{stats.aGrades}</div>
      </div>
      <div className="landing-stat">
        <div className="landing-stat__label">{t("landing.best_rate")}</div>
        <div className="landing-stat__value">
          {stats.bestRate.landing_rate_fpm.toFixed(0)} fpm
        </div>
      </div>
      <div className="landing-stat">
        <div className="landing-stat__label">{t("landing.bounces")}</div>
        <div className="landing-stat__value">{stats.totalBounces}</div>
      </div>
    </div>
  );
}

// ---- Main panel ---------------------------------------------------------

export function LandingPanel() {
  const { t } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [records, setRecords] = useState<LandingRecord[]>([]);
  const [preview, setPreview] = useState<LandingRecord | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  async function refresh() {
    setLoading(true);
    try {
      const [list, current] = await Promise.all([
        invoke<LandingRecord[]>("landing_list"),
        invoke<LandingRecord | null>("landing_get_current"),
      ]);
      setRecords(list);
      setPreview(current ?? null);
    } catch (e) {
      console.warn("landing_list failed", e);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
    // Refresh the preview every 5 s while we're on this tab so the
    // pilot sees their landing scores updating live during rollout.
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, []);

  async function handleDelete(id: string) {
    if (
      !(await confirm({
        message: t("landing.confirm_delete"),
        destructive: true,
      }))
    )
      return;
    try {
      await invoke("landing_delete", { pirepId: id });
      setSelectedId(null);
      await refresh();
    } catch (e) {
      console.warn("landing_delete failed", e);
    }
  }

  // Detail view
  if (selectedId) {
    const rec = records.find((r) => r.pirep_id === selectedId);
    if (rec) {
      return (
        <section className="phase landing-panel">
          {confirmDialog}
          <LandingDetail
            record={rec}
            allRecords={records}
            onBack={() => setSelectedId(null)}
            onDelete={() => handleDelete(rec.pirep_id)}
            isPreview={false}
          />
        </section>
      );
    }
  }

  // Preview-only state (active flight has touched down but record not yet filed)
  return (
    <section className="phase landing-panel">
      {confirmDialog}
      {preview && (
        <div className="landing-preview-card">
          <h3>{t("landing.live_preview")}</h3>
          <LandingDetail
            record={preview}
            allRecords={records}
            onBack={() => setPreview(null)}
            isPreview={true}
          />
        </div>
      )}

      <h2 className="landing-history-title">{t("landing.history")}</h2>
      <HistoryStats records={records} />
      <TrendSparkline records={records} />

      {loading && records.length === 0 && (
        <p className="landing-empty">{t("landing.loading")}</p>
      )}
      {!loading && records.length === 0 && !preview && (
        <p className="landing-empty">{t("landing.no_landings")}</p>
      )}

      {records.length > 0 && (
        <table className="landing-table">
          <thead>
            <tr>
              <th>{t("landing.col_grade")}</th>
              <th>{t("landing.col_when")}</th>
              <th>{t("landing.col_callsign")}</th>
              <th>{t("landing.col_route")}</th>
              <th>{t("landing.col_aircraft")}</th>
              <th>{t("landing.col_rate")}</th>
              <th>{t("landing.col_score")}</th>
            </tr>
          </thead>
          <tbody>
            {records.map((r) => (
              <tr
                key={r.pirep_id}
                className="landing-row"
                onClick={() => setSelectedId(r.pirep_id)}
                tabIndex={0}
              >
                <td>
                  <span
                    className="landing-grade-pill"
                    style={{ background: gradeColor(r.grade_letter) }}
                  >
                    {r.grade_letter}
                  </span>
                </td>
                <td>{fmtDateTime(r.touchdown_at)}</td>
                <td>
                  {r.airline_icao}
                  {r.flight_number}
                </td>
                <td>
                  {r.dpt_airport} → {r.arr_airport}
                </td>
                <td>
                  {r.aircraft_icao || r.aircraft_registration || r.aircraft_title || "—"}
                </td>
                <td>{r.landing_rate_fpm.toFixed(0)} fpm</td>
                <td>{r.score_numeric}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
