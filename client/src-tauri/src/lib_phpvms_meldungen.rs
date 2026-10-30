/// v0.13.18: reine FOB-Ableitung des Fuel-Flow (testbar). Genutzt vom
/// Position-Streamer als Fallback, wenn die Standard-SimVar `ENG FUEL FLOW PPH`
/// nichts liefert (iniBuilds/Aerosoft A340-600 / ToLiss). `fuel_flow ≈ ΔFOB/Δt`.
///
/// - `None`, wenn `dt_secs` unter dem Glättungs-Fenster liegt → Aufrufer hält
///   den letzten Wert (kein Flackern bei kurzen Ticks).
/// - sonst die Rate in kg/h, geclampt auf `[0, FF_MAX]`: Tankung (negativ) oder
///   ein FOB-Sprung (Dump/Reset) ergeben `0` statt eines unsinnigen Werts.
fn derive_fuel_flow_kg_per_h(prev_fob_kg: f32, cur_fob_kg: f32, dt_secs: f32) -> Option<f32> {
    // Wert seit v3: bundle/grenzwerte/sprit.json -> ff_derive_window_s (v2.1.24: 60.0)
    // 4-Mot-Heavy verbrennt grob 5–8 t/h; > 30 t/h ist physikalisch unplausibel.
    // Wert seit v3: bundle/grenzwerte/sprit.json -> ff_max_kg_per_h (v2.1.24: 30_000.0)
    if dt_secs < crate::grenzwerte::grenzwerte().sprit.ff_derive_window_s.wert {
        return None;
    }
    let dt_h = dt_secs / 3600.0;
    let rate = (prev_fob_kg - cur_fob_kg) / dt_h;
    Some(
        if rate.is_finite() && (0.0..=crate::grenzwerte::grenzwerte().sprit.ff_max_kg_per_h.wert).contains(&rate) {
            rate
        } else {
            0.0
        },
    )
}

// ---- v0.16.10 Fuel-Used-Fallback (Task B) ----
// SimVar gilt als "lebendig" sobald sie mehr als diesen Wert meldet —
// dann hat sie Vorrang vor dem FOB-Delta.
// Wert seit v3: bundle/grenzwerte/sprit.json -> fuel_used_simvar_alive_kg (v2.1.24: 0.5)
// FOB-Delta muss mindestens so groß sein bevor der Fallback greift
// (filtert FOB-Mess-Rauschen am Gate raus).
// Wert seit v3: bundle/grenzwerte/sprit.json -> fuel_used_min_delta_kg (v2.1.24: 5.0)
// Steigt der FOB mehr als so viel ÜBER die Baseline, war das ein
// Mid-Flight-/Gate-Refuel → Baseline auf den neuen Maximalwert
// re-latchen statt negative Deltas zu produzieren.
// Wert seit v3: bundle/grenzwerte/sprit.json -> fuel_used_refuel_relatch_kg (v2.1.24: 50.0)
// v0.16.10 QS (M1): Gate-DEFUEL — ein instantaner FOB-ABFALL pro Tick
// über diese Schwelle (vor dem Takeoff) re-latcht die Baseline NACH
// UNTEN. Spiegelt den Block-Fuel-Defuel-Guard (`DEFUEL_THRESHOLD_KG`
// in `update_running_stats`): 200 kg liegt über jedem realistischen
// Engine-Burn eines Tick-Intervalls, also kein False-Positive durch
// normalen Betrieb.
// Wert seit v3: bundle/grenzwerte/sprit.json -> fuel_used_defuel_relatch_kg (v2.1.24: 200.0)

/// PMDG liefert FUEL TOTAL QUANTITY USED nicht (Live-Befund GLO4587
/// 2026-06-11) — Fallback aus FOB-Delta.
///
/// Latcht beim ersten Tick mit Fuel an Bord die FOB-Baseline
/// (`initial_fob_kg`, persistiert via PersistedFlightStats) und leitet
/// `fuel_used_kg` als `baseline − FOB` ab, solange die SimVar-Quelle
/// tot ist (< FUEL_USED_SIMVAR_ALIVE_KG — beide Adapter stempeln das
/// Feld aktuell hart auf 0.0). Kommt die SimVar je zum Leben, gewinnt
/// sie. Refuel-Schutz: FOB > Baseline + 50 kg → Baseline re-latchen
/// (Delta startet danach wieder bei ~0, nie negativ — geclampt).
///
/// v0.16.10 QS (M1) Defuel-Schutz: lädt der Pilot am Gate Fuel AB
/// (SimBrief-Korrektur nach unten), bliebe die Baseline sonst auf dem
/// alten Maximum und `fuel_used` wäre den GANZEN Flug um die Defuel-
/// Menge zu hoch. Solange der Flug noch nicht airborne war
/// (`pre_takeoff`, vom Aufrufer als `stats.takeoff_at.is_none()`
/// durchgereicht) re-latcht ein instantaner Tick-Abfall über
/// `FUEL_USED_DEFUEL_RELATCH_KG` die Baseline NACH UNTEN
/// (`prev_fob_kg` = FOB des Vor-Ticks, `stats.last_fuel_kg` VOR dem
/// Stats-Update dieses Ticks). Airborne re-latchen wir NIE nach unten —
/// Caveat: ein Fuel-Jettison zählt damit als "verbraucht"; das ist
/// gewollt konservativ (abgelassener Sprit ist weg, und ein Downward-
/// Re-Latch in der Luft würde echte Dumps unsichtbar machen und wäre
/// von einem Sensor-Glitch nicht unterscheidbar).
fn fuel_used_effective_kg(
    initial_fob_kg: &mut Option<f32>,
    simvar_used_kg: f32,
    fob_kg: f32,
    pre_takeoff: bool,
    prev_fob_kg: Option<f32>,
) -> f32 {
    if fob_kg > 0.0 {
        match *initial_fob_kg {
            // Flugstart (bzw. erster Tick mit Fuel) → Baseline latchen.
            None => *initial_fob_kg = Some(fob_kg),
            // Refuel: FOB deutlich über Baseline → auf neues Max re-latchen.
            Some(initial) if fob_kg > initial + crate::grenzwerte::grenzwerte().sprit.fuel_used_refuel_relatch_kg.wert => {
                *initial_fob_kg = Some(fob_kg);
            }
            // v0.16.10 QS (M1): Gate-Defuel — instantaner Tick-Abfall
            // > 200 kg, Ergebnis unter der Baseline, noch nicht
            // airborne → Baseline nach unten re-latchen. Gradueller
            // APU-/Taxi-Burn (wenige kg pro Tick) triggert das nie.
            Some(initial)
                if pre_takeoff
                    && fob_kg < initial
                    && prev_fob_kg
                        .is_some_and(|prev| prev - fob_kg > crate::grenzwerte::grenzwerte().sprit.fuel_used_defuel_relatch_kg.wert) =>
            {
                *initial_fob_kg = Some(fob_kg);
            }
            Some(_) => {}
        }
    }
    // SimVar lebt → sie ist authoritativ (kennt z.B. APU-Burn exakt).
    if simvar_used_kg >= crate::grenzwerte::grenzwerte().sprit.fuel_used_simvar_alive_kg.wert {
        return simvar_used_kg;
    }
    match *initial_fob_kg {
        Some(initial) => {
            let delta = initial - fob_kg;
            if delta > crate::grenzwerte::grenzwerte().sprit.fuel_used_min_delta_kg.wert {
                // Clamp ist Belt-and-Braces — delta > 5 impliziert > 0.
                delta.max(0.0)
            } else {
                // Kleines/negatives Delta (Gate, frisches Refuel) →
                // SimVar-Wert (≈ 0) unverändert durchreichen.
                simvar_used_kg
            }
        }
        None => simvar_used_kg,
    }
}

/// Eine Live-Meldung an phpVMS (Lebenszeichen, ACARS-Log) ODER das Einreichen
/// — nie beides zugleich.
///
/// Codex-Abnahme 11.10.2026: Ein Lebenszeichen, das beim Einreichen schon
/// unterwegs war, konnte nach `/file` ankommen; `/update` setzt PIREP-Felder
/// per Massenzuweisung und hätte den eingereichten PIREP verändert. Das
/// Einreichen nimmt diese Sperre (`file_pirep`-Aufrufe), der Arbeiter auch —
/// und prüft danach `flight.stop`. Gesetzt wird `stop` vor dem Einreichen;
/// was danach die Sperre bekommt, sendet nichts mehr.
static PHPVMS_LIVE_MELDUNG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Ein Lebenszeichen für phpVMS (`POST /pireps/{id}/update`).
#[derive(Clone)]
struct Lebenszeichen {
    body: UpdateBody,
    phase: FlightPhase,
    phase_changed: bool,
}

/// Was der Streamer an phpVMS meldet, ohne auf die Antwort zu warten.
struct PhpvmsMeldungen {
    /// Immer nur der NEUESTE Stand: Ein Phasenwechsel während eines laufenden
    /// Lebenszeichens geht nicht verloren, er wird danach mit dem dann
    /// aktuellen Stand gesendet; liegt nichts Neues vor, passiert nichts.
    lebenszeichen: tokio::sync::watch::Sender<Option<Lebenszeichen>>,
    /// ACARS-Zeilen gehen in den gemeinsamen Postausgang
    /// (`acars_einreihen`).
    pirep_id: String,
}

impl PhpvmsMeldungen {
    fn lebenszeichen(&self, body: UpdateBody, phase: FlightPhase, phase_changed: bool) {
        let _ = self.lebenszeichen.send(Some(Lebenszeichen {
            body,
            phase,
            phase_changed,
        }));
    }

    fn acars(&self, eintraege: Vec<api_client::LogEntry>) {
        acars_einreihen(&self.pirep_id, eintraege);
    }
}

/// Lebenszeichen (`POST /pireps/{id}/update`) und ACARS-Log
/// (`POST /pireps/{id}/acars/logs`) in eigenen Aufgaben senden.
///
/// # Warum (AZU 8710, 11.10.2026)
///
/// Bis 2.0.9 wartete der Streamer im Takt auf beide Antworten. phpVMS braucht
/// dafür 1,5 bis 3,5 s je Anfrage — bei jedem Phasenwechsel, alle 30 s für
/// das Lebenszeichen und bei jeder neuen ACARS-Zeile (Fahrwerk, Klappen …).
/// In dieser Zeit stand der Streamer. Gemessen an 119 Flügen: nach JEDEM
/// Aufsetzen eine Lücke von 2 bis 8,6 s statt 0,2 bis 0,5 s. Aus den
/// Streamer-Takten entstehen Anflugstabilität, Gleitpfad und die Rollspur
/// der Bahndisziplin; genau in Anflug und Ausrollen fehlten so Sekunden.
/// Die Positionsmeldungen sind seit v0.6.0 auf dieselbe Weise entkoppelt.
///
/// Scheitert ein Lebenszeichen, wird nach `LEBENSZEICHEN_NEUVERSUCH` mit dem
/// dann neuesten Stand erneut gesendet (vorher: im nächsten Takt). Ein 404
/// beendet den Flug wie bisher (`handle_remote_cancellation` setzt
/// `flight.stop`, der Streamer endet im nächsten Takt).
fn spawn_phpvms_meldungen(
    app: AppHandle,
    flight: Arc<ActiveFlight>,
    client: Client,
    meine_generation: u64,
) -> PhpvmsMeldungen {
    const LEBENSZEICHEN_NEUVERSUCH: Duration = Duration::from_secs(3);
    let (lz_tx, mut lz_rx) = tokio::sync::watch::channel::<Option<Lebenszeichen>>(None);
    {
        let (app, flight, client) = (app.clone(), flight.clone(), client.clone());
        tauri::async_runtime::spawn(async move {
            // Wie Worker und Streamer: Nach dem Fortsetzen (neue Generation)
            // meldet ein ueberholter Arbeiter nichts mehr — sonst ginge ein
            // veralteter Stand an phpVMS, oder ein 404 braeche den
            // fortgesetzten Flug ab (Codex-Abnahme 9fd6d834).
            let ueberholt = {
                let flight = flight.clone();
                move || {
                    flight.stop.load(Ordering::Relaxed)
                        || flight.generation.load(Ordering::SeqCst) != meine_generation
                }
            };
            while lz_rx.changed().await.is_ok() {
                {
                    let Some(lz) = lz_rx.borrow_and_update().clone() else {
                        continue;
                    };
                    let _live = PHPVMS_LIVE_MELDUNG.lock().await;
                    if ueberholt() {
                        return;
                    }
                    let antwort = client.update_pirep(&flight.pirep_id, &lz.body).await;
                    if ueberholt() {
                        return;
                    }
                    match antwort {
                        Ok(()) => {
                            if let Ok(mut stats) = flight.stats.lock() {
                                stats.last_heartbeat_at = Some(Utc::now());
                            }
                            tracing::debug!(
                                pirep_id = %flight.pirep_id,
                                phase = ?lz.phase,
                                phase_changed = lz.phase_changed,
                                flight_time_min = lz.body.flight_time.unwrap_or(0),
                                distance_nm = lz.body.distance.unwrap_or(0.0),
                                "PIREP heartbeat sent"
                            );
                        }
                        Err(ApiError::NotFound) => {
                            drop(_live);
                            handle_remote_cancellation(&app, &flight, "POST update");
                            return;
                        }
                        Err(e) => {
                            tracing::warn!(
                                pirep_id = %flight.pirep_id,
                                phase = ?lz.phase,
                                error = %e,
                                "PIREP heartbeat failed"
                            );
                            drop(_live);
                            tokio::time::sleep(LEBENSZEICHEN_NEUVERSUCH).await;
                            if ueberholt() {
                                return;
                            }
                            // Neuer Versuch mit dem dann neuesten Stand.
                            lz_rx.mark_changed();
                        }
                    }
                }
            }
        });
    }
    PhpvmsMeldungen {
        lebenszeichen: lz_tx,
        pirep_id: flight.pirep_id.clone(),
    }
}
