/// Map SimBrief's text fix `<type>` ("apt", "wpt", "vor", "ndb", "ltlg")
/// to phpVMS's numeric `nav_type` for `POST /pireps/{id}/route`. phpVMS
/// is permissive (`nav_type` is `sometimes|int`); when we can't classify
/// a fix we omit the field and let the server render it generically.
fn simbrief_kind_to_nav_type(kind: &str) -> Option<i32> {
    match kind.to_ascii_lowercase().as_str() {
        "wpt" => Some(1),
        "ndb" => Some(2),
        "vor" => Some(3),
        "apt" => Some(4),
        _ => None,
    }
}

/// v0.16.23: Baut den `Vec<RouteWaypoint>` aus SimBrief-Navlog-Fixes
/// — die Form die `POST /api/pireps/{id}/route` erwartet (0-based order,
/// `nav_type` aus `simbrief_kind_to_nav_type`). Aus `flight_start`
/// extrahiert (cf. der Inline-Bau dort), damit `flight_refresh_route_only`
/// BIT-IDENTISCH dieselbe Route postet wie der Flight-Start. Reine
/// Funktion ohne Score-Beruehrung — bewusst NICHT mit der
/// FlightStats-Mutation gemischt.
fn build_route_waypoints(fixes: &[api_client::RouteFix]) -> Vec<api_client::RouteWaypoint> {
    fixes
        .iter()
        .enumerate()
        .map(|(i, fix)| api_client::RouteWaypoint {
            name: fix.ident.clone(),
            order: i as i32,
            nav_type: simbrief_kind_to_nav_type(&fix.kind),
            lat: fix.lat,
            lon: fix.lon,
        })
        .collect()
}

/// Human-readable single-word label for a `FlightPhase`. Used in the
/// `/acars/logs` text we ship to phpVMS. Pilots already see these in
/// the German activity feed; matching them here keeps the PIREP detail
/// consistent with what was on screen during the flight.
fn phase_human_label(phase: FlightPhase) -> &'static str {
    match phase {
        FlightPhase::Preflight => "Preflight",
        FlightPhase::Boarding => "Boarding",
        FlightPhase::Pushback => "Pushback",
        FlightPhase::TaxiOut => "Taxi Out",
        FlightPhase::TakeoffRoll => "Takeoff Roll",
        FlightPhase::Takeoff => "Takeoff",
        FlightPhase::Climb => "Initial Climb",
        FlightPhase::Cruise => "Cruise",
        FlightPhase::Holding => "Holding",
        FlightPhase::Descent => "Descent",
        FlightPhase::Approach => "Approach",
        FlightPhase::Final => "Final",
        FlightPhase::Landing => "Landing",
        FlightPhase::TaxiIn => "Taxi In",
        FlightPhase::BlocksOn => "On Blocks",
        FlightPhase::Arrived => "Arrived",
        FlightPhase::PirepSubmitted => "PIREP Submitted",
    }
}

/// Read a snapshot from whichever adapter is currently active per
/// the persisted `SimKind`. We try the active adapter first; if it
/// hasn't seen any data yet (e.g. just after startup), we still
/// return None — caller is expected to handle "no snapshot yet".
fn current_snapshot(app: &AppHandle) -> Option<SimSnapshot> {
    let state = app.state::<AppState>();
    let kind = read_sim_config(app).kind;
    if kind.is_xplane() {
        let adapter = state.xplane.lock().expect("xplane lock");
        return adapter.snapshot();
    }
    #[cfg(target_os = "windows")]
    {
        if kind.is_msfs() {
            let adapter = state.msfs.lock().expect("msfs lock");
            return adapter.snapshot();
        }
    }
    None
}

/// Drain any pending touchdown event from the AeroACARS X-Plane Plugin
/// (v0.5.0+ premium mode). Returns `Some` exactly once per landing
/// when the plugin is installed and detected a touchdown edge in its
/// flight-loop callback. `None` if the plugin isn't running, or if
/// the active sim isn't X-Plane, or if no touchdown is pending. The
/// touchdown sampler calls this each tick and prefers premium values
/// over its own RREF-derived edge detection when available.
fn current_premium_touchdown(app: &AppHandle) -> Option<sim_xplane::PremiumTouchdown> {
    let state = app.state::<AppState>();
    let kind = read_sim_config(app).kind;
    if !kind.is_xplane() {
        return None;
    }
    let adapter = state.xplane.lock().expect("xplane lock");
    adapter.take_premium_touchdown()
}

// v0.7.13: `current_premium_status` entfernt — war seit der Refactor-Welle
// in v0.6.x ohne Caller. UI greift direkt via `app.state::<AppState>()` auf
// den X-Plane-Premium-Status zu. Audit Q4-2026-05.

/// Update running stats AND step the flight-phase FSM. Returns the new phase
/// when a transition fires, otherwise `None`.
/// v0.5.40: Anti-Flicker für `engines_running`. Aerosoft A340-600 Pro
/// (und etliche andere Add-ons) zappeln den GENERAL ENG COMBUSTION SimVar
/// pro Tick zwischen 0 und N. Die Phase-FSM sieht dann zufällig false und
/// blockiert Pushback→TaxiOut. Hier: gilt als "effektiv laufend" wenn
/// snap.engines_running > 0 ODER der letzte >0-Reading <2 s her ist.
/// v0.5.44: Extrahiert einen ICAO-Aircraft-Code aus dem rohen Sim-String.
///
/// MSFS liefert oft kuriose Werte fuer ATC MODEL — siehe importJsonl.ts
/// cleanAircraftIcao mit dem gleichen Pattern:
///   "ATCCOM.AC_MODEL A321.0.text" → "A321"
///   "BoeingB738" → "B738"
///   "FNX320"  → "A320" (matched in fallback)
///   "A320"   → "A320"
///
/// Regex: \b([A-Z]\d{2,3}|[A-Z]{2,4}\d{0,3})\b — Boeing/Airbus-typische
/// 4-Zeichen-Codes (B738, A320, C172, DA42, ...). Faellt bei keinem Match
/// zurueck auf die ersten 8 Zeichen, gefiltert auf alphanumerisch.
fn extract_icao_code(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Regex on-demand kompilieren — wird selten genutzt, nicht kritisch
    // genug fuer eine static.
    use regex::Regex;
    let re = Regex::new(r"\b([A-Z]\d{2,3}|[A-Z]{2,4}\d{0,3})\b").ok()?;
    if let Some(caps) = re.captures(raw) {
        if let Some(m) = caps.get(1) {
            return Some(m.as_str().to_string());
        }
    }
    // Fallback: erste 8 alphanumerische Chars uppercase
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_uppercase();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// v0.7.19 GAF-707 Accident-Detection (Heuristik-Pfad).
///
/// Spec §Klassifikator-Trigger-Punkte: wird EINMAL pro Touchdown am
/// TD-Edge aufgerufen (= direkt nach `stats.landing_analysis = Some(...)`
/// im Sampler-Loop). NICHT im 50-Hz-Loop.
///
/// Behaviour:
/// - Sim-Event hat schon Confirmed(SimCrash) gelatcht → Reasons mergen,
///   kind bleibt SimCrash (authoritativ).
/// - Suspected ueber bestehenden None → Suspected latchen.
/// - Confirmed ueber bestehenden Suspected → upgraden auf Confirmed.
/// - Confirmed-vs-Confirmed bei Multi-TD (Touch-and-Go): Reasons werden
///   nur ersetzt wenn die neue |vs_at_edge_fpm| HOEHER ist (Worst-Case-
///   Aggregation per Spec §Leitentscheidung 7).
/// Merged einen frisch gemessenen Peak-G-Wert (z. B. `peak_g_post_500ms`
/// aus der Touchdown-Analyse) in `landing_peak_g_force` — NUR nach oben,
/// nie nach unten: ein echter Gear-Compression-Spike darf nie durch einen
/// schwaecheren Nachfolgewert verdraengt werden (dieselbe „haerterer Wert
/// gewinnt"-Doktrin wie in `apply_accident_heuristic`). `None` (keine
/// Messung verfuegbar) aendert nichts.
///
/// Aus zwei Aufrufstellen zusammengezogen (Codex-Folgefund, adversarial,
/// 05.09.2026, fuenfte Runde), damit sie nicht wieder auseinanderlaufen:
/// einmal VOR `apply_accident_heuristic` (damit der Klassifikator den
/// frischen Wert sieht, nicht den Stand von vor diesem Touchdown), einmal
/// danach fuer die uebrigen Konsumenten von `landing_peak_g_force`.
fn peak_g_force_verschmelzen(stats: &mut FlightStats, gemessen: Option<f32>) {
    if let Some(g) = gemessen {
        let bisher = stats.landing_peak_g_force.unwrap_or(0.0);
        if g > bisher {
            stats.landing_peak_g_force = Some(g);
        }
    }
}

fn apply_accident_heuristic(stats: &mut FlightStats, analysis: &serde_json::Value) {
    // v1.6.3: **Die Unfall-Erkennung spuelt nie weich.** Der Score nimmt die
    // Hoehenkurve, weil sie die Wahrheit besser trifft — hier zaehlt der
    // HAERTERE der beiden Messwerte. Damit kann kein Aufprall unter die
    // Schwelle rutschen, egal welche Quelle im Einzelfall danebenliegt
    // (etwa eine geneigte Bahn, bei der die Hoehenkurve das wegfallende
    // Gelaende mitmisst, oder ein gehaltener AGL-Frame, der die Gerade
    // abflacht). Der im selben Release entfernte Entlagungs-Code trug
    // denselben Gedanken als `MSFS_DELAG_MAX_EDGE_ABS_FPM` = "never soften
    // a crash"; der Riegel darf mit ihm nicht verschwinden. Wirkt nur nach
    // oben: ein Wert, der KEINE Schwelle reisst, aendert nichts.
    let haerteste_sinkrate = {
        let kandidaten = [
            ana_f32(&Some(analysis.clone()), "vs_at_edge_fpm"),
            ana_f32(&Some(analysis.clone()), "vs_geometrie_fpm"),
            ana_f32(&Some(analysis.clone()), "vs_simvar_edge_fpm"),
            ana_f32(&Some(analysis.clone()), "vs_geometrie_ungefiltert_fpm"),
        ];
        kandidaten
            .into_iter()
            .flatten()
            .filter(|v| v.is_finite() && *v < 0.0)
            .fold(None::<f32>, |acc, v| {
                Some(match acc {
                    Some(a) if a.abs() >= v.abs() => a,
                    _ => v,
                })
            })
            .or_else(|| ana_f32(&Some(analysis.clone()), "vs_at_edge_fpm"))
    };
    let input = accident::AccidentHeuristicInput {
        vs_at_edge_fpm: haerteste_sinkrate,
        // v0.12.3 (LE10): Accident-/Crash-Erkennung nutzt BEWUSST den
        // rohen `peak_g_load`, NICHT den EMA-geglätteten `scored_g`.
        // Crash-Detektion ist Extremwert-Detektion — der schärfste Wert
        // ist der relevante; die EMA-Glättung (faire Pilot-Bewertung)
        // würde einen Grenzfall-Crash unter die Schwelle drücken. Nicht
        // versehentlich auf `scored_g` umstellen.
        //
        // v1.6.2: der Extremwert bleibt (keine Glättung), wird aber auf die
        // Referenz-Messkette umgerechnet — die Schwellen hier (3,0 g
        // bestätigt / 2,1 g Verdacht) sind an MSFS-Werten gewachsen, und
        // X-Planes ungefilterter Kanal liest bei gleicher Physik gut das
        // Doppelte über 1 g. Ungerechnet bekäme jede X-Plane-Landung mit
        // 1500 fpm rechnerisch 3,9 g und damit den Unfall-Banner, während
        // dieselbe Landung auf MSFS bei 2,3 g unauffällig bleibt
        // (QS-Befund v1.6.2). Gleiche Physik, gleiches Urteil.
        peak_g_load: stats
            .landing_peak_g_force
            .map(|g| g_auf_referenzkette(g, stats.landing_simulator)),
        // v1.6.3: NIE weichspuelen. Der Score nimmt die Hoehenkurve, weil
        // sie die Wahrheit besser trifft — die Unfall-Erkennung nimmt den
        // HAERTEREN der beiden Messwerte. Damit kann kein Aufprall unter
        // die Schwelle rutschen, egal welche Quelle im Einzelfall daneben
        // liegt (etwa eine geneigte Bahn, bei der die Hoehenkurve das
        // wegfallende Gelaende mitmisst). Der geloeschte Entlagungs-Code
        // trug denselben Gedanken als `MSFS_DELAG_MAX_EDGE_ABS_FPM`; der
        // Riegel darf mit ihm nicht verschwinden.
        sideslip_deg: stats.touchdown_sideslip_deg,
        landing_wing_strike_severity_pct: stats.landing_wing_strike_severity_pct,
        approach_stall_warning_count: Some(stats.approach_stall_warning_count),
        runway_match_found: Some(stats.runway_match.is_some()),
        rollout_distance_m: stats.rollout_distance_m.map(|m| m as f32),
    };
    let new_vs_abs = input.vs_at_edge_fpm.map(|v| v.abs()).unwrap_or(0.0);
    let decision = accident::classify_accident_heuristic(&input);

    match decision {
        accident::AccidentDecision::None => {}
        accident::AccidentDecision::Suspected { kind, reasons } => {
            if !stats.accident_detected && stats.accident_confidence.is_none() {
                // Erste Suspected-Klassifikation. Nicht als accident_detected
                // (das bleibt fuer Confirmed reserviert), aber Felder zur
                // UI/Webapp-Banner-Anzeige setzen.
                stats.accident_kind = Some(kind.as_wire_str().into());
                stats.accident_confidence = Some("medium".into());
                stats.accident_at = stats.landing_at;
                stats.accident_reasons.extend(reasons);
                tracing::warn!(
                    kind = kind.as_wire_str(),
                    reasons = ?stats.accident_reasons,
                    "v0.7.19: heuristic suspected accident"
                );
            }
            // Wenn schon Confirmed → ignorieren (Suspected < Confirmed).
        }
        accident::AccidentDecision::Confirmed { kind, reasons } => {
            let was_sim_crash = stats.accident_kind.as_deref()
                == Some(accident::AccidentKind::SimCrash.as_wire_str());

            if !stats.accident_detected {
                // First Confirmed (entweder vom None oder vom Suspected upgrade).
                stats.accident_detected = true;
                stats.accident_kind = Some(kind.as_wire_str().into());
                stats.accident_confidence = Some("high".into());
                stats.accident_at = stats.accident_at.or(stats.landing_at);
                stats.accident_reasons.extend(reasons);
                tracing::warn!(
                    kind = kind.as_wire_str(),
                    reasons = ?stats.accident_reasons,
                    "v0.7.19: heuristic confirmed accident"
                );
            } else if was_sim_crash {
                // Sim-Event hat schon Confirmed(SimCrash) gelatcht. Kind
                // bleibt; Heuristik-Reasons mergen.
                stats.accident_reasons.extend(reasons);
                tracing::info!(
                    heuristic_kind = kind.as_wire_str(),
                    "v0.7.19: heuristic reasons merged into sim-crash latch"
                );
            } else {
                // Multi-TD Worst-Case-Aggregation: nur ersetzen wenn die
                // neue Touchdown-|vs| hoeher ist (Spec §Leitentscheidung 7
                // Tie-Breaker).
                let prev_vs_abs = stats
                    .accident_reasons
                    .iter()
                    .find_map(|r| r.strip_prefix("vs_at_edge_fpm="))
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v.abs())
                    .unwrap_or(0.0);
                if new_vs_abs > prev_vs_abs {
                    stats.accident_kind = Some(kind.as_wire_str().into());
                    stats.accident_reasons = reasons;
                    stats.accident_at = stats.landing_at;
                    tracing::warn!(
                        kind = kind.as_wire_str(),
                        prev_vs_abs,
                        new_vs_abs,
                        "v0.7.19: multi-touchdown worst-case replaced earlier accident"
                    );
                }
            }
        }
    }
}

fn engines_effectively_running(
    stats: &FlightStats,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
) -> bool {
    if snap.engines_running > 0 {
        return true;
    }
    stats
        .last_engines_running_above_zero_at
        .map(|t| (now - t).num_milliseconds() < 2000)
        .unwrap_or(false)
}

/// v0.16.9: der Takeoff-Latch der drei →Takeoff-Übergänge, an EINER Stelle.
/// Semantik exakt wie die bisherigen Inline-Blöcke (Overwrite, Sampler-
/// bevorzugte Attitude, TOTAL-WEIGHT-Fallback auf ZFW+Fuel) — nur extrahiert,
/// damit Latch-Logik nie wieder pro Call-Site driftet.
/// Gross weight at this snapshot: prefer TOTAL WEIGHT (fuel + payload +
/// empty); fall back to ZFW + fuel only when the SimVar isn't wired.
/// Shared by both takeoff-latch variants so the fallback can never drift.
fn takeoff_weight_from_snapshot(snap: &SimSnapshot) -> Option<f64> {
    if let Some(tw) = snap.total_weight_kg {
        return Some(tw as f64);
    }
    let zfw = snap.zfw_kg.unwrap_or(0.0);
    let weight = zfw as f64 + snap.fuel_total_kg as f64;
    (weight > 0.0).then_some(weight)
}

fn latch_takeoff_stats(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    // VOR dem Ueberschreiben des Abhebe-Tankstands: War das schon das
    // zweite Abheben, gehoeren die Boden-Marken zum ersten Abschnitt.
    sprit_neuer_abschnitt_beim_abheben(stats);
    stats.takeoff_at = Some(now);
    stats.takeoff_fuel_kg = Some(snap.fuel_total_kg);
    sprit_beim_abheben_scharf(stats);
    // v0.5.16: capture pitch + bank for tail-strike / wing-strike
    // maintenance detection (DisposableSpecial dmaintenance reads these
    // as numeric custom fields).
    // v0.5.24: prefer the 50Hz-sampler value (Wheels-Up-frame-genau)
    // ueber den Streamer-Tick-Snapshot. Bei tail-strike-empfindlichen
    // Aircraft wie A321 spart das 2-3° Pitch-Drift gegen den
    // 3-30s-spaeter-Tick.
    stats.takeoff_pitch_deg = stats.sampler_takeoff_pitch_deg.or(Some(snap.pitch_deg));
    stats.takeoff_bank_deg = stats.sampler_takeoff_bank_deg.or(Some(snap.bank_deg));
    if let Some(weight) = takeoff_weight_from_snapshot(snap) {
        stats.takeoff_weight_kg = Some(weight);
    }
}

/// Start oder Weiterflug-Start: Beim Weiterflug vom Boden (siehe
/// `FlightStats::weiterflug_ab_boden_seit`) gelten Abflugzeit, Startgewicht
/// und Lage weiter; der Abhebe-Sprit bleibt leer (neu geladener Tank).
fn takeoff_latch_oder_weiterflug(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    if stats.weiterflug_ab_boden_seit.take().is_some() {
        sprit_neuer_abschnitt_beim_abheben(stats);
        sprit_beim_abheben_scharf(stats);
        stats
            .pending_acars_logs
            .push("Weiterflug nach Sim-Neustart — abgehoben".to_string());
    } else {
        latch_takeoff_stats(stats, snap, now);
    }
}

/// Der Teil des Start-Latch, der nur die Sprit-Auswertung neu beginnt.
fn sprit_beim_abheben_scharf(stats: &mut FlightStats) {
    // v1.7.35: ein neuer Start heisst ein neuer Vergleichspunkt.
    stats.sprit_vergleichspunkt_kg = None;
    stats.sprit_vergleichspunkt_odo_nm = None;
    stats.sprit_zeit_unter_schwelle_s = 0.0;
    stats.sprit_auswertung = None;
    // Auch die Scharfschaltung und die Hoechsthoehe: sonst gilt das Fenster
    // beim zweiten Abschnitt einer Zwischenlandung sofort als scharf und
    // die Schutzwirkung entfaellt.
    stats.sprit_plan_bis_vergleichspunkt_kg = None;
    stats.sprit_plan_rest_ab_vergleichspunkt_nm = None;
    stats.sprit_hoechste_hoehe_ft = None;
    // Ohne das bliebe `sprit_tick` fuer den ganzen zweiten Abschnitt stumm
    // (es steigt bei gesetztem Landesprit aus) — Reserve und Leiter kaemen,
    // Phasen und Zeit nicht.
    stats.landing_fuel_kg = None;
    stats.landing_fuel_puffer = false;
    stats.sprit_landung_odo_nm = None;
    stats.sprit_plan_trip_am_vergleichspunkt_kg = None;
    // Ein echtes Abheben — der Einstieg in der Luft gilt nicht mehr.
    stats.sprit_einstieg_in_der_luft = false;
    stats.sprit_einstieg_ort = None;
    stats.sprit_plan_bis_einstieg_kg = None;
}

/// v0.16.9: Takeoff-Latch-Sicherheitsnetz für Pfade, die airborne werden,
/// OHNE durch `FlightPhase::Takeoff` zu gehen — heute genau das
/// v0.13.17-Airborne-Rescue (Live-Flug BCS8: totes vbz-Signal → Pushback nie
/// verlassen → Rescue forcte Pushback→Climb → `takeoff_fuel_kg` blieb None →
/// OFP-Fuel-Sub-Score „no_actual_burn"-skipped).
///
/// Unterschiede zum echten Takeoff-Latch — bewusst:
/// * `get_or_insert`-Semantik: überschreibt NIE bereits gelatchte Werte.
/// * `takeoff_at`: bevorzugt den frame-genauen 50Hz-Sampler-Edge
///   (`sampler_takeoff_at`, falls der Sampler den Wheels-up sah), sonst jetzt.
/// * Pitch/Bank NUR vom Sampler (echter Wheels-up-Frame): der Rescue-Snapshot
///   steckt schon tief im Climb — dessen ~15° Pitch würde der Tail-Strike-
///   Maintenance-Erkennung einen falschen „Takeoff-Pitch" füttern. Lieber
///   leer als falsch.
/// * Fuel/Weight vom Rescue-Snapshot (≤ 500 ft AGL): Burn seit Rotation ist
///   < 1 % des Trip-Burns — für die OFP-Treue-Bewertung präzise genug, und
///   unendlich besser als „nicht bewertbar".
fn latch_takeoff_stats_if_missing(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    // Weiterflug vom Boden: Abflugwerte bleiben, der Sprit bleibt ungemessen.
    if stats.weiterflug_ab_boden_seit.is_some() {
        sprit_beim_abheben_scharf(stats);
        return;
    }
    if stats.takeoff_at.is_none() {
        stats.takeoff_at = Some(stats.sampler_takeoff_at.unwrap_or(now));
    }
    if stats.takeoff_fuel_kg.is_none() {
        stats.takeoff_fuel_kg = Some(snap.fuel_total_kg);
        // v1.7.36: Der Abhebe-Tankstand stammt von HIER, nicht vom Abheben.
        // Beim Rescue kurz nach dem Start ist das ein paar hundert Meter
        // Steigflug; bei einem Client, der erst im Reiseflug gestartet
        // wurde, der halbe Flug. „Bis Sinkflug" rechnet darum nur ab dem
        // Plan an diesem Ort (siehe `SpritEingang::einstieg_in_der_luft`).
        //
        // Hier nur der ORT. Der Plan dazu wird am Messpunkt gelesen, aus
        // DEMSELBEN Navlog wie der Plan-Anteil dort — und die Route darf
        // bis dahin noch nachgeladen werden.
        stats.sprit_einstieg_in_der_luft = true;
        stats.sprit_einstieg_ort = Some((snap.lat, snap.lon));
    }
    if stats.takeoff_pitch_deg.is_none() {
        stats.takeoff_pitch_deg = stats.sampler_takeoff_pitch_deg;
    }
    if stats.takeoff_bank_deg.is_none() {
        stats.takeoff_bank_deg = stats.sampler_takeoff_bank_deg;
    }
    if stats.takeoff_weight_kg.is_none() {
        stats.takeoff_weight_kg = takeoff_weight_from_snapshot(snap);
    }
}

/// v0.13.17 Airborne-Rescue-Bedingung (rein, testbar). True, wenn die FSM in
/// einer Boden-/Pre-Takeoff-Phase steht, das Flugzeug aber klar in der Luft
/// ist (nicht am Boden + AGL > 500 ft) und der Tick kein Reload-/Slew-Glitch
/// ist. Dann zwingt `step_flight` die Phase auf Climb — damit ein Addon mit
/// kaputten Engine-SimVars (engines_running bleibt 0 → Pushback→Takeoff feuert
/// nie) nicht den ganzen Flug in einer Boden-Phase festhaengt.
fn phase_stuck_on_ground_while_airborne(
    phase: FlightPhase,
    on_ground: bool,
    altitude_agl_ft: f64,
    reload_gap_suspect: bool,
) -> bool {
    let ground_phase = matches!(
        phase,
        FlightPhase::Preflight
            | FlightPhase::Boarding
            | FlightPhase::Pushback
            | FlightPhase::TaxiOut
            | FlightPhase::TakeoffRoll
    );
    ground_phase && !on_ground && altitude_agl_ft > 500.0 && !reload_gap_suspect
}

/// v0.15.20: True für eine FLUG-Phase (in der Luft) — Gegenstück zu Boden-/
/// Departure-Phasen (Boarding/Pushback/TaxiOut/TakeoffRoll/Takeoff/TaxiIn/
/// BlocksOn/Arrived/PirepSubmitted).
///
/// Maßgeblich dafür, ob ein realer Luft→Boden-Edge als Touchdown akzeptiert
/// wird — an ALLEN drei Erfassungsstellen (Sampler-RREF-Edge, Sampler-Premium-
/// Plugin, FSM-Capture). Vorher war das auf `Approach|Final|Landing` beschränkt,
/// wodurch ein Touchdown bei fehl-phasierter FSM (Go-Around → in „Climb" hängen,
/// OWx9) überall verworfen wurde.
///
/// Der Phantom-Touchdown-Schutz (Gear-Force-Wackler beim Taxi auf unebenem
/// Boden) bleibt darüber **vollständig** erhalten: in einer Flug-Phase ist
/// `gear_normal_force_n` durchgehend entlastet → es kann gar kein Gear-Bump-Edge
/// (`prev_in_air=true → on_ground`) entstehen; die Wackler passieren nur am
/// Boden (Taxi), und Boden-Phasen sind hier ausgeschlossen.
fn is_flight_phase(phase: FlightPhase) -> bool {
    matches!(
        phase,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Descent
            | FlightPhase::Holding
            | FlightPhase::Approach
            | FlightPhase::Final
            | FlightPhase::Landing
    )
}

/// Whether the 50 Hz touchdown sampler may accept an air→ground edge in this
/// phase. Flight phases (Climb…Landing) always qualify. `Takeoff` ALSO
/// qualifies once the aircraft has genuinely flown (`was_airborne`, armed at
/// 50 ft AGL): this is the durable fix for short / low VFR bush hops that land
/// before climbing through the 500 ft Takeoff→Climb gate and so are still in
/// `Takeoff` phase at touchdown (real case GSG309 OR72→OR35, peaked 357 ft,
/// phase arc Takeoff→Arrived → the edge was previously blocked → no touchdown).
///
/// Phantom safety: ground/taxi phases (Boarding/TaxiOut/TaxiIn/Arrived) stay
/// excluded, so a taxi gear-bump is never a candidate. Within `Takeoff` the
/// aircraft cannot be on the ground until it lands, so the FIRST air→ground
/// edge is the real touchdown; the once-per-landing guard
/// (`sampler_touchdown_at.is_none()`) then blocks any rollout bump after it.
/// A spurious mid-air on_ground glitch is still rejected by `validate_candidate`
/// (low-AGL-persistence + sustained-ground fail at altitude / on a single tick).
fn is_touchdown_capturable_phase(phase: FlightPhase, was_airborne: bool) -> bool {
    is_flight_phase(phase) || (matches!(phase, FlightPhase::Takeoff) && was_airborne)
}

/// Darf der Sampler jetzt einen NEUEN Aufsetz-Kandidaten aufnehmen (Flanke
/// oder Plugin-Meldung)? Nur in einer Flugphase und nicht, solange die
/// Aufzeichnung pausiert ist — Sim-Pause (`snap.paused`) oder ACARS-Pause
/// (`paused_since`, auch nach Sim-Trennung). Die Phase bleibt in der Pause
/// eine Flugphase (DLA 8260: HOLDING), sie allein schuetzt nicht.
///
/// Ein schon laufender Kandidat wird hier NICHT verworfen: Drueckt der
/// Pilot Esc kurz nach einem echten Aufsetzen, soll die Landung bleiben.
fn aufsetz_kandidat_zulaessig(stats: &FlightStats, sim_pausiert: bool) -> bool {
    is_touchdown_capturable_phase(stats.phase, stats.was_airborne)
        && stats.paused_since.is_none()
        && !sim_pausiert
}

/// Ruht die Aufsetzerkennung, weil der Flug nach einem Sim-Neuladen mitten
/// im Flug pausiert ist (Stream F, `was_just_resumed`)? Dann wirft sie einen
/// offenen Kandidaten und den Messpuffer weg, damit nichts ueber die Pause
/// hinweg bestaetigt wird oder nach dem Fortsetzen im Aufsetzfenster landet.
///
/// Anlass DLA 8260 (12.10.2026, Client 2.0.12): Nach „Sim-Reload erkannt"
/// flog der Pilot im Sim Platzrunden in EDDN — rund zwoelf davon wurden als
/// Landung des pausierten Flugs bestaetigt, jede ueberschrieb die Landerate.
/// Sieht der Aufsetz-Abtaster selbst einen Sim-Neuladen (gleiches Kriterium
/// wie Stream F: aus der Luft plötzlich am Boden, tausende Fuß tiefer)?
fn sampler_sieht_neuladen(phase: FlightPhase, vorher: Option<(bool, f64)>, am_boden: bool, hoehe_ft: f64) -> bool {
    vorher.is_some_and(|(boden, alt)| is_mid_session_sim_crash_recovery(phase, boden, alt, am_boden, hoehe_ft))
}

fn aufsetz_erkennung_ruht_nach_reload(stats: &mut FlightStats, sim_neu_geladen: bool) -> bool {
    if !sim_neu_geladen {
        return false;
    }
    stats.pending_td_at = None;
    stats.pending_td_premium_vs = None;
    stats.pending_td_premium_g = None;
    stats.pending_td_premium_gesehen = false;
    stats.snapshot_buffer.clear();
    true
}

/// Takeoff → Climb AGL gate. A rotorcraft routinely flies an entire leg below
/// the 500 ft fixed-wing gate, which would strand it in `Takeoff` — a phase
/// excluded from [`is_flight_phase`], so the touchdown sampler never captures
/// its landing. A low gate moves it into Climb (a capturable flight phase)
/// once clearly airborne and translating. Fixed-wing keeps 500 ft.
fn takeoff_climb_gate_ft(category: aircraft_category::AircraftCategory) -> f64 {
    if category.is_rotorcraft() {
        60.0
    } else {
        500.0
    }
}

// Sampler touchdown-edge gear-force threshold (N). Below this the gear is
// considered unloaded (in the air); X-Plane `fnrml_gear` spikes far above it
// on a wheel touchdown.
// Wert seit v3: bundle/grenzwerte/landung.json -> sampler_gear_touchdown_threshold_n (v2.1.24: 1.0)

// Seaplane / amphibian water-contact AGL (ft): at or below this height above
// the water surface the floats / hull are settling onto the water.
// Wert seit v3: bundle/grenzwerte/landung.json -> sampler_water_contact_agl_ft (v2.1.24: 3.0)

/// Category-aware "in air" signal for the 50 Hz touchdown sampler.
///
/// For a seaplane / amphibian on water `on_ground` stays false and there is no
/// gear-force, so the wheeled touchdown edge never fires. For those categories
/// — and only while off the runway (`on_ground == false`) — this uses height
/// above the water surface instead: the aircraft counts as "in air" while above
/// [`SAMPLER_WATER_CONTACT_AGL_FT`], so the edge fires when the floats settle
/// onto the water. Everything else (fixed-wing, a helicopter on skids/wheels,
/// any aircraft whose `on_ground` / gear-force is meaningful) uses the unchanged
/// wheeled signal.
///
/// This only produces the candidate EDGE; the touchdown_v2 validation 1.1 s
/// later still gates it (the seaplane path anchors on sustained low-AGL, so a
/// wave bounce / momentary dip is dropped as a FalseEdge). The water branch is
/// gated on a water-capable category, so it can never fire for a fixed-wing or
/// helicopter — those stay byte-identical to the previous behaviour.
fn sampler_in_air_now(
    category: aircraft_category::AircraftCategory,
    on_ground: bool,
    altitude_agl_ft: f64,
    gear_normal_force_n: Option<f32>,
) -> bool {
    if category.water_capable() && !on_ground {
        altitude_agl_ft > crate::grenzwerte::grenzwerte().landung.sampler_water_contact_agl_ft.wert
    } else {
        match gear_normal_force_n {
            Some(force) => force < crate::grenzwerte::grenzwerte().landung.sampler_gear_touchdown_threshold_n.wert,
            None => !on_ground,
        }
    }
}

/// v0.15.24: First on-ground sample in the 50 Hz snapshot buffer — the
/// touchdown moment as the sampler recorded it. Shared by the FSM Landing
/// arm and the sampler-validation stamping path so both read the SAME
/// TD-instant values. `None` when the buffer is empty (resumed flight) or
/// holds no on-ground sample yet — callers fall back to the live snap.
fn touchdown_buffer_sample(stats: &FlightStats) -> Option<TelemetrySample> {
    stats.snapshot_buffer.iter().find(|s| s.on_ground).cloned()
}

// Wie viel spaeter als das bestaetigte Aufsetzen die Pufferprobe liegen
// darf, um noch als Aufsetzmoment zu gelten.
// Wert seit v3: bundle/grenzwerte/landung.json -> aufsetzprobe_toleranz_ms (v2.1.24: 2_000)

/// Behaelt der FSM-Landing-Arm den Stempel des Abtasters? Ja, wenn der
/// Abtaster das Aufsetzen schon bestaetigt und gestempelt hat und der Puffer
/// nur noch Bodenproben von DANACH haelt oder leer ist — nach einer Pause
/// oder einem geleerten Puffer (LTU 154, 26.10.2026). Liegt die Probe davor
/// (erster Kontakt eines Hopsers), stempelt der Arm wie bisher neu
/// (Codex-Pruefung 26.10.2026).
fn abtaster_stempel_bleibt(stats: &FlightStats, td_buf_sample: Option<&TelemetrySample>) -> bool {
    let Some(td_at) = stats.sampler_touchdown_at else {
        return false;
    };
    if gespeicherter_aufsetzpunkt(stats).is_none() {
        return false;
    }
    td_buf_sample.is_none_or(|s| (s.at - td_at).num_milliseconds() > crate::grenzwerte::grenzwerte().landung.aufsetzprobe_toleranz_ms.wert)
}

#[cfg(test)]
#[path = "tests_lib/abtaster_stempel_tests.rs"]
mod abtaster_stempel_tests;

/// v0.15.24: Touchdown-METADATA stamp — extracted verbatim from the FSM
/// Landing arm in `step_flight` so the validated sampler touchdown can stamp
/// the same fields when the FSM never reaches Landing. Short/low VFR bush
/// hops (GSG312, 97OG) land via the 50 Hz sampler while the FSM is still in
/// Takeoff/Climb: the touchdown was captured + scored + published
/// (v0.15.23), but landing IAS/pitch/bank/heading/speed/groundspeed/fuel/
/// coords/profile/sideslip/wind were only ever stamped inside the FSM
/// Landing arm — so the PIREP report silently lost all of them
/// (`build_pirep_payload` omits `None` stats).
///
/// Behaviour-preserving for the FSM path: identical fallback chains, in the
/// identical order — buffered TD-moment sample → MSFS-latched touchdown
/// SimVars → live snap. The `snap` fallback is the worst case (post-rollout
/// values), kept only for resumed flights where the buffer is empty.
///
/// `actual_td_at` anchors the profile timestamps; pass the same TD instant
/// the caller uses for `landing_at` (FSM arm) / `sampler_touchdown_at`
/// (sampler-validation path) so `t_ms = 0` is the contact frame.
/// Simulator-Kennung aus dem Snapshot — dieselben Strings, die der
/// FSM-Landing-Arm stempelt. Muss hier mitlaufen, weil kurze VFR-/Busch-
/// Hops den FSM-Arm nie erreichen und die G-Fairness-Normierung sonst
/// still ausfaellt (QS-Befund v1.6.2).
fn simulator_kennung(snap: &SimSnapshot) -> &'static str {
    match snap.simulator {
        Simulator::Msfs2020 | Simulator::Msfs2024 => "msfs",
        Simulator::XPlane11 | Simulator::XPlane12 => "xplane",
        Simulator::Other => "other",
    }
}

fn stamp_touchdown_metadata(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    actual_td_at: DateTime<Utc>,
    td_buf_sample: Option<&TelemetrySample>,
) {
    // Simulator-Kennung: identisch zum FSM-Landing-Arm, aber auch fuer die
    // Sampler-Landungen (kurze VFR-/Busch-Hops erreichen den FSM-Arm nie).
    // Ohne sie faellt die G-Fairness-Normierung still aus.
    if stats.landing_simulator.is_none() {
        stats.landing_simulator = Some(simulator_kennung(snap));
    }
    let kategorie = aircraft_category::resolve_category(
        snap.aircraft_icao.as_deref(),
        snap.gear_is_skid,
        snap.gear_is_floats,
        snap.gear_is_wheels,
        snap.water_rudder_present,
    );
    if kategorie.is_non_conventional() {
        stats.landing_nicht_konventionell = true;
    }
    // v3 (Kap. 5.2): `meta` des Eingabe-Dokuments. Simfassung: X-Plane aus
    // dem Plugin (`sim_version`), MSFS aus der SimConnect-Anmeldung
    // (`sim_kennung`, „Name Haupt.Neben“); ohne beides `null`.
    stats.landing_sim_meta = Some(serde_json::json!({
        "sim": snap.simulator,
        "sim_fassung": snap.sim_version.clone().or_else(|| stats.sim_kennung.clone()),
        "profil": snap.aircraft_profile,
        "profil_variante": snap.pmdg.as_ref().map(|p| p.variant_label.clone()),
        "kategorie": kategorie.as_tag(),
    }));

    // IAS / pitch / heading: prefer the buffered sample
    // at the actual TD moment (see `td_buf_sample`).
    // Fall back chain — buffered sample → MSFS-latched
    // touchdown SimVars → live snap. The `snap` fallback
    // is the worst case (post-rollout values), kept only
    // for resumed flights where the buffer is empty.
    stats.landing_pitch_deg = Some(
        snap.touchdown_pitch_deg
            .or_else(|| td_buf_sample.map(|s| s.pitch_deg))
            .unwrap_or(snap.pitch_deg),
    );
    // v0.5.16: bank at touchdown for wing-strike maintenance
    // detection. MSFS latches bank too via the touchdown
    // SimVar set; X-Plane via the buffered sample. Live
    // snap as last-resort fallback (resumed flights).
    stats.landing_bank_deg = Some(
        snap.touchdown_bank_deg
            .or_else(|| td_buf_sample.map(|s| s.bank_deg))
            .unwrap_or(snap.bank_deg),
    );
    // Heading capture: MSFS gives us a magnetic-heading
    // touchdown latch; the buffer carries true-heading
    // only. When we fall back to the buffer we approximate
    // magnetic from true via the live magvar delta
    // (true−magnetic at the streamer tick) — small error
    // since magvar doesn't change in 5 s of rollout.
    stats.landing_heading_deg = Some(
        snap.touchdown_heading_mag_deg
            .or_else(|| {
                td_buf_sample.map(|s| {
                    let magvar = snap.heading_deg_true - snap.heading_deg_magnetic;
                    s.heading_true_deg - magvar
                })
            })
            .unwrap_or(snap.heading_deg_magnetic),
    );
    stats.landing_speed_kt = Some(
        td_buf_sample
            .map(|s| s.indicated_airspeed_kt)
            .unwrap_or(snap.indicated_airspeed_kt),
    );
    // v0.5.17: groundspeed at touchdown (parallel to IAS).
    // Buffered sample preferred — live snap is the post-
    // rollout fallback for resumed flights only.
    stats.landing_groundspeed_kt = Some(
        td_buf_sample
            .map(|s| s.groundspeed_kt)
            .unwrap_or(snap.groundspeed_kt),
    );
    // v1.6.3-QS: gemessene Eigengeschwindigkeit. Der Aufsetz-Puffer traegt
    // sie nicht, der Schnappschuss dieses Augenblicks schon — und beim
    // Aufsetzen ist genau das der richtige Zeitpunkt.
    // Aus dem Aufsetz-Puffer, nicht aus dem Schnappschuss — wie jede andere
    // Groesse hier. Der Schnappschuss zeigt beim Stempeln schon das
    // Ausrollen: im Mittel 0,27 s danach (harmlos, rund 0,4 kt), im
    // schlechtesten Prozent aber ueber vier Sekunden. Dann waere der Nenner
    // des Vorhaltewinkels mehrere Knoten zu klein und die Gutschrift zu
    // grosszuegig (QS-Befund Runde 4).
    let tas = td_buf_sample
        .map(|s| s.true_airspeed_kt)
        .filter(|v| v.is_finite() && *v > 0.0)
        .or(Some(snap.true_airspeed_kt).filter(|v| v.is_finite() && *v > 0.0));
    if let Some(v) = tas {
        stats.landing_true_airspeed_kt = Some(v);
    }
    // v1.7.36: Landesprit aus dem Aufsetz-Puffer — wie jede andere Groesse
    // hier. Bis v1.7.35 war er das EINZIGE Landefeld aus dem Schnappschuss,
    // weil der Puffer kein Sprit-Feld hatte. Das schlug bis zu fuenf
    // Sekunden Umkehrschub dem Anflug zu, obwohl das Flugzeug da laengst am
    // Boden war — auf 16 770 kg ein Promille, auf den Rollsprit nach der
    // Landung (Median 90 kg) ueber 20 %.
    //
    // **Der Puffer schlaegt den Schnappschuss, unabhaengig von der
    // Reihenfolge der Stempel.** Der Puffer ist ein 5-s-Ring: Beim zweiten
    // Stempel aus der FSM kann die erste Bodenprobe schon herausgerollt
    // sein, oder der Pfad uebergibt gar keinen Puffer (Resume). Ohne diese
    // Regel wuerde der gute Wert des ersten Stempels dann still durch den
    // schlechteren Schnappschuss des zweiten ersetzt.
    //
    // `> 0.0`, nicht `>= 0.0`: Ein Add-on mit toter Sprit-SimVar liefert
    // 0,0 kg. Mit `>=` waere das ein gueltiger Puffer-Wert, der einen
    // brauchbaren Schnappschuss verdraengt — ein Flugzeug landet aber nicht
    // mit leeren Tanks. Lieber der spaetere echte Wert als ein frueher
    // falscher (QS-Befund R6, 18.09.2026).
    let landesprit_aus_puffer = td_buf_sample
        .map(|s| s.fuel_kg)
        .filter(|v| v.is_finite() && *v > 0.0);
    let landesprit_neu = match (landesprit_aus_puffer, stats.landing_fuel_puffer) {
        // Dieser Stempel hat einen Puffer-Wert — er gilt.
        (Some(v), _) => {
            stats.landing_fuel_puffer = true;
            Some(v)
        }
        // Kein Puffer, aber ein frueherer Stempel hatte einen: der bleibt.
        (None, true) => stats.landing_fuel_kg,
        // Nie ein Puffer-Wert — der Schnappschuss ist der einzige Anhalt.
        (None, false) => Some(snap.fuel_total_kg),
    };
    // Paritaet: Der Stempel kann zweimal laufen. Aendert sich dabei der
    // Landesprit, gehoert die Sprit-Auswertung neu gebildet; sonst truege
    // sie den Wert des ersten Ticks, waehrend die Kachel „Landesprit" den
    // zweiten zeigt — zwei Zahlen fuer dieselbe Sache im selben Tab.
    //
    // „Einmal rechnen" bleibt gewahrt: Beide Stempel gehoeren zu DERSELBEN
    // Landung, und der letzte gilt. Der Rohwert wird dabei NICHT gerundet —
    // die Rundung lebt allein in der Auswertung, fuer die Anzeige.
    if stats.landing_fuel_kg.is_some() && stats.landing_fuel_kg != landesprit_neu {
        stats.sprit_auswertung = None;
    }
    stats.landing_fuel_kg = landesprit_neu;
    // Die Anflugstrecke endet beim ERSTEN Stempel — der zweite kommt aus der
    // FSM, womoeglich schon ein Stueck weiter im Ausrollen.
    if stats.sprit_landung_odo_nm.is_none() {
        stats.sprit_landung_odo_nm = Some(stats.distance_nm);
    }

    // ---- Tier 2/3 BeatMyLanding-aligned extras ----

    // Position + true heading at the touchdown edge — used
    // for runway lookup and disambiguation between parallel
    // runway pairs (08L/08R) via heading match.
    //
    // Pulls from the buffered TD-moment sample (same
    // reasoning as the IAS / pitch / heading capture
    // above): `snap` is up to 5 s late on the streamer
    // tick, time enough for a typical airliner to roll
    // 50–100 m down the runway and drift left/right via
    // nose-wheel correction — the centerline offset
    // sign would invert if the pilot touched down 3 m
    // left and then drifted through center to right of
    // centerline during the rollout. Live bug 2026-05-03
    // (MKJS RWY 07: pilot reported 3 m left, app showed
    // 2.7 m right). Buffer fallback to `snap` only when
    // the buffer is empty (resumed flight).
    stats.landing_lat = Some(td_buf_sample.map(|s| s.lat).unwrap_or(snap.lat));
    stats.landing_lon = Some(td_buf_sample.map(|s| s.lon).unwrap_or(snap.lon));
    // v1.7.35: die eine Sprit-Rechnung — hier und NICHT frueher. Sie friert
    // ein, und die Ausweich-Erkennung darin liest `landing_lat`/`landing_lon`.
    // Stand der Aufruf oben beim Landesprit, sah sie immer eine leere Position
    // und jeder Ausweichflug wurde als Normalflug eingefroren (QS-Runde 4).
    sprit_auswertung_einmal(stats);
    stats.landing_heading_true_deg = Some(
        td_buf_sample
            .map(|s| s.heading_true_deg)
            .unwrap_or(snap.heading_deg_true),
    );

    // Touchdown profile — full buffer reframed to ms-relative
    // timestamps. PIREP renders this as a tiny V/S curve so
    // the pilot can see the flare shape after the fact.
    stats.touchdown_profile = stats
        .snapshot_buffer
        .iter()
        .map(|s| TouchdownProfilePoint {
            t_ms: (s.at - actual_td_at).num_milliseconds() as i32,
            vs_fpm: s.vs_fpm,
            g_force: s.g_force,
            agl_ft: s.agl_ft,
            on_ground: s.on_ground,
            heading_true_deg: s.heading_true_deg,
            groundspeed_kt: s.groundspeed_kt,
            indicated_airspeed_kt: s.indicated_airspeed_kt,
            pitch_deg: s.pitch_deg,
            bank_deg: s.bank_deg,
        })
        .collect();

    // Sideslip / crab angle at touchdown — GEES-aligned:
    //   sideslip = atan2(VEL_BODY_X, VEL_BODY_Z) × 180/π
    // VEL_BODY_X is the right component of velocity in the
    // aircraft's body frame; VEL_BODY_Z is the forward
    // component. The ratio's arctangent IS the angle
    // between the velocity vector and the nose. Native
    // and exact — much better than reconstructing track
    // from successive lat/lon. None when the SimVar isn't
    // wired or the aircraft is essentially stopped (Z<3 fps
    // ≈ 1.8 kt, vector noise dominates below that).
    stats.touchdown_sideslip_deg = match (snap.velocity_body_x_fps, snap.velocity_body_z_fps) {
        (Some(x), Some(z)) if z.abs() > 3.0 => Some(x.atan2(z).to_degrees()),
        _ => None,
    };

    // Headwind / crosswind components at the touchdown
    // frame, native from `AIRCRAFT WIND X/Z`. Z is signed
    // such that positive = tailwind (wind blowing into the
    // aircraft from behind), so headwind = -Z. X positive
    // = wind from the right side.
    // Meteorologischer Wind: die Basis der Seitenwind-Kompensation.
    //
    // v1.6.3-QS: **ueberschreiben, nicht latchen.** Ein `is_none()`-Guard
    // waere hier falsch — er stammt vom flugkonstanten `landing_simulator`,
    // wo er richtig ist. Der Wind ist es nicht: nach einem Touch-and-Go oder
    // einem Durchstarten mit Ausweichlandung rechnete die Kompensation sonst
    // mit dem Wetter des ERSTEN Aufsetzens gegen Bahnkurs und Steuerkurs des
    // LETZTEN. Da die Kompensation nur schenken kann, waeren das im
    // schlechtesten Fall 5 Grad geschenkter Vorhalt fuer eine wirklich
    // schiefe Landung. Alle Nachbarfelder hier (Position, Kurs,
    // Geschwindigkeit) ueberschreiben aus demselben Grund.
    //
    // Aber NUR mit einem gültigen Wert überschreiben. Dieser Stempel läuft
    // zweimal — einmal aus dem Sampler, danach aus der Phasen-Logik, die
    // ausdrücklich überschreibt. Ein blindes Zuweisen würde einen guten
    // Wert des ersten Laufs löschen, sobald der zweite Schnappschuss die
    // Windfelder gerade nicht trägt (X-Plane liefert sie nur bei
    // Windgeschwindigkeit über null, und ein einzelner Tick kann fehlen).
    // Der Rücksetzer für Touch-and-Go räumt sie stattdessen gezielt ab.
    //
    // Die beiden körperfesten Windkomponenten darunter werden dagegen
    // bedingungslos gesetzt, und das ist auch richtig so: beide Adapter
    // liefern sie immer als Wert (nie fehlend), ein Löschen kann dort also
    // gar nicht entstehen.
    if let Some(dir) = snap.wind_direction_deg {
        stats.landing_wind_direction_deg = Some(dir);
    }
    if let Some(kt) = snap.wind_speed_kt {
        stats.landing_wind_speed_kt = Some(kt);
    }
    stats.landing_headwind_kt = snap.aircraft_wind_z_kt.map(|z| -z);
    stats.landing_crosswind_kt = snap.aircraft_wind_x_kt;
}

// v0.16.24: how close (in nm) a second airport must be to the nearest one
// for the touchdown to count as "ambiguous between two fields". Inside this
// margin we prefer the planned `arr_airport` if it's one of the two — so a
// touchdown that is geometrically tied between the planned airport and an
// immediate neighbour never flips away from the plan. 1.5 nm is well below
// the spacing of any two distinct real-world airports a pilot could land on
// in one rollout, yet large enough to absorb the nearest-threshold proxy
// error `find_nearest_airports` introduces (it keys on the closest runway
// threshold, not the airport reference point).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> correlate_airport_ambiguity_nm (v2.1.24: 1.5)
// v0.16.24: search radius (nm) for resolving the ACTUAL landing airport from
// the touchdown coordinate. The touchdown is on a runway, so the nearest
// airport is at most a runway-half-length away (~1.5 nm for the longest
// strips). 6 nm is generous head-room without pulling in distant neighbours.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> correlate_airport_search_radius_nm (v2.1.24: 6.0)
