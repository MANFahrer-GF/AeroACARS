/// Translate a flaps detent (0..5) into the Airbus-style label most
/// virtual airlines (and the activity-log readers) recognise. ONLY
/// used when no PMDG snapshot is available — for PMDG aircraft we
/// take the cockpit-exact Boeing label directly from
/// `pmdg.flap_handle_label` (see flap-detent change-detection block).
/// v1.5.3 (#ifly-audit): RASTERBEWUSSTES Klappen-Label aus Index +
/// Rastenzahl — die Zahl der Rasten verraet die Familie:
///   9 Positionen (num=8, Index 0..8) -> Boeing 737: UP/1/2/5/10/15/25/30/40
///   7 Positionen (num=6)             -> Boeing 777: UP/1/5/15/20/25/30
///   5 Positionen (num=4)             -> Airbus:     0/1/2/3/FULL
/// Alles andere -> None, dann uebernimmt die alte Prozent-Heuristik
/// (detent_label). Anlass: iFly-Flug 11.08.2026 — "Flaps 5" (Raste 3
/// von 8) lief durchs Airbus-Prozentraster und hiess "1+F"; die
/// Standard-Landung "Flaps 30" hiess "3".
fn raster_flap_label(num_positions: u8, index: u8, icao: Option<&str>) -> Option<&'static str> {
    // Raster je Familie in `tabellen/klappen.json` (P-3.54); erste passende
    // Zeile gewinnt. `nur_muster` haelt 5 Rasten fuer Airbus mit 1+F frei
    // (5 Rasten hat z. B. auch der CRJ).
    let muster = icao.map(|m| m.trim().to_ascii_uppercase());
    laufzeit_paket::stand()
        .klappen
        .iter()
        .find(|r| {
            r.rasten == num_positions
                && (r.nur_muster.is_empty() || muster.as_deref().is_some_and(|m| r.nur_muster.iter().any(|x| x == m)))
        })
        .and_then(|r| r.stufen.get(index as usize).map(String::as_str))
}

#[cfg(test)]
#[path = "tests_lib/raster_flap_label_tests.rs"]
mod raster_flap_label_tests;

fn detent_label(detent: u8) -> &'static str {
    match detent {
        // Airbus-Hebel zeigt „0“ (Thomas, 29.09.2026), wie `raster_flap_label`.
        0 => "0",
        1 => "1",
        2 => "1+F",
        3 => "2",
        4 => "3",
        _ => "FULL",
    }
}

/// Order Boeing flap-handle labels for direction-of-change detection.
/// Boeing 737 detents: UP/1/2/5/10/15/25/30/40 (rank 0..8).
/// Boeing 777 detents: UP/1/5/15/20/25/30 (rank 0..6, sharing the
/// numeric ordering with NG3 — both share the rank table since UP=0,
/// 1=1, 5=3, 15=5 etc., and the comparison only cares about
/// monotonicity, not exact values).
fn boeing_detent_rank(label: &str) -> u8 {
    match label {
        "UP" => 0,
        "1" => 1,
        "2" => 2,
        "5" => 3,
        "10" => 4,
        "15" => 5,
        "20" => 6,
        "25" => 7,
        "30" => 8,
        "40" => 9,
        _ => 0,
    }
}

// (Old position-derived sideslip helper deleted — replaced by direct
// `atan2(VEL_BODY_X, VEL_BODY_Z)` computation at the touchdown edge,
// matching GEES. The native body-frame velocities are accurate to
// the floating-point precision MSFS reports them at, no need to
// reconstruct anything from successive lat/lon.)

/// Push the current snapshot's V/S + bank into the approach buffer.
/// Called every step_flight tick during Approach + Final phases. The
/// buffer is capped at APPROACH_BUFFER_MAX so a 30-min hold doesn't
/// blow up memory; oldest entries get dropped first.
/// v0.5.25: erweitertes Approach-Sample mit allen Feldern die für
/// korrekte Stable-Approach-Gate-Analyse gebraucht werden:
///   * `agl_ft`: für Window-Filter (= nur AGL ≤ 1000 ft zählt fuer
///     den Stable-Approach-Gate-Score, ueber 1000 ft ist Vectoring
///     legitim und sollte nicht Pilot-bestrafen)
///   * `gs_kt`: für Soll-Glide-Slope-Berechnung
///     (target_vs ≈ gs_kt × 5.31 für 3°-Glide-Slope)
///   * `selected_runway`: für Late-RWY-Change-Detection. Wenn ATC
///     mid-final die Bahn wechselt, war Pilot zu Maneuver gezwungen,
///     das ist nicht "instabil"
///
/// Andere Naming-Konvention als das `ApproachSample` aus dem
/// `storage`-Crate (welches nur (vs_fpm, bank_deg) ist und an die
/// PIREP-Notes-Sparkline serialisiert wird) — dieses hier ist das
/// reichere internal Buffer-Sample.
///
/// Serialize: v3 (Kap. 5.2) — beide Puffer gehen als `roh.anflugpuffer` ins
/// Eingabe-Dokument.
#[derive(Debug, Clone, Serialize)]
pub struct ApproachBufferSample {
    pub at: DateTime<Utc>,
    pub agl_ft: f32,
    pub msl_ft: f32,
    pub gs_kt: f32,
    pub ias_kt: f32,
    pub vs_fpm: f32,
    pub bank_deg: f32,
    pub heading_true_deg: f32,
    pub gear_position: f32,
    pub flaps_position: f32,
    pub selected_runway: Option<String>,
    pub stall_warning: bool,
    /// Lernpaket AP4 (29.09.2026): Position fuer die geometrische
    /// Gleitpfad-Abweichung. `None`, wenn der Simulator keine endliche
    /// Position liefert — die Probe bleibt trotzdem im Puffer, die
    /// Stabilitaets-Auswertung braucht sie nicht.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Lernpaket AP5: Nicklage fuer die Nick-Unruhe. MSFS und X-Plane
    /// liefern sie beide.
    pub pitch_deg: Option<f32>,
    /// Lernpaket AP5: mittleres N1 in %, fuer die Schub-Umkehrungen — ueber
    /// die Triebwerke mit N1 ≥ 5 % (siehe `n1_mittel_laufend`). Nur Turbinen
    /// fuellen `eng_n1_pct` (MSFS `TURB ENG N1`, X-Plane `ENGN_N1_` seit
    /// v1.9.12); Kolben- und Elektromotoren liefern `None` — dann gibt es
    /// keinen Schubwert, statt einen zu erfinden.
    pub n1_mittel_pct: Option<f32>,
    /// Score-Version 24: koerperfester Wind (Gegenwind positiv, Seitenwind
    /// positiv = von rechts) — Kontext „Wind im Anflug", keine Note.
    pub headwind_kt: Option<f32>,
    pub crosswind_kt: Option<f32>,
}

// Obergrenze fuer eine physikalisch moegliche Sinkrate im Anflug.
//
// Ein Verkehrsflugzeug im Landeanflug sinkt mit 600-1500 fpm; selbst ein
// bewusst steiler Sinkflug bleibt unter 6000. Alles darueber ist kein
// Flugzustand, sondern ein Sim-Artefakt: Pause, Slew, Teleport, Ladebild,
// Positions-Sprung. Grosszuegig auf 10000 gesetzt — es geht nicht darum,
// schlechtes Fliegen wegzufiltern, sondern Werte, die keine Fluglage sind.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> approach_vs_plausibility_cap_fpm (v2.1.24: 10_000.0)

// Bank jenseits davon ist im Anflug ebenfalls kein Flugzustand mehr.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> approach_bank_plausibility_cap_deg (v2.1.24: 90.0)

/// Ein Sample, das keine Fluglage beschreibt, gehoert nicht in den Puffer.
///
/// v0.20.1: Der Anflug-Puffer nahm bisher JEDES Sample ungefiltert. Ein
/// einziger Glitch-Wert vergiftet danach alles, was daraus rechnet — Streuung,
/// Jerk, Gleitpfad-Abweichung, Stabilitaets-Gate. Im VPS-Korpus (678 Landungen)
/// stand deshalb eine Sinkraten-Streuung von **12.810 fpm**; solche Ausreisser
/// ziehen einen Piloten grundlos auf 0 Punkte in der Anflug-Stabilitaet.
///
/// Gefiltert wird an der QUELLE, nicht in den einzelnen Formeln: sonst muesste
/// jede kuenftige Auswertung den Filter erneut richtig hinschreiben — und genau
/// so entstehen die Risse, die uns PIA3452 eingebrockt hat.
fn approach_sample_is_plausible(snap: &SimSnapshot) -> bool {
    // `msl_ft` MUSS mitgeprueft werden: das v2-Gate rechnet mit HAT (Height
    // Above Touchdown) aus genau diesem Feld. Ein NaN rutscht sonst durch, macht
    // die HAT NaN, und jeder Vergleich damit ist false — das Sample verschwindet
    // still aus dem Gate, statt sauber verworfen zu werden.
    let finite = snap.vertical_speed_fpm.is_finite()
        && snap.bank_deg.is_finite()
        && snap.altitude_agl_ft.is_finite()
        && snap.altitude_msl_ft.is_finite()
        && snap.groundspeed_kt.is_finite()
        && snap.indicated_airspeed_kt.is_finite();
    finite
        && snap.vertical_speed_fpm.abs() <= crate::grenzwerte::grenzwerte().anflug_kern.approach_vs_plausibility_cap_fpm.wert
        && snap.bank_deg.abs() <= crate::grenzwerte::grenzwerte().anflug_kern.approach_bank_plausibility_cap_deg.wert
}

/// Standardabweichung einer Stabilitaets-Achse, oder `None` bei zu duenner
/// Stichprobe.
///
/// v0.20.1: EINE Formel. Sie stand vorher zweimal im File (Legacy-Pfad und
/// v2-Pfad) — in einem Commit, dessen Zweck es ist, doppelte Berechnungen
/// derselben Groesse zu beseitigen. Beide Achsen und beide Pfade rufen jetzt
/// hier an, damit Fenster, Filter und Mindest-Stichprobe nicht auseinander
/// laufen koennen.
fn stability_stddev(vals: impl Iterator<Item = f32>) -> Option<f32> {
    let v: Vec<f64> = vals.map(f64::from).collect();
    if v.len() < crate::grenzwerte::grenzwerte().anflug_kern.stability_min_samples.wert {
        return None;
    }
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    Some(var.sqrt() as f32)
}

/// Schreibt die Klappen-Spanne des Fluges fort. NaN/Inf werden verworfen —
/// ein kaputter Messwert darf die Spanne nicht auf „hat sich bewegt" ziehen.
fn merke_klappen_spanne(stats: &mut FlightStats, flaps_position: f32) {
    if !flaps_position.is_finite() {
        return;
    }
    if stats.flaps_min_gesehen.is_none() {
        // Erster Messwert dieses Prozesses. Steht der Abheb-Zeitpunkt noch
        // aus, haben wir den ganzen Klappen-Zyklus vor uns; ist er schon
        // gesetzt, sind wir in einen laufenden Flug hineingeraten (Resume)
        // und duerfen aus Stillstand nicht auf einen toten Kanal schliessen.
        stats.flaps_lueckenlos_beobachtet = stats.takeoff_at.is_none();
    }
    let v = flaps_position.clamp(0.0, 1.0);
    stats.flaps_min_gesehen = Some(stats.flaps_min_gesehen.map_or(v, |m| m.min(v)));
    stats.flaps_max_gesehen = Some(stats.flaps_max_gesehen.map_or(v, |m| m.max(v)));
}

/// Baut den Musterwissen-Kontext fuer die Landekonfiguration.
///
/// Nimmt die bereits aufgeloesten Grenzen entgegen statt sie selbst
/// nachzuschlagen: der Aufrufer braucht sie ohnehin fuer die Wing-Strike-
/// Wertung, und zwei Aufloesungen desselben Musters waeren zwei Quellen fuer
/// dieselbe Vref.
fn konfig_kontext(stats: &FlightStats, limits: &AircraftLimits) -> KonfigKontext {
    // Bewegt heisst: Spanne groesser als eine halbe Raststufe des feinsten im
    // Korpus beobachteten Rasters (1/9 ≈ 0,111 bei der Fokker 28). 0,02 liegt
    // klar darunter und klar ueber dem Rauschen animierter Klappen.
    let bewegt = match (stats.flaps_min_gesehen, stats.flaps_max_gesehen) {
        // Stillstand ist nur dann eine Aussage, wenn wir von Anfang an
        // zugesehen haben — sonst bleibt die Frage offen (`None`), und die
        // Bewertung faellt auf das Verhalten vor v1.6.6 zurueck.
        (Some(min), Some(max)) if (max - min) <= 0.02 => {
            if stats.flaps_lueckenlos_beobachtet {
                Some(false)
            } else {
                None
            }
        }
        (Some(_), Some(_)) => Some(true),
        _ => None,
    };
    KonfigKontext {
        typical_vref_kt: limits.typical_vref_kt,
        flaps_bewegt: bewegt,
        flaps_max_im_flug: stats.flaps_max_gesehen,
    }
}

fn push_approach_sample(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    if !approach_sample_is_plausible(snap) {
        tracing::debug!(
            vs_fpm = snap.vertical_speed_fpm,
            bank_deg = snap.bank_deg,
            agl_ft = snap.altitude_agl_ft,
            "approach sample verworfen — keine plausible Fluglage (Sim-Artefakt)"
        );
        return;
    }
    stats.approach_buffer.push_back(ApproachBufferSample {
        // Tick-Zeit vom Caller (step_flight_at) — nicht Utc::now(): sonst
        // trügen Replay-Läufe Wall-Clock-Timestamps im Approach-Buffer,
        // während landing_at historisch ist → approach_buffer_window
        // fände ein leeres Fenster (QS-Runde 1, Replay-Fidelity).
        at: now,
        agl_ft: snap.altitude_agl_ft as f32,
        msl_ft: snap.altitude_msl_ft as f32,
        gs_kt: snap.groundspeed_kt,
        ias_kt: snap.indicated_airspeed_kt,
        vs_fpm: snap.vertical_speed_fpm,
        bank_deg: snap.bank_deg,
        heading_true_deg: snap.heading_deg_true,
        gear_position: snap.gear_position,
        flaps_position: snap.flaps_position,
        selected_runway: snap.selected_runway.clone(),
        stall_warning: snap.stall_warning,
        // Lernpaket AP4/AP5: fehlende oder kaputte Werte werden `None`, die
        // Probe selbst bleibt — der Plausibilitaetsfilter oben entscheidet
        // weiter nur ueber die Fluglage.
        lat: Some(snap.lat).filter(|v| v.is_finite()),
        lon: Some(snap.lon).filter(|v| v.is_finite()),
        pitch_deg: Some(snap.pitch_deg).filter(|v| v.is_finite()),
        n1_mittel_pct: n1_mittel_laufend(snap.eng_n1_pct.as_deref()),
        // Gleiche Ableitung wie `landing_headwind_kt`: Z positiv = Rueckenwind.
        headwind_kt: snap.aircraft_wind_z_kt.map(|z| -z).filter(|v| v.is_finite()),
        crosswind_kt: snap.aircraft_wind_x_kt.filter(|v| v.is_finite()),
    });
    while stats.approach_buffer.len() > APPROACH_BUFFER_MAX {
        stats.approach_buffer.pop_front();
    }
    // Lernpaket AP4/AP5: dieselbe Probe in den laengeren Forensik-Puffer.
    if let Some(probe) = stats.approach_buffer.back().cloned() {
        stats.anflug_forensik_puffer.push_back(probe);
        while stats.anflug_forensik_puffer.len() > ANFLUG_FORENSIK_PUFFER_MAX {
            stats.anflug_forensik_puffer.pop_front();
        }
    }
}

/// v1.5.0 (#msfs-hud): Aktualisiert die geschätzte Landebahn und deren
/// Gleitwinkel in `stats`, einmal pro Tick.
///
/// Am Boden wird die Schätzung gelöscht statt stehengelassen: nach dem
/// Aufsetzen ist der Korrelations-Match die bessere Quelle, und eine
/// weiterlaufende Schätzung würde die Anzeige nur mit einem Wert versorgen,
/// der bereits überholt ist. Vor dem Abheben gibt es ohnehin nichts
/// vorherzusagen — dort stünde sonst die Startbahn, die zufällig zum
/// Steuerkurs passt.
///
/// Ohne Navdaten für den Zielflughafen (Abruf noch nicht durch, unbekanntes
/// Feld) bleibt es bei `None`. Das ist kein Fehlerfall, sondern der normale
/// Zustand in den ersten Minuten eines Fluges.
fn update_predicted_runway(flight: &ActiveFlight, stats: &mut FlightStats, snap: &SimSnapshot) {
    if snap.on_ground || !stats.was_airborne {
        stats.predicted_runway = None;
        stats.predicted_glideslope_deg = None;
        return;
    }
    let predicted = {
        let cache = match flight.navdata.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        cache.get(&flight.arr_airport).and_then(|apt| {
            runway::predict_landing_runway(&apt.runways, snap.lat, snap.lon, snap.heading_deg_true)
        })
    };
    match predicted {
        Some(p) => {
            stats.predicted_glideslope_deg = p.glideslope_angle;
            stats.predicted_runway = Some(p.designator);
        }
        None => {
            stats.predicted_runway = None;
            stats.predicted_glideslope_deg = None;
        }
    }
}

/// v0.16.6: phase-independent gate for `push_approach_sample` — ONE central
/// per-tick call in `step_flight` (right before the phase `match`) replaces
/// the two arm-local calls that used to sit at the top of the Approach and
/// Final arms.
///
///   * `Approach` / `Final` → ALWAYS push, exactly like the old arm-local
///     calls (they were the unconditional first statement of each arm and
///     nothing executes between the central call site and the old ones, so
///     cadence and ordering are bit-identical for normal flights).
///   * Any other phase → push only on a genuine descending low-level
///     segment: the flight has been airborne, is currently airborne, is
///     below 2000 ft AGL and sinking faster than −100 fpm. Short/low VFR
///     bush hops (GSG312 97OG) land via the 50 Hz sampler while the FSM is
///     still stuck in Takeoff/Climb — without this the approach buffer
///     stayed empty and the stability stats (approach_vs_stddev /
///     stable_* flags) were silently missing from the PIREP. Level or
///     climbing low-level flight does NOT sample (V/S gate); the
///     sampler-path evaluation additionally windows the buffer to the last
///     `SAMPLER_STABILITY_WINDOW_SECS` before touchdown so a long low-level
///     cruise can't poison the gate stats anyway.
fn should_push_approach_sample(
    phase: FlightPhase,
    was_airborne: bool,
    on_ground: bool,
    agl_ft: f64,
    vs_fpm: f32,
) -> bool {
    matches!(phase, FlightPhase::Approach | FlightPhase::Final)
        || (was_airborne && !on_ground && agl_ft < 2000.0 && vs_fpm < -100.0)
}

/// Track the lowest AGL observed during the current Approach/Final
/// segment. The go-around detector compares the *current* AGL against
/// this minimum to decide whether a sustained climb-back has begun.
///
/// Reset to `None` when a go-around is classified (climb starts a
/// fresh approach window) and when the FSM exits Final into Landing
/// (a successful touchdown invalidates the approach minimum).
fn update_lowest_approach_agl(stats: &mut FlightStats, snap: &SimSnapshot) {
    let (agl, _) = hoehe_fuer_durchstart(stats, snap);
    // Only track positive AGL — a brief negative reading from a sim
    // glitch (terrain mesh hiccup) would poison the minimum and make
    // every subsequent sample look like a 200 ft go-around climb.
    if agl < 0.0 {
        return;
    }
    if stats.lowest_agl_during_approach_ft.is_none_or(|prev| agl < prev) {
        stats.lowest_agl_during_approach_ft = Some(agl);
        stats.tiefster_anflugpunkt = Some((snap.lat, snap.lon, snap.heading_deg_true));
    }
}

/// Hoehe, an der der Durchstart-Detektor misst.
///
/// v1.9.16 (DLH2248, LFLL, 01.10.2026): Bisher immer AGL — die Hoehe ueber
/// dem Gelaende UNTER dem Flugzeug. Vor huegeligem Anflug springt sie um
/// hunderte Fuss, ohne dass das Flugzeug steigt: Die AGL fiel auf 709 ft
/// und stand 40 s spaeter bei 1359 ft, waehrend die Hoehe ueber der Bahn
/// (HAT) nie unter 1164 ft kam. Der Pilot hatte auf 1200 ft abgefangen,
/// kein Durchstart — der Detektor meldete trotzdem einen und setzte die
/// Flugphase auf Climb zurueck.
///
/// Mit bekannter Platzhoehe daher HAT (MSL − Platzhoehe), wie das
/// Stabilitaets-Gate; ohne sie bleibt es bei AGL (bisheriges Verhalten).
fn hoehe_fuer_durchstart(stats: &FlightStats, snap: &SimSnapshot) -> (f32, f32) {
    let agl = snap.altitude_agl_ft as f32;
    match stats.arr_airport_elevation_ft {
        Some(elev) => {
            let hat = snap.altitude_msl_ft as f32 - elev;
            let diff = (hat - agl).abs();
            // Die Platzhoehe ist die des GEPLANTEN Ziels. Nach einem Ausweichen
            // auf einen anderen Platz (oder bei falscher Platzhoehe) waere HAT
            // um die Differenz verschoben — dann gilt AGL wie zuvor, statt den
            // Detektor mit einer falschen Bezugshoehe zu blenden.
            if hat > 0.0 && diff <= crate::grenzwerte::grenzwerte().anflug_kern.hat_agl_plausibel_ft.wert {
                // Die enge 1000-ft-Obergrenze nur, wenn HAT und AGL
                // zusammenpassen (Gelaende, kein Hoehenfehler); bei groesserer
                // Differenz bleibt es bei der weiten Grenze von 1500 ft.
                let obergrenze = if diff <= crate::grenzwerte::grenzwerte().anflug_kern.hat_agl_vertraut_ft.wert {
                    1000.0
                } else {
                    1500.0
                };
                (hat, obergrenze)
            } else {
                (agl, 1500.0)
            }
        }
        None => (agl, 1500.0),
    }
}

// Ein Flugplatz (Bezugspunkt) in diesem Umkreis macht „unter 700 ft AGL“ zum Endanflug,
// auch wenn HAT nicht passt (Ausweichplatz, Gelaende vor der Bahn).
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> final_platz_nm (v2.1.24: 5.0)

/// Final nur in Platznaehe (24.10.2026, AUA 561 EDDN–LOWI, Pilot 15): Ueber
/// den Bergen 45 km vor Innsbruck lag das Gelaende unter 700 ft — die Phase
/// sprang auf Final, und beim Wiedersteigen im Anflugverfahren sagte die
/// Kabine ein Durchstarten an. Die Hoehe ueber Grund allein sagt nicht, dass
/// eine Bahn nahe ist: Passt sie zur Hoehe ueber der Zielbahn (HAT) oder liegt
/// ein Flugplatz in `FINAL_PLATZ_NM`, ist es der Endanflug.
fn final_in_platznaehe(stats: &FlightStats, snap: &SimSnapshot) -> bool {
    let hat_passt = stats.arr_airport_elevation_ft.is_none_or(|elev| {
        ((snap.altitude_msl_ft as f32 - elev) - snap.altitude_agl_ft as f32).abs() <= crate::grenzwerte::grenzwerte().anflug_kern.hat_agl_plausibel_ft.wert
    });
    // Erst wenn HAT nicht passt: Bezugspunkte aller Plaetze, auf denen ein
    // Flugzeug landen kann, auch ohne Bahngeometrie und Wasserlandeplaetze
    // (Codex 24.10.2026: sonst bliebe eine Landung dort ohne Final und ohne
    // „Nach der Landung“).
    hat_passt || runway::nearest_airport_reference(snap.lat, snap.lon, crate::grenzwerte::grenzwerte().anflug_kern.final_platz_nm.wert, "").is_some()
}

// Groesster Unterschied HAT↔AGL, den der Durchstart-Detektor noch als
// „gleicher Platz" liest. Gelaende vor Bergplaetzen schwankt um einige
// hundert Fuss (DLH2248: 455 ft).
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> hat_agl_plausibel_ft (v2.1.24: 1200.0)
// Bis zu dieser Differenz gilt HAT als verlaesslich genug fuer die enge
// Obergrenze (tiefster Punkt ≤ 1000 ft HAT). Darueber koennte die
// Platzhoehe falsch sein — dann bleibt die weite Grenze wie vor v1.9.16.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> hat_agl_vertraut_ft (v2.1.24: 600.0)

/// Detect a go-around in progress: aircraft has climbed
/// `GO_AROUND_AGL_RECOVERY_FT` above the lowest AGL seen during this
/// approach, sustained `GO_AROUND_DWELL_SECS` of positive V/S above
/// `GO_AROUND_MIN_VS_FPM`, while airborne with engines running.
///
/// Returns `Some(FlightPhase::Climb)` when classified — the caller
/// should swap `next_phase` to that value so the FSM bumps back to
/// Climb. Side effects on confirmation: bumps `stats.go_around_count`,
/// pushes a human-readable line into `stats.pending_acars_logs`,
/// clears the lowest-AGL tracker (next descent starts a fresh
/// window), and clears the dwell timer.
/// v2-Entscheidung des Vortakts für das Reiseband (17.10.2026, „Stück 2"):
/// `Some(p)`, wenn v2 eine ANDERE Reiseband-Phase meldet als die aktuelle.
/// Bestätigt v2 nur die aktuelle Phase, kommt `None` — damit die übrigen
/// Prüfungen des Arms (Warteschleife im Cruise) weiterlaufen. Anflug,
/// Final und Landung übernimmt v1 NICHT von v2 — dort entscheiden v1s
/// eigene Kanten (Anflug-Kante im selben Takt, Durchstart-Detektor mit
/// Zählung und Forensik-Schnitt, Bodenberührung für den Aufsetz-Zeitpunkt).
fn reiseband_phase(stats: &FlightStats) -> Option<FlightPhase> {
    let ziel = match stats.shadow_phase? {
        p @ (FlightPhase::Climb | FlightPhase::Cruise | FlightPhase::Descent) => p,
        // v2 schon im Anflug, v1 noch im Reiseflug: v1 geht in den
        // Sinkflug, und seine eigene Anflug-Kante entscheidet im nächsten
        // Takt. Sonst bliebe v1 bei einem flachen Anflug (> −500 fpm, der
        // Cruise-Schnellweg greift nicht) bis zum Boden im Reiseflug —
        // ohne Anflug-Puffer, Durchstart-Detektor und Landungskante
        // (Codex-QS 17.10.2026). Nur aus Cruise: im Climb (z. B. nach
        // einem Durchstarter) wäre „Sinkflug" falsch; dort bleibt es beim
        // Versöhner.
        FlightPhase::Approach | FlightPhase::Final if stats.phase == FlightPhase::Cruise => {
            FlightPhase::Descent
        }
        _ => return None,
    };
    (ziel != stats.phase).then_some(ziel)
}

/// Anflug-Kante aus dem Reiseband: tief und sinkend (alte FSM und v2 1:1).
fn anflug_kante(snap: &SimSnapshot) -> bool {
    snap.altitude_agl_ft < 5000.0 && snap.vertical_speed_fpm < 0.0
}

/// Frisches Per-Anflug-Bild beim Eintritt in den Anflug (v0.5.5): Werte
/// eines FRÜHEREN Anflugs dürfen nicht in den Touchdown-V/S-Fallback
/// sickern. Eine Stelle für alle Wege in den Anflug.
fn anflug_beginnt(stats: &mut FlightStats) {
    stats.low_agl_vs_min_fpm = None;
    stats.last_low_agl_vs_fpm = None;
}

fn check_go_around(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
) -> Option<FlightPhase> {
    let lowest = stats.lowest_agl_during_approach_ft?;
    let (agl, obergrenze) = hoehe_fuer_durchstart(stats, snap);
    // Need at least *some* descent to have happened — otherwise a
    // pilot intercepting the glideslope from above would trip the
    // detector the moment we entered Approach.
    //
    // v1.9.16: Mit HAT liegt die Grenze am Stabilitaets-Gate (1000 ft):
    // Ein Abfangen oder Wiederansteigen OBERHALB davon ist kein
    // Fehlanflug, sondern ein Hoehenmanoever im Anflug.
    if lowest > obergrenze {
        return None;
    }
    let conds = agl > lowest + crate::grenzwerte::grenzwerte().phasen_kern.go_around_agl_recovery_ft.wert
        && snap.vertical_speed_fpm > crate::grenzwerte::grenzwerte().phasen_kern.go_around_min_vs_fpm.wert
        && !snap.on_ground
        && snap.engines_running > 0;
    if conds {
        let pending = stats.go_around_climb_pending_since.get_or_insert(now);
        if (now - *pending).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.go_around_dwell_secs.wert {
            // 17.10.2026: Durchstarter nur, wenn am tiefsten Punkt eine
            // Landeschwelle voraus lag. Sonst war es ein Steigen aus dem
            // Anflug (tiefer Überlandflug, Gelände, Höhenänderung) — die
            // Phase geht trotzdem in den Steigflug, aber ohne Zählung
            // (die im PIREP steht und in der Treibstoff-Teilnote eine
            // Mehrweg-Gutschrift gibt). Korpus: 8 von 18 gezählten
            // Durchstartern lagen quer zur Bahn oder 5,3–25 NM entfernt.
            let an_der_bahn = stats.tiefster_anflugpunkt.is_none_or(|(lat, lon, kurs)| {
                runway::schwelle_voraus(lat, lon, kurs, crate::grenzwerte::grenzwerte().phasen_kern.go_around_schwelle_nm.wert, crate::grenzwerte::grenzwerte().phasen_kern.go_around_schwelle_kurs_deg.wert)
            });
            if an_der_bahn {
                stats.go_around_count = stats.go_around_count.saturating_add(1);
                tracing::info!(
                    count = stats.go_around_count,
                    agl_ft = agl,
                    lowest_ft = lowest,
                    vs_fpm = snap.vertical_speed_fpm,
                    "go-around classified — phase reverting to Climb"
                );
                stats.pending_acars_logs.push(format!(
                    "Go-around #{} at {:.0} ft AGL (V/S {:.0} fpm)",
                    stats.go_around_count, agl, snap.vertical_speed_fpm
                ));
            } else {
                tracing::info!(
                    agl_ft = agl,
                    lowest_ft = lowest,
                    vs_fpm = snap.vertical_speed_fpm,
                    "climb-away from the approach without a runway ahead — Climb, no go-around"
                );
            }
            // Der abgebrochene Anflug gilt in beiden Fällen nicht mehr:
            // gemessene Vref und Anflug-Forensik beginnen neu.
            stats.vref_gemessen = None;
            stats.lowest_agl_during_approach_ft = None;
            stats.tiefster_anflugpunkt = None;
            stats.go_around_climb_pending_since = None;
            // Lernpaket AP4/AP5: der abgebrochene Anflug zaehlt nicht fuer
            // die Forensik der naechsten Landung.
            anflug_forensik_schneiden(stats, Some(now));
            // v0.5.11: reset climb_peak_msl so the missed-approach
            // climb back up tracks fresh peaks. Without this, the
            // prior peak (the cruise altitude) stays as reference,
            // and the moment the aircraft descends back through it
            // for the second approach attempt, the low-altitude
            // Climb→Descent trigger could fire prematurely against
            // a stale peak.
            stats.climb_peak_msl = None;
            return Some(FlightPhase::Climb);
        }
    } else {
        // Conditions broke — reset dwell so the next sustained climb
        // starts a fresh timer. Without this, brief bumps would
        // accumulate seconds and falsely confirm the go-around.
        stats.go_around_climb_pending_since = None;
    }
    None
}

/// En-Route-Versöhner: Eignungs-Prüfung (rein, testbar) — dürfen dieses
/// v1/v2-Phasen-Paar überhaupt versöhnt werden? Siehe Doc an
/// `ENROUTE_RECONCILE_DWELL_SECS` für die Gesamt-Mechanik.
///
/// Seit 17.10.2026 („Stück 2") nur noch zwei Fälle:
/// * v1 im Anflug/Final, v2 woanders im Luft-Band oder auf `Landing` —
///   verpasster Durchstarter (tote Triebwerks-SimVars), verpasste
///   Bodenberührung, Steigen aus dem Anflug ohne Durchstarter.
/// * v1 im Reiseband (Climb/Cruise/Descent), v2 schon im Anflug/Final/
///   auf `Landing` — die GSG22-Klasse. `Landing` ist v2s Ground-Evidenz-
///   Selbstbeförderung (v0.19.1), in die eine festgehangene v1 gezogen
///   werden muss, damit ihre Landing→TaxiIn→BlocksOn→Arrived-Kette läuft.
///
/// Paare innerhalb des Reisebands gibt es nicht mehr zu versöhnen: dort
/// übernimmt v1 v2s Phase direkt (`reiseband_phase`). `Holding` als v1
/// bleibt ausgenommen (v2 modelliert es nicht), Boden-/Terminal-Phasen
/// ebenfalls (dort ist v1 autoritativ). Gleichstand ist keine Divergenz.
fn enroute_reconcile_eligible(v1: FlightPhase, v2: FlightPhase) -> bool {
    // Seit 17.10.2026 („Stück 2") übernimmt v1 im Reiseband (Climb/
    // Cruise/Descent) v2s Phase direkt (`reiseband_phase`) — Paare
    // INNERHALB des Reisebands gibt es nichts mehr zu versöhnen. Übrig
    // bleiben die Sicherheitsnetze: v1 im Anflug/Final (verpasster
    // Durchstarter bei toten Triebwerks-SimVars, verpasste Bodenberührung,
    // Steigen aus dem Anflug ohne Durchstart) und v1 im Reiseband, während
    // v2 schon im Anflug/Final/gelandet ist (GSG22-Klasse).
    let v1_reiseband = matches!(
        v1,
        FlightPhase::Climb | FlightPhase::Cruise | FlightPhase::Descent
    );
    let v1_anflug = matches!(v1, FlightPhase::Approach | FlightPhase::Final);
    let v2_anflug = matches!(
        v2,
        FlightPhase::Approach | FlightPhase::Final | FlightPhase::Landing
    );
    let v2_target = v2_anflug
        || matches!(
            v2,
            FlightPhase::Climb | FlightPhase::Cruise | FlightPhase::Descent
        );
    v1 != v2 && ((v1_anflug && v2_target) || (v1_reiseband && v2_anflug))
}

/// Position im natürlichen Flugbogen für die Zug-Richtungs-Frage des
/// En-Route-Versöhners (vorwärts = kürzerer Dwell, rückwärts = längerer —
/// siehe [`ENROUTE_RECONCILE_DWELL_SECS`]). `None` für Phasen außerhalb
/// des Bogens — der Aufrufer behandelt jede Richtung, die sich nicht aus
/// zwei gültigen Rängen ergibt, als "rückwärts" (= konservativer, langer
/// Dwell). Hinter [`enroute_reconcile_eligible`] kommt das nicht vor.
fn enroute_phase_rank(p: FlightPhase) -> Option<u8> {
    match p {
        FlightPhase::Climb => Some(0),
        FlightPhase::Cruise => Some(1),
        FlightPhase::Descent => Some(2),
        FlightPhase::Approach => Some(3),
        FlightPhase::Final => Some(4),
        FlightPhase::Landing => Some(5),
        _ => None,
    }
}

/// Zieht v2 VORWÄRTS im Flugbogen? Nur dann gilt der kürzere Dwell —
/// alles Unklare fällt auf den langen Rückwärts-Dwell zurück.
fn enroute_reconcile_is_forward(v1: FlightPhase, v2: FlightPhase) -> bool {
    matches!(
        (enroute_phase_rank(v1), enroute_phase_rank(v2)),
        (Some(a), Some(b)) if b > a
    )
}

/// Kinematik-Widerspruchs-Gate des En-Route-Versöhners (rein, testbar) —
/// das PRIMÄRE Feuer-Kriterium, siehe [`ENROUTE_RECONCILE_DWELL_SECS`].
/// True nur, wenn die Fenster-Evidenz v1s Phase aktiv WIDERLEGT:
///
/// * `Climb` ⊥ Descending/Ground — die GSG22-Klasse (Climb festgefahren,
///   Flieger sinkt/rollt längst; ein gesunder Steigflug hat nie 45 s+
///   Fenster-Mehrheit „sinkend").
/// * `Cruise` ⊥ Ground — Absurditäts-Guard; ein sinkendes Cruise wird
///   BEWUSST NICHT gezogen: v1s eigener −500-fpm-Trigger deckt jeden
///   echten TOD ab (Korpus: 0 von 632 Flügen in Cruise festgefahren),
///   und ATC-Step-Downs sind bei v1 by design Cruise — ein Zug erzeugte
///   dort nur den in QS-Runde 1 bewiesenen Pull/Recovery-Zyklus.
/// * `Descent` ⊥ Climbing/Ground — die GSG-105-Klasse (Descent-Falle:
///   v1 hat keinen Descent→Climb-Weg zurück). `Level` widerspricht NICHT
///   (Zwischen-Level-Off; v1s B-003-Recovery übernimmt bei |V/S| < 200).
/// * `Approach`/`Final` ⊥ Climbing/Ground — verpasster Go-Around (v1s
///   eigener GA-Detektor braucht `engines_running` > 0 und ist bei toten
///   Engine-SimVars blind) bzw. verpasste Touchdown-Kante.
///
/// `Level` und `Insufficient` widersprechen NIE — Level-Flug ist mit
/// jeder Luft-Phase vereinbar (TOC-Wartezeit, Restrictions, Step-Downs),
/// und ohne Evidenz wird nicht geurteilt.
fn segment_contradicts_v1(v1: FlightPhase, segment: phase_v2::Segment) -> bool {
    use phase_v2::Segment;
    match v1 {
        FlightPhase::Climb => {
            matches!(segment, Segment::Descending | Segment::Ground)
        }
        FlightPhase::Cruise => matches!(segment, Segment::Ground),
        FlightPhase::Descent | FlightPhase::Approach | FlightPhase::Final => {
            matches!(segment, Segment::Climbing | Segment::Ground)
        }
        _ => false,
    }
}

// Above this groundspeed, nobody is taxiing any more — no evidence needed.
// (Airliners taxi at 10–30 kt; the fastest sane taxi, an empty widebody on a
// long straight, tops out around 35–40 kt.)
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_unambiguous_kt (v2.1.24: 60.0)
// Below this, it isn't a takeoff roll no matter what the engines are doing —
// this is the floor a GA aircraft (rotating around 50 kt) must clear early
// enough that its TakeoffRoll phase is not skipped entirely.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_min_kt (v2.1.24: 30.0)
// Sustained acceleration that says "this is a takeoff, not a taxi". A taxiing
// aircraft creeps up on its speed and holds it; a departing one is still
// gaining ~2–3 kt per second at this point. 1.2 kt/s over a 3-second window
// separates the two without demanding a rate a heavy, fully-loaded widebody
// cannot make.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_accel_kt_per_s (v2.1.24: 1.2)
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_accel_window_secs (v2.1.24: 3)
// How many consecutive accelerating windows make a takeoff roll at the MARGINAL
// rate. One is a two-point delta and can be faked by a frame stutter or an
// un-pause; a real roll keeps accelerating for ten seconds and more.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_accel_windows_needed (v2.1.24: 2)
// Acceleration that needs no second opinion. Nothing taxis at this rate, and
// waiting for a second window would be too slow for the aircraft that reach it:
// a bush plane at 5 kt/s rotates ~45 kt, i.e. six seconds after crossing the
// 30 kt floor — its whole TakeoffRoll phase would otherwise be skipped.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_accel_strong_kt_per_s (v2.1.24: 2.5)
// N1 that means "takeoff thrust is set". Only available for addons that expose
// per-engine N1 (the premium set).
//
// QS round 8: 60 % was too low. MSFS reports `TURB ENG N1` as gas-generator
// speed, and a PT6 turboprop IDLES at 52–65 % Ng — so a King Air, TBM, PC-12 or
// FA50 taxiing at idle already satisfied "takeoff thrust", and the detector
// collapsed back to the bare `gs > 30` rule this whole rewrite exists to
// replace. A jet at takeoff power sits at 85–95 % N1, and even a flex/derated
// departure stays above 80 %.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_n1_pct (v2.1.24: 80.0)

/// Is the aircraft actually beginning its takeoff roll?
///
/// v0.19.3. This used to be `on_ground && groundspeed > 30 kt && engines
/// running` — a speed threshold and nothing else. A heavy widebody taxis
/// through 30 kt without trying, so the FSM announced a takeoff roll while the
/// aircraft was still on the taxiway, fell back to TaxiOut, and rolled for real
/// minutes later (field report: DLH367, ToLiss A346). Beyond the confusing log,
/// the flip-flop re-fired the block-off transition and its fuel/weight
/// diagnostic each time round.
///
/// So we ask for evidence of a TAKEOFF, in the order the evidence is
/// trustworthy:
///
///   1. **Speed no taxi reaches** (≥ 60 kt) — done, no further questions.
///   2. **Takeoff thrust**, where the addon tells us (N1 ≥ 60 %).
///   3. **Sustained acceleration** — the addon-agnostic fallback, and the one
///      that actually distinguishes the two: taxiing holds a speed, taking off
///      is still gaining ~2–3 kt every second.
///
/// The 30 kt floor stays, because a Cessna rotates at ~50 kt and its TakeoffRoll
/// phase must not be skipped.
fn takeoff_roll_detected(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) -> bool {
    if !snap.on_ground || !engines_effectively_running(stats, snap, now) {
        stats.takeoff_roll_gs_ref = None;
        // QS round 7: the COUNTER has to go too. Leaving it set meant a single
        // on_ground-flicker tick (the codebase documents those on bumpy runways
        // and with PMDG textures) preserved a stale count of 1 — so the very next
        // passing window reached 2 and fired. The "two consecutive windows"
        // hardening then collapsed back to a single two-point delta, in exactly
        // the noisy conditions it was added to defend against.
        stats.takeoff_roll_accel_windows = 0;
        return false;
    }
    let gs = snap.groundspeed_kt as f64;

    // Track the speed a few seconds back, so we can ask how fast it is changing.
    // Re-baselined whenever the aircraft slows down (a taxi hold, a runway line-
    // up) so an old, low reference cannot later fake an acceleration.
    //
    // TWO consecutive windows are required, not one. A single window is a
    // two-point delta, and a frame stutter or an un-pause can fake one — while a
    // false TakeoffRoll is expensive: the demote only fires below 20 kt, so an
    // aircraft that keeps taxiing at 25-30 kt would stay in TakeoffRoll for the
    // rest of the taxi (and run the streamer at 2 Hz while it does). A real
    // takeoff roll holds its acceleration for far longer than two windows.
    // QS round 8: windows may only be BANKED above the floor. They used to
    // accumulate while the aircraft was merely accelerating onto the taxiway —
    // every 3 s tick bought one — so by the time it crossed 30 kt the count was
    // already at 2, and the first passing window above the floor fired. The
    // "two consecutive windows" rule was therefore worth exactly one window at
    // the only place it mattered, and the DLH367/A346 profile (a heavy still
    // gaining speed as it rolls through 30 kt) would have reproduced the false
    // TakeoffRoll it was written to stop.
    //
    // Below the floor we hold a baseline but count nothing.
    if gs < crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_min_kt.wert {
        stats.takeoff_roll_gs_ref = Some((now, gs));
        stats.takeoff_roll_accel_windows = 0;
        return false;
    }

    let accelerating = match stats.takeoff_roll_gs_ref {
        Some((t0, gs0)) if gs < gs0 => {
            stats.takeoff_roll_gs_ref = Some((now, gs));
            stats.takeoff_roll_accel_windows = 0;
            let _ = t0;
            false
        }
        Some((t0, gs0)) => {
            let dt = (now - t0).num_milliseconds() as f64 / 1000.0;
            if dt >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_accel_window_secs.wert as f64 {
                let accel = (gs - gs0) / dt;
                // Slide the window forward so the next tick measures fresh.
                stats.takeoff_roll_gs_ref = Some((now, gs));
                if accel >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_accel_kt_per_s.wert {
                    stats.takeoff_roll_accel_windows =
                        stats.takeoff_roll_accel_windows.saturating_add(1);
                } else {
                    stats.takeoff_roll_accel_windows = 0;
                }
                // Unambiguous rate: one window is proof. Marginal rate: two.
                accel >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_accel_strong_kt_per_s.wert
                    || stats.takeoff_roll_accel_windows >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_accel_windows_needed.wert
            } else {
                false
            }
        }
        None => {
            stats.takeoff_roll_gs_ref = Some((now, gs));
            false
        }
    };

    if gs >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_unambiguous_kt.wert {
        return true;
    }

    // Takeoff thrust, when the aircraft tells us.
    let takeoff_thrust = snap
        .eng_n1_pct
        .as_ref()
        .and_then(|n1| {
            n1.iter()
                .cloned()
                .fold(None::<f64>, |m, v| Some(m.map_or(v, |m| m.max(v))))
        })
        .is_some_and(|max_n1| max_n1 >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_n1_pct.wert);

    takeoff_thrust || accelerating
}

/// Everything that "the aircraft has come to rest at its arrival stand" means
/// for the flight record: the block-on timestamp and the arrival gate.
///
/// Two code paths reach that state — the ordinary TaxiIn→BlocksOn edge, and
/// the universal Arrived-fallback, which promotes straight to `Arrived` and
/// skips `BlocksOn` entirely (helicopters, cold-and-dark flows, anyone who
/// shuts down without setting the parking brake). Before v0.19.3 only the
/// first path recorded any of it, so a flight that arrived via the fallback
/// silently lost its `arr_gate` — the PIREP's arrival stand came out empty and
/// nobody could tell why. That was invisible while the fallback was rare; when
/// v0.19.1 shortened its dwell to 9 s it became the *normal* way to arrive
/// (see `ARRIVED_FALLBACK_DWELL_SECS`), and the loss became routine.
///
/// Both callers now go through here, so "how the FSM got to Arrived" can no
/// longer change *what the flight record contains*. Each field is
/// `get_or_insert`-style: an earlier, better value is never overwritten.
/// `at_icao`: the field the aircraft is actually standing on. Recorded next to
/// the stand, because a stand name only means something together with the
/// airport it belongs to: an aircraft that shut down at EDDP after a flight
/// planned to EDDF has a real stand — at EDDP — and if the pilot then files the
/// flight as a normal EDDF arrival anyway, "Gate A17" must NOT ride along into
/// the EDDF PIREP. See `arr_gate_for`.
fn settle_at_arrival_stand(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
    at_icao: &str,
) {
    let kandidat_seit = stats
        .blocks_on_kandidat
        .take()
        .filter(|k| kandidat_hier(k, snap.lat, snap.lon))
        .map(|k| k.0);
    if stats.block_on_at.is_none() {
        stats.block_on_at = Some(kandidat_seit.unwrap_or(now));
    }
    // v1.6.1: der fruehere `snap.parking_name`-Zweig ist ersatzlos raus —
    // beide Sims liefern das Feld nachweislich nie (MSFS: SimConnect
    // befuellt es nicht, sim-msfs setzt hart None; X-Plane: dataref.rs
    // hart None). Er stand als totes Sicherheitsnetz VOR dem verlaess-
    // lichen OSM-Weg und haette ihn bei Junk-Werten eines kuenftigen
    // Adapters lautlos praeemptiert. Der OSM-Weg ist derselbe wie am
    // BlocksOn-Gate: ohne ihn verlor jeder Flug, der ueber den Arrived-
    // Fallback ankam (ohne Parkbremse abgestellt, Helikopter, kalt-und-
    // dunkel), seinen Ankunfts-Stand.
    let at_norm = at_icao.trim().to_uppercase();
    // ⚠ IMMER festhalten, auch wenn unten kein Stand gefunden wird — der
    // Nachzieh-Tick (`arrival_retry_ziel`) braucht dieses Feld fuer einen
    // spaeteren Versuch, unabhaengig davon, ob DIESER Aufruf schon einen
    // Treffer hatte. Siehe die Doku an `arr_actual_icao`.
    stats.arr_actual_icao = Some(at_norm.clone());
    if stats.arr_gate.is_none() {
        let osm = stats
            .arr_stands
            .as_deref()
            .filter(|_| stats.arr_stands_icao.as_deref() == Some(at_norm.as_str()));
        let combined = standliste_fuer(&*stats, &at_norm, osm, Some((snap.lat, snap.lon)));
        let fund = combined
            .as_deref()
            .and_then(|list| stands::benannter_stand_bei(list, snap.lat, snap.lon))
            .and_then(|(st, _)| st.name.clone());
        if let Some(name) = fund {
            tracing::info!(stand = %name, at = %at_norm, "arrival stand captured (settle)");
            stats.arr_gate = Some(name);
            stats.arr_gate_icao = Some(at_norm);
        }
    }
}

/// The arrival stand, but only when it belongs to the airport the PIREP is
/// actually being filed against. Otherwise `None` — an EDDP stand on an EDDF
/// arrival is worse than no stand at all.
fn arr_gate_for(stats: &FlightStats, effective_arr_icao: &str) -> Option<String> {
    let gate = stats.arr_gate.as_ref()?;
    match stats.arr_gate_icao.as_deref() {
        // Captured before v0.19.3 (or by a path that didn't record the field):
        // no reason to doubt it.
        None => Some(gate.clone()),
        Some(icao) if icao.eq_ignore_ascii_case(effective_arr_icao.trim()) => Some(gate.clone()),
        Some(_) => None,
    }
}

/// `landing_at`-Rettung aus dem 50-Hz-Sampler, wenn der FSM-Pfad die
/// Final→Landing-Kante übersprungen hat (v0.5.4, historisch inline im
/// universellen Arrived-Fallback; QS-Runde 1: auch der En-Route-Versöhner
/// braucht sie bei einem Zug direkt auf `Landing`, sonst hinterlässt der
/// Zug `landing_at=None` — aufgeblähte flight_time, kein LandingRecord,
/// toter Landing-Arm-Analyzer). Der Sampler latcht den Touchdown
/// phasenunabhängig (v0.15.20), hier werden Zeitstempel + VS/G nur in die
/// FSM-Felder übernommen — niemals überschreibend (`get_or_insert`-
/// Semantik pro Feld).
fn rescue_landing_at_from_sampler(stats: &mut FlightStats, now: DateTime<Utc>) {
    if stats.landing_at.is_some() || stats.takeoff_at.is_none() {
        return;
    }
    if let Some(sampler_at) = stats.sampler_touchdown_at {
        stats.landing_at = Some(sampler_at);
        if let Some(vs) = stats.sampler_touchdown_vs_fpm {
            if stats.landing_rate_fpm.is_none() {
                finalize_landing_rate(stats, vs, None, Some("sampler_carry_over"));
            }
        }
        if let Some(g) = stats.sampler_touchdown_g_force {
            if stats.landing_g_force.is_none() {
                stats.landing_g_force = Some(g);
            }
            if stats.landing_peak_g_force.is_none() {
                stats.landing_peak_g_force = Some(g);
            }
        }
        tracing::info!(
            "landing_at rescued from sampler: vs={:?} g={:?}",
            stats.sampler_touchdown_vs_fpm,
            stats.sampler_touchdown_g_force
        );
    } else {
        stats.landing_at = Some(now);
    }
}

/// v0.5.11: detect entry into a holding pattern.
///
/// Triggered from Cruise or Approach when the aircraft sustains a
/// turn at constant altitude for `HOLDING_ENTRY_DWELL_SECS`.
/// Returns `true` once the dwell elapses; the caller then transitions
/// the FSM to `FlightPhase::Holding`.
///
/// Detection criteria each tick:
///   * |bank| > 15° (banked turn — standard rate is ~25-30°)
///   * |VS| < 200 fpm (level — real holds are within ±100 ft)
///
/// Once both fail (e.g. straight-and-level resumed), the dwell timer
/// resets so brief intermediate level segments during a 360° turn
/// don't accidentally fire (banked → level → banked is normal at
/// hold turn entry/exit).
fn check_holding_entry(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) -> bool {
    let drehung = kurs_drehung_deg(stats, now, crate::grenzwerte::grenzwerte().phasen_kern.holding_kurs_fenster_secs.wert, true);
    if snap.altitude_agl_ft > crate::grenzwerte::grenzwerte().phasen_kern.holding_min_agl_ft.wert && drehung.abs() >= crate::grenzwerte::grenzwerte().phasen_kern.holding_eintritt_drehung_deg.wert {
        tracing::info!(
            drehung_deg = drehung,
            agl_ft = snap.altitude_agl_ft,
            "holding pattern detected — entering Holding phase"
        );
        return true;
    }
    false
}

/// Kursverlauf der Warteschleifen-Erkennung pflegen (jeden Takt in der Luft).
fn kurs_merken(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    if snap.on_ground {
        stats.kurs_verlauf.clear();
        return;
    }
    let level = snap.vertical_speed_fpm.abs() < crate::grenzwerte::grenzwerte().phasen_kern.holding_level_vs_fpm.wert
        && snap.altitude_agl_ft > crate::grenzwerte::grenzwerte().phasen_kern.holding_min_agl_ft.wert;
    stats.kurs_verlauf.push_back((now, snap.heading_deg_true, level));
    while stats
        .kurs_verlauf
        .front()
        .is_some_and(|(t, _, _)| (now - *t).num_seconds() > crate::grenzwerte::grenzwerte().phasen_kern.holding_kurs_fenster_secs.wert)
    {
        stats.kurs_verlauf.pop_front();
    }
}

/// Gleichsinnig aufsummierte Kursänderung der letzten `fenster_secs` (Grad,
/// rechts positiv). `nur_level`: nur Schritte, deren beide Enden im
/// Höhenflug lagen.
fn kurs_drehung_deg(stats: &FlightStats, now: DateTime<Utc>, fenster_secs: i64, nur_level: bool) -> f32 {
    let v = &stats.kurs_verlauf;
    v.iter()
        .zip(v.iter().skip(1))
        .filter(|((t0, _, l0), (_, _, l1))| {
            (now - *t0).num_seconds() <= fenster_secs && (!nur_level || (*l0 && *l1))
        })
        .map(|((_, k0, _), (_, k1, _))| (k1 - k0 + 540.0) % 360.0 - 180.0)
        .sum()
}

/// Seit mindestens `HOLDING_AUSTRITT_FENSTER_SECS` (fast) geradeaus?
fn kurs_ruhig(stats: &FlightStats, now: DateTime<Utc>) -> bool {
    let abgedeckt = stats
        .kurs_verlauf
        .front()
        .is_some_and(|(t, _, _)| (now - *t).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.holding_austritt_fenster_secs.wert);
    abgedeckt
        && kurs_drehung_deg(stats, now, crate::grenzwerte::grenzwerte().phasen_kern.holding_austritt_fenster_secs.wert, false).abs()
            < crate::grenzwerte::grenzwerte().phasen_kern.holding_austritt_drehung_deg.wert
}

// ---- v0.5.11: AGL-derivative touchdown VS estimator -------------------
//
// Pilot's deep analysis pinpointed the bug class in v0.5.5+:
// "approach_vs_min_fpm" picks the most-negative VS across the WHOLE
// approach. A pre-flare descent at -1346 fpm @ 943 ft AGL would win
// over the actual gentle touchdown at -200 fpm @ 0 ft. Result:
// "phantom hard landing" reports.
//
// Correct approach: ONLY count samples close to the ground for
// touchdown VS. The window-tier strategy (LandingRate-1.lua, Volanta)
// uses geometric AGL change over a tight time window centred on the
// actual touchdown moment. Pre-flare descent gets ignored entirely.
//
// Window tiers (most preferred first):
//   1. 750 ms   — dense plugin-fed samples (≥5 samples)
//   2. 1000 ms  — typical RREF cadence
//   3. 1500 ms  — slower RREF (still inside flare)
//   4. 2000 ms  — wider for very sparse RREF
//   5. 3000 ms  — last "real" tier
//   6. 12000 ms — sparse fallback (the famous 9-sec gap in pilot
//                 reports). Lower confidence but better than nothing.
//
// All tiers must satisfy:
//   * Last sample AGL ≤ 5 ft  (genuine touchdown frame, not Sim-glitch)
//   * Earliest sample AGL ≤ 250 ft (= we're below pattern altitude)
//   * Sample count ≥ MIN_SAMPLES (5 for short tiers, 3 for sparse)
//   * Result < 0 fpm (no positive "landing rate" is physical)

// Wert seit v3: bundle/grenzwerte/landung.json -> td_window_tiers_ms (v2.1.24: TD_WINDOW_TIERS_MS)
// Wert seit v3: bundle/grenzwerte/landung.json -> td_agl_max_at_touchdown_ft (v2.1.24: 5.0)
// Wert seit v3: bundle/grenzwerte/landung.json -> td_agl_max_at_window_start_ft (v2.1.24: 250.0)

/// Result of a touchdown VS estimation, including diagnostics so the
/// rest of the system can log which window won and its sample
/// density.
#[derive(Debug, Clone, Copy)]
pub struct TouchdownVsEstimate {
    /// Negative fpm (descent rate). Always strictly < 0.
    pub fpm: f32,
    pub source: &'static str,
    pub window_ms: i64,
    pub sample_count: usize,
}

/// Estimate touchdown VS from snapshot_buffer AGL samples.
///
/// Walks the window tiers in order (shortest first); returns the
/// first tier that satisfies all guards. Returns `None` when no
/// window has enough samples or all windows give a non-negative
/// (= unphysical) result.
fn estimate_xplane_touchdown_vs_from_agl(
    buffer: &std::collections::VecDeque<TelemetrySample>,
    touchdown_at: DateTime<Utc>,
) -> Option<TouchdownVsEstimate> {
    for &(window_ms, min_samples) in &crate::grenzwerte::grenzwerte().landung.td_window_tiers_ms.wert {
        let win_start = touchdown_at - chrono::Duration::milliseconds(window_ms);
        let samples: Vec<&TelemetrySample> = buffer
            .iter()
            .filter(|s| s.at >= win_start && s.at <= touchdown_at)
            .collect();
        if samples.len() < min_samples {
            continue;
        }
        // Touchdown sample (latest) must really be at the ground.
        // v0.5.12: accept on_ground=true OR AGL ≤ 5 ft. MSFS reports
        // AGL ≈ 9.4 ft (sometimes 13 ft) even when on_ground=true at
        // the physical contact frame — this is a sim quirk, not a
        // pre-touchdown sample. Strict AGL ≤ 5 ft alone would reject
        // every MSFS touchdown.
        let last = samples.last()?;
        let is_at_touchdown = last.on_ground || last.agl_ft <= crate::grenzwerte::grenzwerte().landung.td_agl_max_at_touchdown_ft.wert;
        if !is_at_touchdown {
            continue;
        }
        // Window-start sample must already be near the ground (i.e.
        // we're not picking up a high-altitude pre-flare descent).
        let first = samples.first()?;
        if first.agl_ft > crate::grenzwerte::grenzwerte().landung.td_agl_max_at_window_start_ft.wert {
            continue;
        }
        // Geometric descent rate via avg-AGL midpoint trick
        // (LandingRate-1.lua method). For a linear descent the avg
        // is the time-midpoint; the rate from midpoint → now over
        // (timespan / 2) is the geometric descent rate.
        let avg_agl: f32 = samples.iter().map(|s| s.agl_ft).sum::<f32>() / samples.len() as f32;
        let timespan_sec = (last.at - first.at).num_milliseconds() as f32 / 1000.0;
        if timespan_sec < 0.2 {
            continue;
        }
        let agl_midpoint = last.agl_ft - avg_agl;
        let fpm = (agl_midpoint / (timespan_sec / 2.0)) * 60.0;
        // Reject positive (climbing) results — physically wrong for
        // a touchdown estimate.
        if !fpm.is_finite() || fpm >= 0.0 {
            continue;
        }
        return Some(TouchdownVsEstimate {
            fpm,
            source: match window_ms {
                750 => "agl_750ms",
                1000 => "agl_1000ms",
                1500 => "agl_1500ms",
                2000 => "agl_2000ms",
                3000 => "agl_3000ms",
                12000 => "agl_sparse_12s",
                _ => "agl_unknown",
            },
            window_ms,
            sample_count: samples.len(),
        });
    }
    None
}

/// Discard non-negative VS values. Used as a safety filter on every
/// fallback source (sampler, plugin, buffer-min). A positive
/// "landing rate" is physically impossible — the airframe is by
/// definition descending (or settled at 0) at the touchdown frame.
/// Plugin / buffer values can briefly read positive due to gear
/// oscillation rebound; we never want those as the published
/// landing rate.

/// Alle Sinkraten-Messungen eines Aufsetzers.
///
/// `letztes_tief` ist bereits auf die 15-Sekunden-Grenze gefiltert, und
/// alle Werte sind bereits durch `negative_only` gegangen, wo die
/// regulaere Kette das verlangt. Damit ist die Entscheidung darunter
/// rein — und pruefbar.
#[derive(Debug, Clone, Copy, Default)]
struct Sinkratenquellen {
    gerastet: Option<f32>,
    schaetzer_xp: Option<(f32, i64)>,
    schaetzer_msfs: Option<f32>,
    abtaster: Option<f32>,
    puffer_min: Option<f32>,
    tief_agl_min: Option<f32>,
    letztes_tief: Option<f32>,
}

/// Die Aufsetz-Sinkrate UND die Angabe, woher sie stammt.
///
/// # Warum beides aus EINER Entscheidung faellt
///
/// ⚠ Bis v1.7.13 waren es zwei: eine Kette rechnete den Wert, darunter
/// ein „Spiegel der Priority-Chain oben" die Quellenangabe. Sie sind
/// auseinandergelaufen — bei SRR318 (LIMJ, 31.08.2026) meldete die
/// Quelle einen Zweig, der `est.min(last)` rechnet und bei einem
/// Schaetzwert von −1389 fpm niemals 0 ergeben kann. Gespeichert war
/// trotzdem 0, phpVMS bekam `landing_rate = NULL`, die Wartung feuerte
/// nicht.
///
/// Als eigene Funktion, weil die Kette im Streamer sonst nur ueber den
/// Touchdown-Abtaster erreichbar waere — eine eigene Aufgabe, die ein
/// Test nicht aufstellt. Ein Ablauftest darauf war zweimal WIRKUNGSLOS
/// (der Flug erreichte nie einen Aufsetzer), bevor das auffiel.
fn touchdown_vs_bestimmen(
    q: &Sinkratenquellen,
    ist_msfs: bool,
    ist_xplane: bool,
) -> (f32, &'static str) {
    if ist_msfs {
        // MSFS: priority chain (v0.5.12 — validated against 11 real
        // pilot flights, Pete + Michael):
        //   1. Latched SimVar (PLANE TOUCHDOWN NORMAL VELOCITY)
        //   2. Time-tier AGL-Δ (UNCHANGED, GEES-aligned)
        //   3. Buffered VS-min — last resort
        //
        // EXPLICITLY NOT used for MSFS: `abtaster` und `tief_agl_min`
        // (Rueckstoss-Spitzen beim Fahrwerkskontakt) sowie der
        // X-Plane-Schaetzer.
        q.gerastet
            .map(|v| (v, "msfs_simvar_latched"))
            .or_else(|| q.schaetzer_msfs.map(|v| (v, "agl_estimate_msfs")))
            .or_else(|| q.puffer_min.map(|v| (v, "buffer_min")))
            .unwrap_or((0.0, "fallback_zero"))
    } else if ist_xplane {
        // v0.5.35: Fenster > 3000 ms ist unplausibel — es spannt den
        // ganzen Anflug statt der Flare und glaettet die
        // Aufsetz-Sinkrate weg. Dann rettet `letztes_tief`.
        let schaetzer = q.schaetzer_xp.filter(|(_, f)| *f < 3000).map(|(v, _)| v);
        match (schaetzer, q.letztes_tief) {
            (Some(est), Some(last)) => (est.min(last), "agl_estimate_xp_or_last_low"),
            (Some(est), None) => (est, "agl_estimate_xp"),
            (None, Some(last)) => (last, "last_low_agl_vs"),
            (None, None) => q
                .abtaster
                .map(|v| (v, "sampler_gear_force"))
                .or_else(|| q.puffer_min.map(|v| (v, "buffer_min")))
                .or_else(|| q.tief_agl_min.map(|v| (v, "low_agl_vs_min")))
                .unwrap_or((0.0, "fallback_zero")),
        }
    } else {
        // Unbekannter Simulator — der vorsichtigere MSFS-Schaetzer.
        q.gerastet
            .map(|v| (v, "other_latched"))
            .or_else(|| q.schaetzer_msfs.map(|v| (v, "other_agl_estimate")))
            .or_else(|| q.abtaster.map(|v| (v, "other_sampler")))
            .or_else(|| q.puffer_min.map(|v| (v, "other_buffer_min")))
            .unwrap_or((0.0, "other_fallback"))
    }
}

fn negative_only(value: Option<f32>) -> Option<f32> {
    value.filter(|v| v.is_finite() && *v < 0.0)
}
// v1.6.3: Der gesamte MSFS-Entlagungs-Apparat (Konstanten, g-Modell,
// Kurzfenster-Schaetzer, Entscheidungsgatter, Anwendung) ist hier
// ersatzlos entfernt.
//
// Er war ein Pflaster auf dem gedaempften SimVar-Kanal und ist mit der
// Umstellung von `vs_at_edge_fpm` auf die Hoehenkurve gegenstandslos
// geworden. Zwei Gruende, ihn nicht "fuer alle Faelle" liegen zu lassen:
// er lief nachweislich nie (null Marker-Treffer in 692 Aufsetzvorgaengen),
// und er konnte konstruktiv nur nach unten korrigieren — den haeufigeren
// Fall "zu weich gemeldet" haette er nie erfasst. Wer die Herleitung
// braucht: docs/release-notes/v0.16.21.md und der Befundbericht zur
// Sinkrate vom 14.08.2026.

// ---- v0.5.13: Lua-style adaptive 30-sample AGL-Δ estimator ------------
//
// Direct port of LandingRate-1.lua's algorithm (Dan Berry, 2014+,
// X-Plane.org "A New Landing Rate Display"). Matches what Volanta uses
// internally for X-Plane touchdown capture.
//
// Key insight: instead of fixed time windows (750ms/1s/1.5s/...) that
// can pick up pre-flare contamination on sparse RREF feeds, take a
// FIXED COUNT of recent samples (30) and adapt the time window to
// however dense the buffer is.
//
// Effect:
//   * High-fps sim (60+ fps): 30 samples ≈ 0.5 s window — tight, captures
//     just the flare's final phase
//   * Mid-fps sim (30 fps): 30 samples ≈ 1 s window — Lua's reference point
//   * Low-fps sim (10 fps): 30 samples ≈ 3 s window — wider window to
//     compensate for sparse data
//
// AGL guards (TD ≤ 5 ft / on_ground=true, window-start ≤ 250 ft) are
// identical to the time-tier estimator — they're what prevents
// pre-flare descent contamination, regardless of window size.
//
// Used ONLY for the X-Plane priority path. MSFS keeps the older
// time-tier `estimate_xplane_touchdown_vs_from_agl` as fallback (when
// the latched MSFS SimVar is null) — that path is GEES-aligned and
// validated against pilot reports; we don't change MSFS behaviour.
// Wert seit v3: bundle/grenzwerte/landung.json -> lua_style_sample_count (v2.1.24: 30)
// Wert seit v3: bundle/grenzwerte/landung.json -> lua_style_min_samples (v2.1.24: 5)

fn estimate_xplane_touchdown_vs_lua_style(
    buffer: &std::collections::VecDeque<TelemetrySample>,
    touchdown_at: DateTime<Utc>,
) -> Option<TouchdownVsEstimate> {
    // Collect samples up to (and including) touchdown_at, in chronological order.
    let mut samples: Vec<&TelemetrySample> =
        buffer.iter().filter(|s| s.at <= touchdown_at).collect();
    samples.sort_by_key(|s| s.at);
    let n = samples.len();
    if n < crate::grenzwerte::grenzwerte().landung.lua_style_min_samples.wert {
        return None;
    }
    let take = n.min(crate::grenzwerte::grenzwerte().landung.lua_style_sample_count.wert);
    let recent = &samples[n - take..];

    // Touchdown sample (latest) must really be at the ground.
    let last = *recent.last()?;
    let is_at_touchdown = last.on_ground || last.agl_ft <= crate::grenzwerte::grenzwerte().landung.td_agl_max_at_touchdown_ft.wert;
    if !is_at_touchdown {
        return None;
    }
    // Window-start sample must already be in approach footprint
    // (no high-altitude pre-flare contamination).
    let first = *recent.first()?;
    if first.agl_ft > crate::grenzwerte::grenzwerte().landung.td_agl_max_at_window_start_ft.wert {
        return None;
    }

    let avg_agl: f32 = recent.iter().map(|s| s.agl_ft).sum::<f32>() / take as f32;
    let timespan_sec = (last.at - first.at).num_milliseconds() as f32 / 1000.0;
    if timespan_sec < 0.2 {
        return None;
    }
    let agl_midpoint = last.agl_ft - avg_agl;
    let fpm = (agl_midpoint / (timespan_sec / 2.0)) * 60.0;
    if !fpm.is_finite() || fpm >= 0.0 {
        return None;
    }
    Some(TouchdownVsEstimate {
        fpm,
        source: "lua_30_sample",
        window_ms: (timespan_sec * 1000.0) as i64,
        sample_count: take,
    })
}

// Compute population standard deviations of V/S and bank over the
// approach buffer. Returns `(None, None)` when fewer than 3 samples
// (insufficient signal). Single-pass formula (Welford's algorithm
// would be marginally more numerically stable but at this scale it
// makes no difference).
// v0.5.46: Window-Filter analog zu compute_approach_stability_v2.
// Adrian (User-Feedback 2026-05-09): "bei der Anflugstabilität wird noch
// der gerade Teil auf dem LOC ausgewertet, bevor es auf den G/S geht.
// Daher gibt es hier so schlechte Werte." Dazu Flare die letzten 3 sec
// vor TD (Pilot-Pull) faelschlich als Instabilitaet gewertet.
//
// Filter:
//   - AGL <= 1500 ft (LOC-Intercept ueber 1500ft ausgeschlossen)
//   - ts <= TD - 3s wenn td_ts gegeben (Flare-Last-3s ausgeschlossen)
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> approach_stability_agl_cap_ft (v2.1.24: 1500.0)
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> approach_flare_cutoff_ms (v2.1.24: 3000)

// Mindest-Stichprobe fuer eine Stabilitaets-Streuung.
//
// v0.20.1: Vorher reichten 3 Samples. Im VPS-Korpus (678 Landungen) lieferten
// genau die duennen Fenster den Unsinn: bei 1-9 Samples war jede zweite
// V/S-Streuung ueber 1000 fpm, bei einem Flug 12.810. Eine Streuung aus einer
// Handvoll Ticks ist keine Aussage ueber den Anflug — sie ist Rauschen, das
// als Note beim Piloten ankommt. Unter dieser Grenze gibt es lieber KEINEN
// Wert (der Sub-Score wird dann als "nicht bewertet" uebersprungen) als eine
// erfundene Zahl.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> stability_min_samples (v2.1.24: 10)

// v0.16.6: sampler-path stability-evaluation window. When a validated 50 Hz
// sampler touchdown evaluates approach stability (bush flights — the FSM
// never reaches Landing), only approach-buffer samples within this many
// seconds BEFORE the touchdown count. The phase-independent sampling
// (`should_push_approach_sample`) feeds the buffer on ANY descending
// low-level segment, so a 38-min Oregon-bush-tour leg flown at 1500 ft AGL
// would otherwise poison the 1000-ft gate with manoeuvring that has nothing
// to do with the actual approach. 180 s covers even a slow C152 final from
// well outside the gate. FSM-path flights pass `window = None` and are
// unaffected.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> sampler_stability_window_secs (v2.1.24: 180)

/// v0.16.6: time-window filter for the approach buffer — keeps only samples
/// within `dur` before `td` (samples after `td` are dropped too). Shared by
/// `compute_approach_stability_v2` (window = Some) and the sampler-path
/// `compute_approach_stddev` call, so both stats families see the SAME
/// windowed sample set.
fn approach_buffer_window(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    td: DateTime<Utc>,
    dur: chrono::Duration,
) -> std::collections::VecDeque<ApproachBufferSample> {
    buf.iter()
        .filter(|s| s.at <= td && (td - s.at) <= dur)
        .cloned()
        .collect()
}

// v0.12.1 (Stream C LE12): the `max V/S deviation < 500 ft` metric
// ignores samples below this height. A V/S excursion at < 50 ft is the
// flare / ground-effect, not approach instability — counting it produced
// a spurious 826 fpm "deviation" on GSG225 from a small flare balloon.
// The 3-second time-based flare cutoff doesn't reach far enough back
// when the pilot flares early/long; this height floor is the backstop.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> max_vs_dev_floor_ft (v2.1.24: 50.0)

// v0.12.1 (Stream C LE11): if flaps read 0 across the whole approach
// gate while the gear is down AND the gate-entry speed is below this
// IAS, the flaps dataref is untrustworthy (study-level X-Plane add-ons
// such as the Hot-Start CL650 don't drive the standard flap dataref) —
// a genuine flapless landing would be considerably faster. The
// LANDING-CONFIG check then reports "not assessable" instead of a
// false "INCOMPLETE".
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> flaps_unreadable_max_ias_kt (v2.1.24: 160.0)
// Klappenstellung, unterhalb derer eine Landekonfiguration (Fahrwerk
// unten, Landegeschwindigkeit) als Messfehler statt Pilotenfehler gilt —
// siehe Kommentar am `flaps_unreadable`-Check. 0.25 fängt die bekannten
// Skalen-Artefakte (÷5-Doppelnormalisierung → 0.2) und liegt klar unter
// jeder echten Lande-Klappenstufe.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> flaps_implausible_below (v2.1.24: 0.25)
// Klappenstellung (normiert 0..1), ab der die Landekonfiguration am
// 1000-ft-Tor als erfuellt gilt. Gilt fuer jedes Muster, dessen Raster der
// Sim auf 1,0 skaliert — das ist die grosse Mehrheit.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> flaps_landing_min (v2.1.24: 0.70)
// Geschwindigkeitsfenster eines stabilisierten Anflugs oberhalb Vref
// (Flight Safety Foundation ALAR, Briefing Note 7.1: Vref … Vref+20).
//
// Wird als Physik-Gegenprobe gebraucht, wenn die Klappenzahl unter
// `FLAPS_LANDING_MIN` liegt, aber schon das Maximum dieses Fluges ist:
// mit deutlich weniger als Landeklappen kommt niemand auf Vref+20 herunter.
// Passt die Geschwindigkeit, misst die Zahl das Raster des Sims und nicht
// den Piloten.
// Wert seit v3: bundle/grenzwerte/anflug_kern.json -> stabilisiert_ueber_vref_kt (v2.1.24: 20.0)

/// Was `compute_approach_stability_v2` ueber das geflogene Muster wissen
/// muss, um die Landekonfiguration zu beurteilen.
///
/// Alle Felder duerfen `None` sein — dann verhaelt sich die Bewertung exakt
/// wie vor v1.6.6 (Audit-Invariante: keine Regression ohne Daten).
#[derive(Debug, Clone, Copy, Default)]
struct KonfigKontext {
    /// Typische Referenzgeschwindigkeit des Musters aus der Grenzen-Tabelle.
    /// `None` = Muster unbekannt → die Rasterkorrektur unten bleibt aus,
    /// weil ohne Geschwindigkeits-Anker ein fremdes Raster nicht von einer
    /// echten Unter-Klappung zu unterscheiden ist.
    typical_vref_kt: Option<f32>,
    /// Hat sich der Klappenwert im ganzen Flug ueberhaupt je bewegt?
    /// `Some(false)` = der Kanal traegt keine Information.
    flaps_bewegt: Option<bool>,
    /// Hoechste Klappenstellung des ganzen Fluges.
    flaps_max_im_flug: Option<f32>,
}

fn compute_approach_stddev(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    td_ts: Option<DateTime<Utc>>,
) -> (Option<f32>, Option<f32>) {
    let filtered: Vec<&ApproachBufferSample> = buf
        .iter()
        .filter(|s| {
            let in_agl_band = s.agl_ft > 0.0 && s.agl_ft <= crate::grenzwerte::grenzwerte().anflug_kern.approach_stability_agl_cap_ft.wert;
            let pre_flare = match td_ts {
                Some(td) => (td - s.at).num_milliseconds() >= crate::grenzwerte::grenzwerte().anflug_kern.approach_flare_cutoff_ms.wert,
                None => true,
            };
            in_agl_band && pre_flare
        })
        .collect();
    // v0.20.1: EINE Formel (`stability_stddev`) — sie prueft auch die
    // Mindest-Stichprobe (war hier 3; im VPS-Korpus kam genau aus solchen
    // duennen Fenstern der 12.810-fpm-Unsinn). Lieber kein Wert, den der
    // Sub-Score dann als "nicht bewertet" ueberspringt, als eine erfundene
    // Zahl, die als Note beim Piloten landet.
    (
        stability_stddev(filtered.iter().map(|s| s.vs_fpm)),
        stability_stddev(filtered.iter().map(|s| s.bank_deg)),
    )
}
