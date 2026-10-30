/// v0.5.39: Beim TD-Edge — Pre-TD-Buffer (snapshot_buffer, ~5 s @ 50 Hz)
/// in den post_touchdown_buffer kopieren und das Landing-Critical-Window
/// öffnen. Der Sampler füllt den Buffer in den nächsten Iterationen mit
/// Post-TD-Samples weiter und dumpt ihn nach TOUCHDOWN_POST_WINDOW_MS.
/// Räumt die Messwerte ab, die zu EINER Landung gehören und beim nächsten
/// Aufsetzen frisch erhoben werden müssen.
///
/// **Warum das eine eigene Funktion ist.** Es gibt zwei Stellen, an denen
/// eine Landungs-Episode endet und die nächste beginnt: der Rücksetzer nach
/// einem bestätigten Touch-and-Go und der Rücksetzer nach einem
/// Mehrfach-Aufsetzen. Beide standen als Hand-Kopien nebeneinander und sind
/// schon einmal auseinandergelaufen — der ausführliche Kommentar zu
/// `landing_rate_fpm` an einer der beiden Stellen dokumentiert genau so
/// einen Fall. In dieser Release-Arbeit ist es zweimal erneut passiert:
/// erst fehlte der Landewind in beiden Kopien, dann die
/// Eigengeschwindigkeit.
///
/// Betroffen sind nur die Felder, die **latchen** statt zu überschreiben.
/// Alles, was der Aufsetz-Stempel bedingungslos neu setzt, braucht hier
/// nichts zu tun.
fn landung_episode_zuruecksetzen(stats: &mut FlightStats) {
    stats.landing_wind_direction_deg = None;
    stats.landing_wind_speed_kt = None;
    stats.landing_true_airspeed_kt = None;
    // Bug-B-QS-Fund (Codex, Vereinheitlichung 09/2026): genau dasselbe
    // Latch-Problem wie oben, nur fuer den neuen Sampler-Ablehnungsgrund.
    // Ohne diesen Reset koennte die Ablehnung von TD-Kandidat #1 (z.B.
    // InsufficientVoteScore bei einem Bounce) auf die naechste, echte
    // Landung derselben Episode durchschlagen und `finalize_landing_score_
    // if_due` fuer sie blockieren.
    stats.sampler_touchdown_rejected_reason = None;
    // 20.10.2026 (Stück 3): auch „Aufsetzfenster nicht gemessen“ latcht
    // (gesetzt, wenn noch keins steht) und gehoert zu EINEM Aufsetzer. Ohne
    // diesen Reset galt nach einem ungemessenen ersten Aufsetzer auch die
    // finale Landung als nicht messbar — keine Sinkrate, keine Note.
    stats.landung_abdeckung_fehlt = None;
}

/// GAF-707-Folgefund (Bugreport 05.09.2026): reine, testbare Entscheidung,
/// ob der Touchdown-Fenster-Dump JETZT abgeschlossen werden muss — entweder
/// weil die vollen `TOUCHDOWN_POST_WINDOW_MS` verstrichen sind (Normalfall),
/// oder weil das Flugzeug INNERHALB dieses Fensters wieder ueber die
/// T&G-AGL-Schwelle geklettert ist. Der zweite Fall fehlte bis hierher: ein
/// Touchdown blieb per `sampler_touchdown_at` gelatcht, bis der Dump
/// abgeschlossen UND danach ein Klettern > 100 ft beobachtet wurde (siehe
/// Reset-Block in `spawn_touchdown_sampler`, gated auf
/// `touchdown_window_dumped_at.is_some()`). Klettert das Flugzeug aber
/// SCHON WAEHREND des laufenden 10-s-Dump-Fensters wieder hoch — ein Bounce,
/// der Sekunden spaeter in einen Aufprall zurueckfaellt, statt erst nach 10 s
/// zu landen — blieb der Sampler auf dem ERSTEN Touchdown haengen: der
/// `sampler_touchdown_at.is_none()`-Guard am Edge-Detector liess den echten,
/// zweiten (und hier schlimmeren) Aufsetzer nie durch. Da
/// `apply_accident_heuristic` ausschliesslich am Dump haengt, sah die
/// Unfall-Erkennung diesen zweiten Touchdown nie — real reproduziert an
/// PIREP JGXrGwlwa4aOGZpn (31.08.2026): T&G bei -1499 fpm/4,67 G, wenige
/// Sekunden spaeter Aufprall bei -3007 fpm/5,88 G, beide innerhalb der
/// ersten 10 s. Frueh dumpen (mit dem bis dahin gesammelten Puffer) behebt
/// das: der Dump gilt sofort als abgeschlossen, der Reset-Block re-armt den
/// Sampler auf demselben Tick, und der zweite Aufsetzer bekommt seinen
/// eigenen Dump + seine eigene `apply_accident_heuristic`-Auswertung.
fn touchdown_dump_faellig(elapsed_seit_touchdown_ms: i64, agl_jetzt_ft: f32) -> bool {
    elapsed_seit_touchdown_ms >= crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert
        || agl_jetzt_ft > crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert
}

fn open_touchdown_capture_window(stats: &mut FlightStats, now: DateTime<Utc>) {
    stats.landing_critical_until =
        Some(now + chrono::Duration::milliseconds(crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert));
    stats.post_touchdown_buffer.clear();
    for s in &stats.snapshot_buffer {
        stats.post_touchdown_buffer.push_back(*s);
    }
    // Touch-and-Go: erlaubt nicht ein zweites Dump-Event (TD-Capture
    // bleibt einmalig per sampler_touchdown_at-Guard), aber wenn diese
    // Funktion aus irgendeinem Grund doch zweimal liefe → Reset clean.
    stats.touchdown_window_dumped_at = None;
    // GAF-707-Race: neue Episode, neuer Dump — die alten score-relevanten
    // Rohdaten gelten nicht mehr fuer DIESEN Touchdown.
    stats.touchdown_window_score_data_ready_at = None;
}

/// Wie zuverlässig der Aufsetz-Sampler tatsächlich gelaufen ist.
///
/// # Warum diese Messung nötig ist (Befund 12.09.2026)
///
/// Der Sampler soll fünfzigmal je Sekunde eine Probe nehmen. Ob er das
/// geschafft hat, stand bisher nirgends — man konnte es nur im Nachhinein
/// aus den Abständen der gespeicherten Proben erahnen. Das ist zweideutig:
/// Eine dünne Probenreihe kann heissen „die Schleife lief nicht", aber auch
/// „sie lief und bekam nichts" oder „die Proben wurden später verworfen".
///
/// Deshalb misst die Schleife sich hier selbst, und zwar VOR dem ersten
/// Zugriff auf Simulator und Flugdaten — jeder Durchlauf zählt, auch der,
/// der gleich wieder abbricht. Die Uhr ist `Instant` (monoton): eine
/// verstellte Systemzeit soll die Diagnose nicht verfälschen.
///
/// Gemessen wird an der Landung, die diese Untersuchung ausgelöst hat
/// (CFG 2090, X-Plane 12 nach 10:44 h): rund zwei statt fünfzig
/// erfolgreiche Erfassungen je Sekunde.
#[derive(Debug, Clone, Default)]
struct SamplerDiagnose {
    /// Durchläufe insgesamt — gezählt, bevor irgendetwas schiefgehen kann.
    laeufe: u64,
    /// Davon mit gespeicherter Probe.
    proben: u64,
    /// Davon ohne Zustand vom Simulator (`current_snapshot` gab nichts).
    ohne_snapshot: u64,
    /// Davon wegen Sprung-/Reset-Erkennung verworfen.
    verworfen_sprung: u64,
    /// Summe der tatsächlichen Taktabstände, in Millisekunden.
    takt_summe_ms: f64,
    /// Längster einzelner Taktabstand, in Millisekunden.
    takt_max_ms: f64,
    /// Wie viele Takte länger als das Doppelte des Solls (40 ms) brauchten.
    takte_ueber_soll: u64,
    /// Längste Dauer eines einzelnen `current_snapshot`-Aufrufs.
    eingang_max_ms: f64,
    /// Wie viele lange Takte im Endanflug schon ins Protokoll gingen
    /// (gedeckelt, damit ein Dauerproblem das Protokoll nicht flutet).
    lange_takte_gemeldet: u32,
    /// v2.0.17: Größter Abstand zwischen Messen und Auswerten eines
    /// Messwerts. Wartet die Auswertung auf die Flugdaten-Sperre, stauen
    /// sich die Messwerte im Kanal — sie gehen nicht verloren, kommen aber
    /// später an. Diese Zahl zeigt, wie viel später.
    rueckstand_max_ms: f64,
    /// Wie viele große Rückstände schon ins Protokoll gingen (gedeckelt).
    rueckstaende_gemeldet: u32,
}

impl SamplerDiagnose {
    /// Mittlerer Takt in Millisekunden — Soll ist 20.
    fn takt_mittel_ms(&self) -> f64 {
        if self.laeufe < 2 {
            return 0.0;
        }
        self.takt_summe_ms / (self.laeufe - 1) as f64
    }

    /// Tatsächliche Erfassungen je Sekunde. Soll ist 50.
    fn proben_je_sekunde(&self) -> f64 {
        let mittel = self.takt_mittel_ms();
        if mittel <= 0.0 || self.laeufe == 0 {
            return 0.0;
        }
        let anteil = self.proben as f64 / self.laeufe as f64;
        1000.0 / mittel * anteil
    }

    /// Für Protokoll und PIREP. Bewusst wenige, gut lesbare Zahlen.
    fn als_json(&self) -> serde_json::Value {
        serde_json::json!({
            "laeufe": self.laeufe,
            "proben": self.proben,
            "ohne_snapshot": self.ohne_snapshot,
            "verworfen_sprung": self.verworfen_sprung,
            "takt_mittel_ms": (self.takt_mittel_ms() * 10.0).round() / 10.0,
            "takt_max_ms": (self.takt_max_ms * 10.0).round() / 10.0,
            "takte_ueber_soll": self.takte_ueber_soll,
            "eingang_max_ms": (self.eingang_max_ms * 10.0).round() / 10.0,
            "proben_je_sekunde": (self.proben_je_sekunde() * 10.0).round() / 10.0,
            "rueckstand_max_ms": (self.rueckstand_max_ms * 10.0).round() / 10.0,
        })
    }
}

/// Ein Messwert des Aufsetz-Abtasters: alles, was die Auswertung vom
/// Simulator braucht, mit dem Zeitpunkt der Messung.
struct AufsetzMessung {
    snap: SimSnapshot,
    /// Zeitpunkt der Messung. Die Auswertung rechnet NUR mit ihm — nie mit
    /// der Uhr beim Verarbeiten, die nach einem Stau später geht.
    now: DateTime<Utc>,
    /// Der frame-genaue Aufsetzer des X-Plane-Plugins, beim Messen
    /// abgeholt. `current_premium_touchdown` leert ihn (`take`) — er gehört
    /// deshalb zu genau einem Messwert und darf nur hier abgeholt werden.
    premium: Option<sim_xplane::PremiumTouchdown>,
}

/// Ein abgeholter Plugin-Aufsetzer, den keine Auswertung mehr annahm (je
/// Flug). Der Adapter gibt ihn nur einmal heraus (`take`); stürzt die
/// Auswertung genau zwischen Prüfung und Abholen ab, bliebe er sonst
/// verloren. Der nächste Messfaden desselben Flugs reicht ihn zuerst weiter
/// — wie früher, als er bis zum nächsten Takt im Adapter liegen blieb
/// (Codex-Nachprüfung 2.0.17).
static ZURUECKGELEGTER_AUFSETZER: Mutex<Option<(String, sim_xplane::PremiumTouchdown)>> =
    Mutex::new(None);

fn aufsetzer_zuruecklegen(pirep_id: &str, td: sim_xplane::PremiumTouchdown) {
    *ZURUECKGELEGTER_AUFSETZER
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((pirep_id.to_string(), td));
}

fn zurueckgelegten_aufsetzer_nehmen(pirep_id: &str) -> Option<sim_xplane::PremiumTouchdown> {
    let mut platz = ZURUECKGELEGTER_AUFSETZER
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if platz.as_ref().is_some_and(|(p, _)| p == pirep_id) {
        platz.take().map(|(_, td)| td)
    } else {
        None
    }
}

/// Die Diagnose lesen, auch nach einem Absturz eines der beiden Fäden.
fn diagnose_lesen(diagnose: &Mutex<SamplerDiagnose>) -> SamplerDiagnose {
    diagnose.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Der Messfaden des Aufsetz-Abtasters (v2.0.17).
///
/// ⚠ Fasst `flight.stats` NIE an — das ist sein ganzer Zweck. Bis 2.0.16
/// nahm der Abtaster für jeden Messwert die Flugdaten-Sperre; hielt
/// `step_flight` sie beim Aufsetzen (AIB 431, OMDB: 129 ms), fiel die
/// Messung aus. Jetzt wartet höchstens die Auswertung, und die Messwerte
/// stauen sich im Kanal, statt zu fehlen.
///
/// Eigene Sperre nur `diagnose` (kurz, je Takt). Die Auswertung nimmt sie
/// auch unter `flight.stats` — Reihenfolge also immer `stats` → `diagnose`,
/// und dieser Faden nimmt `stats` gar nicht: kein Verklemmen möglich.
///
/// Simulator und Plugin kommen als Lesefunktionen herein (im Betrieb
/// `current_snapshot` / `current_premium_touchdown`) — so lässt sich im Test
/// belegen, dass dieser Faden weitermisst, während die Sperre gehalten wird.
fn aufsetz_messen(
    flight: &ActiveFlight,
    meine_generation: u64,
    diagnose: &Mutex<SamplerDiagnose>,
    an_auswertung: &std::sync::mpsc::Sender<AufsetzMessung>,
    auswertung_lebt: &AtomicBool,
    mut simulator_lesen: impl FnMut() -> Option<SimSnapshot>,
    mut plugin_aufsetzer_abholen: impl FnMut() -> Option<sim_xplane::PremiumTouchdown>,
) {
    // v0.7.18 (B-013): Sim-Position-Reset-Detection.
    //
    // Spec docs/spec/v0.7.18-orphan-flight-cleanup.md §B-013.
    //
    // Quelle: GAF 152 Crash 2026-05-12 — MSFS produzierte einen
    // Sample mit lat/lon ≈ 0/0 / alt=53820 ft mid-flight. Sampler
    // hat das uebernommen und einen 14-G-Crash daraus geloggt.
    //
    // Detection-Kriterien (ein Match reicht zum Verwerfen):
    //   1. lat/lon ≈ 0/0 (innerhalb 0.1°)         — Null-Island-Reset
    //   2. Altitude-Sprung > 10.000 ft seit prev  — physikal unmoeglich
    //   3. Position-Sprung > 10 nmi seit prev     — teleport
    //
    // dt-Guard: Jump-Checks nur bei consecutive samples mit
    // dt <= RESET_CHECK_MAX_DT_SECS. Bei groesserer Luecke
    // (Pause, Disconnect, App-Hintergrund) sind grosse legitime
    // Spruenge erwartbar — wir setzen prev_state dann einfach
    // mit dem neuen Sample neu, kein Reset-Detect.
    let mut prev_sample_for_reset_check: Option<(f64, f64, f64, DateTime<Utc>)> = None;
    let mut reset_warning_logged_this_session = false;
    // Wert seit v3: bundle/grenzwerte/abgabe.json -> reset_null_island_deg (v2.1.24: 0.1)
    // Wert seit v3: bundle/grenzwerte/abgabe.json -> reset_max_alt_jump_ft (v2.1.24: 10_000.0)
    // Strecke und Abstand stehen jetzt in `RESET_LIMITS` bei
    // `classify_segment` — hier bleiben nur die Werte, die diese
    // Schleife selbst auswertet.
    let mut letzter_takt: Option<std::time::Instant> = None;
    // Höchstens eine Meldung „Takt zu lang“ je Sekunde (v2.0.18): Bei 60 statt
    // 200 ms Schwelle wäre die Obergrenze sonst in einem holprigen Anflug
    // verbraucht, und im Ausrollen bliebe es stumm.
    let mut letzte_takt_meldung: Option<std::time::Instant> = None;

    loop {
        // 20 ms = 50 Hz target — matches GEES (`SAMPLE_RATE = 20`),
        // the only open-source reference impl that publishes its
        // sample cadence. The actual upper bound comes from how
        // fast `current_snapshot` returns fresh data; on a
        // typical PC the SimConnect adapter ticks at the rendered
        // frame rate (60-120 fps) so we never under-sample here.
        std::thread::sleep(Duration::from_millis(20));
        // Ueberholte Generation (Flug nach gescheitertem Einreichen
        // fortgesetzt): Der neu gestartete Sampler uebernimmt — sonst
        // liefen zwei parallel, wenn dieser den kurzen Stopp verpasst
        // hat (s. `ActiveFlight::generation`).
        if flight.stop.load(Ordering::Relaxed)
            || flight.generation.load(Ordering::SeqCst) != meine_generation
            || !auswertung_lebt.load(Ordering::SeqCst)
        {
            break;
        }
        // ⚠ Die Zählung steht VOR jedem Zugriff — sonst fehlen genau die
        // Durchläufe, die weiter unten abbrechen, und die Diagnose
        // beschönigt sich selbst.
        let takt_jetzt = std::time::Instant::now();
        let mut takt_abstand_ms = 0.0;
        {
            let mut d = diagnose.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(vorher) = letzter_takt {
                let abstand_ms = takt_jetzt.duration_since(vorher).as_secs_f64() * 1000.0;
                takt_abstand_ms = abstand_ms;
                d.takt_summe_ms += abstand_ms;
                if abstand_ms > d.takt_max_ms {
                    d.takt_max_ms = abstand_ms;
                }
                if abstand_ms > 40.0 {
                    d.takte_ueber_soll += 1;
                }
            }
            d.laeufe += 1;
        }
        letzter_takt = Some(takt_jetzt);

        let eingang_start = std::time::Instant::now();
        let snapshot = simulator_lesen();
        let eingang_ms = eingang_start.elapsed().as_secs_f64() * 1000.0;
        {
            let mut d = diagnose.lock().unwrap_or_else(|e| e.into_inner());
            if eingang_ms > d.eingang_max_ms {
                d.eingang_max_ms = eingang_ms;
            }
            if snapshot.is_none() {
                d.ohne_snapshot += 1;
            }
        }
        let Some(snap) = snapshot else {
            continue;
        };
        let now = Utc::now();

        // v0.7.18 (B-013) Reset-Detection — vor Buffer-Push + Edge-Detect.
        // Dieselbe Regel wie die Streckenzaehlung und das Ausrollen —
        // `classify_segment` mit der hier erprobten Toleranz
        // (`RESET_LIMITS`: 10 NM flach, 10.000 ft, bis 2 s Abstand).
        // Vorher stand die Pruefung dreimal im Code, in drei Fassungen;
        // eine davon zaehlte einen Sprung von 4900 NM als geflogen.
        let is_null_island =
            snap.lat.abs() < crate::grenzwerte::grenzwerte().abgabe.reset_null_island_deg.wert && snap.lon.abs() < crate::grenzwerte::grenzwerte().abgabe.reset_null_island_deg.wert;
        let (alt_jump, dist_jump_nm, urteil) = match prev_sample_for_reset_check {
            Some((p_lat, p_lon, p_alt, p_at)) => {
                let dt = (now - p_at).num_milliseconds() as f64 / 1000.0;
                let alt_diff = (snap.altitude_msl_ft - p_alt).abs();
                let dist_nm = ::geo::distance_m(p_lat, p_lon, snap.lat, snap.lon) / 1852.0;
                let u = classify_segment(
                    (p_lat, p_lon),
                    (snap.lat, snap.lon),
                    Some(alt_diff),
                    Some(dt),
                    reset_limits(),
                );
                (alt_diff, dist_nm, u)
            }
            None => (0.0, 0.0, SegmentVerdict::Flown),
        };
        let ist_sprung = matches!(urteil, SegmentVerdict::Jump(_));
        let is_alt_jump = ist_sprung && alt_jump > crate::grenzwerte::grenzwerte().abgabe.reset_max_alt_jump_ft.wert;

        if is_null_island || ist_sprung {
            if !reset_warning_logged_this_session {
                let reason = if is_null_island {
                    "lat/lon ≈ 0/0 (Null-Island)"
                } else if is_alt_jump {
                    "Altitude-Sprung > 10.000 ft"
                } else {
                    "Position-Sprung > 10 nmi"
                };
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    lat = snap.lat,
                    lon = snap.lon,
                    alt_ft = snap.altitude_msl_ft,
                    alt_jump_ft = alt_jump,
                    dist_jump_nm = dist_jump_nm,
                    reason = reason,
                    "Sim-Position-Reset detektiert — Sample verworfen"
                );
                reset_warning_logged_this_session = true;
            }
            // Sample komplett verwerfen — kein Buffer-Push, kein
            // Edge-Update. prev_sample_for_reset_check absichtlich
            // NICHT updaten, sonst gewoehnt sich der Detector an
            // den Reset und der naechste echte Sample springt
            // wieder >10 nmi von 0/0 weg.
            diagnose
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .verworfen_sprung += 1;
            continue;
        }

        // Sample ist OK ODER dt war zu gross (Resume-Fall) →
        // prev_state mit dem aktuellen Sample neu setzen.
        prev_sample_for_reset_check = Some((snap.lat, snap.lon, snap.altitude_msl_ft, now));

        // AZU 8710 (11.10.2026): Im Endanflug stand der Sampler mehrfach
        // 0,4–0,8 s, ohne dass klar war, worauf er wartete. Jeder lange
        // Takt sagt jetzt, wo die Zeit blieb: vor dem Takt (Faden nicht
        // dran) oder beim Simulator. Auf die Flugdaten-Sperre wartet dieser
        // Faden seit 2.0.17 nicht mehr; wer sie lange hält, meldet
        // `stats_sperre`.
        //
        // v2.0.17: auch nach dem Aufsetzen, solange das Flugzeug noch
        // schnell rollt — dort lag die Lücke von AIB 431, und vorher galt
        // die Meldung nur in der Luft.
        let im_wichtigen_abschnitt = if snap.on_ground {
            snap.groundspeed_kt > 30.0
        } else {
            snap.altitude_agl_ft < 1000.0
        };
        // v2.0.18: ab 60 ms statt 200 ms (Takt) und 50 statt 100 ms
        // (Simulator). Die Lücke von AIB 431 war 129 ms lang und wäre unter
        // der alten Schwelle stumm geblieben. 60 ms = drei Soll-Takte; ein
        // grober Windows-Zeitgeber (bis ~16 ms) bleibt darunter.
        if im_wichtigen_abschnitt && (takt_abstand_ms > 60.0 || eingang_ms > 50.0) {
            let melden = {
                let mut d = diagnose.lock().unwrap_or_else(|e| e.into_inner());
                let melden = d.lange_takte_gemeldet < 200
                    && letzte_takt_meldung
                        .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
                if melden {
                    d.lange_takte_gemeldet += 1;
                    letzte_takt_meldung = Some(std::time::Instant::now());
                }
                melden
            };
            if melden {
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    agl_ft = snap.altitude_agl_ft,
                    am_boden = snap.on_ground,
                    takt_abstand_ms,
                    eingang_ms,
                    "Sampler-Takt im Endanflug zu lang"
                );
            }
        }

        // Nur abholen, wenn jemand auswertet — sonst ginge der einmalige
        // Aufsetzer mit dem Messwert verloren, statt für die nächste
        // Auswertung im Adapter zu bleiben.
        if !auswertung_lebt.load(Ordering::SeqCst) {
            break;
        }
        let premium = zurueckgelegten_aufsetzer_nehmen(&flight.pirep_id)
            .or_else(&mut plugin_aufsetzer_abholen);
        // Die Auswertung ist weg (beendet oder abgestürzt) — dann gibt es
        // niemanden mehr, für den gemessen wird. Ein schon abgeholter
        // Plugin-Aufsetzer wird zurückgelegt, nicht verworfen.
        if let Err(std::sync::mpsc::SendError(messung)) =
            an_auswertung.send(AufsetzMessung { snap, now, premium })
        {
            if let Some(td) = messung.premium {
                aufsetzer_zuruecklegen(&flight.pirep_id, td);
            }
            break;
        }
    }
}

fn spawn_touchdown_sampler(app: AppHandle, flight: Arc<ActiveFlight>) {
    // Guard gegen Doppel-Spawn (mehrere flight_resume etc.) — analog zu
    // streamer_spawned/phpvms_worker_spawned. Ohne dies koennen zwei
    // unabhaengige Aufrufer von flight_resume_confirm (z. B. das
    // Hauptfenster UND ein via LAN-Fernbedienung verbundenes Tablet, siehe
    // remote/bridge.rs) zwei parallele Sampler fuer denselben Flug starten.
    let Some(meine_generation) = startsperre_nehmen(&flight, &flight.touchdown_sampler_spawned)
    else {
        return;
    };
    // Eigener Faden statt Aufgabe im gemeinsamen Pool (AZU 8710, 11.10.2026):
    // Die Aufsetzmessung hat Vorfahrt. Als Pool-Aufgabe kam sie nur dran,
    // wenn ein Arbeits-Faden frei war — blockierte irgendetwas anderes im
    // Client einen davon, fiel der 20-ms-Takt aus (bei AZU 8710 in der Flare
    // 4 statt 50 Proben je Sekunde). Jetzt läuft sie mit eigenem Takt und
    // meldet sich beim Betriebssystem als dringlich an
    // (`flug_aktivitaet::messfaden_vorrang`).
    let flight_fuer_fehler = flight.clone();
    let gestartet = std::thread::Builder::new()
        // Seit 2.0.17 die AUSWERTUNG; gemessen wird in `aufsetz-messfaden`.
        .name("aufsetz-auswertung".into())
        .spawn(move || {
        // Stürzt der Faden ab (Panik), den Merker zurücksetzen — sonst ließe
        // sich die Aufsetzmessung für diesen Flug nie wieder starten
        // (Codex-Gesamtabnahme 12.10.2026).
        struct MerkerBeiPanik(Arc<ActiveFlight>, u64);
        impl Drop for MerkerBeiPanik {
            fn drop(&mut self) {
                // Nur fuer die eigene Generation: Ein ueberholter Faden darf
                // den Merker des neuen nicht freigeben.
                if !std::thread::panicking() {
                    return;
                }
                // Unter `start_sperre`, damit kein Fortsetzen zwischen
                // Pruefung und Freigabe faellt (Codex-Abnahme 9fd6d834).
                let _start = self.0.start_sperre.lock().unwrap_or_else(|e| e.into_inner());
                if self.0.generation.load(Ordering::SeqCst) == self.1 {
                    self.0.touchdown_sampler_spawned.store(false, Ordering::SeqCst);
                    tracing::error!(pirep_id = %self.0.pirep_id, "Aufsetz-Messfaden abgestürzt — Merker zurückgesetzt");
                }
            }
        }
        let _merker = MerkerBeiPanik(flight.clone(), meine_generation);
        // Hilfsfunktionen in der Schleife dürfen weiter Aufgaben starten.
        let laufzeit = tauri::async_runtime::handle();
        let _laufzeit_kontext = laufzeit.inner().enter();
        flug_aktivitaet::messfaden_vorrang();
        // Lebt so lange wie der Sampler — siehe `flug_aktivitaet`.
        let _flug_aktivitaet = flug_aktivitaet::FlugAktivitaet::beginnen();
        tracing::info!(pirep_id = %flight.pirep_id, "touchdown sampler started");
        // v0.4.4: lokale Edge-Tracking-State.
        //
        // X-Plane: bevorzugt `gear_normal_force_n` (Newton, spikt
        // präzise im Frame des physischen Touchdowns) — das macht
        // auch xgs (etabliertes X-Plane-Landing-Speed-Plugin seit ~10
        // Jahren). xgs nutzt exakt `force != 0.0` als Trigger; wir
        // wählen `> 1.0 N` als minimaler Filter gegen Float-Noise und
        // catchen damit immer noch den ersten Kontakt-Frame (echte
        // Touchdowns gehen blitzartig auf mehrere kN — ein 60-300t
        // Airliner mit 1.0g Bremsmoment = mindestens 588-2940 kN auf
        // dem Gear). 1.0 N = ~100g Auflagedruck, also unter jeder
        // physikalisch plausiblen Touchdown-Schwelle.
        //
        // MSFS: gear_normal_force_n ist None → fallback auf
        // `on_ground`-Edge wie vorher (MSFS hat eh den separaten
        // `PLANE TOUCHDOWN NORMAL VELOCITY`-SimVar als Primary).
        let mut prev_in_air: Option<bool> = None;
        // Letzter Messwert (am Boden, Höhe MSL) für die eigene Erkennung eines
        // Sim-Neuladens — siehe `sampler_sieht_neuladen`.
        let mut prev_hoehe: Option<(bool, f64)> = None;
        // ── Messen und Auswerten in ZWEI Fäden (v2.0.17) ─────────────────
        //
        // Anlass AIB 431 (OMDB, 13.10.2026): Seit 2.0.14 hatte jeder Flug
        // genau eine Lücke kurz nach dem Aufsetzen, 58 bis 129 ms, wachsend
        // mit der Größe des Platzes. Der Abtaster nahm für JEDEN Messwert
        // `flight.stats` — und genau dann hielt die Landungsauswertung von
        // `step_flight` diese Sperre. Solange er wartete, maß er nicht.
        //
        // Jetzt misst ein eigener Faden (`aufsetz_messen`), der
        // `flight.stats` nie anfasst, und reicht jeden Messwert mit seinem
        // Zeitstempel durch einen Kanal. Dieser Faden hier wertet aus. Muss
        // er auf die Sperre warten, stauen sich die Messwerte und werden
        // danach der Reihe nach verarbeitet — keiner geht verloren, und jede
        // Auswertung rechnet mit dem Zeitpunkt ihres Messwerts (`now`).
        //
        // Selbstmessung siehe `SamplerDiagnose`; sie liegt hinter einer
        // eigenen kleinen Sperre, nicht in `FlightStats`.
        let diagnose = Arc::new(Mutex::new(SamplerDiagnose::default()));
        let (an_auswertung, messungen) = std::sync::mpsc::channel::<AufsetzMessung>();
        // Lebt die Auswertung noch? Der Messfaden fragt das, bevor er den
        // einmaligen Plugin-Aufsetzer abholt (`take`).
        let auswertung_lebt = Arc::new(AtomicBool::new(true));
        // ⚠ Nach `_merker` angelegt, also VOR ihm aufgeräumt: Endet dieser
        // Faden (auch durch Panik), meldet er sich ab und wartet auf das
        // Ende des Messfadens — erst danach gibt `_merker` den Start frei.
        // Sonst könnte ein sofortiges Fortsetzen einen zweiten Messfaden
        // starten, während der alte noch läuft; nähme der alte den
        // Plugin-Aufsetzer, ginge er mit dem toten Kanal verloren
        // (Codex-Abnahme 2.0.17, P2).
        struct MessfadenBeenden {
            lebt: Arc<AtomicBool>,
            faden: Option<std::thread::JoinHandle<()>>,
        }
        impl Drop for MessfadenBeenden {
            fn drop(&mut self) {
                self.lebt.store(false, Ordering::SeqCst);
                if let Some(faden) = self.faden.take() {
                    let _ = faden.join();
                }
            }
        }
        let mut _messfaden_beenden = MessfadenBeenden {
            lebt: Arc::clone(&auswertung_lebt),
            faden: None,
        };
        {
            let (app_m, flight_m, diagnose_m) = (app.clone(), flight.clone(), diagnose.clone());
            let lebt_m = Arc::clone(&auswertung_lebt);
            let messfaden = std::thread::Builder::new()
                .name("aufsetz-messfaden".into())
                .spawn(move || {
                    // Kein eigener Merker-Rücksetzer: Stürzt dieser Faden
                    // ab, endet die Auswertung mit dem Kanal und setzt den
                    // Merker NACH dem Abarbeiten zurück (Schleifenende
                    // unten). Hier zurückgesetzt, könnte ein Fortsetzen eine
                    // zweite Auswertung starten, während die erste noch
                    // gestaute Messwerte in denselben Puffer schreibt.
                    flug_aktivitaet::messfaden_vorrang();
                    flug_aktivitaet::messfaden_vorrang_windows();
                    aufsetz_messen(
                        &flight_m,
                        meine_generation,
                        &diagnose_m,
                        &an_auswertung,
                        &lebt_m,
                        || current_snapshot(&app_m),
                        || current_premium_touchdown(&app_m),
                    );
                });
            let messfaden = match messfaden {
                Ok(h) => {
                    _messfaden_beenden.faden = Some(h);
                    Ok(())
                }
                Err(e) => Err(e),
            };
            if let Err(e) = messfaden {
                // Wie beim Start des Abtasters selbst: Merker der eigenen
                // Generation freigeben, sonst ließe sich die Messung für
                // diesen Flug nie wieder starten.
                let _start = flight.start_sperre.lock().unwrap_or_else(|e| e.into_inner());
                if flight.generation.load(Ordering::SeqCst) == meine_generation {
                    flight.touchdown_sampler_spawned.store(false, Ordering::SeqCst);
                }
                tracing::error!(pirep_id = %flight.pirep_id, error = %e, "Aufsetz-Messfaden konnte nicht gestartet werden");
                return;
            }
        }

        for messung in messungen {
            if flight.stop.load(Ordering::Relaxed)
                || flight.generation.load(Ordering::SeqCst) != meine_generation
            {
                break;
            }
            let AufsetzMessung { snap, now, premium } = messung;
            // Wie weit die Auswertung hinter der Messung liegt — VOR der
            // Sperre gemessen und gemeldet (keine Logzeile unter `stats`).
            let rueckstand_ms = (Utc::now() - now).num_milliseconds().max(0) as f64;
            let rueckstand_melden = {
                let mut d = diagnose.lock().unwrap_or_else(|e| e.into_inner());
                if rueckstand_ms > d.rueckstand_max_ms {
                    d.rueckstand_max_ms = rueckstand_ms;
                }
                let melden = rueckstand_ms > 100.0 && d.rueckstaende_gemeldet < 40;
                if melden {
                    d.rueckstaende_gemeldet += 1;
                }
                melden
            };
            if rueckstand_melden {
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    rueckstand_ms,
                    am_boden = snap.on_ground,
                    agl_ft = snap.altitude_agl_ft,
                    "Aufsetz-Auswertung liegt hinter der Messung (Messwerte gestaut, nicht verloren)"
                );
            }

            let mut stats = flight.stats.lock().expect("flight stats");
            // 2.0.28: Nach einem Sim-Neuladen mitten im Flug ruht die
            // Aufsetzerkennung, bis der Pilot fortsetzt (DLA 8260, 12.10.2026)
            // — siehe `aufsetz_erkennung_ruht_nach_reload`. `prev_in_air`
            // vergessen: Sonst ergaebe der erste Messwert nach dem Fortsetzen
            // am Boden eine Flanke gegen den Stand von vor dem Neuladen.
            if aufsetz_erkennung_ruht_nach_reload(
                &mut stats,
                flight.was_just_resumed.load(Ordering::SeqCst),
            ) {
                prev_in_air = None;
                prev_hoehe = None;
                if let Some(td) = premium {
                    tracing::debug!(
                        pirep_id = %flight.pirep_id,
                        captured_vs_fpm = td.captured_vs_fpm,
                        "premium TD event dropped (flight paused after sim reload)"
                    );
                }
                continue;
            }
            // Das Pausen-Flag setzt erst der Streamer im nächsten Takt (bis
            // ~3 s); der 50-Hz-Abtaster könnte die Luft→Boden-Flanke des
            // neu geladenen Sims vorher schon bestätigen. Darum erkennt er den
            // Sprung selbst und verwirft ihn (Codex-Review PR #158).
            let neu_geladen = sampler_sieht_neuladen(stats.phase, prev_hoehe, snap.on_ground, snap.altitude_msl_ft);
            prev_hoehe = Some((snap.on_ground, snap.altitude_msl_ft));
            if neu_geladen {
                aufsetz_erkennung_ruht_nach_reload(&mut stats, true);
                prev_in_air = None;
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    alt_ft = snap.altitude_msl_ft,
                    "Aufsetz-Abtaster: Sim-Neuladen erkannt, Messwert verworfen"
                );
                continue;
            }
            // v1.8.1: das Muster frueh festhalten, mit derselben Kette wie im
            // Anflugblock (`sim_core::muster_aufloesen`). Die TCH-Einstufung
            // nach Raederhoehe liest es nach dem Aufsetzen; ohne diesen Schritt
            // fiele sie bei einem spaeten Einstieg auf die Mittelgruppe zurueck,
            // obwohl die Buchung das Muster kennt (Cloud-QS, Befund 6).
            if stats.aufgeloestes_muster.is_none() {
                stats.aufgeloestes_muster = sim_core::muster_aufloesen(
                    snap.aircraft_icao.as_deref(),
                    &flight.aircraft_icao,
                    snap.aircraft_title.as_deref(),
                );
            }
            // v1.6.10: solange das Flugzeug in der Luft ist, den Stand des
            // Sim-Latches mitfuehren. Erst dieser Vorher-Wert macht beim
            // Aufsetzen unterscheidbar, ob der Simulator die Zahl fuer
            // DIESE Landung gesetzt hat — oder ob dort noch die des
            // vorigen Aufsetzens steht (Touch-and-Go, Hopser).
            if !snap.on_ground {
                if let Some(v) = snap.touchdown_vs_fpm {
                    if v.is_finite() {
                        stats.sim_referenz_vor_aufsetzen = Some(v);
                    }
                }
            }
            diagnose.lock().unwrap_or_else(|e| e.into_inner()).proben += 1;
            if let Some(hz) = snap.sim_bildrate_hz {
                stats.bildrate_puffer.push_back((now, hz));
            }
            let bildrate_grenze = now - chrono::Duration::seconds(BILDRATE_PUFFER_SECS);
            while stats
                .bildrate_puffer
                .front()
                .is_some_and(|(at, _)| *at < bildrate_grenze)
            {
                stats.bildrate_puffer.pop_front();
            }
            stats.snapshot_buffer.push_back(TelemetrySample {
                at: now,
                // The snapshot_buffer is the touchdown-window source (feeds the
                // touchdown_v2 cascade, vs_at_edge and landing_peak). It MUST use
                // the raw, lag-free V/S (X-Plane local_vy) — using the damped
                // display VVI here would re-introduce the v0.4.3 VSI-lag bug
                // where a real sink averages to ~0 by the touchdown moment.
                vs_fpm: snap.touchdown_vs_source_fpm(),
                g_force: snap.g_force,
                g_semibody: snap.g_semibody,
                on_ground: snap.on_ground,
                agl_ft: snap.altitude_agl_ft as f32,
                msl_ft: snap.altitude_msl_ft as f32,
                heading_true_deg: snap.heading_deg_true,
                groundspeed_kt: snap.groundspeed_kt,
                true_airspeed_kt: snap.true_airspeed_kt,
                indicated_airspeed_kt: snap.indicated_airspeed_kt,
                lat: snap.lat,
                lon: snap.lon,
                pitch_deg: snap.pitch_deg,
                bank_deg: snap.bank_deg,
                fuel_kg: snap.fuel_total_kg,
                // v0.7.0 — Forensik-v2 fields direkt aus dem Sim-Snapshot.
                // gear_normal_force_n ist X-Plane-only (None bei MSFS),
                // total_weight_kg ist bei beiden Sims meist verfuegbar.
                gear_normal_force_n: snap.gear_normal_force_n,
                total_weight_kg: snap.total_weight_kg,
            });
            if let Some(probe) = stats.snapshot_buffer.back().copied() {
                schwellenspur_fuehren(&mut stats, probe);
            }
            // Bodenhöhe des Flugzeugs mitmessen, solange es ruhig am Boden
            // steht oder rollt (Befund DLH 880, `touchdown_v2::BodenhoehenReferenz`).
            //
            // Nur VOR dem ersten Abheben (Codex-Abnahme 15.09.2026): Danach
            // steht der Wert fest. Ein späterer Bodenabschnitt — Zwischenhalt,
            // Touch-and-Go, ein Szenerieobjekt mit falschem Gelände — darf den
            // Bezug nicht mehr verschieben und damit die Tiefflug-Grenze für
            // die eigentliche Landung anheben.
            if stats.takeoff_at.is_none()
                && stats.sampler_takeoff_at.is_none()
                && touchdown_v2::ist_rollprobe(
                    snap.on_ground,
                    snap.groundspeed_kt,
                    snap.paused,
                    snap.slew_mode,
                )
            {
                stats.bodenhoehe.beobachte(snap.altitude_agl_ft as f32);
            }

            // v0.5.11: running peak-descent VS in the LOW-ALTITUDE zone
            // only (AGL ≤ 250 ft). Earlier versions (v0.5.5+) tracked
            // this across the whole Approach + Final segment, which
            // produced phantom hard-landing reports when a steep
            // pre-flare descent (e.g. -1346 fpm @ 943 ft AGL) won
            // over the actual gentle touchdown.
            //
            // Now: ONLY samples from the low-altitude footprint count.
            // This is a fallback only — the AGL-derivative estimator
            // (estimate_xplane_touchdown_vs_from_agl) is the primary
            // source. low_agl_vs_min_fpm is used when the estimator
            // can't find a valid window (extremely sparse RREF, etc.).
            let approach_or_final =
                matches!(stats.phase, FlightPhase::Approach | FlightPhase::Final);
            if approach_or_final && !snap.on_ground && snap.altitude_agl_ft <= 250.0 {
                let pitch_rad = (snap.pitch_deg as f32) * std::f32::consts::PI / 180.0;
                // Touchdown VS fallback → raw, lag-free signal (X-Plane local_vy),
                // NOT the display VVI, so the landing rate stays responsive.
                let vs_corrected = snap.touchdown_vs_source_fpm() * pitch_rad.cos();
                let curr_min = stats.low_agl_vs_min_fpm.unwrap_or(f32::INFINITY);
                if vs_corrected < curr_min {
                    stats.low_agl_vs_min_fpm = Some(vs_corrected);
                }
                // v0.5.35: speichere AUCH die letzte airborne V/S
                // (= chronologisch jüngste, nicht das Minimum). Wird
                // als Fallback genutzt wenn der Lua-30-Sample-Estimator
                // wegen sparse Sampling versagt. AGL-Window 500ft
                // (gross genug fuer GA mit langsamen DataRefs).
                if snap.altitude_agl_ft <= 500.0 && vs_corrected < -20.0 {
                    // Zeitpunkt der MESSUNG, nicht der Auswertung — nach
                    // einem Stau im Kanal liegen beide auseinander (2.0.17).
                    stats.last_low_agl_vs_fpm = Some((vs_corrected, now));
                }
            }

            // v0.4.4: Edge-Detection direkt im Sampler.
            //
            // Primary signal: `gear_normal_force_n` (X-Plane-only) —
            // spikt im Frame des physischen Touchdowns (xgs-Methode).
            // Fallback: `on_ground`-Flag (für MSFS und falls X-Plane-
            // DataRef mal None liefert).
            //
            // Wir halten `prev_in_air` als rolling-state. Der Edge
            // ist `true → false` (vom in-air zu am-Boden).
            //
            // Capture nur ONCE: sampler_touchdown_at bleibt Some bis
            // Flight-Reset → kein Re-Trigger bei Bouncing nach dem
            // initialen Touchdown.
            // Category-aware "in air" signal. Resolved once per tick from the
            // live ICAO + gear-type flags (FixedWing when unknown).
            let category = aircraft_category::resolve_category(
                snap.aircraft_icao.as_deref(),
                snap.gear_is_skid,
                snap.gear_is_floats,
                snap.gear_is_wheels,
                snap.water_rudder_present,
            );
            // For a seaplane/amphibian on water the wheeled gear / on_ground
            // edge never fires; `sampler_in_air_now` falls back to height above
            // the water surface so the touchdown edge fires when the floats
            // settle, then flows through the SAME pending → validate → capture
            // path (the seaplane validation anchors on sustained low-AGL, so a
            // wave bounce / low pass is dropped). Fixed-wing / helicopter /
            // on-runway are byte-identical to the wheeled branch.
            let in_air_now = sampler_in_air_now(
                category,
                snap.on_ground,
                snap.altitude_agl_ft,
                snap.gear_normal_force_n,
            );
            let edge_detected = matches!(prev_in_air, Some(true)) && !in_air_now;

            // v0.5.0: Premium-Plugin-Override.
            //
            // Wenn der AeroACARS X-Plane Plugin (optional, v0.5.0+) ein
            // Touchdown-Event gesendet hat, übernimmt es den Capture
            // unabhängig von der RREF-Edge — der Plugin sieht das Edge
            // mit Frame-Genauigkeit und kennt das tatsächliche peak
            // descent VS aus seinem eigenen 500ms-Lookback-Buffer
            // (gleicher Algorithmus wie unten, aber INNERHALB von
            // X-Plane gemessen, also keine UDP-Verzögerung / Eviction-
            // Race). Wir brauchen lokal den `prev_in_air`-Reset
            // trotzdem, damit das `bouncing`-Re-Trigger-Guard greift
            // wenn das Aircraft kurz wieder abhebt.
            //
            // `stats.sampler_touchdown_at.is_none()`-Guard: identisch
            // zur RREF-Edge — nur ein Capture pro Landing.
            // v0.7.0 — P1 fix: Premium-Touchdown-Pfad (X-Plane-Plugin) geht
            // jetzt auch ueber pending_td_at + validate_candidate. Vorher
            // wurde sampler_touchdown_at sofort gesetzt + open_capture_window
            // aufgerufen, was die touchdown_v2 Validation umging. Wenn das
            // Plugin einen Float-Streifschuss meldete, wurde der Bug
            // (DAH 3181-Pattern) reproduziert.
            //
            // Jetzt: Premium-event setzt pending_td_at (gleicher Mechanismus
            // wie der RREF-Edge-Pfad). Im subsequent pending-validation-block
            // wird validate_candidate aufgerufen. Bei VALIDATED wird die
            // Premium-VS als override im stats.sampler_touchdown_vs_fpm
            // verwendet (= "frame-perfect" Wert vom Plugin).
            //
            // Effekt: alle Pfade (RREF-edge + Premium-plugin) durchlaufen
            // identische Validation. False-edges werden konsistent gefiltert.
            if let Some(td) = premium {
                // ACHTUNG: current_premium_touchdown() ist drain via take()
                // — seit 2.0.17 im Messfaden abgeholt (`AufsetzMessung`).
                // Das Event ist hier bereits konsumiert — wir muessen es jetzt
                // *immer* speichern (oder bewusst verwerfen), sonst geht es
                // still verloren.
                // v0.15.20: Premium-Touchdown in JEDER Flug-Phase akzeptieren
                // (nicht nur Approach/Final/Landing) — sonst geht der frame-
                // genaue Plugin-Wert bei fehl-phasierter FSM (Go-Around →
                // „Climb", OWx9) verloren. Phantom-Schutz via is_flight_phase
                // (Boden-/Taxi-Phasen bleiben ausgeschlossen).
                // v0.15.23: also capturable in Takeoff once airborne (short/low
                // bush hops that land before the 500 ft Takeoff→Climb gate).
                let capturable_phase = aufsetz_kandidat_zulaessig(&stats, snap.paused);
                match premium_td_uebernehmen(
                    &mut stats,
                    td.captured_vs_fpm,
                    td.captured_g_normal,
                    capturable_phase,
                    now,
                ) {
                    PremiumTd::Verworfen => {
                        // Out-of-phase (Boden/Taxi) oder TD schon abgeschlossen.
                        tracing::debug!(
                            pirep_id = %flight.pirep_id,
                            captured_vs_fpm = td.captured_vs_fpm,
                            capturable_phase = capturable_phase,
                            td_already_captured = stats.sampler_touchdown_at.is_some(),
                            "premium TD event dropped (out of phase or TD already finalised)"
                        );
                    }
                    PremiumTd::NeuerKandidat => {
                        tracing::info!(
                            pirep_id = %flight.pirep_id,
                            captured_vs_fpm = td.captured_vs_fpm,
                            captured_g = td.captured_g_normal,
                            source = "x-plane-plugin-premium",
                            "v0.7.0 premium TD candidate detected — pending validation in 1.1s"
                        );
                    }
                    PremiumTd::Angehaengt { pending_age_ms } => {
                        tracing::info!(
                            pirep_id = %flight.pirep_id,
                            captured_vs_fpm = td.captured_vs_fpm,
                            captured_g = td.captured_g_normal,
                            pending_age_ms = pending_age_ms,
                            source = "x-plane-plugin-premium-late",
                            "v0.7.0 premium TD attached to existing RREF pending candidate"
                        );
                    }
                    PremiumTd::Wiederholt => {
                        tracing::debug!(
                            pirep_id = %flight.pirep_id,
                            captured_vs_fpm = td.captured_vs_fpm,
                            existing_premium_vs = stats.pending_td_premium_vs,
                            "premium TD re-trigger ignored — first premium edge already pending"
                        );
                    }
                }
            }

            // v0.5.24: Takeoff-Edge (Wheels-Up) — opposite of touchdown.
            // Wenn `prev_in_air=false` und jetzt `in_air_now=true`, ist
            // der Flieger gerade abgehoben. Wir capturen Pitch + Bank
            // exakt im Frame (50Hz, <20ms-Genauigkeit). Guard
            // `sampler_takeoff_at.is_none()` verhindert Re-Trigger bei
            // Touch-and-Go im Pattern (= zweiter Wheels-Up nach erstem
            // Touchdown ueberschreibt nicht den initialen Takeoff-Wert).
            let takeoff_edge_detected = matches!(prev_in_air, Some(false)) && in_air_now;
            if takeoff_edge_detected && stats.sampler_takeoff_at.is_none() {
                stats.sampler_takeoff_at = Some(now);
                stats.sampler_takeoff_pitch_deg = Some(snap.pitch_deg);
                stats.sampler_takeoff_bank_deg = Some(snap.bank_deg);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    captured_pitch_deg = snap.pitch_deg,
                    captured_bank_deg = snap.bank_deg,
                    on_ground = snap.on_ground,
                    fnrml_n = snap.gear_normal_force_n,
                    "sampler-side takeoff edge detected (wheels-up frame)"
                );
            }

            // v0.5.45: Phase-Guard gegen Phantom-Touchdowns auf Bush-Strips.
            // Bei unebenem Untergrund kann gear_normal_force_n < 1.0 N kurz
            // unterschritten werden waehrend Taxi (DA40 auf X-Plane Bush-
            // Strip 2026-05-09) — der Sampler interpretiert das sonst als
            // Touchdown. TD-Edge nur akzeptieren wenn die FSM in einer
            // Landing-relevanten Phase ist. Approach-Ausloeser AGL>1500ft
            // ist OK (lange Endanflug-Bahnen), aber Taxi/Boarding/Cruise
            // sind explizit ausgeschlossen.
            // v0.15.20: accept the touchdown edge in any FLIGHT phase (not just
            // Approach/Final/Landing) so a real touchdown isn't lost when the FSM
            // is mis-phased (go-around → "Climb", OWx9 GSG544).
            // v0.15.23: ALSO accept it in Takeoff once airborne — short/low VFR
            // bush hops land before the 500 ft Takeoff→Climb gate, so they're
            // still in Takeoff at touchdown (GSG309 OR72→OR35, peaked 357 ft).
            // Phantom-safe: ground/taxi phases stay excluded (taxi gear-bumps
            // v0.5.45); the FIRST air→ground edge in Takeoff is the real landing
            // and the once-guard blocks rollout bumps after it; a mid-air
            // on_ground glitch is rejected by validate_candidate (low-AGL +
            // sustained-ground fail at altitude / on a single tick).
            // 2.0.28: und nicht, solange die Aufzeichnung pausiert ist
            // (`aufsetz_kandidat_zulaessig`).
            let capturable_phase = aufsetz_kandidat_zulaessig(&stats, snap.paused);
            // v0.7.0 — P0 Fix: Edge-Detection setzt jetzt nur `pending_td_at`.
            // Die echte Validation (touchdown_v2::validate_candidate) folgt 1.1 sec
            // spaeter wenn genug post-edge samples im Buffer sind. False-edges
            // (= Float-Streifschuesse wie bei DAH 3181 sample 125) werden dann
            // verworfen, der Sampler beobachtet weiter den naechsten Edge.
            //
            // Vorher (Bug): erster Edge wurde direkt als TD akzeptiert. DAH 3181
            // Float-Streifschuss mit vs=+104 fpm wurde gescort, der echte TD
            // 4 sec spaeter wurde ignoriert (single-shot is_none() Guard).
            if edge_detected
                && stats.sampler_touchdown_at.is_none()
                && stats.pending_td_at.is_none()
                && capturable_phase
            {
                stats.pending_td_at = Some(now);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    edge_at = %now,
                    edge_agl_ft = snap.altitude_agl_ft,
                    // Log the RAW touchdown source (what actually gets scored),
                    // not the display VVI, so forensic logs match the score.
                    edge_vs_fpm = snap.touchdown_vs_source_fpm(),
                    edge_gear_force_n = ?snap.gear_normal_force_n,
                    "v0.7.0 TD-edge candidate detected — pending validation in 1.1s"
                );
            }

            // v0.7.0 — Pending Validation: 1.1 sec nach edge die post-edge samples
            // sind im snapshot_buffer (TOUCHDOWN_BUFFER_SECS=5sec cutoff = "now-5s",
            // edge bei "now-1.1s" → samples [now-1.1s, now] sind drin).
            if let Some(pending_at) = stats.pending_td_at {
                let elapsed_ms = (now - pending_at).num_milliseconds();
                if elapsed_ms >= 1100 {
                    // Convert snapshot_buffer (TelemetrySample) zu Vec<TouchdownWindowSample>
                    let v2_samples: Vec<TouchdownWindowSample> = stats
                        .snapshot_buffer
                        .iter()
                        .copied()
                        .map(TouchdownWindowSample::from)
                        .collect();
                    // Find candidate sample (first sample at >= pending_at)
                    let cand_idx = v2_samples.iter().position(|s| s.at >= pending_at);
                    if let Some(idx) = cand_idx {
                        let cand_sample = &v2_samples[idx];
                        let candidate = touchdown_v2::TdCandidate {
                            edge_sample_index: idx,
                            edge_at: cand_sample.at,
                            edge_agl_ft: cand_sample.agl_ft,
                            edge_vs_fpm: cand_sample.vs_fpm,
                            edge_gear_force_n: cand_sample.gear_normal_force_n,
                            edge_g_force: cand_sample.g_force,
                            edge_total_weight_kg: cand_sample.total_weight_kg,
                        };
                        // Compute impact_frame fuer A3/B4 (vs_negative_at_impact)
                        let impact_for_validation =
                            touchdown_v2::compute_impact_frame(&v2_samples, candidate.edge_at);
                        let impact_vs = impact_for_validation
                            .as_ref()
                            .map(|r| r.impact_vs_fpm)
                            .unwrap_or(0.0);
                        // sim from snap (Simulator → SimKind)
                        let sim = match snap.simulator {
                            Simulator::Msfs2020 => SimKind::Msfs2020,
                            Simulator::Msfs2024 => SimKind::Msfs2024,
                            Simulator::XPlane11 => SimKind::XPlane11,
                            Simulator::XPlane12 => SimKind::XPlane12,
                            _ => SimKind::Off,
                        };
                        // Category-aware validation (loop-level `category`): a
                        // rotorcraft / seaplane soft set-down has no force/G/vs
                        // spike, so the fixed-wing anchors would drop it.
                        // Hat MSFS den Aufsetzwert für DIESE Landung neu gesetzt?
                        // Vergleich mit dem Stand in der Luft (v1.6.10) — siehe
                        // `touchdown_v2::msfs_bestaetigt_aufsetzen`.
                        let latch_neu = negative_only(snap.touchdown_vs_fpm).is_some_and(|neu| {
                            stats
                                .sim_referenz_vor_aufsetzen
                                .is_some_and(|vorher| (neu - vorher).abs() > 0.5)
                        });
                        let validation = touchdown_v2::msfs_bestaetigt_aufsetzen(
                            touchdown_v2::validate_candidate(
                                &candidate,
                                &v2_samples,
                                sim,
                                impact_vs,
                                category,
                                stats.bodenhoehe.bodenhoehe_ft(),
                            ),
                            sim,
                            latch_neu,
                        );
                        match validation {
                            touchdown_v2::ValidationResult::Validated { result } => {
                                tracing::info!(
                                    pirep_id = %flight.pirep_id,
                                    edge_at = %pending_at,
                                    impact_vs = impact_vs,
                                    "v0.7.0 TD candidate VALIDATED — promoting to sampler_touchdown"
                                );
                                stats.sampler_touchdown_at = Some(pending_at);
                                // Bug B (Codex, Vereinheitlichung 09/2026): eine
                                // ggf. von einem frueheren Edge-Versuch stehen-
                                // gebliebene Ablehnung ist jetzt ueberholt.
                                stats.sampler_touchdown_rejected_reason = None;
                                // Round-3 P2 fix: Premium-VS aus pending-state
                                // (wurde beim Premium-Edge gespeichert weil
                                // take_premium_touchdown() drain ist).
                                let (vs, g) = td_werte_mit_premium(
                                    &stats,
                                    impact_vs,
                                    result.g_force_peak_in_window,
                                );
                                stats.sampler_touchdown_vs_vom_plugin =
                                    stats.pending_td_premium_vs.is_some_and(f32::is_finite);
                                stats.sampler_touchdown_vs_fpm = Some(vs);
                                stats.sampler_touchdown_g_force = Some(g);
                                open_touchdown_capture_window(&mut stats, pending_at);
                                stats.pending_td_at = None;
                                stats.pending_td_premium_vs = None;
                                stats.pending_td_premium_g = None;
                                stats.pending_td_premium_gesehen = false;

                                // v0.15.24: Phase-independent touchdown METADATA.
                                //
                                // Short/low VFR bush hops (GSG312 97OG) land via
                                // this sampler path while the FSM is still stuck
                                // in Takeoff/Climb — the touchdown is captured,
                                // scored and published (v0.15.23), but the
                                // metadata (landing IAS/pitch/bank/heading/
                                // coords/fuel/weight, touchdown profile,
                                // sideslip, wind, RUNWAY correlation) was only
                                // ever stamped in the FSM Landing arm, so the
                                // PIREP report silently lost all those fields.
                                // Stamp them here, at the validated touchdown,
                                // with the SAME helpers the FSM arm calls.
                                //
                                // First-touchdown-wins guard: only stamp when
                                // the FSM arm hasn't already (landing_lat is
                                // its unconditional marker). On normal flights
                                // the FSM Landing arm still runs afterwards and
                                // overwrites these with its own values — its
                                // behaviour is unchanged.
                                //
                                // `pending_at` is the validated contact time —
                                // the same instant the FSM arm would use as
                                // `actual_td_at` (it prefers
                                // `sampler_touchdown_at` once set).
                                //
                                // v0.16.6: the two formerly-known gaps
                                // (approach-stability evaluation + rollout)
                                // are closed below — the central
                                // phase-independent sampling in `step_flight`
                                // keeps the approach_buffer fed on descending
                                // low-level segments, the evaluation here
                                // windows it to the touchdown, and the
                                // streamer drives `rollout_tick` for
                                // sampler-stamped touchdowns.
                                if stats.landing_lat.is_none() {
                                    let td_buf_sample = touchdown_buffer_sample(&stats);
                                    stamp_touchdown_metadata(
                                        &mut stats,
                                        &snap,
                                        pending_at,
                                        td_buf_sample.as_ref(),
                                    );
                                    correlate_touchdown_runway(
                                        &mut stats,
                                        &snap,
                                        &flight,
                                        td_buf_sample.as_ref(),
                                    );
                                    stamp_landing_weight(&mut stats, &snap);

                                    // v0.16.6: approach-stability evaluation on
                                    // the sampler path — mirrors the FSM
                                    // Landing-arm block (same inputs:
                                    // `correlate_touchdown_runway` above has
                                    // just resolved `runway_nav_geometry`
                                    // exactly like the FSM arm does before its
                                    // call), except both stats families are
                                    // windowed to the last
                                    // SAMPLER_STABILITY_WINDOW_SECS before the
                                    // touchdown so a long low-level bush cruise
                                    // doesn't poison the gate. On normal
                                    // flights the FSM Landing arm runs
                                    // afterwards and overwrites all of these
                                    // with its full-buffer values, exactly as
                                    // today (the `landing_lat.is_none()` guard
                                    // above keeps this stamp first-touchdown-
                                    // only, the FSM arm has no guard).
                                    let td_ts = Some(pending_at);
                                    let stab_window =
                                        chrono::Duration::seconds(crate::grenzwerte::grenzwerte().anflug_kern.sampler_stability_window_secs.wert);
                                    let windowed_buf = approach_buffer_window(
                                        &stats.approach_buffer,
                                        pending_at,
                                        stab_window,
                                    );
                                    let (vs_sd, bank_sd) =
                                        compute_approach_stddev(&windowed_buf, td_ts);
                                    // Rotorcraft / seaplane: stability stddevs
                                    // nulled — same category rule as the FSM arm.
                                    let (vs_sd, bank_sd) = if category.is_non_conventional() {
                                        (None, None)
                                    } else {
                                        (vs_sd, bank_sd)
                                    };
                                    stats.approach_vs_stddev_fpm = vs_sd;
                                    stats.approach_bank_stddev_deg = bank_sd;
                                    // Musterquelle wie im FSM-Pfad: Sim zuerst.
                                    let sim_muster =
                                        snap.aircraft_icao.as_deref().and_then(clean_atc_model);
                                    // Dieselbe Auflösung wie überall —
                                    // vorher fehlte hier die dritte Stufe.
                                    let muster = stats
                                        .aufgeloestes_muster
                                        .as_deref()
                                        .or(sim_muster.as_deref())
                                        .unwrap_or(flight.aircraft_icao.trim());
                                    let konfig =
                                        konfig_kontext(&stats, &aircraft_limits_for(muster));
                                    let bezug_ft = anflug_bezugshoehe_ft(
                                        &stats,
                                        &flight.arr_airport,
                                        ist_platzhoehe_navdaten_ft(&flight, &stats),
                                    );
                                    // Score-Version 23: im selben Fenster wie das Gate.
                                    let spaetes = spaetes_final_erkennen(
                                        &stats,
                                        bezug_ft,
                                        pending_at,
                                        Some(stab_window),
                                    );
                                    stats.approach_spaetes_final = spaetes;
                                    let stab_v2 = compute_approach_stability_v2_mit_final(
                                        &stats.approach_buffer,
                                        bezug_ft,
                                        td_ts,
                                        stats
                                            .runway_nav_geometry
                                            .as_ref()
                                            .map(|g| g.glideslope_angle),
                                        Some((pending_at, stab_window)),
                                        konfig,
                                        spaetes.as_ref(),
                                    );
                                    stats.approach_vs_deviation_fpm = stab_v2.vs_deviation_fpm;
                                    stats.approach_max_vs_deviation_below_500_fpm =
                                        stab_v2.max_vs_deviation_below_500_fpm;
                                    stats.approach_bank_stddev_filtered_deg =
                                        stab_v2.bank_stddev_filtered_deg;
                                    stats.approach_vs_stddev_filtered_fpm =
                                        stab_v2.vs_stddev_filtered_fpm;
                                    stats.approach_runway_changed_late =
                                        stab_v2.runway_changed_late;
                                    stats.approach_stable_at_gate = stab_v2.stable_at_gate;
                                    stats.approach_vs_jerk_fpm = stab_v2.vs_jerk_fpm;
                                    stats.approach_ias_stddev_kt = stab_v2.ias_stddev_kt;
                                    stats.approach_excessive_sink = stab_v2.excessive_sink;
                                    stats.approach_stable_config = stab_v2.stable_config;
                                    stats.approach_ias_stddev_unter_500_kt =
                                        stab_v2.ias_stddev_unter_500_kt;
                                    stats.approach_stable_config_500 = stab_v2.stable_config_500;
                                    stats.approach_used_hat = stab_v2.used_hat;
                                    stats.approach_window_sample_count =
                                        Some(stab_v2.window_sample_count);
                                    stats.approach_stable_at_da = stab_v2.stable_at_da;
                                    stats.approach_stall_warning_count =
                                        stab_v2.stall_warning_count;
                                    stats.approach_wind = stab_v2.wind.clone();
                                    // Lernpaket AP4/AP5: wie im FSM-Pfad,
                                    // aber auf das Sampler-Fenster begrenzt.
                                    let platzhoehe = anflug_bezugshoehe_ft(
                                        &stats,
                                        &flight.arr_airport,
                                        ist_platzhoehe_navdaten_ft(&flight, &stats),
                                    );
                                    anflug_forensik_stempeln(
                                        &mut stats,
                                        Some(stab_window),
                                        td_ts,
                                        platzhoehe,
                                    );
                                    tracing::info!(
                                        pirep_id = %flight.pirep_id,
                                        vs_dev_fpm = ?stab_v2.vs_deviation_fpm,
                                        bank_sd_filtered = ?stab_v2.bank_stddev_filtered_deg,
                                        stable_at_gate = ?stab_v2.stable_at_gate,
                                        samples = stab_v2.window_sample_count,
                                        window_secs = crate::grenzwerte::grenzwerte().anflug_kern.sampler_stability_window_secs.wert,
                                        "approach stability v2 computed (sampler path)"
                                    );

                                    // v0.16.6: rollout init — mirror of the FSM
                                    // Landing-entry init, but anchored at the
                                    // buffered touchdown coords (just stamped
                                    // into landing_lat/lon above; the live
                                    // `snap` fallback only fires on resumed
                                    // flights with an empty buffer, same as the
                                    // metadata stamp). The streamer's
                                    // `sampler_path_rollout_tick` accumulates
                                    // from here each tick while the FSM never
                                    // reaches Landing; on normal flights the
                                    // FSM Landing-entry re-initialises these
                                    // with its own values, exactly as today.
                                    stats.rollout_distance_m = Some(0.0);
                                    stats.rollout_finalized = false;
                                    stats.rollout_last_lat =
                                        Some(stats.landing_lat.unwrap_or(snap.lat));
                                    stats.rollout_last_lon =
                                        Some(stats.landing_lon.unwrap_or(snap.lon));
                                }

                                // v0.7.0 P2 fix — emit TouchdownDetected mit
                                // den ECHTEN Werten aus compute_landing_rate-
                                // Cascade (vorher: confidence falsch = sim-string,
                                // source hardcoded, vs_fpm nur impact_vs ohne
                                // Cascade-Fallbacks).
                                let landing_rate = impact_for_validation.as_ref().and_then(|ir| {
                                    touchdown_v2::compute_landing_rate(&v2_samples, ir, category)
                                        .ok()
                                });
                                let impact_at_v = impact_for_validation
                                    .as_ref()
                                    .map(|r| r.impact_at)
                                    .unwrap_or(pending_at);
                                let (final_vs, final_src, final_conf) = match &landing_rate {
                                    Some(lr) => (
                                        lr.vs_fpm,
                                        lr.source.clone(),
                                        format!("{:?}", lr.confidence),
                                    ),
                                    None => (
                                        impact_vs,
                                        "impact_frame_fallback".to_string(),
                                        "Unknown".to_string(),
                                    ),
                                };
                                drop(stats);
                                record_event(
                                    &app,
                                    &flight.pirep_id,
                                    &FlightLogEvent::TouchdownDetected {
                                        timestamp: now,
                                        forensics_version: touchdown_v2::FORENSICS_VERSION,
                                        contact_at: pending_at,
                                        impact_at: impact_at_v,
                                        vs_fpm: final_vs,
                                        confidence: final_conf,
                                        source: final_src,
                                        sim: format!("{:?}", result.sim),
                                    },
                                );
                                stats = flight.stats.lock().expect("flight stats");
                            }
                            touchdown_v2::ValidationResult::FalseEdge { reason, result } => {
                                // Befund DLH 880: Die Meldung nannte nur den Grund —
                                // welche Prüfung durchfiel, liess sich erst aus den
                                // Positionsdaten rekonstruieren. Jetzt stehen die
                                // Einzelergebnisse direkt im Protokoll.
                                tracing::warn!(
                                    pirep_id = %flight.pirep_id,
                                    edge_at = %pending_at,
                                    impact_vs = impact_vs,
                                    reason = ?reason,
                                    g_spitze = result.g_force_peak_in_window,
                                    g_bestanden = ?result.g_force_pass,
                                    tiefflug_bestanden = result.low_agl_persistence_pass,
                                    tiefflug_grenze_ft = ?result.low_agl_grenze_ft,
                                    bodenkontakt_bestanden = ?result.sustained_ground_pass,
                                    sinkrate_bestanden = result.vs_negative_pass,
                                    "v0.7.0 TD candidate FALSE_EDGE — ignoring, continue watching"
                                );
                                // Bug B (Codex, Vereinheitlichung 09/2026): eine
                                // aktive Ablehnung persistieren, damit der FSM-
                                // Fallback in `finalize_landing_score_if_due`
                                // sie respektiert statt sie stillschweigend zu
                                // ueberstimmen. `InsufficientTelemetry` ist KEINE
                                // aktive Ablehnung ("Sampler weiss es nicht", FSM
                                // darf entscheiden) — und muss deshalb das Feld
                                // auch aktiv LEEREN, nicht nur ungesetzt lassen:
                                // sonst wuerde eine Ablehnung eines FRUEHEREN
                                // Kandidaten (z.B. InsufficientVoteScore bei
                                // einem Bounce) stehen bleiben und faelschlich
                                // auf DIESEN, neuen Kandidaten durchschlagen,
                                // obwohl der neue Kandidat nur "zu wenig Daten"
                                // ist, nicht aktiv abgelehnt wurde (Codex-QS-Fund).
                                stats.sampler_touchdown_rejected_reason = match reason {
                                    touchdown_v2::FalseEdgeReason::InsufficientTelemetry => None,
                                    other => Some(other),
                                };
                                stats.ablehnung_gemeldet = false;
                                stats.pending_td_at = None;
                                stats.pending_td_premium_vs = None;
                                stats.pending_td_premium_g = None;
                                stats.pending_td_premium_gesehen = false;
                            }
                        }
                    } else {
                        // Kein sample ab pending_at gefunden — Buffer-Drift, ignore
                        stats.pending_td_at = None;
                        stats.pending_td_premium_vs = None;
                        stats.pending_td_premium_g = None;
                        stats.pending_td_premium_gesehen = false;
                    }
                }
            }

            // Legacy-Pfad fuer Capture-Window-Setup (=Pre-Buffer einkopieren) bleibt
            // unveraendert — wird jetzt nur fuer VALIDATED candidates aufgerufen.
            if false {
                // Pitch-Korrektur: world-frame Y-Velocity → body-axial
                // (xgs-Pattern). Bei typischem Touchdown-Pitch ~3-5°
                // ist cos(pitch)≈0.998, also <0.5% Unterschied. Bei
                // steilen Flares (STOL-Aircraft, ~10°) bis 1.5%.
                // Konsistent mit xgs's `vy * cos(theta * deg2rad)`.
                let pitch_rad = (snap.pitch_deg as f32) * std::f32::consts::PI / 180.0;
                let pitch_cos = pitch_rad.cos();
                let current_vs = snap.vertical_speed_fpm * pitch_cos;
                // VS-min der letzten 2 s — fängt den echten Sinkflug
                // ein bevor Rollout-Damping zugeschlagen hat. v0.5.5
                // erweitert das Fenster von 500 ms auf 2 s nachdem ein
                // Pilot-Test gezeigt hat dass 500 ms bei aggressivem
                // Flare nur Post-Touchdown-Rebound-Samples sieht (alle
                // VS-Werte positiv). 2 s deckt sicher die letzte
                // Flare-Phase ab; falls X-Plane uns RREF mit niedriger
                // Rate liefert haben wir trotzdem genug Samples drin.
                let lookback_start = now - chrono::Duration::milliseconds(2000);
                let recent_vs_min: f32 = stats
                    .snapshot_buffer
                    .iter()
                    .filter(|s| s.at >= lookback_start)
                    .map(|s| {
                        // Pitch-Korrektur auch auf Buffer-Samples — die
                        // sind je nach Aircraft beim Flare auch unter
                        // Nase-hoch-Bedingungen erfasst worden.
                        let p_rad = (s.pitch_deg as f32) * std::f32::consts::PI / 180.0;
                        s.vs_fpm * p_rad.cos()
                    })
                    .fold(f32::INFINITY, f32::min);
                let captured_vs = if recent_vs_min.is_finite() {
                    recent_vs_min.min(current_vs)
                } else {
                    current_vs
                };
                // Peak G im gleichen Window
                let recent_g_peak: f32 = stats
                    .snapshot_buffer
                    .iter()
                    .filter(|s| s.at >= lookback_start)
                    .map(|s| s.g_force)
                    .fold(0.0, f32::max);
                let captured_g = recent_g_peak.max(snap.g_force);

                stats.sampler_touchdown_at = Some(now);
                stats.sampler_touchdown_vs_fpm = Some(captured_vs);
                stats.sampler_touchdown_g_force = Some(captured_g);
                open_touchdown_capture_window(&mut stats, now);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    captured_vs_fpm = captured_vs,
                    current_vs_fpm = current_vs,
                    captured_g = captured_g,
                    fnrml_n = snap.gear_normal_force_n,
                    on_ground = snap.on_ground,
                    "sampler-side touchdown edge detected"
                );
            }
            prev_in_air = Some(in_air_now);

            // v0.5.39: Continuous Post-TD-Capture + Dump.
            //
            // Solange Touchdown detektiert ist und wir noch nicht gedumpt
            // haben, sammelt jeder Sampler-Tick den aktuellen Snapshot ins
            // post_touchdown_buffer (= Pre-TD-Buffer wurde von
            // open_touchdown_capture_window initial reinkopiert). Nach
            // TOUCHDOWN_POST_WINDOW_MS wird der gesamte Buffer als ein
            // TouchdownWindow-Event in die JSONL geflusht — das schließt
            // das ~15-s-Loch (5 s pre + 10 s post @ 50 Hz ≈ 750 Samples)
            // das im Streamer-Tick wegen phpVMS-Latenz entstand.
            let dump_payload: Option<(DateTime<Utc>, Vec<TouchdownWindowSample>, bool)> = {
                let td_at = stats.sampler_touchdown_at;
                let already_dumped = stats.touchdown_window_dumped_at.is_some();
                if let (Some(td_at), false) = (td_at, already_dumped) {
                    // Letzten Snapshot ins post-Buffer (snapshot_buffer.back()
                    // wurde in dieser Iteration oben push_back). Dedup gegen
                    // letzten Eintrag falls open_touchdown_capture_window den
                    // gleichen Snapshot grade reinkopiert hat.
                    if let Some(latest) = stats.snapshot_buffer.back().copied() {
                        let same_as_back = stats
                            .post_touchdown_buffer
                            .back()
                            .map(|s| s.at == latest.at)
                            .unwrap_or(false);
                        if !same_as_back {
                            stats.post_touchdown_buffer.push_back(latest);
                        }
                    }
                    let elapsed_ms = (now - td_at).num_milliseconds();
                    // Frueh-Dump bei Bounce-in-Aufprall — siehe Doc-Kommentar
                    // an `touchdown_dump_faellig`.
                    let agl_jetzt_ft = snap.altitude_agl_ft as f32;
                    let climbed_out_early = agl_jetzt_ft > crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert;
                    if touchdown_dump_faellig(elapsed_ms, agl_jetzt_ft) {
                        if climbed_out_early && elapsed_ms < crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert {
                            tracing::info!(
                                pirep_id = %flight.pirep_id,
                                elapsed_ms,
                                agl_ft = snap.altitude_agl_ft,
                                "early touchdown dump — climbed back above T&G threshold before the 10s window closed; re-arming sampler for the next touchdown so a fast bounce-into-impact isn't lost"
                            );
                        }
                        // v0.13.15 (Pilot-Befund ViolonC 2026-05-31): NICHT
                        // mehr `drain(..)` — der Buffer muss erhalten bleiben,
                        // damit der spaeter beim PIREP-Filing gebaute
                        // LandingRecord (build_landing_record) die vollen 10 s
                        // Post-TD ins touchdown_profile uebernehmen kann. Vorher
                        // war der Buffer hier geleert → der Record sah nur die
                        // ~0.5 s aus dem 5-s-snapshot_buffer, obwohl der Chart
                        // "inkl. 10 s nach TD" verspricht. Der `already_dumped`-
                        // Guard oben verhindert weiteres Anhaengen; geleert wird
                        // der Buffer erst beim Flug-Reset (open_touchdown_capture
                        // _window / End-of-Flight).
                        let samples: Vec<TouchdownWindowSample> = stats
                            .post_touchdown_buffer
                            .iter()
                            .copied()
                            .map(TouchdownWindowSample::from)
                            .collect();
                        stats.touchdown_window_dumped_at = Some(now);
                        // v0.6.0: landing_critical_until wird nicht mehr vom
                        // Streamer konsumiert (Streamer ist vom phpVMS-IO
                        // entkoppelt). Buffer-Dump läuft hier unabhängig
                        // vom Critical-Window-Marker.
                        Some((td_at, samples, climbed_out_early))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let cutoff = now - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().landung.touchdown_buffer_secs.wert);
            while stats.snapshot_buffer.front().is_some_and(|s| s.at < cutoff) {
                stats.snapshot_buffer.pop_front();
            }

            // Forensik-Analyse VOR Lock-Drop berechnen + im stats persistieren
            // damit der Touchdown-Payload-Builder im Streamer-Tick die Felder
            // direkt zur Verfuegung hat (anstatt aus der JSONL re-zu-parsen).
            let prepared_dump = if let Some((edge_at, samples, climbed_out_early)) = dump_payload {
                // ── Reicht die Aufzeichnung für eine Bewertung? ──────────
                //
                // Hier, und nur hier: Das Fenster ist fertig gesammelt, der
                // Bodenkontakt steht fest. Alles Spätere (Kaskade, Kanonik,
                // Score, PIREP, Anzeige) fragt nur noch das Ergebnis ab.
                //
                // Der Kontakt gilt unabhängig davon als erkannt — "gelandet,
                // aber nicht messbar" ist ein gültiger Ausgang.
                stats.landung_abdeckung_fehlt =
                    match touchdown_v2::pruefe_bewertbarkeit(&samples, edge_at) {
                        Ok(()) => None,
                        Err(fehlt) => {
                            tracing::warn!(
                                pirep_id = %flight.pirep_id,
                                groesste_luecke_ms = fehlt.groesste_luecke_ms,
                                proben = fehlt.proben,
                                takt_mittel_ms = diagnose_lesen(&diagnose).takt_mittel_ms(),
                                proben_je_sekunde = diagnose_lesen(&diagnose).proben_je_sekunde(),
                                vorher_gesetzte_note = ?stats.landing_score,
                                "Aufzeichnung im Aufsetzfenster unzureichend — keine Landebewertung"
                            );
                            // Eine bereits vergebene Note muss weg.
                            //
                            // Codex-Befund 12.09.2026 zum ersten Entwurf:
                            // Kommt der Dump spät (Timeout-Pfad hat schon
                            // finalisiert), fiel zwar die Rate weg, `stats.
                            // landing_score` blieb aber stehen — und mehrere
                            // Ausgänge übernehmen genau diesen Altwert
                            // (MQTT-PIREP und phpVMS über
                            // `aggregate_master.or_else(landing_score)`,
                            // `canonical_landing_verdict`, `LandingFinalized`).
                            // Der Finalisierer selbst kommt nicht noch einmal
                            // vorbei, er beendet sich bei vorhandenem Score.
                            stats.landing_score = None;
                            stats.landung_bewertung = None;
                            stats.landung_sofort = None;
                            stats.landung_endgueltig = None;
                            Some(fehlt)
                        }
                    };
                if plugin_sinkrate_uebernehmen(&mut stats) {
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        vs_fpm = ?stats.landing_rate_fpm,
                        "Aufsetzfenster zu dünn — Sinkrate des X-Plane-Plugins übernommen"
                    );
                }
                stats.sampler_diagnose = Some(diagnose_lesen(&diagnose).als_json());

                let analysis = compute_landing_analysis(
                    &samples,
                    edge_at,
                    snap.simulator,
                    stats.landing_sim_referenz_fpm,
                );
                // v1.6.3: die MSFS-Entlagung ist ERSATZLOS entfallen.
                //
                // Sie war ein Pflaster auf dem gedämpften SimVar-Kanal —
                // und lief nachweislich nie: sie bekam den Datenpuffer erst,
                // nachdem er auf 5 s beschnitten war, und sah damit nur
                // Messwerte NACH dem Aufsetzen. Gegenprobe an der
                // Aufzeichnungs-Datenbank: in 692 Aufsetzvorgängen und
                // 885 Flugberichten null Treffer ihrer Marker.
                //
                // Sie ist jetzt auch fachlich überflüssig: `vs_at_edge_fpm`
                // kommt aus der Höhenkurve und ist gar nicht mehr gedämpft
                // (siehe compute_landing_analysis). Zudem konnte sie
                // konstruktiv nur nach unten korrigieren — den häufigeren
                // Fall „zu weich gemeldet" hätte sie nie erfasst.
                // Lernpaket AP4/AP5: Gleitpfad und Anflugruhe mit ins
                // Analyse-JSON (und damit ins Flug-Log), sofern die Landung
                // schon gestempelt ist. Nur lokal — die MQTT-Nutzlast liest
                // aus diesem JSON einzelne Schluessel, nie das Ganze.
                let mut analysis = analysis;
                stats.anflug_forensik.in_analyse_json(&mut analysis);
                // 05.10.2026: Abfangen über die Höhe — Messung für die
                // Teilnote `abfangen` (landing-scoring/src/abfangen.rs).
                let abfangen = abfangen_in_analyse(&stats, &samples, edge_at, &mut analysis);
                stats.abfangen_gesichert = Some(abfangen);
                stats.landing_analysis = Some(analysis.clone());
                // Codex-Folgefund (adversarial, 05.09.2026, fuenfte Runde):
                // `peak_g_post_500ms` steht in `analysis` HIER schon fertig
                // berechnet — aber der Merge nach `stats.landing_peak_g_force`
                // lief bisher nur SPAETER (nach `drop(stats)` + Re-Lock,
                // ~180 Zeilen weiter unten). Fuer den Klassifikator gleich
                // darunter war das zu spaet: nach dem Klettern-Reset zwischen
                // zwei Touchdowns (siehe `touchdown_dump_faellig`-Fix) stand
                // `landing_peak_g_force` hier noch auf `None` — der
                // Klassifikator liefe dann mit `peak_g_load=None` fuer GENAU
                // den Touchdown, dessen G-Wert er eigentlich pruefen soll.
                // Deshalb hier zusaetzlich, VOR dem Klassifikator-Aufruf,
                // derselbe Merge — der spaetere bleibt unveraendert bestehen
                // (fuer die uebrigen Konsumenten von `landing_peak_g_force`,
                // die nicht von diesem einen Aufruf abhaengen).
                let pg_500_frueh = analysis
                    .get("peak_g_post_500ms")
                    .and_then(|v| v.as_f64())
                    .map(|x| x as f32);
                peak_g_force_verschmelzen(&mut stats, pg_500_frueh);
                // v0.7.19 GAF-707: Heuristik-Klassifikator EINMAL pro
                // Touchdown am TD-Edge ausfuehren — direkt nachdem die
                // landing_analysis (mit vs_at_edge_fpm etc) materialisiert
                // wurde. Spec §Klassifikator-Trigger-Punkte. NICHT im
                // 50-Hz-Loop. Worst-Case-Aggregation fuer Multi-TD
                // (Touch-and-Go) ist im Helper.
                apply_accident_heuristic(&mut stats, &analysis);
                // 2.0.14 — Bildrate ueber genau das Fenster, das gleich
                // ins Flugprotokoll geht.
                let bildrate = match (samples.first(), samples.last()) {
                    (Some(a), Some(b)) => bildrate_zusammenfassung(
                        stats
                            .bildrate_puffer
                            .iter()
                            .filter(|(at, _)| *at >= a.at && *at <= b.at)
                            .map(|(_, hz)| *hz),
                    ),
                    _ => None,
                };
                Some((edge_at, samples, analysis, climbed_out_early, bildrate))
            } else {
                None
            };

            // Dump-IO außerhalb des Lock-Holds — record_event macht
            // synchroness File-Append (~1-10 ms auf NTFS), das wollen wir
            // nicht das stats-Lock blockieren lassen.
            drop(stats);
            // v0.16.24: if the sampler-path runway correlation just fell
            // back to OurAirports (divert the pre-fetch missed), kick off the
            // on-demand navdata fetch for the actual landing airport — same
            // finalize-with-best wiring as the FSM path. Idempotent.
            maybe_fetch_actual_landing_navdata(&app, &flight);
            if let Some((edge_at, samples, analysis, climbed_out_early, bildrate)) = prepared_dump {
                bildrate_protokollieren(&flight.pirep_id, edge_at, bildrate);
                // v0.5.49 — IMMEDIATE persist nach dem Setzen von
                // touchdown_window_dumped_at. Vorher wurde der Flag in
                // stats gesetzt, aber save_active_flight wartete auf den
                // nächsten STATS_PERSIST_EVERY_TICKS-Tick. Wenn die App
                // dazwischen quitted (z.B. Pilot schließt sofort nach
                // Landing), war die persistente Kopie noch dumped_at=None
                // → nach Resume fired der Dump erneut. Genau das Symptom
                // aus dem PTO-705-Log (zwei TouchdownWindow-Events,
                // 18:48:25 und 18:51:16 nach Resume). Ein expliziter
                // Save direkt nach dem Setzen verhindert das.
                //
                // Sicherung im eigenen Faden, Flugprotokoll über den
                // geordneten Schreiber (`record_event`): Beides schrieb
                // mehrere hundert KB synchron, und der Sampler misst gleich
                // weiter — ein Hopser kurz danach fiele sonst in eine Lücke
                // (AZU 8710, 11.10.2026).
                let count = samples.len();
                // v0.7.0 — clone fuer touchdown_v2 weiter unten
                // (samples wird gleich in record_event gemoved)
                let samples_for_v2 = samples.clone();
                {
                    let (app, flight) = (app.clone(), flight.clone());
                    std::thread::spawn(move || save_active_flight(&app, &flight));
                }
                record_event(
                    &app,
                    &flight.pirep_id,
                    &FlightLogEvent::TouchdownWindow {
                        timestamp: now,
                        edge_at,
                        samples,
                    },
                );
                record_event(
                    &app,
                    &flight.pirep_id,
                    &FlightLogEvent::LandingAnalysis {
                        timestamp: now,
                        edge_at,
                        analysis: analysis.clone(),
                    },
                );

                // v0.7.0 — Touchdown-Forensik v2 (Spec docs/spec/touchdown-forensics-v2.md).
                // Ersetzt den fruheren vs_at_edge-unconditional-override (= war
                // Bug #1 vom DAH 3181 Float-Streifschuss + 8 weitere Bugs).
                //
                // Neue Cascade:
                //   1. touchdown_v2::compute_impact_frame liefert contact_frame +
                //      impact_frame (= min vs in [contact-250ms, contact+100ms]) +
                //      initial_load_peak.
                //   2. touchdown_v2::compute_landing_rate macht VS-Cascade mit
                //      HARD GUARDS:
                //         vs_at_impact -> smoothed_500ms -> smoothed_1000ms ->
                //         pre_flare_peak -> REJECT (kein Score).
                //   3. Niemals positive Landerate (HARD GUARD strukturell).
                //   4. forensics_version = 2 marker geht in PIREP-payload.
                //
                // Legacy: bei REJECT (z.B. positive vs_at_edge UND alle smoothed
                // positiv = sollte nie passieren) bleibt das Feld unveraendert
                // (= alter SimVar-Wert wenn vorhanden).
                // Category-aware landing rate (loop-level `category`):
                // rotorcraft / seaplane touch down near-zero V/S, so the -10 fpm
                // fixed-wing acceptance floor in the cascade is relaxed.
                let v2_result = touchdown_v2::compute_impact_frame(&samples_for_v2, edge_at)
                    .and_then(|impact_result| {
                        touchdown_v2::compute_landing_rate(
                            &samples_for_v2,
                            &impact_result,
                            category,
                        )
                        .ok()
                    });

                // v0.7.11: Edge-Wert aus dem 50-Hz-Buffer als kanonische
                // Score-Basis. Vorher haben wir bei v2-REJECT den SimVar-
                // latched-Wert behalten — das war unsinnig weil der 50-Hz-
                // Edge-Wert physikalisch genauer ist (interpoliert auf den
                // exakten on_ground-Edge). User-Forderung Mai 12: "warum
                // nutzen wir den MSFS Wert wenn wir das selber ermitteln".
                //
                // Cascade-Ordnung (seit v0.20.0, siehe PIA3452-Kommentar
                // direkt unten):
                //   1. vs_at_edge_fpm aus dem Buffer (= 50-Hz interpoliert,
                //      physikalisch am genauesten — FAR-25.473-Standard)
                //   2. v2-Result (= raffinierte Cascade vs_at_impact ->
                //      smoothed_500ms -> smoothed_1000ms -> pre_flare_peak),
                //      nur wenn kein gueltiger Edge-Wert vorliegt
                //   3. landing_rate_fpm unveraendert (= SimVar-latched
                //      Fallback wenn weder Edge noch v2 verfuegbar — sehr
                //      selten, nur bei Pre-v0.5.43 oder Buffer-Dump-Fail)
                let edge_vs = analysis
                    .get("vs_at_edge_fpm")
                    .and_then(|v| v.as_f64())
                    .map(|x| x as f32)
                    .filter(|v| v.is_finite() && *v < 0.0);

                let pg_500 = analysis
                    .get("peak_g_post_500ms")
                    .and_then(|v| v.as_f64())
                    .map(|x| x as f32);
                // Codex-QS-Fund (Vereinheitlichung 09/2026, Runde 4 — Restrisiko
                // nach dem Zwei-Lock-Fix): `touchdown_window_dumped_at` wird
                // weiter oben SOFORT persistiert (Resume-Sicherheit gegen
                // Doppel-Dump, siehe dessen Setzstelle), aber der Commit HIER
                // (verfeinerte VS-Kaskade + Bereitschaftssignal) bisher nur
                // periodisch. Stuerzt der Prozess in der Luecke dazwischen ab,
                // findet Resume `dumped_at=Some` aber `score_data_ready_at=
                // None` und die alten, groeberen Werte — der Finalizer wartet
                // dann bis zum SAMPLER_DUMP_WAIT_TIMEOUT_MS-Fallback statt die
                // hier laengst berechneten, praeziseren Daten zu sehen. Dieses
                // Flag haelt fest, ob das Bereitschaftssignal in DIESEM Tick
                // gesetzt wurde, damit direkt danach (ausserhalb des Locks,
                // wie bei allen anderen Dump-IO-Aufrufen hier) sofort
                // gespeichert wird — dasselbe Muster wie beim fruehen
                // `touchdown_window_dumped_at`-Save, nur fuer den zweiten
                // Commit. Verkuerzt das Absturz-Fenster von "bis zum naechsten
                // periodischen Save (Sekunden bis Minuten)" auf die paar
                // Millisekunden zwischen den beiden Locks selbst.
                let mut score_data_wurde_bereit = false;
                {
                    let mut s = flight.stats.lock().expect("flight stats");
                    // v3 (P-7.02): Fenster fuer das Eingabe-Dokument behalten.
                    s.landung_fenster = samples_for_v2.clone();
                    // v0.20.0 (PIA3452): Edge gewinnt vor v2 — dieselbe
                    // Ordnung die `canonical_landing_rate_fpm` schon immer
                    // liest. Vorher schrieb v2 (= Fenster-MINIMUM ueber
                    // [contact-250ms, contact+100ms]) in `landing_rate_fpm` /
                    // `landing_peak_vs_fpm`, waehrend Score, LandingRecord und
                    // PIREP ueber die Kanonik weiter `vs_at_edge_fpm` (=
                    // Interpolation exakt am on_ground-Frame) nahmen. Ein
                    // Fenster-Minimum ist konstruktionsbedingt <= der
                    // Interpolation → der Pilot sah drei Zahlen fuer EINEN
                    // Touchdown: Log -233 (v2), Karte/Score/PIREP -206 (Edge),
                    // und die Einstufung unten klassifizierte auf -233. Der
                    // Edge-Wert ist der physikalisch richtige (FAR-25.473-
                    // Standard, siehe scoreBasisVs()), also finalisieren wir
                    // jetzt AUF IHN — v2 bleibt Fallback, wenn kein gueltiger
                    // Edge existiert. v2s eigener Wert bleibt als Forensik-Key
                    // `vs_at_impact_frame_fpm` in der Analysis erhalten.
                    if let Some(edge) = edge_vs {
                        // Confidence kommt weiter von v2 wenn v2 gelaufen ist
                        // (v2 validiert DASS es ein echter Touchdown war),
                        // sonst Medium wie im alten REJECT-Zweig.
                        let conf_str = v2_result
                            .as_ref()
                            .map(|v2| format!("{:?}", v2.confidence))
                            .unwrap_or_else(|| "Medium".to_string());
                        let previous = s.landing_rate_fpm.unwrap_or(0.0);
                        finalize_landing_rate(
                            &mut s,
                            edge,
                            Some(&conf_str),
                            Some("vs_at_edge_50hz"),
                        );
                        if let Some(ref v2) = v2_result {
                            // Forensik-Spur: v2s Fenster-Minimum nicht verlieren.
                            if let Some(map) =
                                s.landing_analysis.as_mut().and_then(|a| a.as_object_mut())
                            {
                                map.insert(
                                    "vs_at_impact_frame_fpm".to_string(),
                                    serde_json::json!(v2.vs_fpm),
                                );
                            }
                            tracing::info!(
                                pirep_id = %flight.pirep_id,
                                vs_fpm = edge,
                                v2_vs_fpm = v2.vs_fpm,
                                previous = previous,
                                source = "vs_at_edge_50hz",
                                confidence = %conf_str,
                                forensics_version = v2.forensics_version,
                                "landing rate finalized on 50Hz edge (v2 value kept as forensic key)"
                            );
                        } else {
                            tracing::info!(
                                pirep_id = %flight.pirep_id,
                                vs_fpm = edge,
                                previous = previous,
                                source = "vs_at_edge_50hz",
                                "v0.7.11: v2-REJECT, override mit vs_at_edge_fpm"
                            );
                        }
                    } else if let Some(ref v2) = v2_result {
                        // Kein gueltiger Edge (positiv / nicht finite / Buffer-
                        // Luecke) → v2 ist die beste verfuegbare Quelle. Die
                        // Kanonik faellt in diesem Fall ohnehin auf
                        // `landing_peak_vs_fpm` durch, also bleibt alles konsistent.
                        let conf_str = format!("{:?}", v2.confidence);
                        finalize_landing_rate(&mut s, v2.vs_fpm, Some(&conf_str), Some(&v2.source));
                        tracing::info!(
                            pirep_id = %flight.pirep_id,
                            vs_fpm = v2.vs_fpm,
                            source = %v2.source,
                            confidence = ?v2.confidence,
                            forensics_version = v2.forensics_version,
                            "touchdown-forensics-v2 landing rate computed (no valid edge)"
                        );
                    } else {
                        // Echter REJECT: weder v2 noch Edge verfuegbar.
                        // Pre-v0.5.43-Eintrag oder kompletter Buffer-Fail.
                        // Behalte den primary-chain Wert (= SimVar-latched).
                        tracing::warn!(
                            pirep_id = %flight.pirep_id,
                            "touchdown-forensics-v2 + edge beide nicht verfuegbar — keeping primary-chain value"
                        );
                    }
                    // Nur ueberschreiben wenn der Buffer-Peak hoeher ist als der
                    // bisher gemessene (= echter Gear-Compression-Spike) —
                    // dieselbe Regel wie der Fruehe-Merge oben vor dem
                    // Klassifikator-Aufruf (`peak_g_force_verschmelzen`), hier
                    // erneut fuer die uebrigen Konsumenten von
                    // `landing_peak_g_force` nach dem Re-Lock.
                    peak_g_force_verschmelzen(&mut s, pg_500);
                    // Re-Klassifizierung mit den neuen Werten
                    // BUG #6 fix: bounce_count from analysis (50Hz Sampler-Wahrheit),
                    // nicht vom Streamer-side s.bounce_count counter.
                    //
                    // v0.7.6 P1-2: `scored_bounce_count` aus dem Analysis-JSON
                    // ist die SSoT fuer den Sub-Score (filtert auf
                    // BOUNCE_SCORED_MIN_AGL_FT, seit Lernpaket AP3 5 ft). Vorher hat das hier
                    // nur die Legacy-Klassifikation (LandingScore::classify)
                    // mit dem 5-ft-Forensic-Count gefuettert UND `s.bounce_count`
                    // wurde nicht zurueckgeschrieben → der spaetere PIREP-Build
                    // las den Streamer-Count (oft 0 weil der erst >35 ft armed)
                    // und der Score-Crate-Sub-Score `bounces` zeigte 100 obwohl
                    // die Forensik einen Hopser gesehen hatte (SAS9987 v0.7.5).
                    // Fix: scored_bounce_count zurueck in s.bounce_count damit
                    // das Sub-Score-Aggregat konsistent ist mit der Forensik.
                    // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-2.
                    let scored_bounce: u8 = analysis
                        .get("scored_bounce_count")
                        .and_then(|v| v.as_u64())
                        .map(|n| n.min(u8::MAX as u64) as u8)
                        // Fallback fuer alte Analysis-JSONs ohne neues Feld
                        // (Replays, JSONL-Recovery): Legacy bounce_count.
                        .or_else(|| {
                            analysis
                                .get("bounce_count")
                                .and_then(|v| v.as_u64())
                                .map(|n| n.min(u8::MAX as u64) as u8)
                        })
                        .unwrap_or(s.bounce_count);
                    s.bounce_count = scored_bounce;
                    // P1/8b: leichte Hopser ebenso aus der 50-Hz-Analyse.
                    s.light_bounce_count = analysis
                        .get("scored_light_bounce_count")
                        .and_then(|v| v.as_u64())
                        .map(|n| n.min(u8::MAX as u64) as u8)
                        .unwrap_or(s.light_bounce_count);
                    // GAF-707-Race, strukturell geloest (Vereinheitlichung
                    // 09/2026): ERST jetzt, nachdem wirklich ALLE score-
                    // relevanten Rohdaten geschrieben sind (VS-Kaskade oben
                    // via finalize_landing_rate, bounce_count gerade eben),
                    // signalisieren wir "Dump-Daten bereit" — bewusst
                    // GETRENNT von `touchdown_window_dumped_at` (das wurde
                    // schon viel frueher gesetzt, in einem ERSTEN Lock-Hold,
                    // rein zum Schutz vor einem Doppel-Dump im 50Hz-Sampler-
                    // Loop). Ohne diese Trennung konnte `finalize_landing_
                    // score_if_due` (laeuft unabhaengig auf dem FSM-Tick)
                    // genau in der Luecke zwischen den beiden Lock-Holds
                    // zugreifen: `touchdown_window_dumped_at` schon Some,
                    // aber die verfeinerte Kaskade noch nicht geschrieben —
                    // und haette dann mit der groeberen Schaetzung gescort,
                    // OHNE das je zu korrigieren (`landing_score.is_some()`-
                    // Guard laesst keine zweite Chance).
                    //
                    // Codex-QS-Fund (Vereinheitlichung 09/2026, Runde 4): bei
                    // einem FRUEHEN Dump (`climbed_out_early` — das Flugzeug
                    // ist ueber die T&G-Schwelle geklettert, BEVOR die vollen
                    // 10s um waren) folgt WEITER UNTEN im selben Sampler-Tick
                    // noch der Multi-TD-Climb-out-Reset, der `sampler_
                    // touchdown_at` (und damit den Fallback-Anker in
                    // `finalize_landing_score_if_due`) unter einem DRITTEN,
                    // separaten Lock wieder raeumt. Zwischen diesem Lock hier
                    // und jenem dritten Lock waeren beide Felder (`sampler_
                    // touchdown_at` UND dieses Bereitschaftssignal) kurzzeitig
                    // gleichzeitig gesetzt — theoretisch ein drittes, sehr
                    // enges Zeitfenster fuer denselben Wettlauf. Der Live-AGL-
                    // Check in `finalize_landing_score_if_due` faengt das zwar
                    // ohnehin ab (das Flugzeug ist ja gerade physisch ueber der
                    // Schwelle), aber sauberer ist, das Fenster erst gar nicht
                    // zu oeffnen: bei einem fruehen, klettern-ausgeloesten Dump
                    // signalisieren wir "bereit" grundsaetzlich NICHT — die
                    // Episode ist per Definition vorlaeufig, es gibt keinen
                    // Grund, sie je als score-bereit zu markieren. Faellt es
                    // NICHT als Touch-and-Go zusammen, greift stattdessen
                    // der bestehende SAMPLER_DUMP_WAIT_TIMEOUT_MS-Fallback.
                    if !climbed_out_early {
                        s.touchdown_window_score_data_ready_at = Some(now);
                        score_data_wurde_bereit = true;
                    }
                    // GAF-707-Race, strukturell geloest (Vereinheitlichung
                    // 09/2026): dieser Dump setzt `landing_score` NICHT mehr
                    // selbst (frueher stand hier `LandingScore::classify(...)`
                    // + direktes `s.landing_score = Some(...)`). Vorher haben
                    // zwei unabhaengig laufende Ticker (dieser Sampler-Task
                    // ~50Hz und der FSM/Streamer-Tick alle 750ms-1s) um
                    // dieselbe Frage konkurriert — "ist die Landung final?" —
                    // je nachdem, wer zuerst lief. Der Dump liefert jetzt nur
                    // noch die ROHDATEN (VS-Kaskade oben, `bounce_count`,
                    // `peak_g_force_verschmelzen`, `apply_accident_heuristic`)
                    // plus das Bereitschaftssignal direkt darueber — die
                    // Score-VERGABE (inkl. desselben B-005-Guards, der hier
                    // vorher lokal dupliziert war) liegt jetzt ausschliesslich
                    // bei `finalize_landing_score_if_due()`, die bei JEDEM
                    // FSM-Tick laeuft und deshalb LIVE auf eine laufende T&G-
                    // Erkennung und auf genau dieses Bereitschaftssignal warten
                    // kann, statt an einem einzelnen Zeitpunkt zu raten.
                }

                // Codex-QS-Fund (s.o.): sofort persistieren, sobald das
                // Bereitschaftssignal wirklich gesetzt wurde — ausserhalb des
                // Locks, wie jede andere Dump-I/O hier. Bei `climbed_out_early`
                // (Signal bewusst nicht gesetzt) entfaellt dieser Save: es gibt
                // nichts Neues zu sichern, und der Multi-TD-Reset raeumt
                // `sampler_touchdown_at` gleich ohnehin wieder ab.
                if score_data_wurde_bereit {
                    save_active_flight(&app, &flight);
                }

                let flare_score = analysis
                    .get("flare_quality_score")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(-1);
                let smooth_500 = analysis
                    .get("vs_smoothed_500ms_fpm")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::NAN);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    sample_count = count,
                    window_ms = crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert,
                    flare_score = flare_score,
                    smooth_500ms_fpm = smooth_500,
                    vs_v2_fpm = v2_result.as_ref().map(|v| v.vs_fpm),
                    vs_v2_source = v2_result.as_ref().map(|v| v.source.as_str()),
                    peak_g_post_500ms = pg_500,
                    "touchdown window + analysis flushed; score recomputed from buffer"
                );
            }

            // v0.7.0 — Phase H Step 2: Multi-TD via Climb-out-Reset.
            //
            // Spec Sektion 6 (LandingEpisode-Lifecycle): wenn aircraft nach
            // einem TD wieder hochsteigt > 100ft AGL, ist der nächste TD eine
            // neue Episode (Touch-and-Go / Go-Around / Re-Approach). Damit
            // der Sampler die zweite Landung erfasst, muessen wir die single-
            // shot Guards zuruecksetzen.
            //
            // Konsequenz fuer PIREP-Score: der LETZTE validierte TD ist der
            // "final landing" TD. Ein dump pro Episode = ein touchdown_window
            // event + landing_analysis event in der JSONL. Score wird beim
            // PIREP-filing aus dem letzten gewonnen.
            //
            // PTO 705 Beispiel: Touch um 07:54:30 -> climbout auf 1560ft AGL ->
            // Re-Approach -> echter Touch um 08:01:29 -> dieser bekommt jetzt
            // eigenen capture-cycle + Score.
            {
                let mut s = flight.stats.lock().expect("flight stats");
                let dumped = s.touchdown_window_dumped_at.is_some();
                let agl_now = snap.altitude_agl_ft as f32;
                // Nur ein ECHTER Steigflug: in der Luft, und mit einem AGL,
                // das zur Hoehe passt. MSFS meldete am 03.05.2026 waehrend
                // eines GSX-Pushbacks AGL = 53.819 ft (PMDG 737) — ohne diese
                // Pruefung loeschte so ein Wert nach der Landung den
                // Landesprit, auch ganz ohne Absturz (Codex-QS 24.09.2026).
                let jetzt = Utc::now();
                let steigt = dumped && wegsteigen_mit_fahrt(&snap);
                if steigflug_gehalten(&mut s.sampler_steigflug_seit, steigt, jetzt) {
                    // 20.10.2026 („Stück 3“): eine Stelle fuer das Abschliessen
                    // einer Landung beim Wegsteigen — dieselbe, die die
                    // Phasenlogik ruft. Bis 2.1.4 setzte dieser Block nur die
                    // Sampler-Haelfte zurueck; Note, Live-Meldung und Phase
                    // blieben beim ersten Aufsetzer (EWG 1599).
                    //
                    // Mit derselben Haltezeit wie die Phasenlogik: Seit hier die
                    // GANZE Landung abgeschlossen wird, haette ein einzelner
                    // AGL-Ausreisser im Ausrollen eine echte Landung zum
                    // Touch-and-Go gemacht. Eine Sekunde reicht noch fuer den
                    // Aufprall Sekunden nach einem Hopser (JGXrGwlwa4aOGZpn,
                    // siehe `touchdown_dump_faellig`) — der Dump selbst faellt
                    // weiter sofort bei 100 ft.
                    aufsetzer_einfrieren(&flight, &mut s, &snap);
                    landung_beim_wegsteigen_abschliessen(&mut s, &snap, jetzt);
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        agl_ft = agl_now,
                        "v0.7.0 Multi-TD reset — climb-out > 100ft AGL detected after dump, ready for next TD"
                    );
                }
            }
        }
        // Ende ohne Stopp und ohne neue Generation: Der Messfaden ist
        // abgestürzt. Erst jetzt, nach dem Abarbeiten, den Merker freigeben
        // — sonst ließe sich die Messung für diesen Flug nie wieder starten.
        if !flight.stop.load(Ordering::Relaxed) {
            let _start = flight.start_sperre.lock().unwrap_or_else(|e| e.into_inner());
            if flight.generation.load(Ordering::SeqCst) == meine_generation {
                flight.touchdown_sampler_spawned.store(false, Ordering::SeqCst);
                tracing::error!(pirep_id = %flight.pirep_id, "Aufsetz-Messfaden unerwartet beendet — Merker zurückgesetzt");
            }
        }
        tracing::info!(pirep_id = %flight.pirep_id, "touchdown sampler stopped");
    });
    if let Err(e) = gestartet {
        // Merker zurücksetzen, sonst unterdrückt er jeden weiteren Startversuch
        // (Codex-Abnahme 11.10.2026) — aber nur für die eigene Generation,
        // sonst gäbe ein überholter Start den Merker des neuen Samplers frei
        // (Codex-Abnahme 057d5a9c).
        let _start = flight_fuer_fehler
            .start_sperre
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if flight_fuer_fehler.generation.load(Ordering::SeqCst) == meine_generation {
            flight_fuer_fehler
                .touchdown_sampler_spawned
                .store(false, Ordering::SeqCst);
        }
        tracing::error!(error = %e, "Aufsetz-Messfaden konnte nicht gestartet werden");
    }
}
