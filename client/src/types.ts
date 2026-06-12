// Mirrors the Rust types in `client/src-tauri/crates/api-client/src/lib.rs`
// and the command return types in `client/src-tauri/src/lib.rs`.
//
// Keep these in sync manually for now. Phase 4: codegen via JSON Schema.

export interface Airline {
  id: number;
  icao: string;
  iata: string | null;
  name: string;
  logo: string | null;
}

export interface Rank {
  name: string | null;
}

export interface Profile {
  id: number;
  pilot_id: number;
  ident: string | null;
  name: string;
  email: string | null;
  airline_id: number | null;
  curr_airport: string | null;
  home_airport: string | null;
  airline: Airline | null;
  rank: Rank | null;
}

export interface LoginResult {
  profile: Profile;
  base_url: string;
}

/** Stable error code from the Rust side; maps to an i18n key. */
export interface UiError {
  code: string;
  message: string;
}

export interface AppInfo {
  name: string;
  version: string;
  commit: string | null;
  /** Small credit line for the Settings footer. */
  credit: string;
}

export interface Airport {
  id: string;
  icao: string | null;
  iata: string | null;
  name: string | null;
}

export interface Distance {
  mi: number | null;
  km: number | null;
  nmi: number | null;
}

export interface SimBrief {
  id: string;
  url: string | null;
  aircraft_id: number | null;
  /** Subfleet info from the SimBrief OFP — drives the Bid-Card aircraft
   *  display ("B738 · Boeing 737-800") and the Pax/Cargo load chips. */
  subfleet: SimBriefSubfleet | null;
}

export interface SimBriefSubfleet {
  /** ICAO of the subfleet (e.g. "B738"). Mapped from `<aircraft><icaocode>`. */
  type_: string | null;
  /** Marketing name (e.g. "Boeing 737-800"). */
  name: string | null;
  /** Per-fare-class load (Pax + Cargo). Used to render "184 PAX" or
   *  "42.5 t cargo" chips on the Bid-Card. */
  fares: SimBriefFare[];
}

export interface SimBriefFare {
  id: number;
  code: string | null;
  name: string | null;
  capacity: number | null;
  count: number | null;
  /** 0 = passenger fare, 1 = cargo (phpVMS convention). */
  type: number | null;
}

/** Aircraft details from phpVMS — fetched via `phpvms_get_aircraft` Tauri
 *  command so the Bid-Card can show the actual reserved tail number
 *  (e.g. "EI-ENI") instead of just the subfleet name. */
export interface AircraftInfo {
  id: number;
  registration: string | null;
  icao: string | null;
  name: string | null;
}

/** SimBrief OFP plan values — fetched via `fetch_simbrief_preview` Tauri
 *  command before the flight starts so the Bid-Card / Briefing tab can
 *  show "your OFP says: Block 13.1t, Burn 9.2t, TOW 73.6t" before the
 *  pilot has tanked/boarded. All weights / fuel quantities in kg. */
export interface SimBriefOfp {
  planned_block_fuel_kg: number;
  planned_burn_kg: number;
  planned_reserve_kg: number;
  planned_zfw_kg: number;
  planned_tow_kg: number;
  planned_ldw_kg: number;
  route: string | null;
  alternate: string | null;
  // v0.3.0 OFP-Identität für Mismatch-Detection.
  /** Flight-Number aus dem OFP (z.B. "DLH123" oder "RYR100"). */
  ofp_flight_number: string;
  /** Origin-Airport ICAO aus dem OFP (z.B. "LOWS"). */
  ofp_origin_icao: string;
  /** Destination-Airport ICAO aus dem OFP (z.B. "EDDB"). */
  ofp_destination_icao: string;
  /** Wann der OFP erstellt wurde (Unix-Timestamp als String). */
  ofp_generated_at: string;
  /** v0.7.12: Pax + Cargo aus dem SimBrief-XML — damit die Bid-Card auch
   *  ohne phpVMS-Bid-Pointer-Subfleet-Fares die Pax/Cargo-Chips zeigt. */
  pax_count?: number;
  cargo_kg?: number;
}

export interface Flight {
  id: string;
  flight_number: string;
  route_code: string | null;
  route_leg: string | null;
  callsign: string | null;
  dpt_airport_id: string;
  arr_airport_id: string;
  alt_airport_id: string | null;
  flight_time: number | null;
  level: number | null;
  route: string | null;
  flight_type: string | null;
  distance: Distance | null;
  airline: Airline | null;
  dpt_airport: Airport | null;
  arr_airport: Airport | null;
  simbrief: SimBrief | null;
}

export interface Bid {
  id: number;
  user_id: number;
  flight_id: string;
  flight: Flight;
}

// ---- Simulator telemetry (mirrors sim-core::SimSnapshot) ----

export type Simulator =
  | "Msfs2020"
  | "Msfs2024"
  | "XPlane11"
  | "XPlane12"
  | "Other";

export interface SimSnapshot {
  timestamp: string; // ISO-8601 UTC
  lat: number;
  lon: number;
  altitude_msl_ft: number;
  /** v0.16.15: Altimeter-Anzeige (mit aktuellem Baro) — primäre UI-Höhe. */
  altitude_indicated_ft: number | null;
  altitude_pressure_ft: number | null;
  altitude_agl_ft: number;
  heading_deg_true: number;
  heading_deg_magnetic: number;
  pitch_deg: number;
  bank_deg: number;
  vertical_speed_fpm: number;
  /** Body-frame velocity components (fps). Used for native sideslip
   *  at touchdown. None when the SimVar isn't wired. */
  velocity_body_x_fps: number | null;
  velocity_body_z_fps: number | null;
  groundspeed_kt: number;
  indicated_airspeed_kt: number;
  true_airspeed_kt: number;
  /** Body-frame wind components (knots). Positive aircraft_wind_x_kt
   *  = crosswind from the right; positive aircraft_wind_z_kt = tailwind. */
  aircraft_wind_x_kt: number | null;
  aircraft_wind_z_kt: number | null;
  g_force: number;
  on_ground: boolean;
  parking_brake: boolean;
  stall_warning: boolean;
  overspeed_warning: boolean;
  paused: boolean;
  slew_mode: boolean;
  simulation_rate: number;
  gear_position: number;
  flaps_position: number;
  engines_running: number;
  fuel_total_kg: number;
  fuel_used_kg: number;
  zfw_kg: number | null;
  payload_kg: number | null;
  /** Gross weight (`TOTAL WEIGHT` SimVar). null when the aircraft addon
   *  doesn't wire it (e.g. Fenix). Already in kg. */
  total_weight_kg: number | null;
  /** Avionics — null when the active aircraft profile doesn't wire
   *  the underlying SimVar/LVar (notably Fenix for COM/NAV). */
  transponder_code: number | null;
  com1_mhz: number | null;
  com2_mhz: number | null;
  nav1_mhz: number | null;
  nav2_mhz: number | null;
  /** Exterior lights — null when the active aircraft profile doesn't
   *  wire the underlying SimVar/LVar. */
  light_landing: boolean | null;
  light_beacon: boolean | null;
  light_strobe: boolean | null;
  light_taxi: boolean | null;
  light_nav: boolean | null;
  light_logo: boolean | null;
  /** 3-state strobe selector — 0=OFF, 1=AUTO, 2=ON. null when only
   *  the binary `light_strobe` is available. */
  strobe_state: number | null;
  /** Autopilot state — same null semantics as lights. */
  autopilot_master: boolean | null;
  autopilot_heading: boolean | null;
  autopilot_altitude: boolean | null;
  autopilot_nav: boolean | null;
  autopilot_approach: boolean | null;
  /** Surfaces */
  spoilers_handle_position: number | null;
  spoilers_armed: boolean | null;
  /** MSFS PUSHBACK STATE: 0/1/2 = pushing, 3 = no pushback. */
  pushback_state: number | null;
  /** Systems */
  apu_switch: boolean | null;
  apu_pct_rpm: number | null;
  battery_master: boolean | null;
  avionics_master: boolean | null;
  pitot_heat: boolean | null;
  /** Combined "any engine has anti-ice on". */
  engine_anti_ice: boolean | null;
  /** Wing / structural deice. */
  wing_anti_ice: boolean | null;
  /** Cabin SEAT BELTS sign — 0=OFF, 1=AUTO, 2=ON (Fenix LVar based). */
  seatbelts_sign: number | null;
  /** Cabin NO SMOKING sign — same enum semantics. */
  no_smoking_sign: number | null;
  /** FCU encoder displays — selected ALT/HDG/SPD/VS. */
  fcu_selected_altitude_ft: number | null;
  fcu_selected_heading_deg: number | null;
  fcu_selected_speed_kt: number | null;
  fcu_selected_vs_fpm: number | null;
  /** Autobrake setting label ("OFF"/"LO"/"MED"/"MAX") or null. */
  autobrake: string | null;
  /** Latched touchdown sample — populated by SimConnect at the moment
   *  the gear hits the ground. null until the first touchdown. */
  touchdown_vs_fpm: number | null;
  touchdown_pitch_deg: number | null;
  touchdown_bank_deg: number | null;
  touchdown_heading_mag_deg: number | null;
  touchdown_lat: number | null;
  touchdown_lon: number | null;
  wind_direction_deg: number | null;
  wind_speed_kt: number | null;
  qnh_hpa: number | null;
  outside_air_temp_c: number | null;
  total_air_temp_c: number | null;
  mach: number | null;
  /** Aircraft empty weight in kg. `null` on Asobo default airliners
   *  (their OEW is bogus — verified ~1422 kg for the A320neo). */
  empty_weight_kg: number | null;
  aircraft_title: string | null;
  aircraft_icao: string | null;
  aircraft_registration: string | null;
  simulator: Simulator;
  sim_version: string | null;
}

export type SimConnectionState = "disconnected" | "connecting" | "connected";

export type SimKind =
  | "off"
  | "msfs2020"
  | "msfs2024"
  | "xplane11"
  | "xplane12";

export interface SimStatus {
  state: SimConnectionState;
  kind: SimKind;
  snapshot: SimSnapshot | null;
  last_error: string | null;
  available: boolean;
}

export type FlightPhase =
  | "preflight"
  | "boarding"
  | "pushback"
  | "taxi_out"
  | "takeoff_roll"
  | "takeoff"
  | "climb"
  | "cruise"
  | "holding"
  | "descent"
  | "approach"
  | "final"
  | "landing"
  | "taxi_in"
  | "blocks_on"
  | "arrived"
  | "pirep_submitted";

export interface ResumableFlight {
  pirep_id: string;
  flight_number: string;
  dpt_airport: string;
  arr_airport: string;
  status: string | null;
}

export interface ActiveFlightInfo {
  pirep_id: string;
  bid_id: number;
  started_at: string;
  /** ICAO of the operating airline, e.g. "DLH". Combined with
   *  `flight_number` to render the full callsign ("DLH155"). */
  airline_icao: string;
  /** Planned aircraft registration from phpVMS (e.g. "D-AIUV"). Empty
   *  when no matching bid / aircraft details could be looked up. */
  planned_registration: string;
  flight_number: string;
  dpt_airport: string;
  arr_airport: string;
  distance_nm: number;
  position_count: number;
  phase: FlightPhase;
  /** v0.16.12 (#phase-v2): snake_case-Phase der SCHATTEN-Engine v2 —
   *  reine Beobachtung (Verifikations-Hilfe), nie entscheidungsrelevant.
   *  Fehlt, bis der erste Schatten-Tick lief (skip_serializing_if). */
  shadow_phase?: FlightPhase;
  /** v0.16.12 (#phase-v2): Fenster-Segment der Schatten-Engine
   *  ("ground" | "climbing" | "level" | "descending" | "insufficient"). */
  shadow_segment?: string;
  block_off_at: string | null;
  takeoff_at: string | null;
  landing_at: string | null;
  block_on_at: string | null;
  landing_rate_fpm: number | null;
  landing_g_force: number | null;
  was_just_resumed: boolean;
  /** v0.12.1 (Stream E): on a resume, true when the sim position looks
   *  like a glitchy crash-reload (persisted phase airborne but sim
   *  on-ground, or implausibly large drift). The resume banner then
   *  disables its 10-second auto-confirm and waits for an explicit click. */
  resume_position_suspect: boolean;
  /** v0.13.0 Stream F: nur gesetzt wenn was_just_resumed=true. Die
   *  letzte gespeicherte Position für die Banner-Anzeige damit der Pilot
   *  weiß WOHIN er sich im Sim repositionieren muss. */
  last_known_lat?: number;
  last_known_lon?: number;
  last_known_alt_ft?: number;
  /** v0.13.0 Stream F: zusätzlich Fuel/Weight/Aircraft aus dem letzten
   *  Sim-Snapshot vor dem Crash/Disconnect. MSFS setzt Fuel beim Reload
   *  oft auf Default — der Pilot sieht den Soll-Wert + stellt ihn manuell
   *  nach, BEVOR er auf "Position prüfen + fortsetzen" klickt. */
  last_known_fuel_kg?: number;
  last_known_zfw_kg?: number;
  last_known_total_weight_kg?: number;
  last_known_aircraft_icao?: string;
  /** Stand the aircraft pushed back from (MSFS ATC PARKING NAME). */
  dep_gate: string | null;
  /** Stand the pilot parked at after arrival. */
  arr_gate: string | null;
  /** Approach runway from `ATC RUNWAY SELECTED` at Final. */
  approach_runway: string | null;
  /** v0.15.19: published glideslope angle (deg) of the approach runway from
   *  live navdata, plausibility-clamped 2–7.5°. Lets the live stable-approach
   *  banner scale its V/S thresholds (gs_factor = tan(g)/tan(3°)) like the
   *  post-flight scorer. null → unknown/implausible → banner uses 3° default. */
  approach_glideslope_angle: number | null;
  /** ISO-8601 UTC timestamp of the most recent successful
   *  position-post, or null if none has succeeded yet. */
  last_position_at: string | null;
  /** ISO-8601 UTC timestamp of the most recent successful PIREP
   *  heartbeat (`POST /pireps/{id}/update`). Used by the debug panel
   *  to confirm the keep-alive is firing — without it phpVMS soft-deletes
   *  the in-flight PIREP after `acars.live_time` hours. */
  last_heartbeat_at: string | null;
  /** Positions sitting in the in-memory outbox waiting for the next POST.
   *  0 means we're online and current (no backlog). */
  queued_position_count: number;
  /** v0.6.2 — Connection-Health vom phpVMS-Worker:
   *  - "live"    → letzter POST war Erfolg (auch wenn queued > 0 = nur Sync-Pause)
   *  - "failing" → letzter POST scheiterte (echter Connection-Loss)
   *  - "blocked" → v0.7.17 (B-007) phpVMS lehnt mit 401/403 ab — Account
   *                gesperrt / inaktiv / API-Key revoked. Worker hat
   *                terminiert, kein Endless-Retry-Spam mehr.
   *  - undefined → IPC-Payload von einer pre-v0.6.2 App (= Migration)
   *  Frontend nutzt das + queued_position_count für die Status-Anzeige.
   *  Optional damit pre-v0.6.2-IPC-Payloads noch deserialisieren. */
  connection_state?: "live" | "failing" | "blocked";
  /** v0.4.1: ISO-8601 UTC-Timestamp wann der Streamer den Sim-
   *  Disconnect detektiert und den Flug pausiert hat. `null` =
   *  normaler Flug; `string` = Cockpit-Tab zeigt Resume-Banner. */
  paused_since: string | null;
  /** v0.4.1: Letzte bekannte Sim-Werte vor Disconnect (für Reposition). */
  paused_last_known: PausedSnapshot | null;
  /** Set when the FSM noticed the aircraft landed somewhere other than
   *  the planned `arr_airport`. The cockpit renders a banner asking
   *  the pilot to confirm the actual destination so the PIREP can be
   *  filed with the correct `arr_airport_id`. Null on normal arrivals. */
  divert_hint: DivertHint | null;
  /** Touch-and-go count detected during the flight. Sustained climb-back
   *  above 100 ft AGL within 30 s of an on-ground edge counts as a T&G;
   *  the FSM also reverts to Climb so subsequent landing detection works
   *  normally. Always 0 on a routine A→B. */
  touch_and_go_count: number;
  /** Confirmed go-around count. Sustained 8 s of climb above lowest
   *  approach AGL + 200 ft with V/S > +500 fpm during Approach/Final
   *  fires this. Independent of T&G — a missed approach without
   *  ground contact only bumps this counter. */
  go_around_count: number;
  // ---- v0.3.0 — SimBrief OFP Plan-Werte für Soll/Ist-Vergleich ----
  /** Plan-Block-Fuel aus dem SimBrief OFP (kg). null wenn der Pilot
   *  keine SimBrief-Verbindung im phpVMS-Profil hat. */
  planned_block_fuel_kg: number | null;
  /** Plan-Trip-Burn aus dem SimBrief OFP (kg). */
  planned_burn_kg: number | null;
  /** Plan-Reserve-Fuel aus dem SimBrief OFP (kg). */
  planned_reserve_kg: number | null;
  /** Plan-ZFW (kg). */
  planned_zfw_kg: number | null;
  /** Plan-TOW (kg). */
  planned_tow_kg: number | null;
  /** Plan-LDW (kg). */
  planned_ldw_kg: number | null;
  /** Plan-Route aus dem OFP (ICAO-codiert). Wird im Briefing-Tab
   *  als monospace-string angezeigt. */
  planned_route: string | null;
  /** Geplanter Alternate-Flughafen (ICAO). */
  planned_alternate: string | null;
  // ---- v0.3.0: MAX-Werte für Overweight-Detection ----
  /** Maximum Zero-Fuel Weight (kg). null bei Custom-Subfleets ohne MAX. */
  planned_max_zfw_kg: number | null;
  /** Maximum Takeoff Weight (kg). */
  planned_max_tow_kg: number | null;
  /** Maximum Landing Weight (kg). */
  planned_max_ldw_kg: number | null;
  // ---- v0.3.0: Live-Loadsheet-Werte ----
  /** Aktuelles Block-Fuel im Tank (kg) — live aus dem Sim. */
  sim_fuel_kg: number | null;
  /** Aktuelles ZFW (kg) — live aus dem Sim. null bei Profilen die's nicht melden. */
  sim_zfw_kg: number | null;
  /** Aktuelles Total-Weight (kg) — entspricht TOW während Boarding. */
  sim_tow_kg: number | null;

  // ─── v0.7.19 GAF-707 Accident-Detection ──────────────────────────
  // (QS-R1 Finding 3): UI muss schon waehrend des aktiven Flugs
  // sehen ob ein Accident gelatcht wurde — damit der Flight-End-
  // Dialog die "War das ein Absturz?"-Frage stellen kann statt
  // den PIREP blind zu filen. Spec §AeroACARS Client Tab "Landung"
  // + §Active Flight / Flight End.
  /** True bei Confirmed Accident. Auch Suspected kann den Dialog
   *  triggern (siehe accident_confidence). */
  accident_detected?: boolean;
  /** "sim_crash" | "impact" | "off_airport_impact" */
  accident_kind?: string | null;
  /** "high" (Confirmed) | "medium" (Suspected) */
  accident_confidence?: string | null;
  accident_reasons?: string[];
  /** ISO-8601 UTC */
  accident_at?: string | null;
  /** v0.12.5 (LE3): Vorbefüll-Werte für das manuelle Filing-Formular. */
  manual_filing_defaults: ManualFilingDefaults;
}

/** v0.12.5 (LE7): how an active flight concluded. `ActiveFlightPanel`
 *  reports this up so `CockpitView` can show the *right* banner — a green
 *  success only for a genuine filing, a neutral notice for a discard. */
export type FlightEndOutcome =
  /** A real PIREP was filed — normal flight-end / manual file. */
  | { kind: "filed"; callsign: string; dpt: string; arr: string }
  /** Cancel was requested but the flight was complete → filed instead. */
  | { kind: "filed_instead"; pirep_id: string }
  /** Filing hit a transient error → PIREP sits in the retry queue. */
  | { kind: "queued"; pirep_id: string }
  /** The flight was genuinely discarded — no PIREP. */
  | { kind: "cancelled" }
  /** v0.13.7: phpVMS returned 404 for the running PIREP — server-side
   *  cancellation (inactivity timeout or admin removal). Banner explains
   *  the situation and prompts a manual file. */
  | { kind: "cancelled_remotely" };

/** v0.12.5 (LE3): Mirrors the Rust-side `ManualFilingDefaults`. Seeds the
 *  ManualFileDialog so the pilot only corrects deviations. All values
 *  optional (null = FSM hasn't captured it yet). */
export interface ManualFilingDefaults {
  distance_nm: number | null;
  cruise_level_ft: number | null;
  flight_time_minutes: number | null;
  landing_rate_fpm: number | null;
  /** ISO-8601 UTC */
  block_off_at: string | null;
  /** ISO-8601 UTC */
  block_on_at: string | null;
  block_fuel_kg: number | null;
  /** Fuel remaining in the tank (kg) — the dialog asks "still in tank",
   *  not "used". Backend computes used = block − remaining. */
  remaining_fuel_kg: number | null;
  /** Detected divert ICAO from the DivertHint, null = no divert. */
  divert_to: string | null;
  /** The originally planned arrival ICAO. */
  planned_arr_icao: string;
  /** true when a divert was detected → reason is mandatory. */
  requires_reason: boolean;
}

/** Mirrors the Rust-side `ReleaseNotes` struct. Returned by the
 *  `fetch_release_notes` Tauri command for the in-app "What's new"
 *  modal. `body` is markdown with optional `## 🇩🇪 Deutsch` /
 *  `## 🇬🇧 English` section markers — the modal splits and renders
 *  just the section matching the current i18n locale. */
export interface ReleaseNotes {
  name: string;
  tag_name: string;
  body: string;
  published_at: string;
  html_url: string;
}

/** See `DivertHint` in lib.rs — populated by the FSM when on-ground +
 *  engines-off + far-from-planned-arrival. */
export interface DivertHint {
  /** Best-guess actual landing airport ICAO. May be null when the
   *  local runways DB found nothing within ~50 nmi (private strip,
   *  off-DB military, scenery-only field). UI then falls back to
   *  manual entry. */
  actual_icao: string | null;
  planned_arr_icao: string;
  planned_alt_icao: string | null;
  /** Distance from the touchdown point to the planned arrival, nmi. */
  distance_to_planned_nmi: number;
  /** "alternate" (matched planned alt — high confidence),
   *  "nearest"   (found something else nearby),
   *  "unknown"   (no airport in range — manual override required). */
  kind: "alternate" | "nearest" | "unknown";
}

/** v0.4.1: Snapshot der letzten bekannten Sim-Werte beim Sim-Disconnect.
 *  Wird im Cockpit-Banner angezeigt damit der Pilot weiß wo er nach
 *  dem Sim-Restart re-positionieren muss. */
export interface PausedSnapshot {
  lat: number;
  lon: number;
  heading_deg: number;
  altitude_ft: number;
  fuel_total_kg: number;
  zfw_kg: number | null;
}

export interface AirportInfo {
  icao: string;
  name: string | null;
  lat: number | null;
  lon: number | null;
}

/** PMDG SDK status for the Settings → PMDG Premium Telemetry section.
 *  Mirrors the backend `PmdgStatusDto` from the `pmdg_status` Tauri
 *  command. Drives the "SDK enabled?" hint and the cockpit-tab
 *  premium-telemetry badge.
 *
 *  Phase H.4 / v0.2.0 — Boeing Premium Telemetry. */
export interface PmdgStatus {
  /** "ng3" = PMDG 737, "x777" = PMDG 777. `null` = no PMDG aircraft. */
  variant: "ng3" | "x777" | null;
  /** True once SimConnect ClientData subscription was registered. */
  subscribed: boolean;
  /** True once a packet has actually arrived (= SDK enabled in
   *  the pilot's options ini). */
  ever_received: boolean;
  /** Seconds since last packet, or null if none. */
  stale_secs: number | null;
  /** True when (variant detected, subscribed, no data flowing for >5s)
   *  — the heuristic for "pilot hasn't enabled the SDK yet". */
  looks_like_sdk_disabled: boolean;
}

/** Status of the optional AeroACARS X-Plane Plugin (v0.5.0+
 *  "Premium Mode"). Mirrors the backend `xplane_premium_status`
 *  Tauri command. Drives the "X-PLANE PREMIUM" badge in Settings →
 *  Debug and (eventually) the cockpit tab.
 *
 *  When the pilot has installed the plugin into their X-Plane
 *  `Resources/plugins/AeroACARS/` folder, `active` flips to true
 *  within 100 ms of X-Plane loading the plugin. Until then it
 *  stays false — and the standard RREF UDP path silently handles
 *  every flight, just at lower precision around touchdown. */
export interface XPlanePremiumStatus {
  /** True when we've received a packet within the last 3 s. */
  active: boolean;
  /** True if any premium packet has arrived this session. */
  ever_seen: boolean;
  /** Total packets received since the X-Plane adapter started. */
  packet_count: number;
  /** Last error from the listener (e.g. bind failure). `null`
   *  while the listener is healthy. */
  last_error: string | null;
}

/** v0.7.18 (B-011): Verwaister PIREP auf phpVMS — Pilot kann ihn
 *  über das Settings-Tab cancellen + Bid droppen + Aircraft freigeben.
 *  Spec docs/spec/v0.7.18-orphan-flight-cleanup.md §B-011. */
export interface OrphanFlight {
  pirep_id: string;
  bid_id: number | null;
  /** phpVMS flight_id (alphanumerisch, "flightid_1"). Wird für den
   *  flight_id-Body-Fallback beim Bid-Drop benötigt. */
  flight_id: string | null;
  flight_number: string | null;
  airline_id: number | null;
  dpt_airport: string | null;
  arr_airport: string | null;
  aircraft_id: number | null;
  aircraft_icao: string | null;
  aircraft_registration: string | null;
  /** ISO-8601 UTC timestamp aus phpVMS created_at. */
  started_at: string | null;
  /** Minuten seit started_at, vom Backend berechnet. */
  age_minutes: number | null;
}

/** v0.16.0 (#LAN-Remote): Mirrors the Rust `RemoteServerStatus` returned by
 *  `remote_server_start` / `remote_server_stop` / `remote_server_status`.
 *  Drives the Settings → LAN-Fernbedienung panel (PIN, candidate LAN URLs,
 *  pairing QR). */
export interface RemoteServerStatus {
  running: boolean;
  /** TCP port the server binds (user-configurable, default 8765). */
  port: number;
  /** `http://<lan-ip>:<port>` for every RFC1918 / link-local interface. */
  urls: string[];
  /** 6-digit pairing PIN (also embedded in the QR as `?pin=`). */
  pin: string;
  /** QR of the primary URL + `?pin=<pin>`, as an `<svg>` data-URL. */
  qr_svg: string;
}
