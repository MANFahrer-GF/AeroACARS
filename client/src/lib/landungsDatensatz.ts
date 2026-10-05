// Der Datensatz einer Landung, wie der Client ihn aufzeichnet
// (storage::LandingRecord in Rust).
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN): Die
// gespiegelten Abschnitte der Landungsanzeige lesen auf BEIDEN Seiten dieses
// Objekt. Die Webapp baut es aus ihrem Touchdown (reines Umbenennen der
// Felder, keine Rechnung) — so zeigen Client und Webapp dieselben Werte aus
// demselben Code (QS 05.10.2026).

import type { SpritAuswertung } from "./sprit";
import type { AnflugGleitpfad, AnflugRuhe } from "../components/AnflugForensikInfo";
import type { GatePunkt } from "./stableGate";

/** Kategorie der Gesamtnote (Rust `aggregate_score_label`). */
export type LandingCategory = "smooth" | "acceptable" | "firm" | "hard" | "severe";

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

  /// `null`, wenn die Landung nicht bewertet werden konnte (siehe
  /// `landung_nicht_bewertbar`). Alte Datensätze haben immer eine Zahl.
  score_numeric: number | null;
  /// `null` bei nicht bewertbarer Landung.
  score_label: string | null;
  /// `null` bei nicht bewertbarer Landung.
  grade_letter: string | null;
  /// Lernpaket AP2: Grund, warum die Gesamtnote gedeckelt ist
  /// ("harte_landung" ab 1,75 g, "ueberlast" ab 2,6 g, "anflug_partial",
  /// "anflug_unstable", "anflug_nicht_gemessen"; ab Score-Version 18
  /// "anflug_partial_gesamt" ≤ 80 und "anflug_unstable_gesamt" ≤ 45). Fehlt bei alten
  /// Datensaetzen und ungedeckelten Landungen.
  score_deckel?: string | null;

  /// `null`, wenn der Aufsetzmoment nicht gemessen wurde.
  landing_rate_fpm: number | null;
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
  // ── v1.7.0 Bahndisziplin ──────────────────────────────────────────
  // Optional, weil Fluege von vor v1.7.0 sie nicht haben. Die Anzeige
  // zeigt das ehrlich an, statt eine leere Querachse zu malen.
  /**
   * Wo die Bahn verlassen wurde — die Stelle, an der die Spur die Bahnkante
   * überschreitet und nicht zurückkommt. Das ist „Bahn geräumt".
   */
  /**
   * Um wie viele Meter die Längsmasse der Rollspur gegen die
   * Landeschwelle verschoben sind.
   *
   * ⚠ NICHT die versetzte Schwelle — das ist eine andere Zahl. Ob die
   * beiden Nullpunkte auseinanderfallen, entscheidet die Datenquelle;
   * nur der Client weiss es. Siehe
   * `BahnFelder::spur_nullpunkt_versatz_m`.
   */
  spur_nullpunkt_versatz_m?: number | null;
  clearance_point_m?: number | null;
  /**
   * Wo die **Bewertung** endet: der Beginn des Ausschwenkens zur Ausfahrt.
   *
   * Nicht dasselbe wie `clearance_point_m` und deshalb ein eigenes Feld.
   * Ein Flugzeug zieht Hunderte Meter vor der Kante nach aussen; dieser
   * Teil gehört zum Abrollen und darf nicht als seitlicher Versatz
   * gewertet werden. Gezeichnet wird die Spur dort aber weiter
   * durchgezogen — sie ist gemessen, sie ist auf der Bahn, und eine
   * gestrichelte Linie mitten auf der Bahn wäre nicht zu erklären.
   */
  scoring_cutoff_m?: number | null;
  /// Bis wohin der Höchstwert mitgewachsen ist — das Messfenster
  /// schliesst unter 60 kt, `scoring_cutoff_m` erst beim Kurswechsel.
  mess_ende_laengs_m?: number | null;
  lateral_skip_reason?: string | null;
  clearance_speed_kt?: number | null;
  clearance_side?: "left" | "right" | null;
  track_width_m?: number | null;
  track_width_source?: "type_table" | "aircraft_file" | null;
  /** Spannweite in Metern — für den Grössenvergleich unter der Grafik. */
  wingspan_m?: number | null;
  /** Bahnbreite in Metern — Grundlage der Queransicht. */
  runway_width_m?: number | null;
  /**
   * Rollwege, die die Bahn treffen (OpenStreetMap-Bodenkarte).
   *
   * Machen die Bewertung nachvollziehbar: Man sieht, welche Ausfahrt vor der
   * genutzten lag und wie weit davor. Optional — ohne sie zeigt die
   * Queransicht einfach keine Stummel.
   */
  runway_exits?: Array<{ name: string; laengs_m: number; seite: "left" | "right" }> | null;
  min_edge_clearance_m?: number | null;
  max_lateral_offset_m?: number | null;
  lateral_samples?: Array<{ laengs_m: number; quer_m: number }> | null;
  /** Frühere Durchgänge auf derselben Bahn (durchgestartet) — nur Anzeige. */
  vorherige_durchgaenge?: Array<{
    lateral_samples: Array<{ laengs_m: number; quer_m: number }>;
  }> | null;
  surface_paved?: boolean | null;
  overrun_m?: number | null;

  planned_block_fuel_kg: number | null;
  planned_burn_kg: number | null;
  planned_tow_kg: number | null;
  planned_ldw_kg: number | null;
  planned_zfw_kg: number | null;
  actual_trip_burn_kg: number | null;
  fuel_efficiency_kg_diff: number | null;
  fuel_efficiency_pct: number | null;
  /** v1.7.35: Sprit-Auswertung ohne Note — vom Rust-Client gerechnet, hier nur gerendert. */
  sprit?: SpritAuswertung | null;
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
  /** v1.6.3: `hoehenkurve` oder `simvar_fallback` — welche Messquelle die
   *  bewertete Sinkrate geliefert hat. NICHT `landing_source` verwenden:
   *  das steht auf `vs_at_edge_50hz` und benennt nur die Buffer-Stelle,
   *  nicht das Verfahren. */
  vs_at_edge_quelle?: string | null;
  /** v1.6.3: Instrument-Wert am Aufsetzpunkt. Nur fuer die Trend-Diagnose
   *  der Forensik — angezeigt wird der bewertete Wert. */
  vs_simvar_edge_fpm?: number | null;
  /** v1.6.9 — Bestandteile der gemessenen Sinkrate.
   *  Es gilt `vs_at_edge_fpm = vs_eigensinken_fpm + vs_gelaende_fpm`.
   *  Der Geländewert ist der BEITRAG zur Zahl: negativ, wenn der Boden
   *  entgegensteigt und die Landung härter aussehen lässt, als sie war. */
  vs_gelaende_fpm?: number | null;
  vs_eigensinken_fpm?: number | null;
  /** v1.6.9 — Aufsetzgeschwindigkeit laut Simulator (nur MSFS). */
  vs_sim_referenz_fpm?: number | null;
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
  /** 05.10.2026: Abfangen über die Höhe — Messwerte der Teilnote
   *  `abfangen` (landing-scoring/src/abfangen.rs). Fehlt bei älteren Flügen. */
  abfangen?: Abfangen | null;
  forensic_sample_count?: number | null;

  // v0.8.3 (#8): Forensische Bounce-Counts — surface fuer den Pilot,
  // damit „kleine" Hopser (bei Altdatensaetzen 5-14 ft, damals score-frei;
  // seit Lernpaket AP3 29.09.2026 zaehlt ab 5 ft beides gleich) trotzdem
  // sichtbar werden statt im UI als „0 Bounces" verloren zu gehen.
  // Quelle: touchdown_v2::compute_landing_rate Forensik-Pipeline.
  /// Hoechster gemessener AGL-Wert in Post-TD-Hopsern, ft.
  /// >= 5 ft = sichtbar und seit Lernpaket AP3 (29.09.2026) auch gewertet;
  /// Altdatensaetze: gewertet erst ab 15 ft.
  bounce_max_agl_ft?: number | null;
  /// Anzahl Hopser >= 5 ft. Subset: forensic_bounce_count >= scored.
  /// Wenn > 0 aber bounce_count = 0 → rein score-freie Hopser.
  forensic_bounce_count?: number | null;
  /// Anzahl gewerteter Hopser (seit Lernpaket AP3 >= 5 ft, Altdaten >= 15 ft;
  /// = was im Score bestraft wird,
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
  /// Gesetzt, wenn die Aufzeichnung im Aufsetzfenster nicht ausreichte.
  ///
  /// Dann gibt es keine Sinkrate und keine Note — die Landung wurde erkannt,
  /// aber nicht gemessen (Untersuchung 12.09.2026, CFG 2090: 0,92 s ohne
  /// Messung genau im Aufsetzmoment, daraus 97 Punkte und Note A+).
  landung_nicht_bewertbar?: { groesste_luecke_ms: number; proben: number } | null;
  /// Das Aufsetzfenster reichte nicht, die Sinkrate kam aber vom Simulator
  /// selbst: Note ja, G-Werte und Hopser nein.
  fenster_unzureichend?: boolean;
  /// Wie zuverlässig der 50-Hz-Sampler lief. Erklärt eine dünne Aufzeichnung.
  sampler_diagnose?: {
    laeufe?: number;
    proben?: number;
    takt_mittel_ms?: number;
    takt_max_ms?: number;
    proben_je_sekunde?: number;
  } | null;
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

  /** Lernpaket AP4 (29.09.2026): geometrische Gleitpfad-Abweichung mit
   *  Quelle — reine Forensik, keine Note. Fehlt bei älteren Landungen. */
  anflug_gleitpfad?: AnflugGleitpfad | null;
  /** Lernpaket AP5: Anflugruhe je Tor, nur Hinweis. */
  anflug_ruhe?: AnflugRuhe | null;

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
  /** v1.8.1: naeherungsweise Raederhoehe ueber der Schwelle (ft). */
  tch_rad_ft?: number | null;
  /** v1.8.1: FAA-Hoehengruppe des Musters (1–4). */
  tch_hoehengruppe?: number | null;
  /** F6 Pilot in Pre-Threshold-Paint gelandet (= illegal DDS-Touchdown). */
  pre_displaced_threshold?: boolean | null;
  // Score-Version 19 (QS 05.10.2026): bisher nur im Touchdown-Payload der
  // Webapp, jetzt auch im Datensatz des Clients (storage::LandingRecord).
  /** Geschwindigkeit über Grund beim Aufsetzen, kt. */
  landing_groundspeed_kt?: number | null;
  /** Rechtweisender Kurs (landing_heading_deg ist missweisend). */
  landing_heading_true_deg?: number | null;
  go_around_count?: number | null;
  landing_wing_strike_severity_pct?: number | null;
  landing_touchdown_zone?: number | null;
  landing_float_distance_m?: number | null;
  landing_vref_deviation_kt?: number | null;
  landing_vref_source?: string | null;
  landing_yaw_rate_deg_per_sec?: number | null;
  landing_brake_energy_proxy?: number | null;
  /** METAR am Zielflughafen (roh). */
  arr_metar?: string | null;
  client_version?: string | null;
  /** Aufsetzpunkt in Grad (zur Nachprüfung der Bahnwerte). */
  landing_lat?: number | null;
  landing_lon?: number | null;
  /** Anflug: Bahnwechsel unter 1500 ft (ATC), stabil bei 200 ft (DA),
   *  Stall-Warnungen — Messwerte zur Einordnung, keine Note. */
  approach_runway_changed_late?: boolean | null;
  approach_stable_at_da?: boolean | null;
  approach_stall_warning_count?: number | null;
}

/// v0.7.1: Stability-Gate-Window-Metadaten (Spec §5.4).
export interface GateWindow {
  start_at_ms: number;
  end_at_ms: number;
  start_height_ft: number;
  end_height_ft: number;
  sample_count: number;
}

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
  /** Score-Version 19: Prüfliste des Stable Gate (nur `stability`). */
  gate?: GatePunkt[] | null;
}

/** Abfangen (Flare) ab dem letzten 50-ft-Durchgang — gleiche Felder wie
 *  `landing_scoring::abfangen::Abfangen`. */
export interface Abfangen {
  dauer_ab_50ft_s?: number | null;
  vs_50ft_fpm?: number | null;
  beginn_hoehe_ft?: number | null;
  vs_aufsetzen_fpm?: number | null;
  reduktion_fpm?: number | null;
  schweben_s?: number | null;
  schweben_m?: number | null;
  max_vs_fpm?: number | null;
  grund_ohne_werte?: string | null;
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
  /** Geschwindigkeit über Grund in kt — Grundlage des Soll-Bandes
   *  (`lib/anflugSollband.ts`). Fehlt bei Aufzeichnungen vor v1.7.35. */
  gs_kt?: number | null;
}
