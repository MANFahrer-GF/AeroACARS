/// Emit the landing analyzer's verdict to the activity log exactly once,
/// the first time `step_flight` finalises a score. Called every tick;
/// idempotent by design — uses an extra "already announced" flag in
/// `FlightStats` so resumed sessions that already filed don't re-emit.
/// Returns `Some(<acars-log-message>)` exactly once (the first tick after
/// the touchdown analyzer locked in a score) so the streamer can mirror
/// the touchdown summary into `POST /pireps/{id}/acars/logs` for the
/// PIREP detail page. Subsequent ticks return None.
/// Der G-Teil einer Protokollzeile. Ohne gemessenes Fenster steht dort
/// "G nicht gemessen" — nie eine erfundene Null (Codex, dritte Abnahme).
fn g_protokolltext(scored_g: Option<f32>, fenster_gemessen: bool) -> String {
    match scored_g {
        Some(g) => format!("G {g:.2}"),
        None if !fenster_gemessen => "G nicht gemessen".to_string(),
        None => "G 0.00".to_string(),
    }
}

/// Die zwei Meldungszeilen zum Touchdown (Aktivitaetslog, ACARS-Spiegel) samt
/// Herkunft der Note (P-4.40, Kap. 4.5): endgueltig = wie bisher (nur
/// Messdaten); `vorlaeufig` = Note mit Vermerk; `modulfehler` = „Bewertung
/// nicht moeglich, wird auf dem Server nachgeholt“.
fn touchdown_meldungen(
    peak_vs: f32,
    g_text: &str,
    bounce_part: &str,
    bewertung: Option<&LandungsBewertung>,
) -> (String, String) {
    let zusatz = match bewertung {
        Some(b) if b.status == "vorlaeufig" => match b.verdict {
            Some(v) => format!(" — Note {} (vorläufig)", v.grade),
            None => " — Bewertung vorläufig".to_string(),
        },
        Some(b) if b.status == "modulfehler" => {
            " — Bewertung nicht möglich, wird auf dem Server nachgeholt".to_string()
        }
        _ => String::new(),
    };
    (
        format!("Touchdown: V/S {peak_vs:.0} fpm, {g_text}{bounce_part}{zusatz}"),
        format!("Touchdown — V/S {peak_vs:.0} fpm, {g_text}{bounce_part}{zusatz}"),
    )
}

fn announce_landing_score(app: &AppHandle, flight: &ActiveFlight) -> Option<String> {
    let mut stats = flight.stats.lock().expect("flight stats");
    if stats.landing_score_announced {
        return None;
    }
    let score = stats.landing_score?;

    // v0.5.41: Auf Sampler-Finalisierung warten (= Buffer wurde 10 s post-TD
    // gedumpt + Score mit vs_at_edge neu berechnet). Verhindert dass der
    // Live-Monitor den ueberholten msfs_simvar_latched-Wert sieht.
    // Fallback-Timeout: 12 s post-TD ohne Finalisierung → trotzdem
    // emitten mit den vorhandenen Werten (= Buffer war evtl. leer wegen
    // Sample-Luecke, oder sampler ist abgestuerzt). Verhindert dass der
    // Touchdown bei einem fehlerhaften Buffer-Path nie gemeldet wird.
    if !stats.landing_score_finalized {
        if let Some(td_at) = stats.sampler_touchdown_at {
            let elapsed_ms = (Utc::now() - td_at).num_milliseconds();
            if elapsed_ms < SAMPLER_DUMP_WAIT_TIMEOUT_MS {
                return None; // weiter warten
            }
            // Timeout — trotzdem freigeben, mit Hinweis im Trace
            tracing::warn!(
                pirep_id = %flight.pirep_id,
                elapsed_ms,
                "score finalization timeout — emitting with sampler-only values (buffer dump may have failed)"
            );
            stats.landing_score_finalized = true;
        }
        // Wenn sampler_touchdown_at None ist (= Sampler hat keinen Edge
        // detektiert, FSM-Pfad scored stattdessen) → sofort freigeben.
    }
    // v0.20.0 (PIA3452): kanonisch lesen, nicht `landing_peak_vs_fpm` direkt.
    // Das Rohfeld konnte v2s Fenster-Minimum tragen, waehrend Karte, Score und
    // PIREP ueber die Kanonik den 50-Hz-Edge zeigten → Pilot sah "-233 fpm" im
    // ACARS-Log und "-206 fpm" auf der Landing-Karte fuer denselben Touchdown.
    let peak_vs = stats.canonical_landing_rate_fpm().unwrap_or(0.0);
    // Codex, dritte Abnahme 13.09.2026: Bei gültiger MSFS-Sinkrate und dünnem
    // Fenster läuft diese Ankündigung weiter. Die G-Werte waren schon gesperrt,
    // wurden hier aber zu "G 0.00" — eine Nullmessung, wo gar nicht gemessen
    // wurde — und die ungemessenen Hopser wurden angehängt. Beides kommt jetzt
    // aus denselben Helfern wie überall sonst.
    let fenster_gemessen = stats.landung_abdeckung_fehlt.is_none();
    // Für Protokoll und Payload bleiben es Optionen — `None` heisst "nicht
    // messbar". Nur die Textzeile und die Einstufung brauchen eine Zahl.
    let peak_g_opt = aus_dem_fenster(&stats, stats.landing_peak_g_force);
    let bounces_opt = hopser_aus_dem_fenster(&stats);
    let bounces = bounces_opt.unwrap_or(0) as u8;
    // v0.12.3 (LE7/LE8): der gescorte G-Wert (EMA, sonst raw_fallback) —
    // für den Activity-/ACARS-Text UND das persistierte LandingScored-
    // Event. `peak_g` (oben) bleibt der rohe Wert für `peak_g_force`.
    let sg = score_g_for_stats(&stats);
    let level = match score {
        LandingScore::Smooth | LandingScore::Acceptable => ActivityLevel::Info,
        LandingScore::Firm => ActivityLevel::Info,
        LandingScore::Hard | LandingScore::Severe => ActivityLevel::Warn,
    };
    let bounce_part = if bounces > 0 {
        format!(
            ", {} bounce{}",
            bounces,
            if bounces == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    // P-4.40: Herkunft der Note fuer die Meldung (der Touchdown-Payload folgt
    // im selben Takt mit demselben Status).
    // Codex-Befund: EINMAL rechnen und festhalten — der Touchdown-Payload im
    // selben Takt liest sie (`touchdown_payload_note`), statt neu zu rechnen.
    let bewertung = touchdown_bewertung_festhalten(flight, &mut stats);
    let g_text = g_protokolltext(sg.as_ref().map(|s| s.scored_g), fenster_gemessen);
    let (meldung_log, meldung_acars) = touchdown_meldungen(peak_vs, &g_text, &bounce_part, bewertung.as_ref());
    // v0.16.27: Das Aktivitäts-/ACARS-Log zeigt beim Touchdown nur noch die
    // MESSDATEN (V/S, G, Bounces) — KEINE Einstufung. Darum brauchen wir hier
    // weder die Aufprall-Note (`letter_grade`) noch den Aggregate-Master-Score:
    // der bewertete Score lebt ausschließlich im Landung-Tab + phpVMS-Feld
    // „Landing Score". (Vorher zeigte der Log-Titel die Note NUR des Aufpralls
    // [V/S+G], der Tab die Note des GANZEN Anflugs [Master] → zwei Metriken,
    // als nackter Buchstabe wirkten sie widersprüchlich: „Touchdown B" vs „A".)
    // v0.8.0: Per-Touchdown Navdata-Source + DDS-Warnung sammeln,
    // bevor wir `stats` droppen.
    let nav_source_msg: Option<(ActivityLevel, String)> =
        stats.runway_match.as_ref().and_then(|rm| {
            let src = stats.runway_source?;
            let label = match src {
                runway::RunwaySource::Navigraph => format!(
                    "Runway {} {} matched via Navigraph (AIRAC {})",
                    rm.runway_ident,
                    rm.airport_ident,
                    stats.runway_nav_cycle.as_deref().unwrap_or("?")
                ),
                runway::RunwaySource::OurAirportsFallback => format!(
                    "Runway {} {} matched via OurAirports-Fallback",
                    rm.runway_ident, rm.airport_ident
                ),
            };
            let level = match src {
                runway::RunwaySource::Navigraph => ActivityLevel::Info,
                runway::RunwaySource::OurAirportsFallback => ActivityLevel::Warn,
            };
            Some((level, label))
        });
    // v0.19.x FIX: this used to hand-roll its own displaced-threshold
    // check (`td_m < 0.0 && td_m > -displaced_m` against the RAW
    // pavement-relative distance) instead of calling `classify_displaced`
    // — the exact sign bug already fixed there (see that function's doc
    // comment): checking `td_m < 0` only catches an undershoot before the
    // physical pavement even starts, never a touchdown ON the displaced-
    // threshold paint itself (0 <= td_m < displaced_m), which is the real
    // violation those chevron markings warn about. It was also gated on
    // `runway_nav_geometry` (Navigraph-only), so it never fired at all
    // for an OurAirports-fallback match. `assess_touchdown` is the single
    // source of truth for this classification (correct sign, both
    // sources) — reuse it instead of a second, divergent copy.
    let dds_warning_msg: Option<String> = assess_touchdown(&stats).dds.and_then(|d| {
        if d.in_pre_threshold_zone {
            Some(format!(
                "Touchdown im Pre-Threshold-Bereich (DDS {:.0} m vor Landing-Threshold)",
                d.displaced_threshold_m
            ))
        } else {
            None
        }
    });
    drop(stats);
    log_activity_handle(
        app,
        level,
        // nur Messdaten — keine Note/Einstufung (siehe Kommentar oben)
        // (V/S signed: negative = descent, matches the PIREP; G = scored EMA,
        // ohne gemessenes Fenster "G nicht gemessen" statt 0.00, 13.09.2026)
        meldung_log,
        None,
    );
    // v0.8.0: zusätzlich die Runway-Match-Source mitloggen damit der
    // Pilot weiß ob die Bewertung auf Jeppesen-Daten oder dem
    // OurAirports-Fallback beruht.
    if let Some((src_level, src_msg)) = nav_source_msg {
        log_activity_handle(app, src_level, src_msg, None);
    }
    if let Some(msg) = dds_warning_msg {
        log_activity_handle(app, ActivityLevel::Error, msg, None);
    }
    record_event(
        app,
        &flight.pirep_id,
        &FlightLogEvent::LandingScored {
            timestamp: Utc::now(),
            score: score.label().to_string(),
            peak_vs_fpm: peak_vs,
            // Im dauerhaften Protokoll kein roher G-Wert und keine Hopser aus
            // einem ungemessenen Fenster — und keine erfundene Null an ihrer
            // Stelle (Codex, dritte und vierte Abnahme).
            peak_g_force: peak_g_opt,
            bounce_count: bounces_opt.map(|n| n.min(u8::MAX as u32) as u8),
            // v0.12.3 (LE7): EMA-Scored-G additiv; peak_g_force bleibt roh.
            // v0.20 (QS-Fix, fallback_zero-Score-Bug): echtes None statt
            // einer erfundenen Zahl im dauerhaften JSONL-Datensatz.
            scored_g_force: sg.as_ref().map(|s| s.scored_g),
            scored_g_method: sg.as_ref().map(|s| s.method.as_str().to_string()),
        },
    );
    // Re-acquire to flag it as announced.
    let mut stats = flight.stats.lock().expect("flight stats");
    stats.landing_score_announced = true;
    // acars/logs-Spiegelung (phpVMS-Serverlog): ebenfalls nur Messdaten, keine
    // Einstufung — der bewertete Score steht im phpVMS-Feld „Landing Score".
    Some(meldung_acars)
}

/// Protokollzeilen, die erst geschrieben werden, wenn der Wert aufgeräumt
/// wird — gedacht für Funktionen, die `flight.stats` halten (siehe
/// `detect_telemetry_changes`). Reihenfolge bleibt erhalten.
struct AufgeschobeneMeldungen<'a> {
    app: &'a AppHandle,
    meldungen: Vec<(ActivityLevel, String, Option<String>, bool)>,
}

impl<'a> AufgeschobeneMeldungen<'a> {
    fn neu(app: &'a AppHandle) -> Self {
        Self {
            app,
            meldungen: Vec::new(),
        }
    }

    /// Wie `log_activity_handle`, nur später.
    fn handle(
        &mut self,
        _app: &AppHandle,
        level: ActivityLevel,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        self.meldungen.push((level, message.into(), detail, true));
    }

    /// Ohne GlitchTip-Meldung — fuer Ereignisse im Flugzeug (Cockpit-Lampen),
    /// die der Pilot sehen soll, die aber kein Fehler der Software sind.
    /// Log-Durchsicht 26.09.2026: eine blinkende Fenix-MASTER-WARNING
    /// erzeugte ~530 Error-Ereignisse in GlitchTip. (Bis 2.0.18 die freie
    /// Funktion `log_activity_ohne_meldung`; sie hatte nur diese Nutzer.)
    fn ohne_meldung(
        &mut self,
        _app: &AppHandle,
        level: ActivityLevel,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        self.meldungen.push((level, message.into(), detail, false));
    }
}

impl Drop for AufgeschobeneMeldungen<'_> {
    fn drop(&mut self) {
        // Beim Abbau nach einer Panik nichts mehr schreiben: Eine zweite
        // Panik im `drop` beendete das ganze Programm. Die Zeilen gehen
        // dann verloren — die Panik selbst steht ohnehin im Log.
        if std::thread::panicking() {
            return;
        }
        for (level, message, detail, an_glitchtip) in self.meldungen.drain(..) {
            log_activity_mit_meldung(self.app, level, message, detail, an_glitchtip);
        }
    }
}

/// Diff cockpit knob state against the last-logged values and emit one
/// activity-feed entry per real change. Called from the streamer loop on
/// every tick. The first call (after a fresh `flight_start` / `flight_adopt`)
/// also logs the aircraft + simulator identity, so the log opens with a
/// "Aircraft: …" line like smartcars does.
fn detect_telemetry_changes(app: &AppHandle, flight: &ActiveFlight, snap: &SimSnapshot) {
    // v2.0.18 → 2.0.19: Protokollzeilen erst NACH dem Freigeben der
    // Flugdaten schreiben. Jede Zeile kopierte unter der Sperre das ganze
    // Aktivitätsprotokoll (bis 1000 Einträge); der Wächter meldete 24 ms
    // bei Michel, als im Steigflug eine Abriss-Warnung geschrieben wurde
    // (14.10.2026). `aufgeschoben` ist VOR `stats` angelegt und wird deshalb
    // NACH ihm aufgeräumt — auch bei jedem vorzeitigen `return`.
    let mut aufgeschoben = AufgeschobeneMeldungen::neu(app);
    let mut stats = flight.stats.lock().expect("flight stats");

    // ---- Aircraft + sim banner — first tick only.
    // Aircraft banner: gated on a dedicated per-flight flag rather
    // than the heuristic "all three diff fields are still None"
    // because the heuristic is too fragile (a stale resumed flight
    // could already have those fields set, suppressing the banner
    // when it should fire). The flag is persisted, so a Tauri
    // restart mid-flight doesn't re-fire — the banner belongs to
    // the flight, not the session.
    let first_tick = !stats.aircraft_banner_logged;
    if first_tick {
        stats.aircraft_banner_logged = true;
        let title = snap.aircraft_title.as_deref().unwrap_or("(unknown)");
        // Some aircraft (Fenix, FBW) return a localization key like
        // "ATCCOM.AC_MODEL A320.0.text" in `ATC MODEL` rather than the plain
        // ICAO. Filter that out — a stray dot in an ICAO never happens.
        let icao_raw = snap.aircraft_icao.as_deref().unwrap_or("");
        // v0.2.4: when the standard ICAO is unusable (empty, or a
        // localisation key like "ATCCOM.AC_MODEL_B738.0.text" that
        // MSFS 2024 hands out for some PMDG liveries), fall back to
        // the PMDG-variant family ICAO. That's accurate to the family
        // (B738/B77W) even if not to the sub-variant.
        let pmdg_fallback_icao: Option<&'static str> = snap.pmdg.as_ref().and_then(|_| {
            // Aircraft path tells us which PMDG family is active.
            // We don't know it from snap.pmdg.variant_label alone,
            // but we do know "PMDG SDK is active" — so we fall
            // back via the variant_label string content.
            let label = &snap.pmdg.as_ref()?.variant_label;
            if label.contains("737") {
                Some("B738")
            } else if label.contains("777") {
                // Pick the most-common 777 variant as default;
                // exact -200LR / -300ER / 777F can't be inferred
                // from the variant label alone (that's a richer
                // mapping we may add later).
                Some("B77W")
            } else {
                None
            }
        });
        let icao = if icao_raw.contains('.') || icao_raw.is_empty() {
            pmdg_fallback_icao.unwrap_or("?")
        } else {
            icao_raw
        };
        let reg = snap
            .aircraft_registration
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("?");
        // NOTE: planned_registration vs sim-reg comparison removed for now
        // — MSFS 2024's pilot-profile tail number overrides the livery's
        // atc_id, so the comparison fired false-positive warnings. We keep
        // `flight.planned_registration` populated server-side (it's still
        // useful for PIREP audit), but the live banner just shows what
        // MSFS reports. Re-enable later via a WASM-based livery reader.
        let _ = &flight.planned_registration;
        // v0.2.4: when PMDG SDK is feeding us premium telemetry, surface
        // that in the profile string so the pilot doesn't see the
        // misleading "Default (standard SimVars)" right before the
        // "PMDG SDK aktiv" entry one tick later.
        let profile_label = if let Some(pmdg) = snap.pmdg.as_ref() {
            format!("{} + {}", snap.aircraft_profile.label(), pmdg.variant_label)
        } else {
            snap.aircraft_profile.label().to_string()
        };
        aufgeschoben.handle(
            app,
            ActivityLevel::Info,
            format!("Aircraft: {title}"),
            Some(format!(
                "Type {icao} · Reg {reg} · Sim {:?} · Profile: {}",
                snap.simulator, profile_label
            )),
        );
    }

    // ---- Profile change (after first tick) — pilot swapped airframes
    if stats.last_logged_profile != Some(snap.aircraft_profile) {
        if !first_tick {
            // v0.2.4: same Premium-First label suffix as the banner —
            // PMDG SDK active means the *premium* layer is doing the
            // heavy lifting even if the underlying SimVar profile
            // is still the generic "Default" one.
            let label = if let Some(pmdg) = snap.pmdg.as_ref() {
                format!("{} + {}", snap.aircraft_profile.label(), pmdg.variant_label)
            } else {
                snap.aircraft_profile.label().to_string()
            };
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                format!("Aircraft profile changed: {}", label),
                None,
            );
        }
        stats.last_logged_profile = Some(snap.aircraft_profile);
    }

    // ---- Squawk
    // Real-world squawks are always 4 octal digits ≥ 1000 — Fenix and a
    // few other study-level airbuses pump the active-edit-digit into
    // TRANSPONDER CODE:1 while the pilot keypad-types, which the FlyByWire
    // / Asobo aircraft don't do. Filter anything < 1000 so the log isn't
    // flooded with "Squawk set to 0004 / 0003 / 0002" while a value like
    // 2523 is being entered. The proper fix lives in the per-aircraft
    // LVar profile (Phase H.4 stage 2).
    if let Some(sq) = snap.transponder_code {
        let is_plausible = sq >= 1000;
        if is_plausible && stats.last_logged_squawk != Some(sq) {
            // Don't spam on the first tick if the value is the boring 1200/7000.
            if !first_tick || (sq != 1200 && sq != 7000) {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Squawk set to {:04}", sq),
                    None,
                );
            }
            stats.last_logged_squawk = Some(sq);
        }
    }

    // ---- XPDR mode (v0.2.4 PMDG + v0.3.0 X-Plane).
    // Cockpit exposes the actual transponder MODE selector
    // (STBY / ALT-OFF / XPNDR / TA / TA-RA) which the standard MSFS
    // SimVar `TRANSPONDER STATE` only exposes as a binary on/off.
    // Now sourced from `snap.xpdr_mode_label` which the MSFS adapter
    // fills from PMDG and the X-Plane adapter fills from the standard
    // `transponder_mode` DataRef. Logged as a separate entry so the
    // squawk-code log keeps its original semantics.
    if let Some(mode) = snap.xpdr_mode_label.as_ref() {
        if !mode.is_empty() && stats.last_logged_pmdg_xpdr_mode.as_deref() != Some(mode.as_str()) {
            // Skip first tick on a "boring" STBY/OFF value to avoid noise
            // when the pilot loads cold-and-dark.
            if !first_tick || !matches!(mode.as_str(), "STBY" | "OFF" | "ALT-OFF" | "GND") {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("XPDR mode: {}", mode),
                    None,
                );
            }
            stats.last_logged_pmdg_xpdr_mode = Some(mode.clone());
        }
    }

    // ---- COM / NAV frequency logging removed (2026-05).
    // In practice frequencies change every sector handoff (Departure
    // → Center → Center → Approach → Tower → Ground), which fills
    // the log without telling the VA admin anything useful. Plus
    // Fenix's RMP doesn't write to the standard SimVars at all, so
    // for that aircraft we'd be logging stale defaults from the
    // sim's COM panel rather than what the pilot actually tuned.
    // Squawk stays in the log — that genuinely changes only ~1-2x
    // per flight at meaningful moments.
    //
    // The fields on the snapshot remain populated so the debug
    // panel and inspector keep working.
    let _ = (
        snap.com1_mhz,
        snap.com2_mhz,
        snap.nav1_mhz,
        snap.nav2_mhz,
        &stats.last_logged_com1,
        &stats.last_logged_com2,
        &stats.last_logged_nav1,
        &stats.last_logged_nav2,
    );

    // ---- Exterior lights
    // Strobe is special: when the aircraft profile gives us the
    // 3-state selector (`snap.strobe_state`), we log OFF/AUTO/ON so
    // the AUTO ↔ ON transition at runway entry/exit is preserved.
    // For aircraft that only expose the binary `light_strobe`, we
    // fall through to the LightsState path below with plain ON/OFF
    // labels.
    let strobe_three_state_active = snap.strobe_state.is_some();
    if let Some(state) = snap.strobe_state {
        if stats.last_logged_strobe_state != Some(state) {
            if stats.last_logged_strobe_state.is_some() {
                let label = match state {
                    0 => "OFF",
                    1 => "AUTO",
                    2 => "ON",
                    _ => return,
                };
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Strobe lights {label}"),
                    None,
                );
            }
            stats.last_logged_strobe_state = Some(state);
        }
    }

    // Premium-First (v0.2.4 + v0.3.0): wing + wheel-well are
    // NG3/777-only via PMDG (MSFS) or laminar/B738-only via the
    // X-Plane 737 family (Zibo / LevelUp / default-738). Both
    // simulators now write into the top-level SimSnapshot fields,
    // so we read directly from snap.light_wing / snap.light_wheel_well.
    // Field is `Some(...)` when the source platform actually reports
    // the switch (kept None on aircraft that don't have it — generic
    // Airbus, Cessna, etc.).
    let has_wing = snap.light_wing.is_some();
    let has_wheel_well = snap.light_wheel_well.is_some();
    let lights = LightsState {
        landing: snap.light_landing.unwrap_or(false),
        beacon: snap.light_beacon.unwrap_or(false),
        // Keep the binary state in the struct so the pill UI still
        // works; the activity-log path below skips Strobe when the
        // 3-state path is active to avoid double entries.
        strobe: snap.light_strobe.unwrap_or(false),
        taxi: snap.light_taxi.unwrap_or(false),
        nav: snap.light_nav.unwrap_or(false),
        logo: snap.light_logo.unwrap_or(false),
        wing: snap.light_wing.unwrap_or(false),
        wheel_well: snap.light_wheel_well.unwrap_or(false),
    };
    if stats.last_logged_lights != Some(lights) {
        if let Some(prev) = stats.last_logged_lights {
            // Log per-light transitions so the pilot sees exactly what changed.
            let mut changes: Vec<(&str, bool, bool)> = vec![
                ("Landing", prev.landing, lights.landing),
                ("Beacon", prev.beacon, lights.beacon),
                ("Taxi", prev.taxi, lights.taxi),
                ("Nav", prev.nav, lights.nav),
                ("Logo", prev.logo, lights.logo),
            ];
            // Only include Strobe in the binary path when the 3-state
            // selector isn't available; otherwise the dedicated
            // OFF/AUTO/ON log entry above already covers it.
            if !strobe_three_state_active {
                changes.push(("Strobe", prev.strobe, lights.strobe));
            }
            // Wing + wheel-well only fire when the source platform
            // actually reports the switch (PMDG / laminar/B738) —
            // prevents bogus "Wing OFF" entries on generic aircraft.
            if has_wing {
                changes.push(("Wing", prev.wing, lights.wing));
            }
            if has_wheel_well {
                changes.push(("Wheel-well", prev.wheel_well, lights.wheel_well));
            }
            for (name, old, new) in changes {
                if old != new {
                    match flicker_verdict(&mut stats.light_flicker, name, Utc::now()) {
                        FlickerVerdict::Log => aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!("{name} lights {}", if new { "ON" } else { "OFF" }),
                            None,
                        ),
                        FlickerVerdict::AnnounceFlashing => aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!("{name} lights: Blinkmodus"),
                            None,
                        ),
                        FlickerVerdict::Suppress => {}
                    }
                }
            }
        }
        stats.last_logged_lights = Some(lights);
    }

    // ---- Autopilot
    let ap = ApState {
        master: snap.autopilot_master.unwrap_or(false),
        heading: snap.autopilot_heading.unwrap_or(false),
        altitude: snap.autopilot_altitude.unwrap_or(false),
        nav: snap.autopilot_nav.unwrap_or(false),
        approach: snap.autopilot_approach.unwrap_or(false),
    };
    if stats.last_logged_ap != Some(ap) {
        if let Some(prev) = stats.last_logged_ap {
            // Master toggle is debounced — see AP_DEBOUNCE_SECS comment.
            if prev.master != ap.master {
                let now = Utc::now();
                if stats.pending_ap_master != Some(ap.master) {
                    // First time we see this new state — start the timer.
                    stats.pending_ap_master = Some(ap.master);
                    stats.pending_ap_master_since = Some(now);
                } else if let Some(since) = stats.pending_ap_master_since {
                    // Same state held for AP_DEBOUNCE_SECS → publish.
                    if (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().cockpit.ap_debounce_secs.wert {
                        aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!("Autopilot {}", if ap.master { "ENGAGED" } else { "OFF" }),
                            None,
                        );
                        stats.last_logged_ap = Some(ap);
                        stats.pending_ap_master = None;
                        stats.pending_ap_master_since = None;
                        return;
                    }
                }
                // Don't update last_logged_ap yet — we're waiting for the
                // debounce. Mode flips below still get to flow through.
            } else {
                // Master agrees with prev → drop any pending debounce.
                stats.pending_ap_master = None;
                stats.pending_ap_master_since = None;
            }
            let modes = [
                ("HDG", prev.heading, ap.heading),
                ("ALT", prev.altitude, ap.altitude),
                ("NAV", prev.nav, ap.nav),
                ("APR", prev.approach, ap.approach),
            ];
            for (name, old, new) in modes {
                if old != new {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("AP {name} {}", if new { "ARMED" } else { "OFF" }),
                        None,
                    );
                }
            }
        }
        // Only update if master is stable (otherwise the debounce branch
        // above already returned). Mode-only changes still want to land.
        if stats
            .last_logged_ap
            .is_none_or(|prev| prev.master == ap.master)
        {
            stats.last_logged_ap = Some(ap);
        }
    }

    // ---- A/THR (v0.16.4 — generic autothrottle).
    // `snap.autothrottle_on` is only Some on profiles with a verified
    // state source (Aerosoft A346 `L:AB_AP_ATHR_LIGHT_ON` annunciator,
    // Fenix `L:S_FCU_ATHR`) — everything else stays None and logs
    // nothing. Same debounce as the AP master: the Fenix S_FCU_*
    // family is a known button-pulse candidate (B-008), so a 0→1→0
    // pulse shorter than AP_DEBOUNCE_SECS produces no entry at all
    // instead of a bogus ENGAGED/OFF pair.
    if let Some(athr) = snap.autothrottle_on {
        if stats.last_logged_autothrottle != Some(athr) {
            let now = Utc::now();
            if stats.pending_autothrottle != Some(athr) {
                // First time we see this new state — start the timer.
                stats.pending_autothrottle = Some(athr);
                stats.pending_autothrottle_since = Some(now);
            } else if let Some(since) = stats.pending_autothrottle_since {
                // Same state held for AP_DEBOUNCE_SECS → publish.
                if (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().cockpit.ap_debounce_secs.wert {
                    // First observation latches silently (no entry on
                    // the initial state, only on transitions).
                    if stats.last_logged_autothrottle.is_some() {
                        aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!(
                                "A/THR {}",
                                if athr {
                                    // v1.5.3 (#ifly-audit): iFly liefert den
                                    // ARM-Schalter, kein Engagement — am Gate
                                    // stand sonst "ENGAGED" (Thomas' Protokoll).
                                    if snap.autothrottle_is_arm {
                                        "ARMED"
                                    } else {
                                        "ENGAGED"
                                    }
                                } else {
                                    "OFF"
                                }
                            ),
                            None,
                        );
                    }
                    stats.last_logged_autothrottle = Some(athr);
                    stats.pending_autothrottle = None;
                    stats.pending_autothrottle_since = None;
                }
            }
        } else {
            // Snap matches last logged again — drop any pending change.
            stats.pending_autothrottle = None;
            stats.pending_autothrottle_since = None;
        }
    }

    // ---- Parking brake
    if stats.last_logged_parking_brake != Some(snap.parking_brake) {
        if stats.last_logged_parking_brake.is_some() {
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                format!(
                    "Parking brake {}",
                    if snap.parking_brake {
                        "SET"
                    } else {
                        "RELEASED"
                    }
                ),
                None,
            );
        }
        stats.last_logged_parking_brake = Some(snap.parking_brake);
    }

    // ---- Engines (count only — per-engine N1/N2 is a future extension)
    // Debounce 5 s: Fenix's GENERAL ENG COMBUSTION SimVar pulses
    // 0/1/0/1 during engine start and at idle, which produced phantom
    // "Engine shutdown / started" pairs in the activity log. We only
    // commit the change once the new count has held steady.
    if stats.last_logged_engines_running != Some(snap.engines_running) {
        let now = Utc::now();
        if stats.pending_engines_running != Some(snap.engines_running) {
            stats.pending_engines_running = Some(snap.engines_running);
            stats.pending_engines_running_since = Some(now);
        } else if let Some(since) = stats.pending_engines_running_since {
            if (now - since).num_seconds() >= 5 {
                if let Some(prev) = stats.last_logged_engines_running {
                    if prev < snap.engines_running {
                        aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!("Engine started — {} running", snap.engines_running),
                            None,
                        );
                    } else if prev > snap.engines_running {
                        aufgeschoben.handle(
                            app,
                            ActivityLevel::Info,
                            format!("Engine shutdown — {} running", snap.engines_running),
                            None,
                        );
                    }
                }
                stats.last_logged_engines_running = Some(snap.engines_running);
                stats.pending_engines_running = None;
                stats.pending_engines_running_since = None;
            }
        }
    } else {
        // Snap matches last logged again — drop any pending change.
        stats.pending_engines_running = None;
        stats.pending_engines_running_since = None;
    }

    // ---- Flaps detent
    // Premium-First (v0.2.4): if PMDG is loaded, use the cockpit-exact
    // Boeing handle label directly ("UP"/"1"/"5"/"15"/"30"/"40"). For
    // generic SimVars we map the normalised 0.0..1.0 position to a 0..5
    // detent — that's accurate for Airbus families (6 positions). For
    // Boeing without PMDG it's a known approximation; pilots flying
    // PMDG (the typical Boeing-study-level case) get the real labels.
    //
    // Detection: pmdg.flap_handle_label is non-empty when PMDG is the
    // source of truth. The pmdg-detent string itself is the activity-
    // log label (no detent_label() lookup needed).
    let pmdg_flap = snap
        .pmdg
        .as_ref()
        .map(|p| p.flap_handle_label.clone())
        .filter(|s| !s.is_empty());
    if let Some(label) = pmdg_flap {
        if stats.last_logged_flap_label.as_deref() != Some(label.as_str()) {
            if let Some(prev) = stats.last_logged_flap_label.as_ref() {
                // Direction: nominal numeric ordering of Boeing detents
                // ("UP" < "1" < "2" < "5" < "10" < "15" < "25" < "30" <
                // "40"). Mapping via boeing_detent_rank keeps the arrow
                // direction sensible for Boeing labels.
                let dir = if boeing_detent_rank(&label) > boeing_detent_rank(prev) {
                    "↓"
                } else {
                    "↑"
                };
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Flaps {dir} {}", label),
                    Some(format!(
                        "IAS {:.0} kt, AGL {:.0} ft",
                        snap.indicated_airspeed_kt, snap.altitude_agl_ft
                    )),
                );
            }
            stats.last_logged_flap_label = Some(label);
            // Keep numeric path in sync to avoid double-firing if the
            // pilot swaps to a non-PMDG aircraft mid-session.
            stats.last_logged_flaps_detent = None;
        }
    } else {
        // v1.5.3 (#ifly-audit): Label bevorzugt RASTERBEWUSST (Index +
        // Rastenzahl -> echte Boeing-/Airbus-Namen), Prozent-Heuristik
        // nur noch als Rueckfall fuer Sims ohne die beiden SimVars.
        let label: String = match (snap.flap_num_positions, snap.flap_handle_index) {
            (Some(num), Some(idx)) => raster_flap_label(num, idx, snap.aircraft_icao.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| format!("Raste {idx}/{num}")),
            _ => {
                let d = (snap.flaps_position.clamp(0.0, 1.0) * 5.0).round() as u8;
                detent_label(d).to_string()
            }
        };

        // Entprellen (siehe FlightStats-Kommentar): erst loggen, wenn
        // das neue Label FLAP_LOG_STABLE_SECS stabil steht — der
        // 3-Sekunden-"FULL"-Spike des iFly faellt damit durch.
        // Wert seit v3: bundle/grenzwerte/cockpit.json -> flap_log_stable_secs (v2.1.24: 4)
        let now = Utc::now();
        if stats.last_logged_flap_label.as_deref() == Some(label.as_str()) {
            stats.pending_flap_label = None;
            stats.pending_flap_since = None;
        } else {
            if stats.pending_flap_label.as_deref() != Some(label.as_str()) {
                stats.pending_flap_label = Some(label.clone());
                stats.pending_flap_since = Some(now);
            }
            let stabil = stats
                .pending_flap_since
                .map(|t| (now - t).num_seconds() >= crate::grenzwerte::grenzwerte().cockpit.flap_log_stable_secs.wert)
                .unwrap_or(false);
            if stabil {
                // Erste Beobachtung eines Flugs wird nicht geloggt —
                // wie bisher: erst ab der zweiten Stellung gibt es
                // eine Zeile (sonst staende beim Start "Flaps UP").
                if stats.last_logged_flap_label.is_some() {
                    let dir = match stats.last_logged_flap_pos {
                        Some(prev) if snap.flaps_position > prev => "↓",
                        Some(_) => "↑",
                        None => "↓",
                    };
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("Flaps {dir} {}", label),
                        Some(format!(
                            "IAS {:.0} kt, AGL {:.0} ft",
                            snap.indicated_airspeed_kt, snap.altitude_agl_ft
                        )),
                    );
                }
                stats.last_logged_flap_label = Some(label);
                stats.last_logged_flap_pos = Some(snap.flaps_position);
                stats.last_logged_flaps_detent = None;
                stats.pending_flap_label = None;
                stats.pending_flap_since = None;
            }
        }
    }

    // ---- Stall / overspeed warnings (always loud)
    if stats.last_logged_stall != Some(snap.stall_warning) {
        if snap.stall_warning {
            aufgeschoben.handle(
                app,
                ActivityLevel::Warn,
                "Stall warning".to_string(),
                Some(format!(
                    "IAS {:.0} kt, AGL {:.0} ft",
                    snap.indicated_airspeed_kt, snap.altitude_agl_ft
                )),
            );
        }
        stats.last_logged_stall = Some(snap.stall_warning);
    }
    if stats.last_logged_overspeed != Some(snap.overspeed_warning) {
        if snap.overspeed_warning {
            aufgeschoben.handle(
                app,
                ActivityLevel::Warn,
                "Overspeed warning".to_string(),
                Some(format!("IAS {:.0} kt", snap.indicated_airspeed_kt)),
            );
        }
        stats.last_logged_overspeed = Some(snap.overspeed_warning);
    }

    // ---- Gear handle (UP / DOWN — discretised so a moving handle
    //      doesn't fire mid-cycle entries).
    let gear_down = snap.gear_position >= 0.5;
    if stats.last_logged_gear_down != Some(gear_down) {
        if let Some(prev) = stats.last_logged_gear_down {
            if prev != gear_down {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Gear {}", if gear_down { "DOWN" } else { "UP" }),
                    Some(format!(
                        "AGL {:.0} ft, IAS {:.0} kt",
                        snap.altitude_agl_ft, snap.indicated_airspeed_kt
                    )),
                );
            }
        }
        stats.last_logged_gear_down = Some(gear_down);
    }

    // ---- Spoilers / speed brake.
    // We split into two events: armed (auto-deploy on touchdown) and
    // deployed (handle physically pulled back ≥5% — pilot using it
    // as speed brake or after touchdown).
    if let Some(armed) = snap.spoilers_armed {
        if stats.last_logged_spoilers_armed != Some(armed) {
            if let Some(prev) = stats.last_logged_spoilers_armed {
                if prev != armed {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("Speed brake {}", if armed { "ARMED" } else { "DISARMED" }),
                        None,
                    );
                }
            }
            stats.last_logged_spoilers_armed = Some(armed);
        }
    }
    if let Some(pos) = snap.spoilers_handle_position {
        let deployed = pos > 0.05;
        if stats.last_logged_spoilers_deployed != Some(deployed) {
            if let Some(prev) = stats.last_logged_spoilers_deployed {
                if prev != deployed {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!(
                            "Spoilers {}",
                            if deployed { "DEPLOYED" } else { "RETRACTED" }
                        ),
                        if deployed {
                            Some(format!("Handle {:.0}%", pos * 100.0))
                        } else {
                            None
                        },
                    );
                }
            }
            stats.last_logged_spoilers_deployed = Some(deployed);
        }
    }

    // ---- APU. Bool-only "running" — combine the master-switch state
    //      with the RPM threshold so we log "APU started" only once
    //      the unit is actually up, not the moment the switch flips.
    if let Some(switch) = snap.apu_switch {
        let running = switch && snap.apu_pct_rpm.unwrap_or(0.0) >= 95.0;
        if stats.last_logged_apu_running != Some(running) {
            if let Some(prev) = stats.last_logged_apu_running {
                if prev != running {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("APU {}", if running { "started" } else { "shutdown" }),
                        None,
                    );
                }
            }
            stats.last_logged_apu_running = Some(running);
        }
    }

    // ---- Electrical / pneumatic systems.
    log_bool_change(
        &mut aufgeschoben,
        snap.battery_master,
        &mut stats.last_logged_battery_master,
        "Battery master",
    );
    log_bool_change(
        &mut aufgeschoben,
        snap.avionics_master,
        &mut stats.last_logged_avionics_master,
        "Avionics master",
    );
    log_bool_change(
        &mut aufgeschoben,
        snap.pitot_heat,
        &mut stats.last_logged_pitot_heat,
        "Pitot heat",
    );
    log_bool_change(
        &mut aufgeschoben,
        snap.engine_anti_ice,
        &mut stats.last_logged_engine_anti_ice,
        "Engine anti-ice",
    );
    log_bool_change(
        &mut aufgeschoben,
        snap.wing_anti_ice,
        &mut stats.last_logged_wing_anti_ice,
        "Wing anti-ice",
    );

    // ---- Seat-belts and no-smoking, both 0=OFF 1=AUTO 2=ON.
    // The adapters map two-position switches (Fenix `L:S_OH_SIGNS`,
    // 0/1) onto 0/2, so 1 always means AUTO here. `L:S_OH_SIGNS_SMOKING` is
    // 0/1/2 (the toggle branches between 0 and 2 explicitly).
    if let Some(v) = snap.seatbelts_sign {
        if stats.last_logged_seatbelts_sign != Some(v) {
            if stats.last_logged_seatbelts_sign.is_some() {
                let label = match v {
                    0 => "OFF",
                    1 => "AUTO",
                    _ => "ON",
                };
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Seat belts {label}"),
                    None,
                );
            }
            stats.last_logged_seatbelts_sign = Some(v);
        }
    }
    log_three_state_change(
        &mut aufgeschoben,
        snap.no_smoking_sign,
        &mut stats.last_logged_no_smoking_sign,
        "No smoking",
    );

    // ---- Autobrake (string label).
    if let Some(ab) = snap.autobrake.as_ref() {
        if stats.last_logged_autobrake.as_ref() != Some(ab) {
            if let Some(prev) = stats.last_logged_autobrake.as_ref() {
                if prev != ab {
                    aufgeschoben.handle(app, ActivityLevel::Info, format!("Autobrake {ab}"), None);
                }
            }
            stats.last_logged_autobrake = Some(ab.clone());
        }
    }

    // FCU encoder values (Selected ALT/HDG/SPD/V/S) intentionally
    // NOT logged. The Fenix LVars (`L:E_FCU_*`) are encoder click
    // counters, not real engineering values — Selected ALT 23 means
    // 23 clicks, not FL230 — so the log entries were misleading.
    // The standard `AUTOPILOT * VAR` SimVars give us the real values
    // already, but those don't update via LVar anyway. User feedback
    // (2026-05-02): "die brauchen wir doch nicht bekommen wir doch
    // eh schon aus dem standart" → drop entirely.

    // ---- PMDG SDK premium telemetry (Phase H.4 / v0.2.0) ----
    // Activity-log entries derived from snap.pmdg, the cockpit-
    // exact state we read via PMDG's SimConnect ClientData
    // channel. No-op if no PMDG aircraft is loaded or the SDK
    // isn't enabled (snap.pmdg is None in both cases).
    if let Some(p) = &snap.pmdg {
        // Capture variant label once for the PIREP custom fields.
        if stats.pmdg_variant_label.is_none() && !p.variant_label.is_empty() {
            stats.pmdg_variant_label = Some(p.variant_label.clone());
        }

        // Once-per-flight: aircraft identification banner.
        if !stats.pmdg_detected_logged {
            stats.pmdg_detected_logged = true;
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                format!("{} — PMDG SDK aktiv", p.variant_label),
                Some("Premium-Cockpit-Daten verfügbar (FMA, MCP, V-Speeds, FMC)".to_string()),
            );
        }

        // Once-per-flight: V-speeds banner when all four are set.
        // Pilots typically enter these via FMC PERF-INIT page; the
        // banner timestamps when the perf-init was completed.
        if !stats.pmdg_v_speeds_logged {
            if let (Some(v1), Some(vr), Some(v2), Some(vref)) =
                (p.fmc_v1_kt, p.fmc_vr_kt, p.fmc_v2_kt, p.fmc_vref_kt)
            {
                stats.pmdg_v_speeds_logged = true;
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("V-Speeds gesetzt: V1 {v1} · VR {vr} · V2 {v2} · VREF {vref}"),
                    None,
                );
            }
        }

        // FMA mode changes — single combined string so a switch from
        // "N1 / VNAV / LNAV" to "SPD / VNAV / LNAV" produces ONE log
        // entry, not three. Empty modes filtered out.
        let fma_combined = format_fma(&p.fma_speed_mode, &p.fma_pitch_mode, &p.fma_roll_mode);
        if stats.last_logged_pmdg_fma.as_deref() != Some(fma_combined.as_str()) {
            if let Some(prev) = stats.last_logged_pmdg_fma.as_deref() {
                if prev != fma_combined {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("FMA: {fma_combined}"),
                        None,
                    );
                }
            }
            stats.last_logged_pmdg_fma = Some(fma_combined);
        }

        // MCP selected speed — round to nearest knot (or .01 Mach).
        // We track as i16 so we get cheap equality. Value > 10 = knots,
        // ≤ 10 = Mach × 100. Both are valid pilot intents.
        if let Some(spd) = p.mcp_speed_raw {
            let spd_int = if spd > 10.0 {
                spd.round() as i16
            } else {
                (spd * 100.0).round() as i16
            };
            if stats.last_logged_pmdg_mcp_speed != Some(spd_int) {
                if stats.last_logged_pmdg_mcp_speed.is_some() {
                    let display = if spd > 10.0 {
                        format!("{spd:.0} kt")
                    } else {
                        format!("M {spd:.2}")
                    };
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("MCP IAS: {display}"),
                        None,
                    );
                }
                stats.last_logged_pmdg_mcp_speed = Some(spd_int);
            }
        }

        // MCP heading — round to nearest degree.
        if let Some(hdg) = p.mcp_heading_deg {
            if stats.last_logged_pmdg_mcp_heading != Some(hdg) {
                if stats.last_logged_pmdg_mcp_heading.is_some() {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("MCP HDG: {hdg:03}°"),
                        None,
                    );
                }
                stats.last_logged_pmdg_mcp_heading = Some(hdg);
            }
        }

        // MCP altitude — log only on significant changes (≥ 100 ft)
        // so a moving knob doesn't spam.
        if let Some(alt) = p.mcp_altitude_ft {
            let last = stats.last_logged_pmdg_mcp_altitude;
            let significant = match last {
                Some(prev) => alt.abs_diff(prev) >= 100,
                None => true,
            };
            if significant && last != Some(alt) {
                if last.is_some() {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("MCP ALT: {alt} ft"),
                        None,
                    );
                }
                stats.last_logged_pmdg_mcp_altitude = Some(alt);
            }
        }

        // MCP V/S — log on changes ≥ 100 fpm.
        if let Some(vs) = p.mcp_vs_fpm {
            let last = stats.last_logged_pmdg_mcp_vs;
            let significant = match last {
                Some(prev) => (vs - prev).abs() >= 100,
                None => true,
            };
            if significant && last != Some(vs) {
                if last.is_some() {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("MCP V/S: {vs:+} fpm"),
                        None,
                    );
                }
                stats.last_logged_pmdg_mcp_vs = Some(vs);
            }
        }

        // A/T armed.
        if stats.last_logged_pmdg_at_armed != Some(p.at_armed) {
            if stats.last_logged_pmdg_at_armed.is_some() {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("A/T {}", if p.at_armed { "armed" } else { "disarmed" }),
                    None,
                );
            }
            stats.last_logged_pmdg_at_armed = Some(p.at_armed);
        }

        // A/P engaged (CMD A or B).
        if stats.last_logged_pmdg_ap_engaged != Some(p.ap_engaged) {
            if stats.last_logged_pmdg_ap_engaged.is_some() {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!(
                        "A/P {}",
                        if p.ap_engaged {
                            "engaged"
                        } else {
                            "disengaged"
                        }
                    ),
                    None,
                );
            }
            stats.last_logged_pmdg_ap_engaged = Some(p.ap_engaged);
        }

        // Takeoff config warning was here pre-v0.3.0 — moved out to a
        // simulator-agnostic block (after the PMDG `if let Some(p)`)
        // because both PMDG (MSFS) and X-Plane Zibo/LevelUp 737 fill
        // the universal `snap.takeoff_config_warning` field.

        // ---- v0.2.2 wider integration ----

        // FMC thrust-limit mode (777). Empty string = NG3 (no
        // thrust-limit field) — skip there, NG3 derives MCP_annunN1
        // for similar purpose elsewhere.
        if !p.thrust_limit_mode.is_empty()
            && stats.last_logged_pmdg_thrust_mode.as_deref() != Some(p.thrust_limit_mode.as_str())
        {
            if stats.last_logged_pmdg_thrust_mode.is_some() {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("Thrust mode: {}", p.thrust_limit_mode),
                    None,
                );
            }
            stats.last_logged_pmdg_thrust_mode = Some(p.thrust_limit_mode.clone());
        }

        // 777 Electronic Checklist — log once per phase on rising
        // edge + capture for the PIREP custom field. Skips
        // silently for NG3 (ecl_complete = None).
        if let Some(ecl) = p.ecl_complete {
            // Capture for final PIREP — sticky union of all phases
            // ever ticked.
            let cap = stats.pmdg_ecl_phases_complete.get_or_insert([false; 10]);
            for (i, &done) in ecl.iter().enumerate() {
                if done {
                    cap[i] = true;
                }
            }
            for (idx, &done) in ecl.iter().enumerate() {
                if done && !stats.last_logged_pmdg_ecl[idx] {
                    let label = match idx {
                        0 => "Preflight",
                        1 => "Before Start",
                        2 => "Before Taxi",
                        3 => "Before Takeoff",
                        4 => "After Takeoff",
                        5 => "Descent",
                        6 => "Approach",
                        7 => "Landing",
                        8 => "Shutdown",
                        9 => "Secure",
                        _ => continue,
                    };
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("ECL: {label} complete"),
                        None,
                    );
                    stats.last_logged_pmdg_ecl[idx] = true;
                }
            }
        }

        // PMDG-authoritative APU bit (777). More accurate than
        // the standard-SimVar APU_PCT_RPM heuristic. NG3 leaves
        // this None and falls back to the standard APU detection.
        if let Some(apu) = p.apu_running {
            if stats.last_logged_pmdg_apu != Some(apu) {
                if stats.last_logged_pmdg_apu.is_some() {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("APU {} (PMDG)", if apu { "started" } else { "shutdown" }),
                        None,
                    );
                }
                stats.last_logged_pmdg_apu = Some(apu);
            }
        }

        // Wheel chocks (777). Sets/removes around pushback time.
        if let Some(chocks) = p.wheel_chocks_set {
            if stats.last_logged_pmdg_chocks != Some(chocks) {
                if stats.last_logged_pmdg_chocks.is_some() {
                    aufgeschoben.handle(
                        app,
                        ActivityLevel::Info,
                        format!("Chocks {}", if chocks { "ON" } else { "OFF" }),
                        None,
                    );
                }
                stats.last_logged_pmdg_chocks = Some(chocks);
            }
        }
    }

    // ---- Takeoff config warning (universal — PMDG + X-Plane 737)
    // Only on rising edge (warning turning ON). Falling edge
    // (pilot fixed it) doesn't warrant a log line. Source field
    // is filled by both adapters: MSFS via the PMDG snapshot()
    // merge, X-Plane via the `laminar/B738/annunciator/takeoff_config`
    // DataRef. None on aircraft that don't have an EICAS check.
    if let Some(to_warn) = snap.takeoff_config_warning {
        if stats.last_logged_pmdg_to_warning != Some(to_warn) {
            if to_warn {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Warn,
                    "TAKEOFF CONFIG warning".to_string(),
                    Some("Cockpit zeigt rote Warnung — TO-Setup unvollständig".to_string()),
                );
            }
            stats.last_logged_pmdg_to_warning = Some(to_warn);
        }
    }

    // ---- v0.16.10 Premium-Cockpit-Daten (FMA, V-Speeds, Warnings, …) ----
    // Quellen: profile-gated LVar-Mapper (Fenix/FBW/INI-A350/INI-A340/
    // Aerosoft-A346/MD-11) + PMDG-Premium-Override. Felder sind None wenn
    // die Quelle sie nicht liefert — None loggt NIE. Latch-/Debounce-
    // Regeln pro Zeile: siehe Feld-Doku auf FlightStats.
    let now = Utc::now();

    // (0) v0.16.10 QS (M5): Connect-Grace — direkt nach einem Stream-
    // (Re-)Start lesen die Addon-LVars 0.0 bis das WASM sie befüllt;
    // ohne Grace latchte Tick 1 Bogus-Werte und Tick 2 loggte Phantom-
    // Transitionen ("Baro: STD gesetzt" / "MASTER CAUTION" beim
    // Connect/Resume). Während des Fensters wird KEIN Premium-Tracker
    // gefüttert (das Premium-Logging ist der letzte Abschnitt dieser
    // Funktion → early return überspringt nur Premium-Zeilen).
    {
        let FlightStats {
            premium_log_last_tick_at,
            premium_log_warmup_until,
            ..
        } = &mut *stats;
        if premium_log_warmup_active(premium_log_last_tick_at, premium_log_warmup_until, now) {
            return;
        }
    }

    // (1) Generic-FMA-Zeile — NUR ohne PMDG: das PMDG-FMA-Logging oben
    // ist für PMDG authoritativ, doppelt loggen wäre Spam. Erster Wert
    // latcht stumm (wie beim PMDG-Logger), Änderungen debounced 3 s.
    if snap.pmdg.is_none() {
        if let Some(line) = fmt_fma_line(
            snap.fma_lateral_mode.as_deref(),
            snap.fma_vertical_mode.as_deref(),
            snap.fma_thrust_mode.as_deref(),
        ) {
            if stats
                .premium_fma
                .update(line.clone(), now, crate::grenzwerte::grenzwerte().cockpit.premium_fma_debounce_secs.wert)
            {
                aufgeschoben.handle(app, ActivityLevel::Info, format!("FMA: {line}"), None);
            }
        }
    }

    // (2) Takeoff-V-Speeds — loggt sobald V1+VR+V2 komplett sind
    // (= FMS-Perf-Init fertig, der erste Treffer ist der informative)
    // und bei jeder späteren Änderung. FLEX gehört zum Latch-Key
    // (Perf-Daten-Änderung), das "· TOGA"-Suffix nicht (Lever-Bewegung).
    // v0.16.10 QS (M3): NUR ohne PMDG — apply_pmdg_premium_override
    // mappt die FMC-V-Speeds in die generischen v1_kt/…-Felder, und
    // PMDG hat oben bereits sein eigenes "V-Speeds gesetzt:"-Banner
    // (pmdg_v_speeds_logged). Ohne Gate käme jede PMDG-Perf-Eingabe
    // doppelt ins Log.
    if snap.pmdg.is_none() {
        if let (Some(v1), Some(vr), Some(v2)) = (snap.v1_kt, snap.vr_kt, snap.v2_kt) {
            let key = (
                v1.round() as i64,
                vr.round() as i64,
                v2.round() as i64,
                snap.flex_temp_c.map(|t| t.round() as i64),
            );
            if stats.last_logged_vspeeds != Some(key) {
                stats.last_logged_vspeeds = Some(key);
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    fmt_vspeeds_line(v1, vr, v2, snap.flex_temp_c, snap.thrust_gate.as_deref()),
                    None,
                );
            }
        }
    }

    // (3) Approach-Speeds — nur in Descent/Approach/Final (vorher sind
    // VAPP/VLS oft noch Climb-Artefakte bzw. nicht final berechnet).
    // Erster Some-Wert loggt sofort (update_first_logs), danach re-log
    // bei Änderung. v0.16.10 QS (B1): VLS/VAPP driften auf FBW/INI
    // laufend (Gewichts-/Konfig-Neuberechnung, Flap-Sweep) → Latch-Key
    // auf 5-kt-Buckets quantisiert + 10 s Debounce, sonst 20-70 Zeilen
    // pro Approach. v0.16.10 QS (M3): NUR ohne PMDG (VREF kommt dort
    // aus dem Override, das PMDG-Banner loggt sie bereits).
    if snap.pmdg.is_none()
        && matches!(
            stats.phase,
            FlightPhase::Descent | FlightPhase::Approach | FlightPhase::Final
        )
    {
        if let Some(line) = fmt_approach_speeds_line(snap.vapp_kt, snap.vls_kt, snap.vref_kt) {
            let bucket = |v: Option<f64>| v.map(|x| (x / crate::grenzwerte::grenzwerte().cockpit.approach_speeds_bucket_kt.wert).round() as i64);
            let key = (
                bucket(snap.vapp_kt),
                bucket(snap.vls_kt),
                bucket(snap.vref_kt),
            );
            if stats.premium_approach_speeds.update_first_logs(
                key,
                now,
                crate::grenzwerte::grenzwerte().cockpit.approach_speeds_debounce_secs.wert,
            ) {
                aufgeschoben.handle(app, ActivityLevel::Info, line, None);
            }
        }
    }

    // (4) MASTER CAUTION / MASTER WARNING — Transitionen, debounced 2 s
    // (Annunciator-Tests am Boden flackern), erster Wert latcht stumm.
    // v0.16.10 QS (Minor 7): am Boden mit 0 laufenden Engines komplett
    // unterdrückt (Tracker wird NICHT gefüttert) — Cold&Dark-Preflight-
    // Self-Tests (Overhead-Tests, IRS-Align, Recall-Checks) zünden die
    // Lampen reihenweise, ohne dass es ein meldenswertes Ereignis wäre.
    // Nach Engine-Start latcht der erste Wert stumm (First-Silent).
    let preflight_self_test_window = snap.on_ground && snap.engines_running == 0;
    if let Some(mc) = snap.master_caution.filter(|_| !preflight_self_test_window) {
        if stats
            .premium_master_caution
            .update(mc, now, crate::grenzwerte::grenzwerte().cockpit.master_annunciator_debounce_secs.wert)
        {
            if mc {
                aufgeschoben.ohne_meldung(app, ActivityLevel::Warn, "MASTER CAUTION", None);
            } else {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    "MASTER CAUTION aus".to_string(),
                    None,
                );
            }
        }
    }
    if let Some(mw) = snap.master_warning.filter(|_| !preflight_self_test_window) {
        if stats
            .premium_master_warning
            .update(mw, now, crate::grenzwerte::grenzwerte().cockpit.master_annunciator_debounce_secs.wert)
        {
            if mw {
                aufgeschoben.ohne_meldung(app, ActivityLevel::Error, "🔴 MASTER WARNING", None);
            } else {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    "MASTER WARNING aus".to_string(),
                    None,
                );
            }
        }
    }

    // (5)+(6) Umkehrschub + Ground-Spoiler — je einmal pro Landung
    // (Rising-Edge am Boden), re-arm sobald airborne: ein Touch-and-Go
    // bekommt so für die nächste Landung wieder eigene Einträge.
    // v0.16.10 QS (Minor 6): zusätzlich Groundspeed > 40 kt an der
    // Rising-Edge verlangt — Lever-Checks beim Taxi (Reverser kurz
    // ziehen, Spoiler-Test am Stand) loggen sonst "Umkehrschub
    // aktiviert" ohne Landung. Echte Landungen/RTOs liegen weit drüber.
    // Borrow-Split: stats ist ein MutexGuard, Feld-Borrows splitten
    // nicht durch den Deref — also die Felder einzeln rausziehen.
    {
        let fast_enough = snap.groundspeed_kt > crate::grenzwerte::grenzwerte().cockpit.reverser_spoiler_min_gs_kt.wert;
        let FlightStats {
            last_seen_reverser,
            reverser_logged_this_landing,
            last_seen_ground_spoilers,
            ground_spoilers_logged_this_landing,
            ..
        } = &mut *stats;
        if once_per_landing_rising_edge(
            snap.reverser_deployed,
            snap.on_ground,
            fast_enough,
            last_seen_reverser,
            reverser_logged_this_landing,
        ) {
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                "Umkehrschub aktiviert".to_string(),
                None,
            );
        }
        if once_per_landing_rising_edge(
            snap.ground_spoilers_active,
            snap.on_ground,
            fast_enough,
            last_seen_ground_spoilers,
            ground_spoilers_logged_this_landing,
        ) {
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                "Ground Spoiler ausgefahren".to_string(),
                None,
            );
        }
    }

    // (7) Baro STD/QNH — erster Wert latcht stumm (beim Connect steht
    // der Baro halt irgendwie), danach loggt jede Transition.
    if let Some(std_set) = snap.baro_std {
        if stats.last_logged_baro_std != Some(std_set) {
            if stats.last_logged_baro_std.is_some() {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    if std_set {
                        "Baro: STD gesetzt"
                    } else {
                        "Baro: QNH gesetzt"
                    }
                    .to_string(),
                    None,
                );
            }
            stats.last_logged_baro_std = Some(std_set);
        }
    }

    // (8) Flugzeug-eigene FWC-Phase — NUR die A346 (ToLiss), deren Nummerierung
    // an einem vollständigen echten Flug verifiziert ist (siehe
    // `a346_fwc_phase_label`). Rein informativ; treibt die FSM nicht.
    //
    // Der Präfix "FWC" ist Absicht: die Meldung kommt vom Flugwarnrechner des
    // Flugzeugs, nicht von unserer Phasen-Erkennung. Ohne ihn stünde "TOUCHDOWN"
    // (die FWC-Schwelle) direkt neben unserer Phase "Landing" — und in v0.19.2
    // stand genau deshalb "APPROACH" im Reiseflug, mit einer um eine Position
    // verschobenen Tabelle. Die FBW A32NX bleibt bewusst stumm: sie meldet je
    // nach Addon-Version zehn ODER zwölf Werte.
    if let Some(phase_label) = snap.flight_phase_aircraft.as_ref() {
        if stats.premium_aircraft_phase.update(
            phase_label.clone(),
            now,
            crate::grenzwerte::grenzwerte().cockpit.aircraft_phase_debounce_secs.wert,
        ) {
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                format!("FWC: {phase_label}"),
                None,
            );
        }
    }

    // (9) Below-G/S-Alert — einmal pro Approach; re-arm wenn der Alert
    // > BELOW_GS_REARM_SECS geklärt ist oder die FlightPhase wechselt
    // (Go-Around → neuer Approach darf wieder loggen). Erster Wert
    // latcht stumm — ein beim Connect bereits aktiver Alert zählt als
    // "schon geloggt".
    if stats.below_gs_armed_phase != Some(stats.phase) {
        stats.below_gs_armed_phase = Some(stats.phase);
        stats.below_gs_logged_this_approach = false;
    }
    if let Some(below) = snap.below_gs_alert {
        // Borrow-Split: Felder einzeln rausziehen damit der Helper
        // drei &mut auf disjunkte stats-Felder bekommt.
        let FlightStats {
            last_seen_below_gs,
            below_gs_logged_this_approach,
            below_gs_clear_since,
            ..
        } = &mut *stats;
        if below_gs_should_log(
            below,
            now,
            last_seen_below_gs,
            below_gs_logged_this_approach,
            below_gs_clear_since,
        ) {
            aufgeschoben.handle(
                app,
                ActivityLevel::Warn,
                "Below Glideslope".to_string(),
                None,
            );
        }
    }

    // (10) Kabinenhöhen-Warnung — selten + ernst, nur Rising-Edge,
    // erster Wert latcht stumm.
    if let Some(cab) = snap.cabin_altitude_warning {
        if stats.last_logged_cabin_alt_warning != Some(cab) {
            if stats.last_logged_cabin_alt_warning.is_some() && cab {
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Error,
                    "🔴 Kabinenhöhe-Warnung".to_string(),
                    None,
                );
            }
            stats.last_logged_cabin_alt_warning = Some(cab);
        }
    }

    // (11) Stab-out-of-trim — debounced 5 s (Annunciator pulst während
    // Trim-Läufen), nur die Rising-Edge loggt.
    if let Some(oot) = snap.stab_out_of_trim {
        if stats
            .premium_stab_out_of_trim
            .update(oot, now, crate::grenzwerte::grenzwerte().cockpit.stab_trim_debounce_secs.wert)
            && oot
        {
            aufgeschoben.handle(
                app,
                ActivityLevel::Warn,
                "Stabilizer out of trim".to_string(),
                None,
            );
        }
    }

    // (12) Schubhebel-Detent — nur INI-Profile liefern das Gate-Label;
    // generisches None loggt nie. Erster Wert latcht stumm (der Hebel
    // steht beim Connect irgendwo), Wechsel debounced 3 s.
    if let Some(gate) = snap.thrust_gate.as_ref() {
        if stats
            .premium_thrust_gate
            .update(gate.clone(), now, crate::grenzwerte::grenzwerte().cockpit.thrust_gate_debounce_secs.wert)
        {
            aufgeschoben.handle(
                app,
                ActivityLevel::Info,
                format!("Schubhebel: {gate}"),
                None,
            );
        }
    }
}

/// Combine FMA sub-modes into a single human-readable string.
/// Empty sub-modes are filtered out so a partially-engaged FMA
/// reads "VNAV / LNAV" rather than " / VNAV / LNAV".
fn format_fma(speed: &str, pitch: &str, roll: &str) -> String {
    let parts: Vec<&str> = [speed, pitch, roll]
        .iter()
        .copied()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" / ")
    }
}

/// v0.16.10: Generic-FMA-Zeile aus den drei normalisierten Sub-Modes
/// (lateral | vertical | thrust). None-/Leer-Teile werden weggelassen
/// ("NAV | SPEED" wenn vertical fehlt); alles None ⇒ None — der
/// Aufrufer loggt dann gar nicht (anders als das PMDG-`format_fma`,
/// das "—" liefert, weil dort ein leeres FMA selbst informativ ist).
fn fmt_fma_line(
    lateral: Option<&str>,
    vertical: Option<&str>,
    thrust: Option<&str>,
) -> Option<String> {
    let parts: Vec<&str> = [lateral, vertical, thrust]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

/// v0.16.10: Takeoff-V-Speeds-Zeile. FLEX-Suffix wenn eine Assumed-Temp
/// gesetzt ist, sonst "· TOGA" wenn der Schubhebel GERADE im
/// TOGA-Detent steht (nur INI-Profile liefern das Gate).
fn fmt_vspeeds_line(
    v1: f64,
    vr: f64,
    v2: f64,
    flex_temp_c: Option<f64>,
    thrust_gate: Option<&str>,
) -> String {
    let mut line = format!("V-Speeds: V1 {v1:.0} · VR {vr:.0} · V2 {v2:.0}");
    if let Some(flex) = flex_temp_c {
        line.push_str(&format!(" · FLEX {flex:.0}°C"));
    } else if thrust_gate == Some("TOGA") {
        line.push_str(" · TOGA");
    }
    line
}

/// v0.16.10: Approach-Speeds-Zeile (VAPP Airbus / VLS Airbus / VREF
/// Boeing) — None-Teile weggelassen, alles None ⇒ None (kein Log).
fn fmt_approach_speeds_line(
    vapp: Option<f64>,
    vls: Option<f64>,
    vref: Option<f64>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(v) = vapp {
        parts.push(format!("VAPP {v:.0}"));
    }
    if let Some(v) = vls {
        parts.push(format!("VLS {v:.0}"));
    }
    if let Some(v) = vref {
        parts.push(format!("VREF {v:.0}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("Approach-Speeds: {}", parts.join(" · ")))
    }
}

/// v0.16.10: Einmal-pro-Landung-Rising-Edge-Detektor (Umkehrschub,
/// Ground-Spoiler). `true` ⇒ jetzt loggen. Regeln:
///   * loggt nur eine false→true-Transition WÄHREND on_ground,
///   * v0.16.10 QS (Minor 6): nur wenn an der Rising-Edge auch
///     `fast_enough` gilt (Groundspeed > 40 kt) — Taxi-Lever-Checks
///     loggen nicht und VERBRAUCHEN den Einmal-Latch auch nicht
///     (eine echte spätere Landung loggt weiterhin),
///   * erster beobachteter Wert latcht stumm (Resume mitten im
///     Rollout produziert keinen nachträglichen Eintrag),
///   * einmal pro Landung — re-arm sobald airborne (Touch-and-Go
///     bekommt für die nächste Landung einen frischen Eintrag).
fn once_per_landing_rising_edge(
    value: Option<bool>,
    on_ground: bool,
    fast_enough: bool,
    last_seen: &mut Option<bool>,
    logged_this_landing: &mut bool,
) -> bool {
    if !on_ground {
        *logged_this_landing = false;
    }
    let Some(v) = value else { return false };
    let rising = *last_seen == Some(false) && v;
    *last_seen = Some(v);
    if rising && on_ground && fast_enough && !*logged_this_landing {
        *logged_this_landing = true;
        true
    } else {
        false
    }
}

/// v0.16.10: Below-G/S-Alert-Latch. `true` ⇒ "Below Glideslope"
/// jetzt loggen. Regeln:
///   * nur die false→true-Transition loggt,
///   * erster beobachteter Wert latcht stumm — ein beim Connect
///     bereits aktiver Alert zählt als "schon geloggt",
///   * einmal pro Approach — re-arm erst wenn der Alert
///     `BELOW_GS_REARM_SECS` am Stück geklärt war (der Phase-Wechsel-
///     Re-Arm sitzt im Aufrufer, der die FlightPhase kennt).
fn below_gs_should_log(
    below: bool,
    now: DateTime<Utc>,
    last_seen: &mut Option<bool>,
    logged_this_approach: &mut bool,
    clear_since: &mut Option<DateTime<Utc>>,
) -> bool {
    if below {
        *clear_since = None;
    } else {
        if clear_since.is_none() {
            *clear_since = Some(now);
        }
        if let Some(since) = *clear_since {
            if (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().cockpit.below_gs_rearm_secs.wert {
                *logged_this_approach = false;
            }
        }
    }
    let result = match *last_seen {
        None => {
            if below {
                *logged_this_approach = true;
            }
            false
        }
        Some(prev) => {
            if !prev && below && !*logged_this_approach {
                *logged_this_approach = true;
                true
            } else {
                false
            }
        }
    };
    *last_seen = Some(below);
    result
}

/// Activity-log helper for 3-state cabin signs (OFF / AUTO / ON).
fn log_three_state_change(
    // Gepuffert (v2.0.19): wird unter `flight.stats` aufgerufen.
    aufgeschoben: &mut AufgeschobeneMeldungen<'_>,
    snap_value: Option<u8>,
    last_logged: &mut Option<u8>,
    label: &str,
) {
    let Some(v) = snap_value else { return };
    if *last_logged != Some(v) {
        if last_logged.is_some() {
            let state = match v {
                0 => "OFF",
                1 => "AUTO",
                2 => "ON",
                _ => return,
            };
            let app = aufgeschoben.app;
            aufgeschoben.handle(app, ActivityLevel::Info, format!("{label} {state}"), None);
        }
        *last_logged = Some(v);
    }
}

// v0.7.13: `fcu_debounce()` entfernt — wurde fuer Fenix-LVar-Logging
// gebraucht, der Plan ist verworfen (Fenix-Encoder-Klicks sind nicht
// engineering-Werte). Falls revival fuer `AUTOPILOT * VAR`-SimVars:
// aus git history rausziehen. Audit Q4-2026-05.

/// Helper for bool-only telemetry change logging — checks Option,
/// initialises silently on first read, and emits an activity-log line
/// on every subsequent transition.
fn log_bool_change(
    // Gepuffert (v2.0.19): wird unter `flight.stats` aufgerufen.
    aufgeschoben: &mut AufgeschobeneMeldungen<'_>,
    snap_value: Option<bool>,
    last_logged: &mut Option<bool>,
    label: &str,
) {
    let Some(v) = snap_value else { return };
    if *last_logged != Some(v) {
        if let Some(prev) = *last_logged {
            if prev != v {
                let app = aufgeschoben.app;
                aufgeschoben.handle(
                    app,
                    ActivityLevel::Info,
                    format!("{label} {}", if v { "ON" } else { "OFF" }),
                    None,
                );
            }
        }
        *last_logged = Some(v);
    }
}

fn snapshot_to_position(snap: &SimSnapshot) -> PositionEntry {
    PositionEntry {
        // v1.6.14 — Idempotenz-Schluessel. Wird GENAU HIER vergeben, beim
        // Erzeugen des Punktes, und wandert danach unveraendert durch
        // Outbox, Persistenz und jeden Requeue. Nur so ist ein erneuter
        // Versand derselbe Punkt fuer phpVMS statt ein neuer.
        //
        // Nicht beim Senden vergeben — dann bekaeme jeder Retry eine neue
        // ID und der Schluessel waere wertlos.
        id: uuid::Uuid::new_v4().to_string(),
        lat: snap.lat,
        lon: snap.lon,
        // v0.16.15: `altitude` = Altimeter-Anzeige (was der Pilot sieht,
        // PMDG-FL360-Befund KLM1540: GPS-Hoehe lag +1.100 ft drueber);
        // `altitude_msl` bleibt bewusst die geometrische Hoehe.
        altitude: snap.altitude_indicated_ft.unwrap_or(snap.altitude_msl_ft),
        altitude_agl: Some(snap.altitude_agl_ft),
        altitude_msl: Some(snap.altitude_msl_ft),
        heading: Some(snap.heading_deg_magnetic),
        gs: Some(snap.groundspeed_kt),
        vs: Some(snap.vertical_speed_fpm),
        ias: Some(snap.indicated_airspeed_kt),
        // phpVMS expects pounds for both the on-board total and the flow.
        fuel: Some((snap.fuel_total_kg as f64 * KG_TO_LB) as f32),
        fuel_flow: snap
            .fuel_flow_kg_per_h
            .map(|kgph| (kgph as f64 * KG_TO_LB) as f32),
        transponder: snap.transponder_code,
        autopilot: snap.autopilot_master,
        // distance-to-destination needs arrival airport coords; filled in
        // by `step_flight` once the airports cache has them. Leave None
        // for now so we don't send a misleading 0.
        distance: None,
        log: build_position_log(snap),
        sim_time: snap.timestamp.to_rfc3339(),
    }
}

/// v0.13.8: log-Blob fuer phpVMS-Positions deaktiviert.
///
/// Hintergrund: Bei einem 3.5h-Flug entstanden mit der vorherigen
/// vollen log-Payload ~400 KB JSON nur fuer das log-Feld (Lights,
/// COM/NAV, AP-Modi, State-Bits, Env, ATC, Profile) - 99% davon
/// duplizierten sich zwischen aufeinanderfolgenden Cruise-Samples.
/// Pilot-Befund: phpVMS-PIREP-Detail-Seite oeffnet sich langsam bei
/// grossen Fluegen.
///
/// Loesung: phpVMS bekommt nur noch die strukturierten Felder
/// (lat/lon/alt/gs/ias/vs/fuel/heading/transponder/autopilot/distance/
/// sim_time). Das log-Feld bleibt None.
///
/// Die volle Telemetrie (Lights, COM/NAV, AP-Modi, State, Env, ATC,
/// Profile) geht weiterhin **komplett** an den VPS-Recorder via MQTT
/// (separater Stream, anderer Code-Pfad). Forensik + Live-Map + Reply
/// sind unveraendert. Nur phpVMS sieht die schlankere Variante - das
/// PIREP-Detail-Page rendert dadurch zigfach schneller.
///
/// Wenn man jemals wieder ein paar Felder ans phpVMS schicken will
/// (z. B. nur Lights+State bei Phase-Wechsel) - hier wieder anbauen,
/// idealerweise mit einem Filter "nur emittieren wenn sich was vs.
/// last_sent geaendert hat".
fn build_position_log(_snap: &SimSnapshot) -> Option<String> {
    None
}
