/// Build the body for an in-flight `POST /api/pireps/{id}/update` call.
/// Used both by the periodic heartbeat (every `HEARTBEAT_INTERVAL`) and by
/// phase-change callbacks. Sends monotonically growing `flight_time` and
/// `distance` so Eloquent always sees the model as dirty and bumps
/// `pireps.updated_at`, which is what the cleanup cron checks.
/// Spec sim-disconnect-auto-resume F3 (Block-Time-Korrektur):
/// Reine Subtraktions-Helper, extrahiert damit Unit-Tests gegen die
/// Saturating-Arithmetik laufen koennen ohne den vollen FlightStats-
/// Snapshot bauen zu muessen.
///
/// Verhalten:
/// - `pause_total_secs <= 0`             → raw_secs unveraendert
/// - `pause_total_secs > i32::MAX`       → clamp auf i32::MAX
/// - `pause_total_secs >= raw_secs`      → 0 (Underflow-Schutz)
/// - sonst                               → `raw_secs - pause_total_secs`
/// Wie viele der aufgezeichneten Pause-Sekunden tatsaechlich INNERHALB
/// von `[from, to]` lagen — statt blind die gesamte Session-Pausensumme
/// zu nehmen.
///
/// Root Cause (ITY 1358, 07.09.2026): `pause_total_duration_secs` zaehlt
/// JEDE Pause der Session, auch eine, die komplett VOR dem Start lag (ein
/// 36-Minuten-Verbindungsabbruch waehrend des Boardings). Wurde die
/// trotzdem von der Start→Landung-Flugzeit abgezogen, kam eine viel zu
/// kurze `flight_time` heraus (44 min echte Luftzeit − 36 min
/// vorstartige Pause ≈ 8 min) — genug, um GSGs Zeitraffer-Integritäts-
/// pruefung (`DistancePlausibility`) faelschlich anschlagen zu lassen.
///
/// Segmente, die das Fenster nur teilweise ueberlappen, zaehlen nur mit
/// ihrem ueberlappenden Anteil — nicht alles-oder-nichts.
///
/// Fallback: ist `segments` leer (Session von vor v1.7.20 wiederaufgenommen,
/// oder aus einem anderen Grund ohne Segment-Detail), bleibt das alte
/// Verhalten (die volle Summe abziehen) erhalten — ohne Segmente laesst
/// sich nicht sagen, auf welcher Seite von `from`/`to` die Pause lag, und
/// stillschweigend auf 0 zu fallen wuerde eine echte Sim-Crash-Pause
/// WAEHREND des Fluges wieder als Flugzeit zaehlen.
fn pause_secs_within(
    segments: &[PauseSegment],
    total_secs: i64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> i64 {
    if segments.is_empty() {
        return total_secs.max(0);
    }
    segments
        .iter()
        .map(|s| {
            let start = s.started_at.max(from);
            let end = s.ended_at.min(to);
            (end - start).num_seconds().max(0)
        })
        .sum()
}

fn flight_time_with_pause_secs(raw_secs: i32, pause_total_secs: i64) -> i32 {
    let pause_secs_i32 = pause_total_secs.clamp(0, i32::MAX as i64) as i32;
    raw_secs.saturating_sub(pause_secs_i32).max(0)
}

fn build_heartbeat_body(
    snap: &SimSnapshot,
    stats: &FlightStats,
    current_phase: FlightPhase,
) -> UpdateBody {
    let now = Utc::now();
    let pause_window: Option<(DateTime<Utc>, DateTime<Utc>)> =
        match (stats.takeoff_at, stats.landing_at) {
            (Some(t), Some(l)) if l > t => Some((t, l)),
            (Some(t), None) => Some((t, now)),
            // Pre-takeoff: fall back to block-off so cancellation cron sees
            // movement during the boarding/taxi period too.
            _ => stats.block_off_at.map(|b| (b, now)),
        };
    let raw_flight_time_secs = match (stats.takeoff_at, stats.landing_at) {
        (Some(t), Some(l)) if l > t => (l - t).num_seconds().max(0) as i32,
        (Some(t), None) => (now - t).num_seconds().max(0) as i32,
        _ => stats
            .block_off_at
            .map(|b| (now - b).num_seconds().max(0) as i32)
            .unwrap_or(0),
    };
    // Spec sim-disconnect-auto-resume F3 (Block-Time-Korrektur):
    // Pause-Akkumulator von der Wall-Clock-Flugzeit abziehen damit
    // 23 min Sim-Crash oder 30 min MSFS-Esc-Pause nicht als 23/30
    // zusaetzliche Flugminuten im PIREP-Heartbeat landen. Saturating-
    // Arithmetik via Helper. Nur der mit `pause_window` ueberlappende
    // Anteil zaehlt — siehe `pause_secs_within` (ITY 1358, 07.09.2026).
    let pause_secs_for_heartbeat = pause_window
        .map(|(from, to)| {
            pause_secs_within(
                &stats.pause_segments,
                stats.pause_total_duration_secs,
                from,
                to,
            )
        })
        .unwrap_or(0);
    let flight_time_secs =
        flight_time_with_pause_secs(raw_flight_time_secs, pause_secs_for_heartbeat);
    // Same fuel arithmetic as the file body: block - remaining, in pounds.
    // Round to whole kg before lb conversion so the live-map / dashboard
    // shows clean integer kg values (no `5890.29 kg` artefacts from the
    // round-trip).
    let fuel_used = match (stats.block_fuel_kg, stats.last_fuel_kg) {
        (Some(b), Some(c)) if b > c => Some(((b - c) as f64).round() * KG_TO_LB),
        _ => None,
    };
    // Prefer the peak altitude observed (matches the FILE-time `level`),
    // but fall back to the current MSL altitude during early climb so the
    // live map shows something sensible before peak gets captured.
    let level = stats
        .peak_altitude_ft
        .map(|ft| ((ft / 100.0).round() * 100.0) as i32)
        .or_else(|| {
            if snap.altitude_msl_ft > 100.0 {
                Some(((snap.altitude_msl_ft / 100.0).round() * 100.0) as i32)
            } else {
                None
            }
        })
        .map(|v| v.max(0));
    // phpVMS' `Pirep::progress_percent` uses PHP's `empty()` to gate
    // the distance/upper_bound division. `empty(0)` AND `empty(null)`
    // are BOTH true in PHP — so the v0.3.0 fix (send `None` when
    // distance < 0.5 nm) never worked: phpVMS still saw null, fell
    // back to upper_bound=1, computed 1/1 = 100 % progress, and
    // showed "Geflogene Route: 100%" during Boarding. Live bug
    // confirmed 2026-05-05 (pilot Michel D. on YBBN→NWWW).
    //
    // Fix: send a tiny non-zero floor (0.001 nm) until real distance
    // accumulates. `empty(0.001)` is false → PHP runs the real
    // division → 0.001 / planned_distance ≈ 0.00..% → displayed as
    // 0 % during boarding, ramps up correctly once we move.
    let distance = Some(stats.distance_nm.max(0.001));
    let flight_time_min = if flight_time_secs >= 60 {
        Some(flight_time_secs / 60)
    } else {
        None
    };
    // v0.3.0: send block_fuel on every heartbeat so phpVMS' live page
    // can compute "Verbleibender Treibstoff = block_fuel − fuel_used"
    // correctly. Without this the missing column defaults to 0 and the
    // remaining-fuel display reads as "−<fuel_used>" (bug reported by
    // pilot on 2026-05-04: "−17008 kg" mid-cruise). Same kg→lb round-
    // trip as fuel_used so the dashboard shows clean integer values.
    let block_fuel = stats.block_fuel_kg.map(|b| (b as f64).round() * KG_TO_LB);
    UpdateBody {
        state: None,
        source: None,
        status: phase_to_status(current_phase).map(|s| s.to_string()),
        flight_time: flight_time_min,
        distance,
        fuel_used,
        block_fuel,
        level,
        source_name: Some(format!("AeroACARS/{}", env!("CARGO_PKG_VERSION"))),
        notes: None,
        // Always-fresh timestamp so Eloquent sees the row as dirty
        // even on heartbeats where no other field changed (boarding
        // / long stable cruise). See `UpdateBody::updated_at` doc.
        updated_at: Some(now.to_rfc3339()),
        // Heartbeat-only path — divert-finalize fields stay None.
        arr_airport_id: None,
        landing_rate: None,
        score: None,
        submitted_at: None,
        block_on_time: None,
    }
}

/// Handle a 404 from the live PIREP endpoints (positions / update). phpVMS
/// soft-deletes in-flight PIREPs after `acars.live_time` hours of no
/// `updated_at` bump (cron `RemoveExpiredLiveFlights`); when that happens,
/// every subsequent post returns 404 and the streamer would otherwise
/// retry-spam forever. This stops the streamer cleanly and surfaces the
/// situation to the user so they can re-file as a manual PIREP.
fn handle_remote_cancellation(app: &AppHandle, flight: &Arc<ActiveFlight>, source: &str) {
    if flight.cancelled_remotely.swap(true, Ordering::SeqCst) {
        // Already handled by an earlier 404 in the same cycle.
        return;
    }
    // v0.6.0: Outbox leeren — PIREP existiert serverseitig nicht mehr,
    // weiteres POSTen würde 404 liefern. Verhindert orphan-persist im
    // worker-stop-branch.
    // Vergiftete Sperre nicht weiterwerfen: Der Aufruf kommt auch aus dem
    // ACARS-Postausgang, und eine Panik dort beendete dessen einzigen Besitzer.
    flight
        .position_outbox
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    flight.stop.store(true, Ordering::Relaxed);
    log_activity_handle(
        app,
        ActivityLevel::Error,
        "PIREP wurde vom Server gecancelt".to_string(),
        Some(format!(
            "{source} antwortet mit 404 — phpVMS hat den laufenden PIREP entfernt (vermutlich Inaktivitäts-Timeout). Bitte als Manual-PIREP einreichen."
        )),
    );
    // Best-effort UI nudge — the dashboard listens for this and pops the
    // manual-PIREP banner. Failure to emit isn't fatal; the activity-log
    // entry above is the durable signal.
    let payload = serde_json::json!({
        "pirep_id": flight.pirep_id,
        "source": source,
    });
    let _ = tauri::Emitter::emit(app, "pirep_cancelled_remotely", payload.clone());
    // v0.16.0 (#LAN-Remote): fan out to LAN WS subscribers.
    app.state::<AppState>()
        .remote_events
        .send(remote::RemoteEvent::new(
            "pirep_cancelled_remotely",
            payload,
        ));
}

/// Score, Custom-Felder und Notiz eines PIREP aus EINER Bewertung.
///
/// Codex-Befund zu P-4.40: Score, Feld „Landing Score“ und Notiz rechneten
/// unter derselben Stats-Sperre je fuer sich. Faellt das Paketmodul dabei nur
/// kurz aus, tragen sie widerspruechliche Zustaende (Score endgueltig, Notiz
/// „vorlaeufig“). Beide Einreichwege bauen deshalb hier.
fn pirep_bewertete_teile(
    flight: &ActiveFlight,
    stats: &mut FlightStats,
    effective_arr_icao: &str,
) -> (Option<i32>, HashMap<String, String>, String) {
    // v3 (P-4.10): die letzte gespeicherte Ausgabe ohne Neurechnung; nur ohne
    // passende gespeicherte rechnet die Einreichung selbst. Datensatz,
    // MQTT-PIREP und Dokument lesen sie danach aus `stats.pirep_rechnung`.
    let pin_jetzt_da = !crate::bewertung::pin_fehlt(&flight.pirep_id);
    let rechnung = match gespeicherte_rechnung(stats, effective_arr_icao, pin_jetzt_da) {
        Some(r) => r,
        None => {
            tracing::info!(
                pirep_id = %flight.pirep_id,
                gespeichert = stats.landung_endgueltig.is_some() || stats.landung_sofort.is_some(),
                "PIREP: keine passende gespeicherte Bewertung — Einreichung rechnet selbst"
            );
            pirep_rechnung_bilden(flight, stats, effective_arr_icao)
        }
    };
    let bewertung = landungs_bewertung_aus(stats, &rechnung.ergebnis);
    stats.pirep_rechnung = Some(rechnung);
    let stats = &*stats;
    (
        bewertung.and_then(|b| b.verdict).map(|v| v.numeric),
        build_pirep_fields_mit(flight, stats, effective_arr_icao, bewertung.as_ref()),
        build_pirep_notes_mit(flight, stats, effective_arr_icao, bewertung.as_ref()),
    )
}

/// Wie [`build_pirep_fields_mit`], rechnet die Bewertung selbst — nur fuer
/// Tests, die die Felder allein pruefen.
#[cfg(test)]
fn build_pirep_fields(
    flight: &ActiveFlight,
    stats: &FlightStats,
    effective_arr_icao: &str,
) -> HashMap<String, String> {
    let b = canonical_landing_bewertung(flight, stats, effective_arr_icao);
    build_pirep_fields_mit(flight, stats, effective_arr_icao, b.as_ref())
}

/// Wie [`build_pirep_notes_mit`], rechnet die Bewertung selbst — nur fuer
/// Tests, die die Notiz allein pruefen.
#[cfg(test)]
fn build_pirep_notes(
    flight: &ActiveFlight,
    stats: &FlightStats,
    effective_arr_icao: &str,
) -> String {
    let b = canonical_landing_bewertung(flight, stats, effective_arr_icao);
    build_pirep_notes_mit(flight, stats, effective_arr_icao, b.as_ref())
}

/// Build the custom-fields map sent in `POST /api/pireps/{id}/file`. Field
/// names follow the de-facto vmsACARS convention so VAs that already configured
/// fields for vmsACARS see them populate without any work.
/// „G55 (55-867b805a584d)“ bzw. „G0 eingebaut (0-…)“ fuer das PIREP-Feld.
fn paket_feldtext(generation: u64, kennung: &str) -> String {
    if generation == 0 {
        format!("G0 eingebaut ({kennung})")
    } else {
        format!("G{generation} ({kennung})")
    }
}

fn build_pirep_fields_mit(
    flight: &ActiveFlight,
    stats: &FlightStats,
    // v0.16.24 (QS-2 FIX A): the EFFECTIVE arrival airport (the
    // pilot-confirmed divert if any, else the planned `arr_airport`) — the
    // same value `build_pirep_payload` / `build_landing_record` pass into the
    // runway trust check. The "Landing Score" custom field below derives its
    // aggregate via `compute_aggregate_master_score`, which trusts the runway
    // geometry only when the matched ICAO equals the effective arrival; passing
    // the planned `arr_airport` here would skip the rollout/LDA sub-score on a
    // divert and disagree with the native `score` + the local record. On-plan
    // this equals `flight.arr_airport` → byte-identical.
    effective_arr_icao: &str,
    // Die EINE Bewertung dieses PIREP-Aufbaus (siehe [`pirep_bewertete_teile`]).
    bewertung: Option<&LandungsBewertung>,
) -> HashMap<String, String> {
    // Single-form custom-fields set: human-readable Title Case with
    // units. Earlier versions emitted EVERY field twice (Title Case +
    // snake_case "for SQL-sortable leaderboards") which made the
    // PIREP-detail list balloon to 60+ entries with obvious
    // duplicates ("Block Fuel: 9733 kg" right next to "block_fuel_kg:
    // 9733"). VA admins who want raw numbers can hit the database
    // directly — no point doubling everything in the UI.
    //
    // Notes block (separate PIREP field) still carries the prose
    // summary; THIS map carries the structured key-value pairs.
    let mut f: HashMap<String, String> = HashMap::new();

    f.insert(
        "Source".into(),
        format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")),
    );
    // v3 (Beta 2): Paketstand des Flugs fuer Admins. Eigenes Feld statt
    // `source_name` — den lesen IntegrityGate (Prefix „AeroACARS"),
    // GsgLogbook/StratosLogbook („simulator") und SkyOps (exakter Filter).
    if let Some(id) = laufzeit_paket::flug_pin(&flight.pirep_id) {
        f.insert("AeroACARS Paket".into(), paket_feldtext(id.generation, &id.to_string()));
    }

    // v0.7.19 GAF-707 Accident-Detection: strukturierte Felder fuer den
    // VA-Admin PIREP-Detail-View. Spec §PIREP-Payload.
    f.insert(
        "Accident Classifier Version".into(),
        accident::ACCIDENT_CLASSIFIER_VERSION.into(),
    );
    if stats.accident_detected {
        f.insert("Accident".into(), "TRUE".into());
        if let Some(kind) = stats.accident_kind.as_deref() {
            f.insert("Accident Kind".into(), kind.into());
        }
        if let Some(c) = stats.accident_confidence.as_deref() {
            f.insert("Accident Confidence".into(), c.into());
        }
        if !stats.accident_reasons.is_empty() {
            f.insert("Accident Reasons".into(), stats.accident_reasons.join(", "));
        }
        if let Some(t) = stats.accident_at {
            f.insert(
                "Accident Detected At".into(),
                t.format("%H:%M:%S UTC").to_string(),
            );
        }
    }

    // v0.3.0: Aircraft-Type / Name / Reg im PIREP-Custom-Field damit
    // VA-Admins auf der phpVMS-Detail-Seite ohne Lookup wissen was
    // geflogen wurde (vorher nur PMDG-Variant; jetzt für jeden
    // Aircraft-Typ aus dem Bid-Aircraft-Lookup).
    if !flight.aircraft_icao.is_empty() {
        let label = if !flight.aircraft_name.is_empty() {
            format!("{} ({})", flight.aircraft_icao, flight.aircraft_name)
        } else {
            flight.aircraft_icao.clone()
        };
        f.insert("Aircraft Type".into(), label);
    }
    if !flight.planned_registration.is_empty() {
        f.insert("Aircraft Reg".into(), flight.planned_registration.clone());
    }

    // v1.5.5 Stand-Erkennung: erkannte Staende als Custom-Fields — die
    // phpVMS-Flugbericht-Seite zeigt sie automatisch, und das DBasic-
    // FlightBoard auf dem GSG-Dashboard rendert sie als "Gate X" (es
    // matcht die Slugs departure-gate/arrival-gate — exakt die alte
    // vmsACARS-Konvention, daher GATE, nicht Stand). arr NUR ueber
    // arr_gate_for(): ein Stand vom falschen Flughafen (Divert) waere
    // schlimmer als keiner.
    //
    // ⚠ v1.7.17 Runde 12 (externe Gegenpruefung, Codex, adversarial,
    // 04.09.2026): bei einem bekanntlich veralteten, von AeroACARS selbst
    // geposteten Wert explizit LEEREN statt das Feld wegzulassen. `/file`
    // traegt diese Felder wie `post_pirep_fields` als Upsert ein: ein
    // weggelassener Schluessel loescht einen fruehen LIVE-Post NICHT. Die
    // Einreichung laeuft ueber `file_pirep_with_retry` + `pirep_queue` —
    // in dieser Datei der dauerhafteste Zustellweg (siehe aber die
    // Ehrlichkeits-Notiz im Pruefstand-Dokument zur ~50-Versuche-Grenze
    // der Warteschlange). Die Live-Rueckzugs-Meldung (Runden 6-11) bleibt
    // als schneller Best-Effort-Pfad WAEHREND des Fluges bestehen.
    //
    // ⚠ v1.7.17 Runde 13 (externe Gegenpruefung, Codex, adversarial,
    // 04.09.2026): NUR leeren, wenn `dep_gate_withdrawal_pending` gesetzt
    // ist — also AeroACARS SELBST einen jetzt veralteten Wert live
    // gepostet hat. Die Runde-12-Fassung leerte UNBEDINGT, sobald lokal
    // kein Stand vorlag — das trifft aber auch den weit ueberwiegenden
    // Normalfall (kein Stand je erkannt, kein Fehler) und haette dabei
    // einen moeglicherweise von ANDERSWO gesetzten Wert (Dispatcher,
    // anderes Tool) zerstoert, ohne dass AeroACARS je etwas Falsches
    // gepostet hatte. Ohne anhaengigen Rueckzug bleibt das Feld wie vor
    // Runde 12 einfach weg — kein Eingriff in fremde Daten.
    if stats.dep_gate_withdrawal_pending {
        f.insert(
            "Departure Gate".into(),
            stats
                .dep_gate
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("")
                .to_string(),
        );
    } else if let Some(g) = stats.dep_gate.as_deref().filter(|s| !s.is_empty()) {
        f.insert("Departure Gate".into(), g.to_string());
    }
    if let Some(g) = arr_gate_for(stats, effective_arr_icao) {
        f.insert("Arrival Gate".into(), g);
    }

    // ---- Times (HH:MM:SS UTC, glanceable) + durations ----
    fn fmt_time(t: &DateTime<Utc>) -> String {
        t.format("%H:%M:%S UTC").to_string()
    }
    if let Some(t) = stats.block_off_at {
        f.insert("Blocks Off Time".into(), fmt_time(&t));
    }
    if let Some(t) = stats.takeoff_at {
        f.insert("Takeoff Time".into(), fmt_time(&t));
    }
    if let Some(t) = stats.landing_at {
        f.insert("Landing Time".into(), fmt_time(&t));
    }
    if let Some(t) = stats.block_on_at {
        f.insert("Blocks On Time".into(), fmt_time(&t));
    }
    if let (Some(off), Some(on)) = (stats.block_off_at, stats.block_on_at) {
        f.insert(
            "Total Block Time".into(),
            humanize_duration_minutes((on - off).num_minutes()),
        );
    }
    if let (Some(takeoff), Some(landing)) = (stats.takeoff_at, stats.landing_at) {
        f.insert(
            "Total Flight Time".into(),
            humanize_duration_minutes((landing - takeoff).num_minutes()),
        );
    }
    if let (Some(off), Some(takeoff)) = (stats.block_off_at, stats.takeoff_at) {
        f.insert(
            "Taxi Out Time".into(),
            humanize_duration_minutes((takeoff - off).num_minutes()),
        );
    }
    if let (Some(land), Some(blocks_on)) = (stats.landing_at, stats.block_on_at) {
        f.insert(
            "Taxi In Time".into(),
            humanize_duration_minutes((blocks_on - land).num_minutes()),
        );
    }

    // ---- Weights & fuel (skip 0-value to avoid Fenix-bogus garbage) ----
    if let Some(w) = stats.takeoff_weight_kg.filter(|v| *v > 0.0) {
        f.insert("Takeoff Weight".into(), format!("{:.0} kg", w));
    }
    if let Some(w) = stats.landing_weight_kg.filter(|v| *v > 0.0) {
        f.insert("Landing Weight".into(), format!("{:.0} kg", w));
    }
    if let Some(b) = stats.block_fuel_kg.filter(|v| *v > 0.0) {
        f.insert("Block Fuel".into(), format!("{:.0} kg", b));
    }
    if let Some(fuel) = stats.landing_fuel_kg.filter(|v| *v > 0.0) {
        f.insert("Landing Fuel".into(), format!("{:.0} kg", fuel));
    }
    if let (Some(b), Some(c)) = (stats.block_fuel_kg, stats.last_fuel_kg) {
        if b > 0.0 && b > c {
            f.insert("Fuel Used".into(), format!("{:.0} kg", b - c));
        }
    }

    // ---- Touchdown analysis ----
    // v0.7.1 Round-2 P2-Fix: "Landing Score" Custom-Field nutzt jetzt
    // den Aggregate-Master-Score (gleicher Wert wie App/Web/MQTT/
    // phpVMS-native). Vorher zeigte es den Touchdown-Klassifikator-
    // Score — Pilot sah in der phpVMS-Detailansicht zwei verschiedene
    // Zahlen ("Score: 77/100" oben, "Landing Score: A+ (smooth) —
    // 100/100" unten). Fallback auf Touchdown-Klassifikation wenn
    // Crate keinen Aggregate liefert.
    if let Some(verdict) = bewertung.and_then(|b| b.verdict) {
        f.insert("Landing Score".into(), verdict.headline());
    }
    // v0.7.21 (MS713-QS): canonical_landing_rate_fpm() — sonst zeigt
    // das PIREP-Custom-Feld "Landing Rate" einen anderen Wert als das
    // Live-Touchdown und der Pilot-Client.
    if let Some(rate) = stats.canonical_landing_rate_fpm() {
        // SIGNED — negative on descent. Matches both LandingToast and
        // the latched SimVar reading. Pilots want to see the minus.
        //
        // v0.15.14: KEIN Peak-Override mehr. Vorher überschrieb hier
        // `landing_peak_vs_fpm` (= Peak-Sinkrate, z.B. -444) den kanonischen
        // Wert (`rate`, z.B. -264). Folge: das PIREP-Feld "Landing Rate" zeigte
        // -444, während Score + phpVMS-`landing_rate`-Spalte + Live-Map den
        // kanonischen FAR-25.473-Aufsetzwert (-264) nutzten → ZWEI verschiedene
        // Lande-Raten auf derselben PIREP (Live-Report Thomas, GLG2/MGGT mit
        // ausgeprägtem Flare: Edge -264 vs Peak -444). Jetzt durchgängig die
        // kanonische Quelle (= dieselbe wie der Score). Der Peak bleibt als
        // klar beschriftetes Forensik-Detail in der Live-Map sichtbar.
        //
        // v0.5.16: NUMERIC value only (no " fpm" suffix). The
        // FatihKoz/DisposableSpecial dmaintenance plugin reads this
        // field and runs `is_numeric()` against it; "-1641 fpm"
        // returns false there → hard-landing penalty silently
        // skipped. Pure numeric makes the maintenance check work.
        f.insert("Landing Rate".into(), format!("{:.0}", rate));
    }
    // v0.20 (QS-Fix, fallback_zero-Score-Bug): `score_g_for_stats` selbst
    // fragen statt der rohen Felder — sonst wuerde ein fallback_zero-Flug
    // (Zahl ist erfunden, `.is_some()` aber trotzdem true) einen falschen
    // G-Wert an das dmaintenance-Plugin weiterreichen (Schadens-Straf-
    // Berechnung auf Basis einer Zahl, die nie gemessen wurde).
    // v1.6.2: auf die Referenzkette umgerechnet — das dmaintenance-Plugin
    // und die SkyAdventures-Erdung legen feste G-Schwellen an, die aus der
    // MSFS-only-Zeit stammen. Roh weitergereicht bekäme ein X-Plane-Pilot
    // fuer denselben Stoss haerteren Schaden als ein MSFS-Pilot.
    if let Some(g) = scored_g_fuer_punkte(stats).map(|scored_g| recorder::ScoredG {
        scored_g,
        raw_peak: stats.canonical_peak_g_force().unwrap_or(scored_g),
        method: score_g_for_stats(stats)
            .map(|s| s.method)
            .unwrap_or(recorder::ScoredGMethod::RawFallback),
    }) {
        // v0.5.16: pure numeric (no " G" suffix). Some maintenance
        // plugins also read this; same is_numeric() reasoning.
        //
        // v0.12.3: das phpVMS-/Wartungs-Feld trägt denselben **gescorten**
        // (EMA, FOQA-Methode) G-Wert wie die Pilot-Anzeige — nicht den
        // rohen 50-Hz-Einzelframe-Peak. Pilot-Anzeige und Maintenance-
        // Penalty-Basis müssen konsistent sein (sonst sieht der Pilot
        // 1.78, das Plugin straft auf 1.95).
        f.insert("Landing G-Force".into(), format!("{:.2}", g.scored_g));
    }
    if let Some(p) = stats.landing_pitch_deg {
        // v0.5.16: pure numeric — `landing-pitch` slug is the tail-
        // strike-on-landing trigger in dmaintenance.
        f.insert("Landing Pitch".into(), format!("{:+.1}", p));
    }
    if let Some(b) = stats.landing_bank_deg {
        // v0.5.16: bank at touchdown — `landing-roll` slug is the
        // wing-strike-on-landing trigger in dmaintenance (default
        // limit 15°). Pure numeric.
        f.insert("Landing Roll".into(), format!("{:+.1}", b));
    }
    if let Some(p) = stats.takeoff_pitch_deg {
        // v0.5.16: `takeoff-pitch` slug — tail-strike-on-takeoff
        // trigger in dmaintenance. Pure numeric.
        f.insert("Takeoff Pitch".into(), format!("{:+.1}", p));
    }
    if let Some(b) = stats.takeoff_bank_deg {
        // v0.5.16: `takeoff-roll` slug — wing-strike-on-takeoff
        // trigger in dmaintenance. Pure numeric.
        f.insert("Takeoff Roll".into(), format!("{:+.1}", b));
    }
    if let Some(s) = stats.landing_speed_kt.filter(|v| *v > 0.0) {
        f.insert("Landing Speed".into(), format!("{:.0} kt", s));
    }
    if let Some(h) = stats.landing_heading_deg {
        f.insert("Landing Heading".into(), format!("{:03.0}°", h));
    }
    if let Some(slip) = stats.touchdown_sideslip_deg {
        f.insert("Touchdown Sideslip".into(), format!("{:+.1}°", slip));
    }
    if let Some(hw) = stats.landing_headwind_kt {
        if hw >= 0.0 {
            f.insert("Touchdown Headwind".into(), format!("{:.0} kt", hw));
        } else {
            f.insert("Touchdown Tailwind".into(), format!("{:.0} kt", -hw));
        }
    }
    if let Some(xw) = stats.landing_crosswind_kt {
        let side = if xw >= 0.0 { "from right" } else { "from left" };
        f.insert(
            "Touchdown Crosswind".into(),
            format!("{:.0} kt {}", xw.abs(), side),
        );
    }
    if stats.bounce_count > 0 {
        // Suppress the "Bounces: 0" row — every clean landing had it
        // and it added noise without information.
        f.insert("Bounces".into(), stats.bounce_count.to_string());
    }
    // P1/8b (Score-Version 26): leichte Hopser erscheinen auf der GSG-Seite
    // (PIREP-Felder) wie in Client und Webapp — nur wenn kein normaler da
    // ist, denn dann zaehlen sie nicht (Thomas 22.10.2026).
    if stats.bounce_count == 0 && stats.light_bounce_count > 0 {
        f.insert("Light Bounces".into(), stats.light_bounce_count.to_string());
    }

    // ---- Touch-and-Go + Go-Around counters (v0.1.26) ----
    // Only surface when nonzero — every routine PIREP has zero of
    // both, no point adding empty rows. The detailed touchdown list
    // (peak V/S + G per event) lives in the Notes prose so it doesn't
    // bloat the structured fields with N rows on a training flight.
    // v0.20 (QS-Fix, DLH2064 2026-07-23): über `effective_touchdown_events`
    // statt des rohen Vecs — sonst kann eine gerastete FinalLanding (siehe
    // Doc-Kommentar dort) diese Zeile ganz unterdrücken oder "N T&G, kein
    // Final" vorgaukeln, obwohl eine echte Landung stattfand.
    let effective_events = effective_touchdown_events(stats);
    let tg_count = effective_events
        .iter()
        .filter(|e| matches!(e.kind, TouchdownKind::TouchAndGo))
        .count();
    if tg_count > 0 || effective_events.len() > 1 {
        // "Touchdowns: 4 (3 T&G + final)" — gives the VA admin one
        // glanceable line that matches the Notes story.
        let total = effective_events.len();
        let suffix = if tg_count == 0 {
            String::new()
        } else if total == tg_count {
            format!(" ({} T&G)", tg_count)
        } else {
            format!(" ({} T&G + final)", tg_count)
        };
        f.insert("Touchdowns".into(), format!("{}{}", total, suffix));
    }
    // 20.10.2026 (Stück 3, EWG 1599): der haerteste Aufsetzer — die
    // GSG-Wartung (Hard Landing Check) liest ihn neben "Landing Rate", damit
    // ein harter Aufsetzer vor einem Durchstart nicht verschwindet. NUR die
    // Zahl, aus demselben Grund wie bei "Landing Rate" (v0.5.16): die
    // Wartung prueft `is_numeric()`. Zeit, Art und G stehen in den Notizen
    // (Abschnitt TOUCHDOWNS).
    if let (_, Some(h)) = aufsetzer_fuer_pirep(stats) {
        f.insert("Hardest Touchdown".into(), format!("{:.0}", h.peak_vs_fpm));
    }
    if stats.go_around_count > 0 {
        f.insert("Go-Arounds".into(), stats.go_around_count.to_string());
    }

    // ---- Approach Stability + Rollout (Landing Analyzer Stage 1) ----
    if let Some(sd) = stats.canonical_vs_stddev_fpm() {
        f.insert("Approach V/S Stddev".into(), format!("{:.0} fpm", sd));
    }
    // v0.20.0: Kanonik — das Feld zeigte den rohen Legacy-Wert (ungefiltert,
    // Fenster bis 1500 ft AGL), waehrend der Score den gefilterten bewertet.
    if let Some(sd) = stats.canonical_bank_stddev_deg() {
        f.insert("Approach Bank Stddev".into(), format!("{:.1}°", sd));
    }
    if let Some(meters) = stats.rollout_distance_m {
        // Single field with both units — "935 m (3068 ft)" instead of
        // three separate Rollout Distance / rollout_distance_m /
        // rollout_distance_ft entries.
        f.insert(
            "Rollout Distance".into(),
            format!("{:.0} m ({:.0} ft)", meters, meters * 3.28084),
        );
    }

    // ---- PMDG Premium Telemetry (Phase H.4 / v0.2.0) ----
    // Only emitted when the pilot flew a PMDG aircraft AND had the
    // SDK enabled (so we got real cockpit data). Field names use
    // "PMDG …" prefix so VA admins can filter PIREPs that have
    // premium-telemetry coverage.
    if let Some(label) = stats.pmdg_variant_label.as_deref() {
        f.insert("PMDG Aircraft".into(), label.to_string());
    }
    if let Some(fnum) = stats
        .pmdg_fmc_flight_number
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        f.insert("PMDG FMC Flight #".into(), fnum.to_string());
    }
    if let Some((v1, vr, v2)) = stats.pmdg_v_speeds_takeoff {
        f.insert(
            "PMDG V-Speeds (Takeoff)".into(),
            format!("V1 {v1} · VR {vr} · V2 {v2} kt"),
        );
    }
    if let Some(vref) = stats.pmdg_vref_at_landing {
        f.insert("PMDG VREF (Landing)".into(), format!("{vref} kt"));
    }
    if let Some(deg) = stats.pmdg_takeoff_flaps_planned {
        f.insert("PMDG Plan TO Flaps".into(), format!("{deg}°"));
    }
    if let Some(deg) = stats.pmdg_landing_flaps_planned {
        f.insert("PMDG Plan LDG Flaps".into(), format!("{deg}°"));
    }
    if let Some(ab) = stats.pmdg_autobrake_at_landing.as_deref() {
        f.insert("PMDG Autobrake (Landing)".into(), ab.to_string());
    }
    if stats.pmdg_takeoff_config_warning_seen {
        f.insert(
            "PMDG TO Config Warning".into(),
            "JA — Warnung war während TakeoffRoll aktiv".to_string(),
        );
    }
    // 777 ECL phase summary — only emit when at least ONE phase
    // was completed (avoids "PMDG ECL: 0/10" on aircraft without
    // ECL like NG3).
    if let Some(ecl) = stats.pmdg_ecl_phases_complete {
        let labels = [
            "Preflight",
            "BeforeStart",
            "BeforeTaxi",
            "BeforeTakeoff",
            "AfterTakeoff",
            "Descent",
            "Approach",
            "Landing",
            "Shutdown",
            "Secure",
        ];
        let done: Vec<&str> = ecl
            .iter()
            .zip(labels.iter())
            .filter_map(|(d, l)| if *d { Some(*l) } else { None })
            .collect();
        if !done.is_empty() {
            f.insert(
                "PMDG ECL Complete".into(),
                format!("{} / 10 ({})", done.len(), done.join(", ")),
            );
        }
    }

    // ---- SimBrief OFP plan + fuel-efficiency (Landing Analyzer Stage 2) ----
    // Surface the dispatcher's plan alongside the actuals, and compute
    // a fuel-efficiency delta so the VA can grade burn discipline.
    if let Some(b) = stats.planned_block_fuel_kg {
        f.insert("Plan Block Fuel".into(), format!("{:.0} kg", b));
    }
    if let Some(b) = stats.planned_burn_kg {
        f.insert("Plan Trip Burn".into(), format!("{:.0} kg", b));
    }
    if let Some(r) = stats.planned_reserve_kg {
        f.insert("Plan Reserve Fuel".into(), format!("{:.0} kg", r));
    }
    // v1.7.35: Sprit-Auswertung ohne Note — die fertigen Werte, nichts gerechnet.
    sprit_pirep_felder(stats.sprit_auswertung.as_ref(), &mut f);
    if let Some(z) = stats.planned_zfw_kg {
        f.insert("Plan ZFW".into(), format!("{:.0} kg", z));
    }
    if let Some(t) = stats.planned_tow_kg {
        f.insert("Plan TOW".into(), format!("{:.0} kg", t));
    }
    if let Some(l) = stats.planned_ldw_kg {
        f.insert("Plan LDW".into(), format!("{:.0} kg", l));
    }
    if let Some(route) = stats.planned_route.as_deref().filter(|s| !s.is_empty()) {
        f.insert("Plan Route".into(), route.to_string());
    }
    if let Some(alt) = stats.planned_alternate.as_deref().filter(|s| !s.is_empty()) {
        f.insert("Plan Alternate".into(), alt.to_string());
    }
    // Fuel efficiency: compare actual trip burn to the planned trip
    // burn. Use takeoff_fuel_kg − landing_fuel_kg as the actual trip
    // burn (excludes taxi out/in, matching SimBrief's est_burn).
    if let (Some(plan_burn), Some(toff), Some(land)) = (
        stats.planned_burn_kg,
        stats.takeoff_fuel_kg,
        stats.landing_fuel_kg,
    ) {
        if plan_burn > 0.0 && toff > 0.0 && land >= 0.0 && toff > land {
            let actual_burn = toff - land;
            let diff = actual_burn - plan_burn; // +ve = burned more than planned
            let pct = (diff / plan_burn) * 100.0;
            let sign = if diff >= 0.0 { "+" } else { "" };
            f.insert(
                "Fuel Efficiency".into(),
                format!("{sign}{:.0} kg ({sign}{:.1}%)", diff, pct),
            );
        }
    }

    // ---- Runway correlation (Tier 2) ----
    if let Some(rw) = &stats.runway_match {
        // Composite line covers airport+ident+length+surface in one
        // glanceable cell instead of five separate snake_case rows.
        f.insert(
            "Touchdown Runway".into(),
            format!(
                "{}/{} ({:.0} m, {})",
                rw.airport_ident,
                rw.runway_ident,
                rw.length_ft as f64 * 0.3048,
                rw.surface,
            ),
        );
        f.insert(
            "Centerline Offset".into(),
            format!("{:.1} m {}", rw.centerline_distance_m.abs(), rw.side),
        );
        let (punkt, laenge) = aufsetzpunkt_texte(rw);
        f.insert("Touchdown Past Threshold".into(), format!("{punkt} ({laenge})"));
    }

    // ---- ATC-derived gates and approach runway (from MSFS SimVars) ----
    if let Some(g) = stats.dep_gate.as_ref().filter(|s| !s.is_empty()) {
        f.insert("Departure Gate".into(), g.clone());
    }
    if let Some(g) = arr_gate_for(stats, effective_arr_icao).filter(|s| !s.is_empty()) {
        f.insert("Arrival Gate".into(), g);
    }
    if let Some(rw) = stats.approach_runway.as_ref().filter(|s| !s.is_empty()) {
        f.insert("Approach Runway (ATC)".into(), rw.clone());
    }

    // ---- Distance / cruise level ----
    if stats.distance_nm > 0.0 {
        f.insert(
            "Flown Distance".into(),
            format!("{:.1} nm", stats.distance_nm),
        );
    }
    if let Some(fl) = stats.peak_altitude_ft {
        f.insert("Cruise Level".into(), format!("FL{:.0}", fl / 100.0));
    }

    // ---- METAR snapshots ----
    if let Some(raw) = stats.dep_metar_raw.as_ref().filter(|s| !s.is_empty()) {
        f.insert("Departure METAR".into(), raw.clone());
    }
    if let Some(raw) = stats.arr_metar_raw.as_ref().filter(|s| !s.is_empty()) {
        f.insert("Arrival METAR".into(), raw.clone());
    }

    f
}

fn humanize_duration_minutes(minutes: i64) -> String {
    let m = minutes.max(0);
    let h = m / 60;
    let r = m % 60;
    if h == 0 {
        format!("{}m", r)
    } else {
        format!("{}h {:02}m", h, r)
    }
}

/// Build the human-readable summary that goes into the PIREP `notes` field —
/// a concise multi-line text that's always visible regardless of how the VA
/// configured custom fields.
fn build_pirep_notes_mit(
    flight: &ActiveFlight,
    stats: &FlightStats,
    effective_arr_icao: &str,
    // Die EINE Bewertung dieses PIREP-Aufbaus (siehe [`pirep_bewertete_teile`]).
    bewertung: Option<&LandungsBewertung>,
) -> String {
    use std::fmt::Write;
    let mut s = String::new();

    // v0.7.19 GAF-707 Accident-Detection: bei Confirmed Accident
    // einen Banner ueber den normalen Header schreiben, damit der
    // VA-Admin in der PIREP-Liste sofort sieht dass das KEIN
    // normaler Flug war. Suspected wird hier NICHT als Banner
    // gezeigt — der Pilot hat den Override-Pfad gewaehlt oder
    // confirmed via Dialog. Spec §PIREP-Payload / Notes.
    if stats.accident_detected {
        if let (Some(kind), Some(confidence)) = (
            stats.accident_kind.as_deref(),
            stats.accident_confidence.as_deref(),
        ) {
            let kind_enum = match kind {
                "sim_crash" => accident::AccidentKind::SimCrash,
                "off_airport_impact" => accident::AccidentKind::OffAirportImpact,
                _ => accident::AccidentKind::Impact,
            };
            let banner =
                accident::build_accident_notes(kind_enum, confidence, &stats.accident_reasons);
            s.push_str("=== ACCIDENT DETECTED ===\n");
            s.push_str(&banner);
            s.push_str("\n=========================\n\n");
        }
    }

    // Header — flight number + route + (when known) callsign airline.
    let _ = write!(
        s,
        "{} · {} → {}",
        flight.flight_number, flight.dpt_airport, flight.arr_airport
    );

    // ---- Times ----
    let mut wrote_section = false;
    let start_section = |s: &mut String, title: &str| {
        s.push_str("\n\n");
        s.push_str(title);
        s.push('\n');
    };
    let any_time = stats.block_off_at.is_some()
        || stats.takeoff_at.is_some()
        || stats.landing_at.is_some()
        || stats.block_on_at.is_some();
    if any_time {
        start_section(&mut s, "TIMES (UTC)");
        wrote_section = true;
        if let Some(t) = stats.block_off_at {
            let _ = writeln!(s, "  Blocks off    {}", t.format("%H:%M:%S"));
        }
        if let Some(t) = stats.takeoff_at {
            let _ = writeln!(s, "  Takeoff       {}", t.format("%H:%M:%S"));
        }
        if let Some(t) = stats.landing_at {
            let _ = writeln!(s, "  Landing       {}", t.format("%H:%M:%S"));
        }
        if let Some(t) = stats.block_on_at {
            let _ = writeln!(s, "  Blocks on     {}", t.format("%H:%M:%S"));
        }
        if let (Some(off), Some(on)) = (stats.block_off_at, stats.block_on_at) {
            let _ = writeln!(
                s,
                "  Block time    {}",
                humanize_duration_minutes((on - off).num_minutes())
            );
        }
        if let (Some(t1), Some(t2)) = (stats.takeoff_at, stats.landing_at) {
            let _ = writeln!(
                s,
                "  Flight time   {}",
                humanize_duration_minutes((t2 - t1).num_minutes())
            );
        }
        if let (Some(off), Some(takeoff)) = (stats.block_off_at, stats.takeoff_at) {
            let _ = writeln!(
                s,
                "  Taxi out      {}",
                humanize_duration_minutes((takeoff - off).num_minutes())
            );
        }
        if let (Some(land), Some(on)) = (stats.landing_at, stats.block_on_at) {
            let _ = writeln!(
                s,
                "  Taxi in       {}",
                humanize_duration_minutes((on - land).num_minutes())
            );
        }
    }

    // ---- Touchdown ----
    if stats.landing_at.is_some() {
        start_section(&mut s, "TOUCHDOWN");
        wrote_section = true;
        // v0.7.21 (MS713-QS): canonical_landing_rate_fpm() statt direkt.
        if let Some(rate) = stats.canonical_landing_rate_fpm() {
            let _ = writeln!(s, "  Landing rate  {:.0} fpm", rate);
        }
        // v0.20.0: der GESCORTE (EMA) G-Wert — derselbe, den Score-Karte,
        // ACARS-Log und das PIREP-Custom-Field "Landing G-Force" zeigen.
        // `landing_g_force` ist der Roh-Wert am Touchdown-Frame und wich
        // sichtbar ab (dritter G-Wert neben Roh-Peak und EMA).
        // v0.20 (QS-Fix, fallback_zero-Score-Bug): `score_g_for_stats`
        // selbst fragen statt der rohen `.is_some()`-Felder — sonst stand
        // hier "G-force 0.15 G" in den Notes, obwohl nie eine echte Landung
        // erfasst wurde (genau der Joel-EWG3552-Befund).
        if let Some(g) = score_g_for_stats(stats) {
            let _ = writeln!(s, "  G-force       {:.2} G", g.scored_g);
        }
        if let Some(p) = stats.landing_pitch_deg {
            let _ = writeln!(s, "  Pitch         {:+.1}°", p);
        }
        if let Some(kt) = stats.landing_speed_kt.filter(|v| *v > 0.0) {
            let _ = writeln!(s, "  Speed         {:.0} kt", kt);
        }
        if let Some(slip) = stats.touchdown_sideslip_deg {
            let _ = writeln!(s, "  Sideslip      {:+.1}°", slip);
        }
        if let Some(hw) = stats.landing_headwind_kt {
            // Positive = headwind, negative = tailwind. We swap the
            // label rather than printing a negative number.
            if hw >= 0.0 {
                let _ = writeln!(s, "  Headwind      {:.0} kt", hw);
            } else {
                let _ = writeln!(s, "  Tailwind      {:.0} kt", -hw);
            }
        }
        if let Some(xw) = stats.landing_crosswind_kt {
            // Positive = from right, negative = from left.
            let side = if xw >= 0.0 { "from right" } else { "from left" };
            let _ = writeln!(s, "  Crosswind     {:.0} kt {}", xw.abs(), side);
        }
        let _ = writeln!(s, "  Bounces       {}", stats.bounce_count);
        if let Some(sd) = stats.canonical_vs_stddev_fpm() {
            let _ = writeln!(s, "  Apr V/S σ     {:.0} fpm", sd);
        }
        // v0.20.0: Kanonik (siehe build_pirep_fields).
        if let Some(sd) = stats.canonical_bank_stddev_deg() {
            let _ = writeln!(s, "  Apr Bank σ    {:.1}°", sd);
        }
        if let Some(m) = stats.rollout_distance_m {
            let _ = writeln!(s, "  Rollout       {:.0} m", m);
        }
        // v0.20.0: DIESELBE Bewertung wie das Custom-Field "Landing Score"
        // und der Landungs-Tab. Vorher baute dieser Block die Note aus der
        // Touchdown-KLASSE (numeric ∈ {100,80,60,30,0}) → auf derselben
        // phpVMS-Seite stand "A+ (SMOOTH, 100/100)" neben
        // "A (smooth) — 92/100".
        if let Some(verdict) = bewertung.and_then(|b| b.verdict) {
            let _ = writeln!(
                s,
                "  Grade         {} ({}, {}/100)",
                verdict.grade,
                verdict.label.to_uppercase(),
                verdict.numeric
            );
        }
        // P-4.40 (Kap. 4.5): Herkunft der Note, wenn nicht das Paketmodul
        // gerechnet hat. Beim Modulfehler steht das Feld „Landing Score“ leer.
        if let Some(v) = bewertung.and_then(bewertung_vermerk) {
            let _ = writeln!(s, "  Bewertung     {v}");
        }
    }

    // ---- Touchdown history (T&G + final) ----
    // Only render when something noteworthy happened: at least one
    // T&G, OR more than one touchdown event total. A routine A→B with
    // a single final landing skips this section to keep notes short.
    // v0.20 (QS-Fix, DLH2064 2026-07-23): siehe die andere Stelle oben —
    // dieselbe Begründung, dieselbe Lücke (roher Vec statt Helper).
    let effective_events = effective_touchdown_events(stats);
    let tg_count = effective_events
        .iter()
        .filter(|e| matches!(e.kind, TouchdownKind::TouchAndGo))
        .count();
    if tg_count > 0 || effective_events.len() > 1 {
        start_section(&mut s, "TOUCHDOWNS");
        wrote_section = true;
        for (i, ev) in effective_events.iter().enumerate() {
            let label = match ev.kind {
                TouchdownKind::TouchAndGo => "T&G",
                TouchdownKind::FinalLanding => "Final",
            };
            let _ = writeln!(
                s,
                "  #{:<2} {:<5} {} V/S {:+.0} fpm · G {:.2} · bounces {}",
                i + 1,
                label,
                ev.timestamp.format("%H:%M:%S"),
                ev.peak_vs_fpm,
                ev.peak_g,
                ev.sub_bounces,
            );
        }
    }
    if stats.go_around_count > 0 {
        start_section(&mut s, "GO-AROUNDS");
        wrote_section = true;
        let _ = writeln!(s, "  Count         {}", stats.go_around_count);
    }

    // ---- Runway ----
    if let Some(rw) = &stats.runway_match {
        start_section(&mut s, "RUNWAY");
        wrote_section = true;
        let _ = writeln!(
            s,
            "  Touchdown     {}/{}  ({}, {:.0} m long)",
            rw.airport_ident,
            rw.runway_ident,
            rw.surface,
            rw.length_ft as f64 * 0.3048,
        );
        let _ = writeln!(
            s,
            "  Centerline    {:.1} m  ({})",
            rw.centerline_distance_m.abs(),
            rw.side
        );
        let _ = writeln!(s, "  Past thresh   {}", aufsetzpunkt_texte(rw).0);
    }

    // ---- Fuel & Weight ----
    let any_fuel = stats.block_fuel_kg.filter(|v| *v > 0.0).is_some()
        || stats.takeoff_weight_kg.filter(|v| *v > 0.0).is_some()
        || stats.landing_weight_kg.filter(|v| *v > 0.0).is_some()
        || stats.landing_fuel_kg.filter(|v| *v > 0.0).is_some();
    if any_fuel {
        start_section(&mut s, "FUEL & WEIGHT");
        wrote_section = true;
        if let Some(b) = stats.block_fuel_kg.filter(|v| *v > 0.0) {
            let _ = writeln!(s, "  Block fuel    {:.0} kg", b);
        }
        if let (Some(b), Some(c)) = (stats.block_fuel_kg, stats.last_fuel_kg) {
            if b > 0.0 && b > c {
                let _ = writeln!(s, "  Fuel used     {:.0} kg", b - c);
            }
        }
        if let Some(f) = stats.landing_fuel_kg.filter(|v| *v > 0.0) {
            let _ = writeln!(s, "  Landing fuel  {:.0} kg", f);
        }
        if let Some(w) = stats.takeoff_weight_kg.filter(|v| *v > 0.0) {
            let _ = writeln!(s, "  TOW           {:.0} kg", w);
        }
        if let Some(w) = stats.landing_weight_kg.filter(|v| *v > 0.0) {
            let _ = writeln!(s, "  LDW           {:.0} kg", w);
        }
        // Stage 2: SimBrief plan + efficiency delta.
        if let Some(b) = stats.planned_burn_kg {
            let _ = writeln!(s, "  Plan burn     {:.0} kg", b);
        }
        if let Some(b) = stats.planned_block_fuel_kg {
            let _ = writeln!(s, "  Plan block    {:.0} kg", b);
        }
        if let Some(t) = stats.planned_tow_kg {
            let _ = writeln!(s, "  Plan TOW      {:.0} kg", t);
        }
        if let Some(l) = stats.planned_ldw_kg {
            let _ = writeln!(s, "  Plan LDW      {:.0} kg", l);
        }
        if let (Some(plan_burn), Some(toff), Some(land)) = (
            stats.planned_burn_kg,
            stats.takeoff_fuel_kg,
            stats.landing_fuel_kg,
        ) {
            if plan_burn > 0.0 && toff > land && toff > 0.0 && land >= 0.0 {
                let actual = toff - land;
                let diff = actual - plan_burn;
                let pct = (diff / plan_burn) * 100.0;
                let sign = if diff >= 0.0 { "+" } else { "" };
                let _ = writeln!(
                    s,
                    "  Efficiency    {sign}{:.0} kg ({sign}{:.1}%)",
                    diff, pct
                );
            }
        }
    }

    // ---- Stand / Runway (ATC-cleared) ----
    // Der Ankunfts-Standplatz zählt nur, wenn er auch ausgegeben wird (er wird
    // unterdrückt, wenn er von einem anderen Flughafen stammt als dem, gegen den
    // eingereicht wird — siehe `arr_gate_for`). Sonst stünde hier eine leere
    // Abschnitts-Überschrift ohne Inhalt.
    let any_atc = stats.dep_gate.as_ref().is_some_and(|v| !v.is_empty())
        || arr_gate_for(stats, effective_arr_icao).is_some_and(|v| !v.is_empty())
        || stats
            .approach_runway
            .as_ref()
            .is_some_and(|v| !v.is_empty());
    if any_atc {
        start_section(&mut s, "GATES & ATC");
        wrote_section = true;
        if let Some(g) = stats.dep_gate.as_ref().filter(|v| !v.is_empty()) {
            let _ = writeln!(s, "  Departure gate  {}", g);
        }
        if let Some(g) = arr_gate_for(stats, effective_arr_icao).filter(|v| !v.is_empty()) {
            let _ = writeln!(s, "  Arrival gate    {}", g);
        }
        if let Some(rw) = stats.approach_runway.as_ref().filter(|v| !v.is_empty()) {
            let _ = writeln!(s, "  Approach rwy    {}", rw);
        }
    }

    // ---- Distance ----
    if stats.distance_nm > 0.0 || stats.peak_altitude_ft.is_some() {
        start_section(&mut s, "DISTANCE & LEVEL");
        wrote_section = true;
        let _ = writeln!(s, "  Flown         {:.1} nm", stats.distance_nm);
        if let Some(fl) = stats.peak_altitude_ft {
            let _ = writeln!(s, "  Cruise level  FL{:.0}", fl / 100.0);
        }
        let _ = writeln!(s, "  Positions     {}", stats.position_count);
    }

    // ---- METAR ----
    let dep_metar = stats.dep_metar_raw.as_ref().filter(|v| !v.is_empty());
    let arr_metar = stats.arr_metar_raw.as_ref().filter(|v| !v.is_empty());
    if dep_metar.is_some() || arr_metar.is_some() {
        start_section(&mut s, "METAR");
        wrote_section = true;
        if let Some(m) = dep_metar {
            let _ = writeln!(s, "  Departure  {}", m);
        }
        if let Some(m) = arr_metar {
            let _ = writeln!(s, "  Arrival    {}", m);
        }
    }

    // Footer
    if wrote_section {
        s.push('\n');
    } else {
        s.push_str("\n\n");
    }
    let _ = write!(
        s,
        "AeroACARS {} · Forensik v{}",
        env!("CARGO_PKG_VERSION"),
        touchdown_v2::FORENSICS_VERSION
    );

    // v0.7.1 Phase 0 Schatten-Validation (Spec docs/spec/v0.7.1-landing-ux-fairness.md
    // §6 Phase 0 Step 4): die landing-scoring Crate berechnet parallel den
    // Master-Score aus den gleichen Feldern wie das TS-LandingPanel +
    // libComputeSubScores. Der Wert wird NICHT in den PIREP-Notes
    // geschrieben (Pilot-sichtbar) sondern via tracing::info!() geloggt
    // — Drift wird im Log-File sichtbar, Pilot sieht nichts.
    //
    // KRITISCH: Inputs muessen identisch sein zu LandingPanel.tsx:137-149
    //   peakVs = landing_peak_vs_fpm ?? landing_rate_fpm
    //   peak_g  = landing_peak_g_force (NICHT landing_g_force)
    //   fuel_efficiency_pct: actual_burn = takeoff_fuel - landing_fuel
    //                        (NICHT block_fuel - landing_fuel — siehe
    //                         build_landing_record lib.rs:6467-6477)
    //
    // Phase-0-Acceptance: dieser Schatten-Wert MUSS dem TS-berechneten
    // Master-Score entsprechen (Bit-Drift = Phase-0-Bug).
    // Phase 2 Update: nutzt jetzt planned_burn + actual_trip_burn
    // (statt fuel_efficiency_pct Legacy-Pfad), planned_zfw + planned_tow
    // damit sub_loadsheet skipped/scored werden kann.
    // Der Sprit-Wert stand hier als handgeschriebene Kopie von
    // `actual_burn_for_record` — byteweise dieselbe Rechnung. Er kommt
    // jetzt aus `scoring_eingang`, wie überall.
    let mut crate_input = scoring_eingang(
        stats,
        muster_fuer_landung(stats, &flight.aircraft_icao),
        Some(Ausweichziel {
            eingereicht: effective_arr_icao,
            geplant: &flight.arr_airport,
        }),
        flight.plan_strecke_nm,
    );
    // v0.10.0 (#runway-utilization-score): Shadow-Validation muss den
    // gleichen Algorithmus rechnen wie der echte PIREP-Pfad — sonst
    // hätten wir Drift gegen uns selbst.
    // ⚠ Das EINGEREICHTE Ziel, nicht das geplante.
    //
    // Hier stand `flight.arr_airport`. Bei einem Divert loeste die
    // Bahngeometrie damit gegen den Planflughafen auf — der Kontroll-
    // score im Protokoll wich vom echten ab, und zwar genau in den
    // Faellen, in denen man ihn braucht. Die drei anderen Aufrufe
    // uebergaben schon immer das effektive Ziel; dieser eine ist beim
    // Herausloesen der Funktion (24.08.2026) uebersehen worden.
    fill_v2_rollout_fields(&mut crate_input, stats, effective_arr_icao);
    // Codex-Befund zu P-4.40: keine eigene Rechnung mehr — protokolliert wird
    // die EINE Bewertung dieses PIREP-Aufbaus, also genau der Wert, der in
    // Score, Feld und Notiz steht. (Hier lief bis dahin eine vierte Rechnung
    // unter derselben Sperre, mit Teilnoten-Zaehlern im Log.)
    if let Some(crate_master) = bewertung.and_then(|b| b.verdict).map(|v| v.numeric) {
        // v0.7.1 P2.6-Fix: Drift-Vergleich gegen den ECHTEN
        // Touchdown-Klassifikator-Score (LandingScore::numeric) ist
        // unsinnig — das ist eine andere Score-Definition (vs/g/bounce-
        // basierte Kategorie). Der Schatten-Wert ist jetzt selbst der
        // gewichtete Aggregate-Score und wird im UI auch so verwendet
        // (P1.3-Fix). Wir loggen nur, damit Audit-Trail im Log bleibt
        // welcher Master-Score in welchem PIREP gelandet ist.
        let touchdown_class = stats.landing_score.map(|sc| sc.label());
        tracing::info!(
            pirep_id = %flight.pirep_id,
            master_score_v0_7_1 = crate_master,
            touchdown_class = ?touchdown_class,
            crate_input_vs_fpm = ?crate_input.vs_fpm,
            crate_input_peak_g = ?crate_input.peak_g_load,
            crate_input_bounces = ?crate_input.bounce_count,
            bewertung_status = bewertung.map(|b| b.status),
            "v0.7.1 Master-Score-Aggregate (Crate-SSoT)"
        );
    }

    s
}
