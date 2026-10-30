/// Connection-State-Konstanten für ActiveFlight.connection_state.
const CONN_STATE_LIVE: u8 = 0;
const CONN_STATE_FAILING: u8 = 1;
/// v0.7.17 (B-007): phpVMS hat 401/403 geliefert — Pilot-Account ist
/// gesperrt, inaktiv, oder der API-Key wurde server-side revoked. Der
/// Worker stoppt nach diesem State (kein endloses Retry-Spam mehr),
/// die UI zeigt einen klaren Hinweis. Pilot muss VA-Admin kontaktieren
/// und sich danach neu einloggen.
const CONN_STATE_BLOCKED: u8 = 2;

/// On-disk representation of an active flight, used for resume after a client
/// crash. Not the same as `ActiveFlight` because we only persist serializable,
/// non-Mutex fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedFlight {
    pirep_id: String,
    bid_id: i64,
    /// v0.7.7: phpVMS-Flight-Identifier (siehe `ActiveFlight.flight_id`).
    /// `#[serde(default)]` damit pre-v0.7.7-Resume-Files weiter
    /// deserialisierbar bleiben — Default ist `String::new()`.
    #[serde(default)]
    flight_id: String,
    /// v1.7.32: geplante Streckenlaenge (NM) — Grundlage der
    /// Mehrweg-Gutschrift auf der OFP-Achse. Ohne Persistenz haette ein
    /// Neustart mitten im Flug die Gutschrift verloren und dem Piloten
    /// die strengeren Baender ohne Ausgleich gelassen (externe QS, Codex
    /// 16.09.2026). `serde(default)` → None bei aelteren Resume-Dateien.
    #[serde(default)]
    plan_strecke_nm: Option<f64>,
    /// v0.7.8 v1.5: Bid.flight.callsign Persistenz fuer Resume.
    /// serde(default) → None bei pre-v0.7.8-Resume-Files.
    #[serde(default)]
    bid_callsign: Option<String>,
    /// v0.7.8 v1.5: Profile.callsign Persistenz fuer Resume.
    /// serde(default) → None bei pre-v0.7.8-Resume-Files.
    #[serde(default)]
    pilot_callsign: Option<String>,
    /// v3 (Etappe 1a): Stand des Laufzeit-Pakets, mit dem dieser Flug lief
    /// (P-3.30). Der Lader liest das Feld beim Start roh aus der Datei
    /// (`laufzeit_paket::starten`). `serde(default)` → None bei 2.x-Dateien.
    #[serde(default)]
    runtime_bundle_id: Option<runtime_bundle::RuntimeBundleId>,
    started_at: DateTime<Utc>,
    /// Wann die Datei zuletzt geschrieben wurde — der einzige belastbare
    /// Hinweis darauf, ob der Flug noch lebt. `started_at` sagt darüber
    /// nichts: Ein Langstreckenflug ist nach 14 Stunden immer noch aktiv,
    /// ein abgestürzter Client nach 20 Minuten schon nicht mehr.
    ///
    /// `#[serde(default)]` → Dateien aus älteren Fassungen kennen das Feld
    /// nicht; dann bleibt `None`, und die Bewertung fällt auf `started_at`
    /// zurück (das alte Verhalten, nur ohne Löschen).
    #[serde(default)]
    zuletzt_geschrieben: Option<DateTime<Utc>>,
    #[serde(default)]
    airline_icao: String,
    /// v0.4.0: phpVMS-Airline-Logo-URL (für Discord-Webhook-Embeds).
    /// `#[serde(default)]` damit Snapshots aus älteren Builds beim
    /// Resume nicht crashen — fehlendes Feld wird zu `None`.
    #[serde(default)]
    airline_logo_url: Option<String>,
    #[serde(default)]
    planned_registration: String,
    flight_number: String,
    dpt_airport: String,
    arr_airport: String,
    fares: Vec<(i64, i32)>,
    /// Snapshot of running flight statistics (distance, fuel, phase
    /// FSM state, captured timestamps). Written by the position
    /// streamer every `STATS_PERSIST_EVERY_TICKS` posts; read by
    /// `try_resume_flight` so a Tauri restart mid-flight doesn't lose
    /// fuel-burn or distance numbers — that was the root cause of the
    /// "0 distance / 0 fuel" PIREPs we saw before Phase H.4.
    #[serde(default)]
    stats: PersistedFlightStats,
    /// Codex-Folgefund (adversarial, 05.09.2026, fuenfte/siebte Runde):
    /// server-verifizierte Piloten-ID (`Profile.pilot_id`, als String) des
    /// Accounts, der DIESEN Flug gestartet/uebernommen hat — aus
    /// `AppState::active_flight_owner_identity` bei jedem Speichern
    /// uebernommen, nicht aus `current_client(&state)` zum Speicherzeitpunkt
    /// (der koennte, waehrend derselbe Flug noch laeuft, bereits einem
    /// anderen, spaeter eingeloggten Piloten gehoeren — `phpvms_logout`
    /// leert `state.client`, aber NICHT `state.active_flight`).
    /// `#[serde(default)]` = `None` fuer Alt-Dateien (vor diesem Feld) — der
    /// Resume-Pfad behandelt das wie einen Fremd-Eigentuemer: serverseitig
    /// ueber die IN_PROGRESS-Liste des aktuellen Accounts verifizieren, statt
    /// den PIREP direkt per ID abzufragen (siehe `try_resume_flight`).
    #[serde(default)]
    owner_identity: Option<String>,
}

/// Persistable subset of `FlightStats`. The activity-log edge-detector
/// state (`last_logged_*`) is intentionally NOT persisted — after a
/// resume we restart the diff detection cleanly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedFlightStats {
    #[serde(default)]
    distance_nm: f64,
    #[serde(default)]
    position_count: u32,
    #[serde(default)]
    phase: FlightPhase,
    #[serde(default)]
    block_off_at: Option<DateTime<Utc>>,
    #[serde(default)]
    takeoff_at: Option<DateTime<Utc>>,
    #[serde(default)]
    landing_at: Option<DateTime<Utc>>,
    #[serde(default)]
    block_on_at: Option<DateTime<Utc>>,
    #[serde(default)]
    takeoff_weight_kg: Option<f64>,
    #[serde(default)]
    takeoff_fuel_kg: Option<f32>,
    /// v0.5.16: pitch / bank at takeoff (rotation moment).
    #[serde(default)]
    takeoff_pitch_deg: Option<f32>,
    #[serde(default)]
    takeoff_bank_deg: Option<f32>,
    #[serde(default)]
    landing_rate_fpm: Option<f32>,
    #[serde(default)]
    landing_g_force: Option<f32>,
    #[serde(default)]
    landing_pitch_deg: Option<f32>,
    /// v0.5.16: bank angle at touchdown for wing-strike detection.
    #[serde(default)]
    landing_bank_deg: Option<f32>,
    #[serde(default)]
    landing_speed_kt: Option<f32>,
    /// v0.5.17: groundspeed at touchdown.
    #[serde(default)]
    landing_groundspeed_kt: Option<f32>,
    /// Gemessene Eigengeschwindigkeit beim Aufsetzen (v1.6.3-QS).
    #[serde(default)]
    landing_true_airspeed_kt: Option<f32>,
    #[serde(default)]
    landing_weight_kg: Option<f64>,
    #[serde(default)]
    landing_heading_deg: Option<f32>,
    #[serde(default)]
    landing_fuel_kg: Option<f32>,
    /// v1.7.36: Stammt `landing_fuel_kg` aus dem Aufsetz-Puffer? Dann darf
    /// ein spaeterer Stempel ohne Puffer ihn nicht durch den Schnappschuss
    /// ersetzen. Persistiert, damit ein Neustart zwischen den beiden
    /// Stempeln die Herkunft nicht vergisst.
    #[serde(default)]
    landing_fuel_puffer: bool,
    #[serde(default)]
    block_fuel_kg: Option<f32>,
    #[serde(default)]
    last_fuel_kg: Option<f32>,
    /// v0.16.10: FOB-Baseline für den Fuel-Used-Fallback (PMDG liefert
    /// FUEL TOTAL QUANTITY USED nicht — Live-Befund GLO4587). Persistiert
    /// damit Resume die Baseline behält; `#[serde(default)]` hält alte
    /// Resume-Files ladbar (None → Re-Latch beim ersten Tick).
    #[serde(default)]
    initial_fob_kg: Option<f32>,
    #[serde(default)]
    last_lat: Option<f64>,
    #[serde(default)]
    last_lon: Option<f64>,
    /// Wann die Distanz-Basislinie gesetzt wurde — ohne die Zeit laesst sich
    /// nicht sagen, ob ein Segment plausibel ist. Siehe `MAX_SEGMENT_SPEED_KT`.
    #[serde(default)]
    last_pos_at: Option<DateTime<Utc>>,
    #[serde(default)]
    landing_peak_vs_fpm: Option<f32>,
    #[serde(default)]
    landing_peak_g_force: Option<f32>,
    /// v0.7.1 (Spec F4 + P2.2-D): Confidence + Source werden ATOMAR
    /// mit landing_rate_fpm via `finalize_landing_rate`-Helper gesetzt.
    /// Damit koennen sie nicht zum finalen PIREP-Wert auseinanderlaufen.
    /// "High" | "Medium" | "Low" | "VeryLow"
    #[serde(default)]
    landing_confidence: Option<String>,
    /// Reichte die Aufzeichnung im Aufsetzfenster? Persistiert aus demselben
    /// Grund wie `landing_simulator` eine Zeile tiefer: Startet der Client
    /// zwischen Aufsetzen und Einreichen neu, fiele die Sperre sonst weg —
    /// und über den in `landing_analysis` gespeicherten Edge-Wert entstünde
    /// wieder eine Note für eine Landung, die niemand gemessen hat (QS-Befund
    /// 12.09.2026 zum eigenen Umbau).
    #[serde(default)]
    landung_abdeckung_fehlt: Option<touchdown_v2::FehlendeAbdeckung>,
    /// Die Sampler-Diagnose gehört zur Begründung und überlebt deshalb
    /// denselben Neustart.
    #[serde(default)]
    sampler_diagnose: Option<serde_json::Value>,
    /// Bodenhöhe des Flugzeugs aus dem Rollen (Befund DLH 880). Überlebt
    /// den Neustart, sonst fiele ein in der Luft wiederaufgenommener Flug
    /// auf die absolute 5-ft-Grenze zurück, obwohl gemessen wurde.
    #[serde(default)]
    bodenhoehe: touchdown_v2::BodenhoehenReferenz,
    /// "vs_at_impact" | "smoothed_500ms" | "smoothed_1000ms" | "pre_flare_peak"
    #[serde(default)]
    /// Welcher Simulator die Landung geliefert hat. Persistiert, weil die
    /// G-Fairness-Normierung daran haengt: ohne das Feld faellt sie nach
    /// einem Neustart zwischen Aufsetzen und Einreichen still aus, und
    /// derselbe Landestoss bekommt zwei verschiedene Noten (QS-Befund
    /// v1.6.2). Als String, weil `&'static str` nicht serialisierbar ist.
    landing_simulator: Option<String>,
    /// Der Simulator, mit dem beim Aufsetzen zugeordnet wurde.
    ///
    /// Persistiert aus demselben Grund wie `landing_simulator`: Ohne ihn
    /// steht `aufsetz_simulator` nach einem Neustart auf `None`, und das
    /// Nachholen kehrt sofort zurueck — die spaet eingetroffene Szenerie
    /// erreicht den Flug dann nie mehr. Als String, wie der Nachbar.
    #[serde(default)]
    aufsetz_simulator: Option<String>,
    /// Wie oft die Bahn schon zugeordnet wurde. Persistiert, damit der
    /// Riegel im Recorder ueber einen Neustart hinweg monoton bleibt.
    #[serde(default)]
    bahn_revision: u32,
    landing_source: Option<String>,
    /// Rechtweisender Wind beim Aufsetzen — Basis der Seitenwind-
    /// Kompensation der Ausrichtungs-Achse. Persistiert aus demselben
    /// Grund wie `landing_simulator`: sonst faellt die Kompensation nach
    /// einem Neustart still aus.
    #[serde(default)]
    landing_wind_direction_deg: Option<f32>,
    #[serde(default)]
    landing_wind_speed_kt: Option<f32>,
    #[serde(default)]
    bounce_count: u8,
    /// P1/8b: leichte Hopser (Score-Version 26), wie `bounce_count`.
    #[serde(default)]
    light_bounce_count: u8,
    #[serde(default)]
    landing_score: Option<LandingScore>,
    #[serde(default)]
    landing_score_announced: bool,
    #[serde(default)]
    dep_gate: Option<String>,
    /// v1.7.17 Runde 14 — siehe die ausfuehrliche Begruendung am Feld in
    /// `FlightStats`.
    #[serde(default)]
    dep_gate_field_post_confirmed: bool,
    /// v1.7.17 Runde 13 — siehe die ausfuehrliche Begruendung an der
    /// Schreibstelle in `PersistedFlightStats::from`.
    #[serde(default)]
    dep_gate_withdrawal_pending: bool,
    /// v1.7.17 Runde 16 — siehe die ausfuehrliche Begruendung am Feld in
    /// `FlightStats`.
    #[serde(default)]
    dep_gate_withdrawal_awaiting_confirmation: bool,
    #[serde(default)]
    arr_gate: Option<String>,
    /// v0.19.3: did this flight actually pass through the `BlocksOn` phase?
    /// Persisted, because the block-on fuel/weight diagnostic keys off it and
    /// the in-memory phase-transition list does NOT survive a resume.
    #[serde(default)]
    blocks_on_reached: bool,
    /// v0.19.3: the airport the arrival stand was captured at.
    #[serde(default)]
    arr_gate_icao: Option<String>,
    /// v1.7.16 R3: der Flughafen, auf dem der Flieger laut
    /// `settle_at_arrival_stand` TATSAECHLICH steht — persistiert aus
    /// demselben Grund wie `divert_hint`: ein Neustart mitten in BlocksOn/
    /// Arrived kann `settle_at_arrival_stand` nicht erneut aufrufen (die
    /// Phase ist schon erreicht), und ohne diesen Wert griffe
    /// `arrival_retry_ziel` nach einem Neustart auf das GEPLANTE statt das
    /// tatsaechliche Ziel zurueck (externe Gegenpruefung, Codex,
    /// adversarial, 04.09.2026, Runde 3).
    #[serde(default)]
    arr_actual_icao: Option<String>,
    /// v0.19.3: last position the aircraft was seen at (survives a resume, unlike
    /// the distance baseline `last_lat/lon`).
    #[serde(default)]
    last_known_lat: Option<f64>,
    #[serde(default)]
    last_known_lon: Option<f64>,
    #[serde(default)]
    last_known_alt_ft: Option<f64>,
    #[serde(default)]
    last_known_hdg_deg: Option<f32>,
    #[serde(default)]
    last_known_gs_kt: Option<f32>,
    #[serde(default)]
    last_known_at: Option<DateTime<Utc>>,
    /// v0.19.3 (QS round 8): the detected divert. Persisted because two gates
    /// depend on it and the fallback that mints it cannot re-run once the flight
    /// has reached `Arrived` — an app restart used to cost the pilot his divert
    /// banner and his ACARS filing.
    #[serde(default)]
    divert_hint: Option<DivertHint>,
    #[serde(default)]
    approach_runway: Option<String>,
    #[serde(default)]
    cruise_peak_msl: Option<f32>,
    #[serde(default)]
    climb_peak_msl: Option<f32>,
    #[serde(default)]
    peak_altitude_ft: Option<f32>,
    #[serde(default)]
    aircraft_banner_logged: bool,
    // ---- Tier 1/2/3 (BeatMyLanding-aligned) touchdown extras ----
    #[serde(default)]
    touchdown_sideslip_deg: Option<f32>,
    #[serde(default)]
    touchdown_profile: Vec<TouchdownProfilePoint>,
    /// v0.15.24: the in-app live-map flown track ([lon,lat]), persisted so a
    /// mid-flight app restart keeps the pre-restart trail (flight_get_track
    /// returns the full track post-restore, not just the post-restart segment).
    #[serde(default)]
    track: Vec<[f64; 2]>,
    #[serde(default)]
    runway_match: Option<runway::RunwayMatch>,
    // v0.16.24 (QS-2 FIX B): persist the runway-correlation source/geometry so
    // a DIVERT survives a suspend/resume between touchdown and PIREP filing.
    // `save_active_flight` runs right after the touchdown dump; filing is the
    // later manual `flight_end`. Without these, an app restart in between reset
    // them to None/false → the finalize gate (`runway_source ==
    // OurAirportsFallback`) and the on-demand-fetch gate
    // (`runway_fallback_at_touchdown`) were both defeated and the divert filed
    // with coarse OurAirports geometry. `#[serde(default)]` keeps OLD persisted
    // blobs (saved before this change) deserializing cleanly — missing fields →
    // None/false → identical to today's pre-change resume behaviour (never
    // worse). On-plan is unaffected: the touchdown already stamps Navigraph, and
    // even if reset, the finalize gate's "not OurAirportsFallback → provisional
    // verbatim" is the correct no-op.
    #[serde(default)]
    runway_source: Option<runway::RunwaySource>,
    #[serde(default)]
    runway_nav_cycle: Option<String>,
    #[serde(default)]
    runway_nav_geometry: Option<aeroacars_mqtt::navdata::NavRunway>,
    #[serde(default)]
    runway_ils_navigraph: Option<IlsNavigraph>,
    #[serde(default)]
    runway_correlation_icao: Option<String>,
    #[serde(default)]
    runway_fallback_at_touchdown: bool,
    #[serde(default)]
    landing_lat: Option<f64>,
    #[serde(default)]
    landing_lon: Option<f64>,
    #[serde(default)]
    landing_heading_true_deg: Option<f32>,
    /// Headwind component at touchdown (knots). Positive = wind from
    /// the front (= reduces required ground roll). Negative = tailwind.
    /// Sourced from `AIRCRAFT WIND Z` (negated, since +Z is tailwind
    /// in MSFS body-axis convention).
    #[serde(default)]
    landing_headwind_kt: Option<f32>,
    /// Crosswind component at touchdown (knots). Positive = from the
    /// right side; negative = from the left. Sourced from
    /// `AIRCRAFT WIND X`. Achtung: MSFS meldet dort NEGATIV fuer Wind von
    /// rechts — der Adapter dreht das Vorzeichen seit v1.6.3, hier steht
    /// also die Vertragslesart (positiv = von rechts), nicht die rohe.
    #[serde(default)]
    landing_crosswind_kt: Option<f32>,
    // ---- Landing Analyzer (Stage 1) ----
    #[serde(default)]
    approach_vs_stddev_fpm: Option<f32>,
    #[serde(default)]
    approach_bank_stddev_deg: Option<f32>,
    /// v0.20.1: die GEFILTERTEN Werte muessen den Neustart ueberleben.
    ///
    /// Vorher wurden nur die Legacy-Felder persistiert. Stuerzte die App nach
    /// dem Touchdown und vor dem Filing ab (der Stratos-Doppel-Submit-Fall),
    /// waren die v2-Werte nach dem Restore weg — die Kanonik fiel auf das alte
    /// Fenster zurueck und der PIREP wurde anders bewertet als der Landungs-Tab
    /// es zeigte. Derselbe Flug, zwei Noten, je nachdem ob zwischendurch ein
    /// Neustart lag. Genau die Krankheit, die dieses Release ausraeumt.
    #[serde(default)]
    approach_vs_stddev_filtered_fpm: Option<f32>,
    #[serde(default)]
    approach_bank_stddev_filtered_deg: Option<f32>,
    #[serde(default)]
    rollout_distance_m: Option<f64>,
    /// v1.7.15 (N7): Ausrollen und Spur ueberleben den Neustart. Ohne sie
    /// baute der Einreich-Nachtrag mit leerer Spur `rollout_final:
    /// false` und drehte eine schon endgueltige Zeile zurueck; und der
    /// Streamer konnte nach dem Neustart nie mehr `spur_fertig` sehen.
    #[serde(default)]
    rollout_finalized: bool,
    #[serde(default)]
    bahn_spur: Vec<(f32, f32)>,
    /// Spuren früherer Durchgänge (durchgestartet) — überlebt den Neustart
    /// wie die Spur selbst.
    #[serde(default)]
    bahn_vorige_spuren: Vec<(String, Vec<(f32, f32)>)>,
    #[serde(default)]
    abflug_icao: Option<String>,
    #[serde(default)]
    bahn_spur_laeuft: bool,
    #[serde(default)]
    bahn_spur_bis: Option<DateTime<Utc>>,
    /// v1.7.15 (N6): Eine offene Korrektur ueberlebt den Neustart.
    #[serde(default)]
    bahn_nachtrag_offen: bool,
    /// v1.7.15 (Runde 3, P1): ALLES, was aus der Spur abgeleitet ist —
    /// nicht nur die Spur. Nach einem Neustart sendete der Streamer den
    /// Nachtrag erneut (Sperre war `None`), mit persistierter Spur, aber
    /// ohne Marken: Die gingen als `null` hinaus und LOESCHTEN beim
    /// Recorder gute Werte. Der Einreich-Weg konnte es nicht heilen, weil
    /// die Aenderungserkennung das Drittel nicht sah.
    #[serde(default)]
    bahn_max_querversatz_m: Option<f64>,
    #[serde(default)]
    bahn_overrun_m: Option<f64>,
    #[serde(default)]
    bahn_proben: u32,
    #[serde(default)]
    bahn_fenster_zu: bool,
    #[serde(default)]
    bahn_fenster_zu_laengs_m: Option<f64>,
    #[serde(default)]
    bahn_raeum_laengs_m: Option<f64>,
    #[serde(default)]
    bahn_raeum_gs_kt: Option<f64>,
    #[serde(default)]
    bahn_raeum_seite: Option<String>,
    #[serde(default)]
    bahn_kante_laengs_m: Option<f64>,
    #[serde(default)]
    bahn_kante_gs_kt: Option<f64>,
    #[serde(default)]
    bahn_raeum_kurs_diff: Option<f64>,
    #[serde(default)]
    bahn_ausfahrt_kandidat: Option<AusfahrtKandidat>,
    #[serde(default)]
    rollout_finalize_reason: Option<String>,
    #[serde(default)]
    landing_touchdown_zone: Option<u8>,
    #[serde(default)]
    landing_float_distance_m: Option<f32>,
    /// Der zuletzt GESENDETE Nachtrag (Aufsetzzeit, Revision). Seeded die
    /// Sperre des Streamers nach einem Neustart — sonst sendet er blind
    /// erneut, auch wenn sich nichts geaendert hat.
    #[serde(default)]
    letzter_nachtrag: Option<(i64, u32)>,
    /// v1.7.15 (Runde 4, N14): die uebrigen EINGABEN von `bahn_herkunft`
    /// und `bahn_felder`. Ohne sie kippte nach einem Neustart
    /// `bahn_geometrie_quelle` auf „navdaten", die Korrekturbetraege und
    /// `sim_kennung` wurden als `null` geloescht. Bewusst NICHT
    /// persistiert: `szenerie_auskunft` (hunderte Rollwege) und
    /// `arr_ground_geojson` — ihre Ableitung, die Ausfahrten, wird bei
    /// fehlender Eingabe AUSGELASSEN statt genullt (siehe `BahnWire`).
    #[serde(default)]
    szenerie_uebernahme: Option<szenerie_bahn::UebernahmeBericht>,
    #[serde(default)]
    szenerie_diagnose: Option<String>,
    #[serde(default)]
    sim_kennung: Option<String>,
    #[serde(default)]
    runway_tch_actual_ft: Option<f32>,
    #[serde(default)]
    fahrwerk_spurweite_m: Option<f64>,
    #[serde(default)]
    bahn_spur_bezug_veraltet: bool,
    #[serde(default)]
    szenerie_status_fest: Option<String>,
    /// v1.7.15 (Runde 5, N27): das aufgeloeste Muster — sonst faellt
    /// `wingspan_m` nach einem Neustart auf das GEBUCHTE Muster.
    #[serde(default)]
    aufgeloestes_muster: Option<String>,
    /// Bordbuch (26.09.2026): Laufzustand, damit ein Neustart der App die
    /// schon abgehakten Punkte und laufenden Fristen behält.
    #[serde(default)]
    bordbuch: bordbuch::Zustand,
    // ---- Landing Analyzer (Stage 2): SimBrief OFP plan ----
    #[serde(default)]
    planned_block_fuel_kg: Option<f32>,
    #[serde(default)]
    planned_burn_kg: Option<f32>,
    #[serde(default)]
    planned_reserve_kg: Option<f32>,
    #[serde(default)]
    planned_zfw_kg: Option<f32>,
    #[serde(default)]
    planned_tow_kg: Option<f32>,
    #[serde(default)]
    planned_ldw_kg: Option<f32>,
    #[serde(default)]
    planned_route: Option<String>,
    #[serde(default)]
    planned_atc_callsign: Option<String>,
    /// v0.15.x: Navlog-Fixes mit-persistieren, damit ein Resume die
    /// Map-Strecke (Wegpunkte) behält statt auf die Great-Circle-Linie zu fallen.
    #[serde(default)]
    planned_waypoints: Vec<api_client::RouteFix>,
    #[serde(default)]
    planned_alternate: Option<String>,
    /// v1.7.35: Sprit-Messwerte ueberleben einen Neustart. Altbestand ohne
    /// die Felder liest `None`/0 — und die Auswertung zeigt dann nur, was
    /// sie hat (Reserve, Leiter), nie erfundene Phasen.
    #[serde(default)]
    sprit_vergleichspunkt_kg: Option<f32>,
    /// v1.7.40: Sprit je Wegpunkt — Messungen und zuletzt erreichter
    /// Routenabschnitt überleben den Neustart.
    #[serde(default)]
    sprit_wp_messungen: Vec<SpritWpMessung>,
    #[serde(default)]
    sprit_wp_segment: Option<usize>,
    #[serde(default)]
    sprit_vergleichspunkt_odo_nm: Option<f64>,
    #[serde(default)]
    sprit_plan_rest_nm: Option<f64>,
    #[serde(default)]
    sprit_plan_bis_tod_kg: Option<f32>,
    #[serde(default)]
    sprit_zeit_unter_schwelle_s: f64,
    #[serde(default)]
    sprit_schwelle_ft: Option<f64>,
    /// v1.7.36: Plan-Anteil und Plan-Reststrecke am gemessenen ORT — nicht
    /// mehr am TOD-Fix. Siehe `sprit_plan_am_ort`.
    #[serde(default)]
    sprit_plan_bis_vergleichspunkt_kg: Option<f32>,
    #[serde(default)]
    sprit_plan_rest_ab_vergleichspunkt_nm: Option<f64>,
    /// v1.7.36: die beiden Boden-Marken. Ohne Persistenz verloere ein
    /// Neustart zwischen Anlassen und Abheben den Rollsprit.
    #[serde(default)]
    engine_start_fuel_kg: Option<f32>,
    #[serde(default)]
    engine_off_fuel_kg: Option<f32>,
    #[serde(default)]
    sprit_hoechste_hoehe_ft: Option<f64>,
    /// v1.7.36: am Messpunkt bzw. beim Aufsetzen eingefroren, und der
    /// Einstieg in der Luft — siehe die gleichnamigen Felder in `FlightStats`.
    #[serde(default)]
    sprit_plan_trip_am_vergleichspunkt_kg: Option<f32>,
    #[serde(default)]
    sprit_landung_odo_nm: Option<f64>,
    #[serde(default)]
    sprit_einstieg_in_der_luft: bool,
    #[serde(default)]
    sprit_einstieg_ort: Option<(f64, f64)>,
    #[serde(default)]
    sprit_plan_bis_einstieg_kg: Option<f32>,
    #[serde(default)]
    planned_taxi_kg: Option<f32>,
    #[serde(default)]
    planned_contingency_kg: Option<f32>,
    #[serde(default)]
    planned_alternate_burn_kg: Option<f32>,
    #[serde(default)]
    planned_extra_kg: Option<f32>,
    /// Die eingefrorene Auswertung selbst. Ohne sie wuerde ein Resume nach
    /// der Landung neu rechnen — und weil `distance_nm` beim Rollen zum Gate
    /// weiterlaeuft, kaeme eine andere Anflugstrecke heraus als der Client
    /// bereits gezeigt hat.
    #[serde(default)]
    sprit_auswertung: Option<landing_scoring::sprit::SpritAuswertung>,
    /// v0.19.3: the arrival airport's reference coordinates (Navigraph → phpVMS
    /// → OurAirports; see `airport_reference_pos`). Persisted so a resumed
    /// flight doesn't lose the ability to tell where it is.
    #[serde(default)]
    planned_arr_ref_pos: Option<(f64, f64)>,
    /// Which source the above came from — persisted alongside it, so a resumed
    /// flight doesn't downgrade a Navigraph position to an OurAirports one.
    #[serde(default)]
    planned_arr_ref_source: u8,
    // v0.3.0: MAX-Werte aus dem OFP für Overweight-Detection.
    #[serde(default)]
    planned_max_zfw_kg: Option<f32>,
    #[serde(default)]
    planned_max_tow_kg: Option<f32>,
    #[serde(default)]
    planned_max_ldw_kg: Option<f32>,
    // ---- Touch-and-Go + Go-Around tracking (v0.1.26) ----
    // Persisted so a Tauri restart mid-flight (or a planned resume
    // after the pilot closed the app for lunch) doesn't wipe the
    // training-flight audit trail. `pending_acars_logs` is NOT
    // persisted on purpose — it's a transient queue between FSM
    // tick and streamer tick; if the streamer didn't drain it
    // before shutdown, those log lines are simply lost. Resume
    // semantics for the GA detector: lowest_agl is preserved so a
    // pilot who quits the app on short final and resumes can still
    // get their GA correctly classified.
    #[serde(default)]
    touchdown_events: Vec<TouchdownEvent>,
    #[serde(default)]
    touch_and_go_pending_since: Option<DateTime<Utc>>,
    #[serde(default)]
    go_around_count: u32,
    #[serde(default)]
    lowest_agl_during_approach_ft: Option<f32>,
    /// 17.10.2026: Position und Kurs am tiefsten Anflugpunkt (Durchstart
    /// nur nahe einer Bahn zählen).
    #[serde(default)]
    tiefster_anflugpunkt: Option<(f64, f64, f32)>,
    #[serde(default)]
    go_around_climb_pending_since: Option<DateTime<Utc>>,
    // v0.7.17 (B-010): Descent-Dwell-Timer persistieren damit Resume
    // mitten in einem Climb-Trim-Spike nicht den Timer verliert.
    #[serde(default)]
    descent_pending_since: Option<DateTime<Utc>>,
    // v0.5.11 holding-pattern tracking (Uhren seit 17.10.2026 ersetzt
    // durch den Kursverlauf, der nach einem Neustart neu beginnt)
    #[serde(default)]
    previous_phase_before_holding: Option<FlightPhase>,

    // v0.5.45: Sampler-State persistieren damit App-Resume nach einem
    // bereits-detektierten Touchdown nicht ein zweites Mal capturet
    // (User-Report GSG 302 X-Plane DA40 Bush-Strip 2026-05-09: 1×
    // flight_resumed nach erstem TD → Sampler hat beim echten Landing
    // nochmal gefeuert weil Guard-State nach Resume None war).
    #[serde(default)]
    sampler_touchdown_at: Option<DateTime<Utc>>,
    /// Bug B (Codex, Vereinheitlichung 09/2026): eine aktive `FalseEdge`-
    /// Ablehnung des Samplers persistieren, damit `finalize_landing_score_if_due`
    /// sie nach App-Resume weiterhin respektiert. `#[serde(default)]` →
    /// None bei pre-Feld Resume-Files (harmlos: gleiches Verhalten wie vorher).
    #[serde(default)]
    sampler_touchdown_rejected_reason: Option<touchdown_v2::FalseEdgeReason>,
    #[serde(default)]
    sampler_takeoff_at: Option<DateTime<Utc>>,
    #[serde(default)]
    touchdown_window_dumped_at: Option<DateTime<Utc>>,
    /// GAF-707-Race, strukturell geloest (Vereinheitlichung 09/2026):
    /// `touchdown_window_dumped_at` wird sehr frueh gesetzt (verhindert
    /// Doppel-Dump im 50Hz-Sampler-Loop + Resume-Sicherheit, siehe dessen
    /// Setzstelle) — aber die score-relevanten Rohdaten (verfeinerte VS-
    /// Kaskade, `bounce_count`) werden danach noch unter einem ZWEITEN,
    /// spaeteren Lock geschrieben. Ohne dieses zweite Signal konnte
    /// `finalize_landing_score_if_due` dazwischenfunken und mit der
    /// groeberen, noch unverfeinerten Kanonik scoren — und das dann NIE
    /// mehr korrigieren (`landing_score.is_some()`-Guard). `#[serde(default)]`
    /// → None bei pre-Feld Resume-Files (Finalizer faellt dann auf seinen
    /// bestehenden Timeout-Fallback zurueck, harmlos).
    #[serde(default)]
    touchdown_window_score_data_ready_at: Option<DateTime<Utc>>,
    #[serde(default)]
    landing_score_finalized: bool,
    /// v0.7.7: aktuelle SimBrief-OFP-ID — siehe FlightStats. `#[serde(default)]`
    /// damit pre-v0.7.7-Resume-Files weiter parseable bleiben.
    #[serde(default)]
    simbrief_ofp_id: Option<String>,
    /// v0.7.7: raw SimBrief `<params><time_generated>` — siehe FlightStats.
    #[serde(default)]
    simbrief_ofp_generated_at: Option<String>,
    /// „Zeiten gegen Flugplan": Planzeiten des OFP — siehe FlightStats.
    #[serde(default)]
    ofp_planzeiten: landing_scoring::flugzeiten::FlugZeiten,
    /// Kabinenansagen (Beta) — siehe FlightStats. `#[serde(default)]`, damit
    /// aeltere `active_flight.json` weiter laden (alle vier `None`).
    #[serde(default)]
    pax_count: Option<i32>,
    #[serde(default)]
    max_passengers: Option<i32>,
    #[serde(default)]
    flight_type: Option<String>,
    #[serde(default)]
    airline_name: Option<String>,
    /// Umleitung (Beta) — siehe FlightStats.
    #[serde(default)]
    umleitung_ziel: Option<String>,
    #[serde(default)]
    utc_versatz_abflug_min: Option<i32>,
    #[serde(default)]
    utc_versatz_ziel_min: Option<i32>,
    /// Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator):
    /// Summe aller Pause-Sekunden seit Flugstart. `#[serde(default)]`
    /// damit pre-Spec `active_flight.json` weiter ladbar bleibt — alte
    /// Resume-Files defaulten auf 0 (keine bekannten Pausen, ist OK).
    #[serde(default)]
    pause_total_duration_secs: i64,
    /// Spec sim-disconnect-auto-resume F2: Liste der einzelnen Pause-
    /// Bloecke (Audit-/Debug-Daten). `#[serde(default)]` damit alte
    /// Resume-Files weiter laden — defaulten auf leeren Vec.
    #[serde(default)]
    pause_segments: Vec<PauseSegment>,
    /// QS 06.10.2026: überleben einen Neustart zwischen Aufsetzen und
    /// Einreichen — sonst rechnete die Note beim Einreichen ohne Abfangen
    /// neu, und der Datensatz meldete „kein später Bahnwechsel / 0 Stall-
    /// Warnungen", obwohl der Touchdown die echten Werte trug.
    #[serde(default)]
    abfangen_gesichert: Option<landing_scoring::abfangen::Abfangen>,
    #[serde(default)]
    approach_runway_changed_late: bool,
    #[serde(default)]
    approach_stall_warning_count: u32,
    #[serde(default)]
    approach_wind: Option<landing_scoring::anflug_wind::AnflugWind>,
    /// QS 06.10.2026 (Runde 4): die übrigen nur am Aufsetzen gesetzten
    /// Anflug-/Aufsetzwerte des Datensatzes — sonst fehlten sie nach einem
    /// Neustart im Client, während die Webapp sie aus dem Touchdown zeigt.
    #[serde(default)]
    approach_stable_at_da: Option<bool>,
    /// Score-Version 23: spaetes Final / Kurvenanflug dieser Landung.
    #[serde(default)]
    approach_spaetes_final: Option<spaetes_final::SpaetesFinal>,
    #[serde(default)]
    landing_vref_deviation_kt: Option<f32>,
    #[serde(default)]
    landing_vref_source: Option<String>,
    #[serde(default)]
    landing_vref_toleranz_kt: Option<f32>,
    #[serde(default)]
    landing_vref_formel_kt: Option<f32>,
    #[serde(default)]
    landing_vref_landeklappen: Option<bool>,
    #[serde(default)]
    landing_yaw_rate_deg_per_sec: Option<f32>,
    #[serde(default)]
    landing_brake_energy_proxy: Option<f32>,
    #[serde(default)]
    landing_decel_mps2: Option<f32>,
    #[serde(default)]
    landing_wing_strike_severity_pct: Option<f32>,
    #[serde(default)]
    arr_metar_raw: Option<String>,
    /// QS 06.10.2026 (Runde 6): Version, die die Landung gemessen hat. Ein
    /// Update mit Neustart vor dem Einreichen trug sonst die neue Version in
    /// den Datensatz, während der Touchdown (Webapp) die alte nannte.
    #[serde(default)]
    client_version_aufsetzen: Option<(String, DateTime<Utc>)>,
    /// QS 06.10.2026 (Runde 5): die Eingaben des Anflug-Urteils — sonst
    /// rechnete die Note nach einem Neustart vor dem Einreichen ohne
    /// Gleitpfad, Ruck, IAS-Streuung, Sinkrate und Konfiguration (QAF434:
    /// „teilweise" am Aufsetzen, „stabil" im PIREP).
    #[serde(default)]
    anflug_gleitpfad: Option<storage::AnflugGleitpfad>,
    #[serde(default)]
    anflug_ruhe: Option<storage::AnflugRuhe>,
    #[serde(default)]
    approach_vs_jerk_fpm: Option<f32>,
    #[serde(default)]
    approach_ias_stddev_kt: Option<f32>,
    #[serde(default)]
    approach_excessive_sink: Option<bool>,
    #[serde(default)]
    approach_stable_config: Option<bool>,
    #[serde(default)]
    approach_ias_stddev_unter_500_kt: Option<f32>,
    #[serde(default)]
    approach_stable_config_500: Option<bool>,
    /// Spec v0.7.15 F5/F6: aktuelle Pause-Reason fuer App-Restart-
    /// Persistenz. `#[serde(default)]` → None bei pre-v0.7.15 Files.
    #[serde(default)]
    current_pause_reason: Option<PauseReason>,
    /// v0.20 (Process-Integrity): siehe FlightStats-Feld gleichen Namens.
    /// `#[serde(default)]` → None bei pre-v0.20 Files.
    #[serde(default)]
    disconnect_sim_liveness: Option<sim_core::process_probe::ProcessLiveness>,
    /// v0.20 (Process-Integrity): letzter periodisch persistierter Snapshot
    /// (unabhängig davon, ob je ein SimDisconnect-Pause registriert wurde) —
    /// die Vergleichs-Basis für `try_resume_flight()`s Discontinuity-Check.
    /// `#[serde(default)]` → None bei pre-v0.20 Files.
    #[serde(default)]
    last_persisted_snapshot: Option<PausedSnapshot>,
    /// v0.20 (Process-Integrity): Ergebnis des App-Neustart-Resume-Discontinuity-
    /// Checks, falls er getriggert hat. Bleibt bis zum PIREP-Filing erhalten
    /// (siehe `build_pirep_payload`). `#[serde(default)]` → None bei pre-v0.20 Files.
    #[serde(default)]
    resume_discontinuity: Option<ResumeDiscontinuity>,
    /// ALLE Spruenge dieses Fluges, in der Reihenfolge, in der sie
    /// auffielen. `resume_discontinuity` ist der juengste davon.
    ///
    /// Ein Flug kann mehrfach unterbrochen werden; vorher ueberschrieb der
    /// zweite Sprung den ersten still, und die PIREP-Notiz nannte nur den
    /// letzten (Cloud-QS 23.09.2026). `serde(default)` → leer bei Fluegen
    /// aus einer aelteren Fassung.
    #[serde(default)]
    resume_spruenge: Vec<ResumeDiscontinuity>,
    /// v0.20 (Process-Integrity): true wenn der App-Neustart, der diesen Flug
    /// wiederaufgenommen hat, unsauber war (`FlightResumed.previous_exit_clean
    /// == Some(false)`). None wenn nie ein App-Neustart passierte. Bleibt bis
    /// zum PIREP-Filing erhalten. `#[serde(default)]` → None bei pre-v0.20 Files.
    #[serde(default)]
    app_restart_was_unclean: Option<bool>,
    /// v0.20 (Process-Integrity): `age.num_minutes()` vom App-Neustart-Resume
    /// (dasselbe, was auch ins `FlightResumed`-JSONL-Event geht) — hier
    /// zusätzlich gespiegelt, damit es beim PIREP-Filing (viel später) noch
    /// verfügbar ist. `#[serde(default)]` → None bei pre-v0.20 Files.
    #[serde(default)]
    resume_gap_minutes: Option<i64>,
    /// Die LAENGSTE Neustart-Luecke dieses Fluges (Sekunden). Waechst nur.
    ///
    /// `resume_gap_minutes` beschreibt nur den LETZTEN Neustart: Nach 15 min
    /// Absturz und einem spaeteren 20-s-Update stand dort 0 — und die
    /// Abgabe-Sperre vergass die lange Unterbrechung (Cloud-QS 23.09.2026,
    /// dritte Runde).
    #[serde(default)]
    resume_luecke_max_secs: Option<i64>,
    /// Siehe FlightStats-Feld gleichen Namens.
    #[serde(default)]
    weiterflug_ab_boden_seit: Option<DateTime<Utc>>,
    /// v0.16.12 (#phase-v2): geplante Cruise-Altitude (ft) — persistiert,
    /// damit die Schatten-Engine nach einem Resume ihren `cruise_ref`
    /// behält. `#[serde(default)]` → None bei pre-v0.16.12-Files.
    #[serde(default)]
    planned_cruise_alt_ft: Option<f64>,
    /// 17.10.2026 („Stück 2"): höchste beobachtete Reiseflughöhe der
    /// v2-Engine (`obsref`). Seit v2 das Reiseband entscheidet, hängt die
    /// Level-off-Regel im Sinkflug an ihr — ohne sie fiele die Engine nach
    /// einem Neustart im Reiseflug auf den 240-s-Ersatz zurück.
    #[serde(default)]
    v2_reiseflughoehe_ft: Option<f64>,
}

impl PersistedFlightStats {
    fn snapshot_from(stats: &FlightStats) -> Self {
        Self {
            distance_nm: stats.distance_nm,
            position_count: stats.position_count,
            phase: stats.phase,
            block_off_at: stats.block_off_at,
            takeoff_at: stats.takeoff_at,
            landing_at: stats.landing_at,
            block_on_at: stats.block_on_at,
            takeoff_weight_kg: stats.takeoff_weight_kg,
            takeoff_fuel_kg: stats.takeoff_fuel_kg,
            takeoff_pitch_deg: stats.takeoff_pitch_deg,
            takeoff_bank_deg: stats.takeoff_bank_deg,
            landing_rate_fpm: stats.landing_rate_fpm,
            landing_g_force: stats.landing_g_force,
            landing_pitch_deg: stats.landing_pitch_deg,
            landing_bank_deg: stats.landing_bank_deg,
            landing_speed_kt: stats.landing_speed_kt,
            landing_groundspeed_kt: stats.landing_groundspeed_kt,
            landing_true_airspeed_kt: stats.landing_true_airspeed_kt,
            landing_weight_kg: stats.landing_weight_kg,
            landing_heading_deg: stats.landing_heading_deg,
            landing_fuel_kg: stats.landing_fuel_kg,
            landing_fuel_puffer: stats.landing_fuel_puffer,
            block_fuel_kg: stats.block_fuel_kg,
            last_fuel_kg: stats.last_fuel_kg,
            initial_fob_kg: stats.initial_fob_kg,
            last_lat: stats.last_lat,
            last_lon: stats.last_lon,
            last_pos_at: stats.last_pos_at,
            landing_peak_vs_fpm: stats.landing_peak_vs_fpm,
            landing_peak_g_force: stats.landing_peak_g_force,
            // v0.7.1 (Spec F4 + P2.2-D): Confidence + Source persistieren
            // damit ein Re-Open des PIREP nach App-Restart konsistent bleibt.
            landing_confidence: stats.landing_confidence.clone(),
            landung_abdeckung_fehlt: stats.landung_abdeckung_fehlt.clone(),
            sampler_diagnose: stats.sampler_diagnose.clone(),
            bodenhoehe: stats.bodenhoehe.clone(),
            landing_source: stats.landing_source.clone(),
            landing_wind_direction_deg: stats.landing_wind_direction_deg,
            landing_wind_speed_kt: stats.landing_wind_speed_kt,
            landing_simulator: stats.landing_simulator.map(|s| s.to_string()),
            aufsetz_simulator: stats
                .aufsetz_simulator
                .map(|s| serde_json::to_string(&s).unwrap_or_default()),
            bahn_revision: stats.bahn_revision,
            bounce_count: stats.bounce_count,
            light_bounce_count: stats.light_bounce_count,
            landing_score: stats.landing_score,
            landing_score_announced: stats.landing_score_announced,
            dep_gate: stats.dep_gate.clone(),
            // v1.7.17 Runde 14 (externe Gegenpruefung, Codex, adversarial,
            // 04.09.2026): OHNE Persistierung wuerde ein App-Neustart
            // NACH einem bestaetigten Live-Post, aber VOR dem Boarding-
            // Ende, die Bestaetigung verlieren — der Riegel saehe dann
            // faelschlich "nie gepostet" und wuerde `dep_gate_withdrawal_
            // pending` nie setzen, obwohl der Server-Wert tatsaechlich von
            // AeroACARS stammt und jetzt veraltet ist.
            dep_gate_field_post_confirmed: stats.dep_gate_field_post_confirmed,
            // v1.7.17 Runde 13 (externe Gegenpruefung, Codex, adversarial,
            // 04.09.2026): OHNE Persistierung wuerde ein App-Neustart
            // zwischen Boarding-Ende (Riegel setzt das Flag) und der
            // finalen Einreichung das Flag stillschweigend auf `false`
            // zuruecksetzen — der bekanntlich veraltete, live gepostete
            // Wert waere bei der Einreichung dann NICHT mehr geleert
            // worden (`build_pirep_fields` liesse ihn weg statt zu
            // leeren), obwohl `dep_gate` selbst schon korrekt `None` war.
            dep_gate_withdrawal_pending: stats.dep_gate_withdrawal_pending,
            // v1.7.17 Runde 16 (externe Gegenpruefung, Codex, adversarial,
            // 04.09.2026): OHNE Persistierung wuerde ein App-Neustart
            // zwischen dem Boarding-Ende-Riegel (setzt dieses Flag, wenn
            // die Bestaetigung noch aussteht) und der Bestaetigung selbst
            // die eingefrorene Entscheidung verlieren — ein spaeter doch
            // noch erfolgreicher Post bliebe dann folgenlos.
            dep_gate_withdrawal_awaiting_confirmation: stats
                .dep_gate_withdrawal_awaiting_confirmation,
            arr_gate: stats.arr_gate.clone(),
            blocks_on_reached: stats.blocks_on_reached,
            arr_gate_icao: stats.arr_gate_icao.clone(),
            arr_actual_icao: stats.arr_actual_icao.clone(),
            last_known_lat: stats.last_known_lat,
            last_known_lon: stats.last_known_lon,
            last_known_alt_ft: stats.last_known_alt_ft,
            last_known_hdg_deg: stats.last_known_hdg_deg,
            last_known_gs_kt: stats.last_known_gs_kt,
            last_known_at: stats.last_known_at,
            divert_hint: stats.divert_hint.clone(),
            approach_runway: stats.approach_runway.clone(),
            cruise_peak_msl: stats.cruise_peak_msl,
            climb_peak_msl: stats.climb_peak_msl,
            peak_altitude_ft: stats.peak_altitude_ft,
            aircraft_banner_logged: stats.aircraft_banner_logged,
            touchdown_sideslip_deg: stats.touchdown_sideslip_deg,
            touchdown_profile: stats.touchdown_profile.clone(),
            track: stats.track.clone(),
            runway_match: stats.runway_match.clone(),
            // v0.16.24 (QS-2 FIX B): persist the runway-correlation source/geometry.
            runway_source: stats.runway_source,
            runway_nav_cycle: stats.runway_nav_cycle.clone(),
            runway_nav_geometry: stats.runway_nav_geometry.clone(),
            runway_ils_navigraph: stats.runway_ils_navigraph.clone(),
            runway_correlation_icao: stats.runway_correlation_icao.clone(),
            runway_fallback_at_touchdown: stats.runway_fallback_at_touchdown,
            landing_lat: stats.landing_lat,
            landing_lon: stats.landing_lon,
            landing_heading_true_deg: stats.landing_heading_true_deg,
            landing_headwind_kt: stats.landing_headwind_kt,
            landing_crosswind_kt: stats.landing_crosswind_kt,
            approach_vs_stddev_fpm: stats.approach_vs_stddev_fpm,
            approach_bank_stddev_deg: stats.approach_bank_stddev_deg,
            approach_vs_stddev_filtered_fpm: stats.approach_vs_stddev_filtered_fpm,
            approach_bank_stddev_filtered_deg: stats.approach_bank_stddev_filtered_deg,
            rollout_distance_m: stats.rollout_distance_m,
            rollout_finalized: stats.rollout_finalized,
            bahn_spur: stats.bahn_spur.clone(),
            abflug_icao: stats.abflug_icao.clone(),
            bahn_vorige_spuren: stats.bahn_vorige_spuren.clone(),
            bahn_spur_laeuft: stats.bahn_spur_laeuft,
            bahn_spur_bis: stats.bahn_spur_bis,
            bahn_nachtrag_offen: stats.bahn_nachtrag_offen,
            bahn_max_querversatz_m: stats.bahn_max_querversatz_m,
            bahn_overrun_m: stats.bahn_overrun_m,
            bahn_proben: stats.bahn_proben,
            bahn_fenster_zu: stats.bahn_fenster_zu,
            bahn_fenster_zu_laengs_m: stats.bahn_fenster_zu_laengs_m,
            bahn_raeum_laengs_m: stats.bahn_raeum_laengs_m,
            bahn_raeum_gs_kt: stats.bahn_raeum_gs_kt,
            bahn_raeum_seite: stats.bahn_raeum_seite.clone(),
            bahn_kante_laengs_m: stats.bahn_kante_laengs_m,
            bahn_kante_gs_kt: stats.bahn_kante_gs_kt,
            bahn_raeum_kurs_diff: stats.bahn_raeum_kurs_diff,
            bahn_ausfahrt_kandidat: stats.bahn_ausfahrt_kandidat.clone(),
            rollout_finalize_reason: stats.rollout_finalize_reason.clone(),
            landing_touchdown_zone: stats.landing_touchdown_zone,
            landing_float_distance_m: stats.landing_float_distance_m,
            letzter_nachtrag: stats.letzter_nachtrag,
            szenerie_uebernahme: stats.szenerie_uebernahme.clone(),
            szenerie_diagnose: stats.szenerie_diagnose.clone(),
            sim_kennung: stats.sim_kennung.clone(),
            runway_tch_actual_ft: stats.runway_tch_actual_ft,
            fahrwerk_spurweite_m: stats.fahrwerk_spurweite_m,
            bahn_spur_bezug_veraltet: stats.bahn_spur_bezug_veraltet,
            szenerie_status_fest: stats.szenerie_status_fest.clone(),
            aufgeloestes_muster: stats.aufgeloestes_muster.clone(),
            bordbuch: stats.bordbuch.clone(),
            planned_block_fuel_kg: stats.planned_block_fuel_kg,
            planned_burn_kg: stats.planned_burn_kg,
            planned_reserve_kg: stats.planned_reserve_kg,
            planned_zfw_kg: stats.planned_zfw_kg,
            planned_tow_kg: stats.planned_tow_kg,
            planned_ldw_kg: stats.planned_ldw_kg,
            planned_route: stats.planned_route.clone(),
            planned_atc_callsign: stats.planned_atc_callsign.clone(),
            planned_waypoints: stats.planned_waypoints.clone(),
            planned_alternate: stats.planned_alternate.clone(),
            planned_arr_ref_pos: stats.planned_arr_ref_pos,
            sprit_vergleichspunkt_kg: stats.sprit_vergleichspunkt_kg,
            sprit_wp_messungen: stats.sprit_wp_messungen.clone(),
            sprit_wp_segment: stats.sprit_wp_segment,
            sprit_vergleichspunkt_odo_nm: stats.sprit_vergleichspunkt_odo_nm,
            sprit_plan_rest_nm: stats.sprit_plan_rest_nm,
            sprit_plan_bis_tod_kg: stats.sprit_plan_bis_tod_kg,
            sprit_zeit_unter_schwelle_s: stats.sprit_zeit_unter_schwelle_s,
            sprit_schwelle_ft: stats.sprit_schwelle_ft,
            sprit_plan_bis_vergleichspunkt_kg: stats.sprit_plan_bis_vergleichspunkt_kg,
            sprit_plan_rest_ab_vergleichspunkt_nm: stats.sprit_plan_rest_ab_vergleichspunkt_nm,
            engine_start_fuel_kg: stats.engine_start_fuel_kg,
            engine_off_fuel_kg: stats.engine_off_fuel_kg,
            sprit_hoechste_hoehe_ft: stats.sprit_hoechste_hoehe_ft,
            sprit_plan_trip_am_vergleichspunkt_kg: stats.sprit_plan_trip_am_vergleichspunkt_kg,
            sprit_landung_odo_nm: stats.sprit_landung_odo_nm,
            sprit_einstieg_in_der_luft: stats.sprit_einstieg_in_der_luft,
            sprit_einstieg_ort: stats.sprit_einstieg_ort,
            sprit_plan_bis_einstieg_kg: stats.sprit_plan_bis_einstieg_kg,
            planned_taxi_kg: stats.planned_taxi_kg,
            planned_contingency_kg: stats.planned_contingency_kg,
            planned_alternate_burn_kg: stats.planned_alternate_burn_kg,
            planned_extra_kg: stats.planned_extra_kg,
            sprit_auswertung: stats.sprit_auswertung.clone(),
            planned_arr_ref_source: stats.planned_arr_ref_source,
            planned_max_zfw_kg: stats.planned_max_zfw_kg,
            planned_max_tow_kg: stats.planned_max_tow_kg,
            planned_max_ldw_kg: stats.planned_max_ldw_kg,
            touchdown_events: stats.touchdown_events.clone(),
            touch_and_go_pending_since: stats.touch_and_go_pending_since,
            go_around_count: stats.go_around_count,
            lowest_agl_during_approach_ft: stats.lowest_agl_during_approach_ft,
            tiefster_anflugpunkt: stats.tiefster_anflugpunkt,
            go_around_climb_pending_since: stats.go_around_climb_pending_since,
            descent_pending_since: stats.descent_pending_since,
            previous_phase_before_holding: stats.previous_phase_before_holding,
            // v0.5.45: Sampler-State persistieren gegen Resume-Re-Capture
            sampler_touchdown_at: stats.sampler_touchdown_at,
            sampler_touchdown_rejected_reason: stats.sampler_touchdown_rejected_reason,
            sampler_takeoff_at: stats.sampler_takeoff_at,
            touchdown_window_dumped_at: stats.touchdown_window_dumped_at,
            touchdown_window_score_data_ready_at: stats.touchdown_window_score_data_ready_at,
            landing_score_finalized: stats.landing_score_finalized,
            // v0.7.7 OFP-Refresh-Foundation
            simbrief_ofp_id: stats.simbrief_ofp_id.clone(),
            simbrief_ofp_generated_at: stats.simbrief_ofp_generated_at.clone(),
            ofp_planzeiten: stats.ofp_planzeiten,
            // Kabinenansagen (Beta)
            pax_count: stats.pax_count,
            max_passengers: stats.max_passengers,
            flight_type: stats.flight_type.clone(),
            airline_name: stats.airline_name.clone(),
            umleitung_ziel: stats.umleitung_ziel.clone(),
            utc_versatz_abflug_min: stats.utc_versatz_abflug_min,
            utc_versatz_ziel_min: stats.utc_versatz_ziel_min,
            // Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator)
            pause_total_duration_secs: stats.pause_total_duration_secs,
            pause_segments: stats.pause_segments.clone(),
            abfangen_gesichert: stats.abfangen_gesichert.clone(),
            approach_runway_changed_late: stats.approach_runway_changed_late,
            approach_stall_warning_count: stats.approach_stall_warning_count,
            approach_wind: stats.approach_wind.clone(),
            approach_stable_at_da: stats.approach_stable_at_da,
            approach_spaetes_final: stats.approach_spaetes_final,
            landing_vref_deviation_kt: stats.landing_vref_deviation_kt,
            landing_vref_source: stats.landing_vref_source.map(str::to_string),
            landing_vref_toleranz_kt: stats.landing_vref_toleranz_kt,
            landing_vref_formel_kt: stats.landing_vref_formel_kt,
            landing_vref_landeklappen: stats.landing_vref_landeklappen,
            landing_yaw_rate_deg_per_sec: stats.landing_yaw_rate_deg_per_sec,
            landing_brake_energy_proxy: stats.landing_brake_energy_proxy,
            landing_decel_mps2: stats.landing_decel_mps2,
            landing_wing_strike_severity_pct: stats.landing_wing_strike_severity_pct,
            arr_metar_raw: stats.arr_metar_raw.clone(),
            // Nach dem Aufsetzen schreibt die Sicherung der Prozess, der die
            // Landung gemessen hat — also dessen Version, solange keine
            // ältere Sicherung sie für DIESES Aufsetzen schon trägt.
            client_version_aufsetzen: stats
                .landing_at
                .map(|at| (aufzeichnende_client_version(stats), at)),
            anflug_gleitpfad: stats.anflug_forensik.gleitpfad.clone(),
            anflug_ruhe: stats.anflug_forensik.ruhe.clone(),
            approach_vs_jerk_fpm: stats.approach_vs_jerk_fpm,
            approach_ias_stddev_kt: stats.approach_ias_stddev_kt,
            approach_excessive_sink: stats.approach_excessive_sink,
            approach_stable_config: stats.approach_stable_config,
            approach_ias_stddev_unter_500_kt: stats.approach_ias_stddev_unter_500_kt,
            approach_stable_config_500: stats.approach_stable_config_500,
            current_pause_reason: stats.current_pause_reason,
            disconnect_sim_liveness: stats.disconnect_sim_liveness,
            last_persisted_snapshot: stats.last_persisted_snapshot.clone(),
            resume_discontinuity: stats.resume_discontinuity,
            resume_spruenge: stats.resume_spruenge.clone(),
            app_restart_was_unclean: stats.app_restart_was_unclean,
            resume_gap_minutes: stats.resume_gap_minutes,
            resume_luecke_max_secs: stats.resume_luecke_max_secs,
            weiterflug_ab_boden_seit: stats.weiterflug_ab_boden_seit,
            // v0.16.12 (#phase-v2)
            planned_cruise_alt_ft: stats.planned_cruise_alt_ft,
            v2_reiseflughoehe_ft: stats.shadow_engine.beobachtete_reiseflughoehe_ft(),
        }
    }

    fn apply_to(self, stats: &mut FlightStats) {
        stats.distance_nm = self.distance_nm;
        stats.position_count = self.position_count;
        // Don't clobber the freshly-initialised Boarding phase with the
        // serde-default Preflight when the persisted JSON predates the
        // stats sidecar (no `phase` field → FlightPhase::default() →
        // Preflight, which has no matching arm in step_flight, so the
        // FSM would freeze and never advance past it).
        if self.phase != FlightPhase::Preflight {
            stats.phase = self.phase;
        }
        stats.block_off_at = self.block_off_at;
        stats.takeoff_at = self.takeoff_at;
        stats.landing_at = self.landing_at;
        stats.block_on_at = self.block_on_at;
        stats.takeoff_weight_kg = self.takeoff_weight_kg;
        stats.takeoff_fuel_kg = self.takeoff_fuel_kg;
        stats.takeoff_pitch_deg = self.takeoff_pitch_deg;
        stats.takeoff_bank_deg = self.takeoff_bank_deg;
        stats.landing_rate_fpm = self.landing_rate_fpm;
        stats.landing_g_force = self.landing_g_force;
        stats.landing_pitch_deg = self.landing_pitch_deg;
        stats.landing_bank_deg = self.landing_bank_deg;
        stats.landing_speed_kt = self.landing_speed_kt;
        stats.landing_groundspeed_kt = self.landing_groundspeed_kt;
        stats.landing_true_airspeed_kt = self.landing_true_airspeed_kt;
        stats.landing_weight_kg = self.landing_weight_kg;
        stats.landing_heading_deg = self.landing_heading_deg;
        stats.landing_fuel_kg = self.landing_fuel_kg;
        stats.landing_fuel_puffer = self.landing_fuel_puffer;
        stats.block_fuel_kg = self.block_fuel_kg;
        stats.last_fuel_kg = self.last_fuel_kg;
        stats.initial_fob_kg = self.initial_fob_kg;
        stats.last_lat = self.last_lat;
        stats.last_lon = self.last_lon;
        stats.last_pos_at = self.last_pos_at;
        stats.landing_peak_vs_fpm = self.landing_peak_vs_fpm;
        stats.landing_peak_g_force = self.landing_peak_g_force;
        stats.landing_confidence = self.landing_confidence;
        stats.landung_abdeckung_fehlt = self.landung_abdeckung_fehlt;
        stats.sampler_diagnose = self.sampler_diagnose;
        stats.bodenhoehe = self.bodenhoehe;
        stats.landing_source = self.landing_source;
        stats.landing_wind_direction_deg = self.landing_wind_direction_deg;
        stats.landing_wind_speed_kt = self.landing_wind_speed_kt;
        // Zurueck auf die drei bekannten Kennungen; alles andere gilt als
        // unbekannt und laesst die Normierung (bewusst) aus.
        stats.landing_simulator = match self.landing_simulator.as_deref() {
            Some("msfs") => Some("msfs"),
            Some("xplane") => Some("xplane"),
            Some("other") => Some("other"),
            _ => None,
        };
        // ⚠ Der Rueckfall ist NICHT Bequemlichkeit.
        //
        // Eine Flugdatei aus v1.7.14 kennt `aufsetz_simulator` nicht,
        // fuehrt aber `landing_simulator`. Ohne diesen Zweig kehrt
        // `bahn_am_aufsetzpunkt_nachholen` bei genau den Fluegen sofort
        // zurueck, die auf v1.7.15 fortgesetzt werden — also bei den
        // ersten, die die neue Mechanik ueberhaupt sehen wuerden.
        //
        // Fuer die Szenerie zaehlt nur die FAMILIE (`quelle_fuer`
        // unterscheidet X-Plane, MSFS und sonst nichts), und genau die
        // steht in `landing_simulator`. Die Fassung innerhalb der
        // Familie ist dort verloren; sie spielt an dieser Stelle keine
        // Rolle.
        stats.aufsetz_simulator = self
            .aufsetz_simulator
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .or_else(|| match self.landing_simulator.as_deref() {
                Some("msfs") => Some(sim_core::Simulator::Msfs2024),
                Some("xplane") => Some(sim_core::Simulator::XPlane12),
                _ => None,
            });
        stats.bahn_revision = self.bahn_revision;
        stats.bounce_count = self.bounce_count;
        stats.light_bounce_count = self.light_bounce_count;
        stats.landing_score = self.landing_score;
        stats.landing_score_announced = self.landing_score_announced;
        stats.dep_gate = self.dep_gate;
        stats.dep_gate_field_post_confirmed = self.dep_gate_field_post_confirmed;
        stats.dep_gate_withdrawal_pending = self.dep_gate_withdrawal_pending;
        // v1.7.17 Runde 17 (externe Gegenpruefung, Codex, adversarial,
        // 04.09.2026): eine ueber einen Neustart hinweg eingefrorene, noch
        // unbestaetigte Rueckzugsentscheidung kann nach dem Resume NIE MEHR
        // aufgeloest werden — der Task, der die Bestaetigung geliefert
        // haette, ist mit dem alten Prozess gestorben. Bewusst NICHT als
        // "wahrscheinlich erfolgreich" behandeln und automatisch
        // zurueckziehen: das koennte einen fremden, nie von AeroACARS
        // beruehrten Server-Wert zerstoeren (Runde 14). Bewusst NICHT als
        // "wahrscheinlich fehlgeschlagen" behandeln und schweigend
        // ignorieren: das liesse einen tatsaechlich geposteten, jetzt
        // veralteten Wert dauerhaft unkorrigiert stehen. Es gibt keine
        // dritte, sichere Option ohne serverseitige Versionierung/
        // Read-Back, die es nicht gibt (siehe Pruefstand-Dokument, Runde
        // 17) — das Flag wird deshalb zurueckgesetzt, statt als ewig
        // haengender "erledige mich"-Zustand liegen zu bleiben, den nichts
        // je erledigen wird, und die Unsicherheit wird protokolliert.
        if self.dep_gate_withdrawal_awaiting_confirmation {
            tracing::warn!(
                "dep_gate_withdrawal_awaiting_confirmation ueberlebte einen Neustart \
                 unaufgeloest — die urspruengliche Bestaetigung ist mit dem alten \
                 Prozess verloren; kein automatischer Rueckzug (koennte fremde Daten \
                 zerstoeren), aber ein moeglicherweise geposteter, jetzt veralteter \
                 Departure-Gate-Wert kann auf dem Server stehen bleiben"
            );
        }
        stats.dep_gate_withdrawal_awaiting_confirmation = false;
        stats.arr_gate = self.arr_gate;
        // A snapshot written before v0.19.3 has no `blocks_on_reached` (serde
        // default = false). Derive it from the persisted phase so a flight
        // resumed at/after the gate isn't told it never blocked on.
        stats.blocks_on_reached = self.blocks_on_reached
            || matches!(
                self.phase,
                FlightPhase::BlocksOn | FlightPhase::Arrived | FlightPhase::PirepSubmitted
            );
        stats.arr_gate_icao = self.arr_gate_icao;
        stats.arr_actual_icao = self.arr_actual_icao;
        stats.last_known_lat = self.last_known_lat;
        stats.last_known_lon = self.last_known_lon;
        stats.last_known_alt_ft = self.last_known_alt_ft;
        stats.last_known_hdg_deg = self.last_known_hdg_deg;
        stats.last_known_gs_kt = self.last_known_gs_kt;
        stats.last_known_at = self.last_known_at;
        stats.divert_hint = self.divert_hint;
        stats.approach_runway = self.approach_runway;
        stats.cruise_peak_msl = self.cruise_peak_msl;
        stats.climb_peak_msl = self.climb_peak_msl;
        stats.peak_altitude_ft = self.peak_altitude_ft;
        stats.aircraft_banner_logged = self.aircraft_banner_logged;
        stats.touchdown_sideslip_deg = self.touchdown_sideslip_deg;
        stats.touchdown_profile = self.touchdown_profile;
        stats.track = self.track;
        stats.runway_match = self.runway_match;
        // v0.16.24 (QS-2 FIX B): restore the runway-correlation source/geometry
        // so a resumed divert keeps its provisional/finalized state.
        stats.runway_source = self.runway_source;
        stats.runway_nav_cycle = self.runway_nav_cycle;
        stats.runway_nav_geometry = self.runway_nav_geometry;
        stats.runway_ils_navigraph = self.runway_ils_navigraph;
        stats.runway_correlation_icao = self.runway_correlation_icao;
        stats.runway_fallback_at_touchdown = self.runway_fallback_at_touchdown;
        stats.landing_lat = self.landing_lat;
        stats.landing_lon = self.landing_lon;
        stats.landing_heading_true_deg = self.landing_heading_true_deg;
        stats.landing_headwind_kt = self.landing_headwind_kt;
        stats.landing_crosswind_kt = self.landing_crosswind_kt;
        stats.approach_vs_stddev_fpm = self.approach_vs_stddev_fpm;
        stats.approach_bank_stddev_deg = self.approach_bank_stddev_deg;
        stats.approach_vs_stddev_filtered_fpm = self.approach_vs_stddev_filtered_fpm;
        stats.approach_bank_stddev_filtered_deg = self.approach_bank_stddev_filtered_deg;
        stats.rollout_distance_m = self.rollout_distance_m;
        stats.rollout_finalized = self.rollout_finalized;
        stats.bahn_spur = self.bahn_spur;
        stats.abflug_icao = self.abflug_icao;
        stats.bahn_vorige_spuren = self.bahn_vorige_spuren;
        stats.bahn_spur_laeuft = self.bahn_spur_laeuft;
        stats.bahn_spur_bis = self.bahn_spur_bis;
        stats.bahn_nachtrag_offen = self.bahn_nachtrag_offen;
        stats.bahn_max_querversatz_m = self.bahn_max_querversatz_m;
        stats.bahn_overrun_m = self.bahn_overrun_m;
        stats.bahn_proben = self.bahn_proben;
        stats.bahn_fenster_zu = self.bahn_fenster_zu;
        stats.bahn_fenster_zu_laengs_m = self.bahn_fenster_zu_laengs_m;
        stats.bahn_raeum_laengs_m = self.bahn_raeum_laengs_m;
        stats.bahn_raeum_gs_kt = self.bahn_raeum_gs_kt;
        stats.bahn_raeum_seite = self.bahn_raeum_seite;
        stats.bahn_kante_laengs_m = self.bahn_kante_laengs_m;
        stats.bahn_kante_gs_kt = self.bahn_kante_gs_kt;
        stats.bahn_raeum_kurs_diff = self.bahn_raeum_kurs_diff;
        stats.bahn_ausfahrt_kandidat = self.bahn_ausfahrt_kandidat;
        stats.rollout_finalize_reason = self.rollout_finalize_reason;
        stats.landing_touchdown_zone = self.landing_touchdown_zone;
        stats.landing_float_distance_m = self.landing_float_distance_m;
        stats.letzter_nachtrag = self.letzter_nachtrag;
        stats.szenerie_uebernahme = self.szenerie_uebernahme;
        stats.szenerie_diagnose = self.szenerie_diagnose;
        stats.sim_kennung = self.sim_kennung;
        stats.runway_tch_actual_ft = self.runway_tch_actual_ft;
        stats.fahrwerk_spurweite_m = self.fahrwerk_spurweite_m;
        stats.bahn_spur_bezug_veraltet = self.bahn_spur_bezug_veraltet;
        stats.szenerie_status_fest = self.szenerie_status_fest;
        stats.aufgeloestes_muster = self.aufgeloestes_muster;
        stats.bordbuch = self.bordbuch;
        stats.planned_block_fuel_kg = self.planned_block_fuel_kg;
        stats.planned_burn_kg = self.planned_burn_kg;
        stats.planned_reserve_kg = self.planned_reserve_kg;
        stats.planned_zfw_kg = self.planned_zfw_kg;
        stats.planned_tow_kg = self.planned_tow_kg;
        stats.planned_ldw_kg = self.planned_ldw_kg;
        stats.planned_route = self.planned_route;
        stats.planned_atc_callsign = self.planned_atc_callsign;
        stats.planned_waypoints = self.planned_waypoints;
        stats.planned_alternate = self.planned_alternate;
        stats.planned_arr_ref_pos = self.planned_arr_ref_pos;
        stats.sprit_vergleichspunkt_kg = self.sprit_vergleichspunkt_kg;
        stats.sprit_wp_messungen = self.sprit_wp_messungen;
        stats.sprit_wp_segment = self.sprit_wp_segment;
        stats.sprit_vergleichspunkt_odo_nm = self.sprit_vergleichspunkt_odo_nm;
        stats.sprit_plan_rest_nm = self.sprit_plan_rest_nm;
        stats.sprit_plan_bis_tod_kg = self.sprit_plan_bis_tod_kg;
        stats.sprit_zeit_unter_schwelle_s = self.sprit_zeit_unter_schwelle_s;
        stats.sprit_schwelle_ft = self.sprit_schwelle_ft;
        stats.sprit_plan_bis_vergleichspunkt_kg = self.sprit_plan_bis_vergleichspunkt_kg;
        stats.sprit_plan_rest_ab_vergleichspunkt_nm = self.sprit_plan_rest_ab_vergleichspunkt_nm;
        stats.engine_start_fuel_kg = self.engine_start_fuel_kg;
        stats.engine_off_fuel_kg = self.engine_off_fuel_kg;
        stats.sprit_hoechste_hoehe_ft = self.sprit_hoechste_hoehe_ft;
        stats.sprit_plan_trip_am_vergleichspunkt_kg = self.sprit_plan_trip_am_vergleichspunkt_kg;
        stats.sprit_landung_odo_nm = self.sprit_landung_odo_nm;
        stats.sprit_einstieg_in_der_luft = self.sprit_einstieg_in_der_luft;
        stats.sprit_einstieg_ort = self.sprit_einstieg_ort;
        stats.sprit_plan_bis_einstieg_kg = self.sprit_plan_bis_einstieg_kg;
        stats.planned_taxi_kg = self.planned_taxi_kg;
        stats.planned_contingency_kg = self.planned_contingency_kg;
        stats.planned_alternate_burn_kg = self.planned_alternate_burn_kg;
        stats.planned_extra_kg = self.planned_extra_kg;
        stats.sprit_auswertung = self.sprit_auswertung;
        stats.planned_arr_ref_source = self.planned_arr_ref_source;
        stats.planned_max_zfw_kg = self.planned_max_zfw_kg;
        stats.planned_max_tow_kg = self.planned_max_tow_kg;
        stats.planned_max_ldw_kg = self.planned_max_ldw_kg;
        stats.touchdown_events = self.touchdown_events;
        stats.touch_and_go_pending_since = self.touch_and_go_pending_since;
        stats.go_around_count = self.go_around_count;
        stats.lowest_agl_during_approach_ft = self.lowest_agl_during_approach_ft;
        stats.tiefster_anflugpunkt = self.tiefster_anflugpunkt;
        stats.go_around_climb_pending_since = self.go_around_climb_pending_since;
        stats.descent_pending_since = self.descent_pending_since;
        stats.previous_phase_before_holding = self.previous_phase_before_holding;
        // v0.5.45: Sampler-State restoren — verhindert Re-Capture nach Resume
        // wenn der Touchdown vor dem Quit/Restart bereits gefeuert hat.
        stats.sampler_touchdown_at = self.sampler_touchdown_at;
        stats.sampler_touchdown_rejected_reason = self.sampler_touchdown_rejected_reason;
        stats.sampler_takeoff_at = self.sampler_takeoff_at;
        stats.touchdown_window_dumped_at = self.touchdown_window_dumped_at;
        stats.touchdown_window_score_data_ready_at = self.touchdown_window_score_data_ready_at;
        stats.landing_score_finalized = self.landing_score_finalized;
        // v0.7.7 OFP-Refresh-Foundation
        stats.simbrief_ofp_id = self.simbrief_ofp_id;
        stats.simbrief_ofp_generated_at = self.simbrief_ofp_generated_at;
        stats.ofp_planzeiten = self.ofp_planzeiten;
        // Kabinenansagen (Beta): Wiedereinstieg braucht dieselben Eingaben.
        stats.pax_count = self.pax_count;
        stats.max_passengers = self.max_passengers;
        stats.flight_type = self.flight_type;
        stats.airline_name = self.airline_name;
        stats.umleitung_ziel = self.umleitung_ziel;
        stats.utc_versatz_abflug_min = self.utc_versatz_abflug_min;
        stats.utc_versatz_ziel_min = self.utc_versatz_ziel_min;
        // Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator):
        // ueber `save_active_flight`/Resume hinweg erhalten. Damit
        // wird ein App-Restart waehrend laufender Pause die akkumu-
        // lierte Zeit nicht verlieren (Spec-Szenario 3.4.2).
        stats.pause_total_duration_secs = self.pause_total_duration_secs;
        stats.pause_segments = self.pause_segments;
        stats.abfangen_gesichert = self.abfangen_gesichert;
        stats.approach_runway_changed_late = self.approach_runway_changed_late;
        stats.approach_stall_warning_count = self.approach_stall_warning_count;
        stats.approach_wind = self.approach_wind;
        stats.approach_stable_at_da = self.approach_stable_at_da;
        stats.approach_spaetes_final = self.approach_spaetes_final;
        stats.landing_vref_deviation_kt = self.landing_vref_deviation_kt;
        // Feste Werte (siehe Vref-Zuordnung beim Aufsetzen).
        stats.landing_vref_source = self.landing_vref_source.as_deref().map(|s| match s {
            "pmdg" => "pmdg",
            "fbw" => "fbw",
            "fmc" => "fmc",
            "kalibriert" => "kalibriert",
            "faa" => "faa",
            "faa_ungeprueft" => "faa_ungeprueft",
            "icao_default" => "icao_default",
            _ => "unknown",
        });
        stats.landing_vref_toleranz_kt = self.landing_vref_toleranz_kt;
        stats.landing_vref_formel_kt = self.landing_vref_formel_kt;
        stats.landing_vref_landeklappen = self.landing_vref_landeklappen;
        stats.landing_yaw_rate_deg_per_sec = self.landing_yaw_rate_deg_per_sec;
        stats.landing_brake_energy_proxy = self.landing_brake_energy_proxy;
        stats.landing_decel_mps2 = self.landing_decel_mps2;
        stats.landing_wing_strike_severity_pct = self.landing_wing_strike_severity_pct;
        stats.arr_metar_raw = self.arr_metar_raw;
        stats.client_version_aufsetzen = self.client_version_aufsetzen;
        stats.anflug_forensik.gleitpfad = self.anflug_gleitpfad;
        stats.anflug_forensik.ruhe = self.anflug_ruhe;
        stats.approach_vs_jerk_fpm = self.approach_vs_jerk_fpm;
        stats.approach_ias_stddev_kt = self.approach_ias_stddev_kt;
        stats.approach_excessive_sink = self.approach_excessive_sink;
        stats.approach_stable_config = self.approach_stable_config;
        stats.approach_ias_stddev_unter_500_kt = self.approach_ias_stddev_unter_500_kt;
        stats.approach_stable_config_500 = self.approach_stable_config_500;
        stats.current_pause_reason = self.current_pause_reason;
        stats.disconnect_sim_liveness = self.disconnect_sim_liveness;
        stats.last_persisted_snapshot = self.last_persisted_snapshot;
        stats.resume_discontinuity = self.resume_discontinuity;
        stats.resume_spruenge = self.resume_spruenge;
        stats.app_restart_was_unclean = self.app_restart_was_unclean;
        stats.resume_gap_minutes = self.resume_gap_minutes;
        stats.resume_luecke_max_secs = self.resume_luecke_max_secs;
        stats.weiterflug_ab_boden_seit = self.weiterflug_ab_boden_seit;
        // v0.16.12 (#phase-v2): cruise_ref + Divergenz-Aggregat restoren.
        // Die Engine selbst wird NICHT persistiert (wärmt sich nach dem
        // Resume in ≤ 2 Fenstern wieder auf — dokumentiert).
        stats.planned_cruise_alt_ft = self.planned_cruise_alt_ft;
        stats
            .shadow_engine
            .reiseflughoehe_uebernehmen(self.v2_reiseflughoehe_ft);
    }
}

#[cfg(test)]
#[path = "tests_lib/arr_actual_icao_resume_tests.rs"]
mod arr_actual_icao_resume_tests;
