/// v0.5.49 — Übersetzt technische Netzwerk-Fehler in für Piloten lesbare
/// Texte. Hauptzweck: Windows-Socket-Codes wie "1236" oder "10054" sind
/// für den Pilot kryptisch — wir mappen sie auf "Verbindung verloren"
/// + erwartete Selbst-Heilung.
///
/// Heuristik: wir matchen auf Teilstrings im Display-String von
/// reqwest::Error, plus die ein Windows-Code-Suffixe wie "(os error 1236)"
/// die Tokio/Reqwest auf Windows in den Error-Text schreiben.
fn friendly_net_error(e: &ApiError) -> String {
    let s = format!("{e}");
    let s_lower = s.to_lowercase();
    if s.contains("1236") || s_lower.contains("connection invalid") {
        "Verbindung wurde unterbrochen (1236 — Router-NAT-Eviction o.ä.). Wiederversuch automatisch.".to_string()
    } else if s.contains("10053")
        || s_lower.contains("connection abort")
        || s_lower.contains("software caused")
    {
        "Verbindung lokal abgebrochen (10053). Wiederversuch automatisch.".to_string()
    } else if s.contains("10054") || s_lower.contains("connection reset") {
        "Verbindung vom Server beendet (10054). Wiederversuch automatisch.".to_string()
    } else if s.contains("10060") || s_lower.contains("timed out") || s_lower.contains("timeout") {
        "Server-Antwort-Timeout. Wiederversuch automatisch.".to_string()
    } else if s_lower.contains("dns") || s_lower.contains("name resolution") {
        "DNS-Auflösung fehlgeschlagen. Internet-Verbindung prüfen.".to_string()
    } else if s_lower.contains("connect") {
        "Konnte phpVMS-Server nicht erreichen. Wiederversuch automatisch.".to_string()
    } else {
        format!("Netzwerk-Fehler: {s}")
    }
}

/// Open (or create) the per-flight JSONL recorder. None when we can't
/// resolve the app data dir — caller treats it as "skip recording for
/// this tick" rather than failing the streamer.
fn open_flight_recorder(app: &AppHandle, pirep_id: &str) -> Option<FlightRecorder> {
    let dir = app.path().app_data_dir().ok()?;
    FlightRecorder::open(dir, pirep_id).ok()
}

/// Open the landing-history store. None when the app data dir can't be
/// resolved — caller treats that as "skip recording, history is best-
/// effort" rather than failing the PIREP file.
fn open_landing_store(app: &AppHandle) -> Option<LandingStore> {
    let dir = app.path().app_data_dir().ok()?;
    LandingStore::open(dir).ok()
}

/// Open the local deleted-landings tombstone (see
/// [`storage::DeletedLandingsTombstone`]). Same "best-effort, None on
/// unresolvable app dir" contract as `open_landing_store`.
fn open_deleted_landings_tombstone(app: &AppHandle) -> Option<DeletedLandingsTombstone> {
    let dir = app.path().app_data_dir().ok()?;
    DeletedLandingsTombstone::open(dir).ok()
}

/// v0.7.5 Phase-Safety Hotfix (Spec docs/spec/flight-phase-state-machine.md §13.8):
/// Reine pure-function fuer den Universal Arrived-Fallback. URO913 zeigte historisch:
/// Pilot rollte mit `engines_running=0` aber `groundspeed_kt = 141 -> 42` ueber ~31.5s
/// zum Cargo-Stand → Fallback feuerte `Arrived` obwohl der Flieger noch rollte.
///
/// Ab v0.7.5: `groundspeed_kt < 1.0` Pflicht — echtes Stillstand statt nur "on ground".
///
/// Returns true wenn der ARRIVED_FALLBACK_DWELL_SECS-Dwell starten/laufen darf.
pub fn arrived_fallback_conditions_basic(
    on_ground: bool,
    engines_running: u8,
    groundspeed_kt: f32,
) -> bool {
    on_ground && engines_running == 0 && groundspeed_kt < 1.0
}

/// v0.17.x (#Premium, Aircraft-Scan): reine Bedingung für den engines-
/// UNABHÄNGIGEN Stillstands-Fallback (siehe [`ARRIVED_STANDSTILL_DWELL_SECS`]).
/// Anders als [`arrived_fallback_conditions_basic`] verlangt sie NICHT
/// `engines_running == 0` — genau dafür ist sie da: Addons, deren
/// Triebwerkszähler nach dem Shutdown klemmt (Contrail FA50 Trijet bleibt
/// bei 3), würden sonst nie Auto-File auslösen. Stattdessen: am geplanten
/// Ziel (`near_planned`), voll gestanden (< 1 kt), Parkbremse gesetzt. Der
/// Aufrufer koppelt das an den längeren Dwell, damit ein kurzer Halt mit
/// laufenden Triebwerken (Warten vor dem Gate) nicht als Ankunft durchgeht.
///
/// WICHTIG (v0.17.x QA-Review, NICHT gelockert): die Parkbremsen-Pflicht
/// bleibt bewusst hart. Der naheliegende Gedanke "was, wenn das Addon auch
/// die Parkbremse nicht treibt" wurde geprüft und verworfen — ohne die
/// Bremse ließe sich ein Rollhalt (z. B. Warten auf eine freie Bahn zum
/// Kreuzen während des Taxi-in, real mehrere Minuten) nicht mehr von einer
/// echten Ankunft unterscheiden: Position/Geschwindigkeit sind während eines
/// solchen Halts identisch mit "geparkt". Ob der Contrail FA50 die Stock-
/// `parking_brake`-SimVar tatsächlich setzt, ist noch NICHT am echten Flug
/// verifiziert (siehe QA-Notiz) — das ist die richtige Reihenfolge: erst am
/// Flugdaten-Log von D-BETI bestätigen, dann ggf. gezielt lockern (z. B. über
/// einen zusätzlichen, wirklich addon-unabhängigen Ankunfts-Indikator statt
/// über pure Standzeit).
pub fn arrived_standstill_condition(
    near_planned: bool,
    on_ground: bool,
    groundspeed_kt: f32,
    parking_brake: bool,
) -> bool {
    near_planned && on_ground && groundspeed_kt < 1.0 && parking_brake
}

#[cfg(test)]
#[path = "tests_lib/arrived_standstill_tests.rs"]
mod arrived_standstill_tests;

/// v0.7.5 Phase-Safety Hotfix (Spec §13.9):
/// Reine pure-function: muss `holding_pending_since` resetet werden bei diesem
/// Phase-Wechsel? PTO105 (Bug-Beleg): Approach -> Holding mit nur 5.2s Aufenthalt —
/// alter Pending-State aus einer frueheren Phase wurde mitgeschleppt.
///
/// Regel: bei jedem Phase-Wechsel der NICHT `→ Holding` ist, Pending-Reset.
pub fn should_reset_holding_pending(prev_phase: FlightPhase, next_phase: FlightPhase) -> bool {
    next_phase != prev_phase && next_phase != FlightPhase::Holding
}

// ─── v0.7.6 P1-3: Runway-Geometry-Trust ───────────────────────────────
//
// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
//
// Real-Beleg fuer den Bug: GSG303 v0.7.5 (ldoGzaxbb9aNYE9n)
//   - arr_airport=OR66 aber runway_match_icao=K5S9 (3.5 km daneben)
//   - landing_float_distance_m=-613 (Touchdown 613 m vor Threshold)
//   - Score behandelt das trotzdem als "TD Zone 1, excellent stop"
//
// Trust-Check: ICAO muss matchen, Centerline-Offset darf max 200 m sein,
// Float-Distance darf max -100 m sein. Werte sind hardcoded (v1.2).

// Maximale Centerline-Distanz (m) vom Touchdown-Punkt zur erkannten
// Runway-Mittellinie. Daruber: vermutlich falscher Runway-Match.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> runway_trust_max_centerline_offset_m (v2.1.24: 200.0)

// Minimum-Float-Distance (m). Werte unter -100 m bedeuten der
// Touchdown war so weit vor der Threshold dass die Runway-Geometrie
// nicht mehr stimmen kann. Liegt typische TDZ-Marker-Versetzungen
// (e.g. EDDM 26L) komfortabel im trusted-Bereich.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> runway_trust_min_float_distance_m (v2.1.24: -100.0)

/// v0.7.6 P1-3: Pure-function Trust-Check ohne FlightStats-Abhaengigkeit
/// damit die Logik 1:1 testbar ist. Returns (trusted, reason).
///
/// Reasons (in Pruefungs-Reihenfolge):
///   "no_runway_match"           — Kein Runway-Match in der DB (Privatplatz,
///                                 Off-DB-Bahn) — UI sollte hier KEINE
///                                 alarmierende Pille zeigen, einfach die
///                                 Geometry-Tiles ausblenden
///   "icao_mismatch"             — Match-ICAO existiert aber != arr/divert
///                                 (echter Bug-Indikator: falscher Flughafen)
///   "centerline_offset_too_large" — > 200 m
///   "negative_float_distance"   — < -100 m
///   None                         — alle Checks pass
///
/// **Wichtig (v1.1-Refinement):** "kein Match" und "falscher Match" sind
/// semantisch verschieden — bei Privatplaetzen / Bush-Strips ist es normal
/// dass die ourairports-DB keinen Eintrag hat, das ist KEIN Pilot-Fehler.
/// "falscher Match" dagegen ist ein echter Datenintegritaets-Bug
/// (GSG303-Klasse: arr=OR66, gematcht auf K5S9 weil 3.5 km daneben).
pub fn runway_geometry_trust_check(
    matched_icao: Option<&str>,
    arr_airport: &str,
    divert_to: Option<&str>,
    centerline_offset_m: Option<f32>,
    float_distance_m: Option<f32>,
) -> (bool, Option<&'static str>) {
    // 1) Kein Match → eigener Reason, nicht als "icao_mismatch" verkaufen.
    let matched = match matched_icao {
        Some(m) if !m.is_empty() => m,
        _ => return (false, Some("no_runway_match")),
    };
    // 2) Match existiert aber != arr/divert → echtes Mismatch.
    //
    // v0.7.6 P3 Refinement: case-insensitive Vergleich. ICAOs sind in
    // unserer DB praktisch immer uppercase, aber externe phpVMS-Daten
    // oder OFP-Imports koennten "edDM" oder "eddm" liefern und das
    // sollte keine falsche icao_mismatch-Warnung triggern.
    let icao_ok = matched.eq_ignore_ascii_case(arr_airport)
        || divert_to.is_some_and(|d| matched.eq_ignore_ascii_case(d));
    if !icao_ok {
        return (false, Some("icao_mismatch"));
    }
    // Centerline-Offset (None ist OK — bedeutet "kein Wert verfuegbar",
    // nicht "Wert zu gross"; wir bestrafen nicht das Fehlen einer
    // optionalen Diagnostik).
    if let Some(offset) = centerline_offset_m {
        if offset.abs() > crate::grenzwerte::grenzwerte().ausrollen.runway_trust_max_centerline_offset_m.wert {
            return (false, Some("centerline_offset_too_large"));
        }
    }
    // Float-Distance (None ist OK — siehe Centerline-Begruendung)
    if let Some(float) = float_distance_m {
        if float < crate::grenzwerte::grenzwerte().ausrollen.runway_trust_min_float_distance_m.wert {
            return (false, Some("negative_float_distance"));
        }
    }
    (true, None)
}

/// v0.7.6 P1-2 / P2-B Helper: zentraler Read-Sites-Wrapper fuer den
/// scoring-relevanten Bounce-Count. Aktuell ist `stats.bounce_count`
/// das Feld das sowohl Streamer-Tally (35-ft armed counter) als auch
/// nach dem Analysis-Override den scored_bounce_count haelt — ein
/// Caller der das Feld direkt liest weiss nicht ob das Override schon
/// gelaufen ist oder nicht.
///
/// Dieser Helper macht die Semantik im Score-Pfad explizit: "ich will
/// den scored_bounce_count, mit Fallback auf das was im stats steht".
/// Falls in v0.8.x die FlightStats umstrukturiert werden (eigenes
/// `scored_bounce_count`-Feld), aendert sich nur diese eine Stelle.
///
/// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-2,
/// Refinement nach Thomas-Review (P2-B fragility).
/// Aufsetzer für die Bewertung — 0, wenn nicht gemessen werden konnte.
///
/// Eine Zahl aus einem löchrigen Fenster wäre hier besonders heikel: Ein
/// Hopser zwischen zwei fehlenden Proben bleibt unsichtbar, und "0 Aufsetzer"
/// läse sich als saubere Landung. Ohne ausreichende Aufzeichnung fliesst der
/// Wert deshalb gar nicht erst in die Bewertung ein (der Gesamtscore
/// entfällt ohnehin, siehe `canonical_landing_rate_fpm`).
fn scored_bounce_count_for_score(stats: &FlightStats) -> u32 {
    // stats.bounce_count wurde von der Override-Logik (lib.rs ~9869-9891)
    // mit scored_bounce_count aus dem analysis-JSON ueberschrieben falls
    // der 50-Hz-Sampler-Buffer vollstaendig war. Ohne Override haelt das
    // Feld den Streamer-Tally (35-ft armed counter) — auch das ist eine
    // konservative Annaeherung an "echter Bounce" (35 ft >= 15 ft scored
    // threshold), sodass kein Pilot faelschlich bestraft wird wenn der
    // Sampler-Buffer fehlt.
    stats.bounce_count as u32
}

// ─── v0.7.8 SimBrief-direct: Flight-Match-Verifikation ──────────────────
//
// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §6.
//
// Wenn AeroACArS direkt von SimBrief den latest OFP holt, muss verifiziert
// werden dass dieser zum aktiven Flug gehoert. Pilot koennte zwischen
// flight_start und Refresh einen OFP fuer einen ANDEREN Flug regeneriert
// haben (z.B. PPL-Training-Run). dpt+arr+callsign-Match faengt das ab.
//
// Match-Regeln:
//   - Origin/Destination ICAO: case-insensitive exakt
//   - Callsign: normalisierter Vergleich (kein blinder Suffix-Match)
//
// Pure functions — getestet ohne Tauri-State.

/// v0.7.8: Normalisiert eine Callsign-Form auf vergleichbares Format.
/// Entfernt Whitespace + Bindestrich + Underscore, uppercase.
/// "DLH-100" → "DLH100", "GSG 100" → "GSG100", "dlh100" → "DLH100".
pub fn normalize_callsign(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .collect::<String>()
        .to_ascii_uppercase()
}

/// v0.7.8: Trennt normalisierte Callsign-Form in (prefix, number).
/// "DLH100" → ("DLH", "100"), "100" → ("", "100"),
/// "GSG100A" → ("GSG", "100A").
pub fn split_callsign(cs: &str) -> (String, String) {
    let split_at = cs.find(|c: char| c.is_ascii_digit()).unwrap_or(cs.len());
    let (prefix, number) = cs.split_at(split_at);
    (prefix.to_string(), number.to_string())
}

/// v0.7.8 v1.5: Baut eine Liste von Kandidaten-Callsigns auf gegen die
/// der SimBrief-OFP-Callsign matchen darf. Spec §6.1.2.
///
/// Real-World-Beispiel (GSG):
///   - airline_icao = "CFG", flight_number = "1504" (Bid)
///   - bid_callsign = Some("CFG4TK") oder None
///   - pilot_callsign = Some("4TK") (aus Profile)
///
/// Kandidaten:
///   1. "CFG1504"  (airline + flight_number)
///   2. "1504"     (number-only)
///   3. "CFG4TK"   (bid.flight.callsign wenn vorhanden)
///   4. "CFG4TK"   (airline + pilot_callsign)
///   5. "4TK"      (pilot_callsign allein)
///
/// SimBrief-Callsign muss gegen EINEN davon exakt matchen (nach
/// Normalisierung). Leere Strings werden ausgefiltert.
pub fn build_candidate_callsigns(
    airline_icao: &str,
    flight_number: &str,
    bid_callsign: Option<&str>,
    pilot_callsign: Option<&str>,
) -> Vec<String> {
    let airline = airline_icao.trim().to_ascii_uppercase();
    let flight_n = flight_number.trim();
    let mut candidates: Vec<String> = Vec::new();

    // (1) airline_icao + flight_number
    if !airline.is_empty() && !flight_n.is_empty() {
        candidates.push(normalize_callsign(&format!("{}{}", airline, flight_n)));
    }
    // (2) flight_number allein (number-only-mode)
    if !flight_n.is_empty() {
        candidates.push(normalize_callsign(flight_n));
    }
    // (3) v1.5: bid.flight.callsign direkt
    if let Some(cs) = bid_callsign.filter(|s| !s.trim().is_empty()) {
        candidates.push(normalize_callsign(cs));
    }
    // (4) v1.5: airline + pilot_callsign
    if let Some(pc) = pilot_callsign.filter(|s| !s.trim().is_empty()) {
        if !airline.is_empty() {
            candidates.push(normalize_callsign(&format!("{}{}", airline, pc)));
        }
        // (5) v1.5: pilot_callsign allein
        candidates.push(normalize_callsign(pc));
    }

    // Dedupe + Leere entfernen.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

/// v0.7.8: Verifiziert ob ein SimBrief-OFP zum aktiven AeroACARS-Flug passt.
/// Match-Regeln (v1.5 — Kandidaten-Liste statt strenger 1:1-Vergleich):
///   1. Origin/Destination ICAO: case-insensitive exakt
///   2. Callsign: SimBrief-Callsign (normalisiert) muss gegen EINEN von
///      mehreren validen Kandidaten exakt matchen. Siehe
///      `build_candidate_callsigns` fuer die volle Liste.
///   3. Empty SimBrief-Callsign → toleranter Match (= dpt+arr genuegen).
///
/// **Wichtig (Spec v1.1 P1-3):** KEIN blinder Suffix-Match — Kandidaten
/// muessen exakt sein. "DLH1100" matched NICHT mit Kandidat "100".
///
/// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §6.1.
pub fn ofp_matches_active_flight(
    ofp_origin: &str,
    ofp_destination: &str,
    ofp_callsign: &str,
    active_dpt: &str,
    active_arr: &str,
    active_airline_icao: &str,
    active_flight_number: &str,
    active_bid_callsign: Option<&str>,
    active_pilot_callsign: Option<&str>,
) -> bool {
    // 1. Origin / Destination MUESSEN matchen (case-insensitive).
    let dpt_ok = ofp_origin.trim().eq_ignore_ascii_case(active_dpt.trim());
    let arr_ok = ofp_destination
        .trim()
        .eq_ignore_ascii_case(active_arr.trim());
    if !dpt_ok || !arr_ok {
        return false;
    }

    let simbrief_norm = normalize_callsign(ofp_callsign);
    if simbrief_norm.is_empty() {
        // SimBrief liefert kein Callsign → toleranter Mode: dpt+arr genuegen.
        // Real selten — SimBrief-OFP traegt typisch immer einen Callsign.
        return true;
    }

    // 2. v1.5: Kandidaten-Liste bauen aus allen verfuegbaren Quellen.
    let candidates = build_candidate_callsigns(
        active_airline_icao,
        active_flight_number,
        active_bid_callsign,
        active_pilot_callsign,
    );

    // 3. Exakter Match gegen Kandidat (= verhindert "DLH1100 vs 100"-
    //    False-Positive). split_callsign-Number-Vergleich nur fuer den
    //    Number-only-Fall (SimBrief liefert "1504" oder "4TK" ohne
    //    Airline-Prefix, dann werden gegen die number-Part-Kandidaten
    //    verglichen).
    let (simbrief_prefix, simbrief_number) = split_callsign(&simbrief_norm);

    if simbrief_prefix.is_empty() {
        // SimBrief number-only: vergleiche gegen number-Parts aller
        // Kandidaten. "1504" matcht "CFG1504" (number "1504"), "4TK"
        // matcht "CFG4TK" (number "4TK").
        return candidates.iter().any(|c| {
            let (_, c_number) = split_callsign(c);
            !c_number.is_empty() && c_number == simbrief_number
        });
    }

    // SimBrief mit Prefix: exakter Match gegen Kandidaten-Liste.
    candidates.contains(&simbrief_norm)
}

/// v0.7.21 (Nachhaltiger Fix, GAF/MS713-QS): kanonischer V/S-fpm-Wert.
///
/// Hintergrund: drei verschiedene Felder im `FlightStats` haben einen
/// V/S-Anteil (`landing_rate_fpm`, `landing_peak_vs_fpm`,
/// `landing_analysis["vs_at_edge_fpm"]`). Vor v0.7.21 wurden je nach
/// Code-Stelle unterschiedliche Felder direkt referenziert — Resultat:
/// phpVMS sah einen anderen Wert als Pilot-Client/Webapp/Discord-Bot.
/// v0.7.17 B-015a hatte einen `score_basis_vs_fpm(&stats)`-Helper
/// eingefuehrt, aber dessen Anwendung war an einigen Stellen vergessen
/// worden (FileBody.landing_rate ging direkt auf `landing_rate_fpm`).
///
/// Ab v0.7.21: JEDE Anzeige-/Filing-/Publish-Stelle MUSS
/// `stats.canonical_landing_rate_fpm()` aufrufen statt einzelne Felder
/// direkt zu lesen. Der freistehende `score_basis_vs_fpm()`-Helper
/// bleibt als duenner Shim erhalten (delegiert hierhin), damit
/// bestehender Code nicht broken bricht.
///
/// Cascade-Reihenfolge (matched `scoreBasisVs()` im TS-Frontend):
///   1. `vs_at_edge_fpm` aus `stats.landing_analysis` (50-Hz-Buffer-
///      Edge, FAR 25.473 Engineering-Standard) — wenn `< 0`
///   2. `stats.landing_peak_vs_fpm` (Touchdown-Window-Peak, Streamer-
///      Tick-Cadence)
///   3. `stats.landing_rate_fpm` (Streamer-Tick-Snapshot am TD-Frame)
/// Ist dieser Wert ueberhaupt eine Landerate?
///
/// v0.20.2: `vs_at_edge_fpm` ist eine reine lineare Interpolation zwischen dem
/// letzten Sample in der Luft und dem ersten am Boden (siehe Buffer-Dump) —
/// **ohne jeden Guard**. Seit v0.20.0 ist genau dieser Wert die Kanonik, waehrend
/// touchdown_v2 (das seinen Riegel `finalize_vs` laengst hat: nie positiv, nie
/// unter -3000 fpm) nur noch Fallback ist. Damit wurde der UNGESCHUETZTE Pfad
/// zur Wahrheit und der geschuetzte zum Ersatz.
///
/// Dass das keine Theorie ist, zeigt Flug 804 (ELLX, 14.07.2026): Edge-Wert
/// **+24,47 fpm** — eine positive Sinkrate beim Aufsetzen. Aufgefangen wurde er
/// nur, weil die alte Bedingung zufaellig `< 0` lautete. Ein negativ-absurder
/// Glitch (-30.000) waere ungebremst in Score, Log und PIREP gewandert.
///
/// Nutzt DIESELBE Untergrenze wie `touchdown_v2::finalize_vs` — importiert, nicht
/// abgeschrieben, damit es nicht zwei Definitionen von "plausibel" geben kann.
///
/// Eine Abweichung bleibt bewusst: `finalize_vs` laesst exakt 0.0 fpm zu (es hat
/// eigene Kategorie-Floors fuer Helis und Wasserflugzeuge, die bewusst mit
/// nahezu null Sinkrate aufsetzen). Hier verlangen wir strikt < 0, weil die
/// Kanonik sonst einen Glitch mit 0.0 als "Landung" ausliefern koennte, ohne dass
/// touchdown_v2 je gefragt wurde. Bei 0.0 faellt sie auf den naechsten Kandidaten
/// — und der laeuft durch touchdown_v2s eigenen Guard.
fn landing_rate_is_plausible(fpm: f32) -> bool {
    fpm.is_finite() && fpm < 0.0 && fpm >= crate::grenzwerte::grenzwerte().aufsetzen.vs_floor_fpm.wert
}

impl FlightStats {
    /// SSoT fuer den V/S-Wert in fpm den Pilot/PIREP/MQTT/phpVMS sehen.
    /// Direkter Zugriff auf `stats.landing_rate_fpm` /
    /// `stats.landing_peak_vs_fpm` ist in Anzeige-/Filing-Pfaden ein
    /// Bug — siehe MS713-QS-Befund 2026-05-13.
    pub fn canonical_landing_rate_fpm(&self) -> Option<f32> {
        // ⚠ ZUERST: Wurde die Landung überhaupt gemessen?
        //
        // Diese Funktion ist die eine Stelle, die Anzeige, PIREP, MQTT und
        // phpVMS befragen. Deshalb sitzt die Sperre hier und nicht in einer
        // der Quellen: Codex hat am 12.09.2026 fünf Wege gefunden, über die
        // sonst doch wieder eine Zahl durchkommt (Höhenregression am
        // Aufsetzpunkt, Interpolation zwischen Luft- und Bodenprobe ohne
        // zeitliche Obergrenze, die Bevorzugung des Edge-Werts hier, ein
        // erhaltener Altwert, und der Timeout-Fallback nach zwölf Sekunden).
        // Ein Riegel in `compute_landing_rate` allein hätte keinen davon
        // erwischt.
        if self.landung_abdeckung_fehlt.is_some() {
            // Die Ausnahme gilt dem WERT, nicht dem Quellennamen.
            //
            // Codex-Befund 12.09.2026 zum ersten Entwurf: Die Prüfung stand
            // nur auf dem Quellenstring und liess danach die ganze Kaskade
            // laufen — in der gewinnt `landing_analysis.vs_at_edge_fpm`.
            // Bei `landing_source = "msfs_simvar_latched"` und einem Edge-Wert
            // von −10,4 fpm aus dem kaputten Fenster kam also genau die Zahl
            // heraus, gegen die diese Sperre gebaut wurde. Die Ausnahme
            // legitimierte den Wert, den sie ausschliessen sollte.
            //
            // Deshalb hier: Nur der vom Simulator selbst gemessene Wert wird
            // durchgereicht, nichts aus unserem Fenster.
            if !quelle_misst_der_simulator_selbst(self.landing_source.as_deref()) {
                return None;
            }
            return self
                .landing_peak_vs_fpm
                .or(self.landing_rate_fpm)
                .filter(|v| v.is_finite() && *v < 0.0);
        }
        // v0.20.2: JEDER Kandidat der Kaskade muss plausibel sein — nicht nur
        // der erste. Vorher war der Edge-Wert gegen "positiv" geprueft und die
        // beiden Fallbacks gegen gar nichts. Ein Glitch-Sample konnte also
        // ueber den Fallback trotzdem als Landerate in Score, Log und PIREP
        // wandern. Bleibt kein Kandidat uebrig, gibt es KEINE Landerate — das
        // ist ehrlicher als eine erfundene Zahl (touchdown_v2 lehnt aus genau
        // diesem Grund seit jeher ab, statt zu raten).
        let edge = self
            .landing_analysis
            .as_ref()
            .and_then(|v| v.get("vs_at_edge_fpm"))
            .and_then(|v| v.as_f64())
            .map(|x| x as f32);
        // v0.20 (QS-Fix, fallback_zero-Score-Bug): `landing_peak_vs_fpm`/
        // `landing_rate_fpm` werden ATOMAR zusammen mit `landing_source`
        // geschrieben (`finalize_landing_rate`). Steht dort "fallback_zero"
        // oder "other_fallback" (alle echten Quellen scheiterten, der Streamer
        // hat 0.0 als Platzhalter eingesetzt — siehe vs_source-Kaskade
        // lib.rs ~24834), ist die Zahl PER DEFINITION erfunden, egal wie
        // plausibel sie fuer sich genommen aussieht (0 fpm ist ja ein
        // voellig normaler echter Wert — genau deshalb faellt das der reinen
        // Plausibilitaets-Pruefung oben nie auf). Diese zwei Kandidaten daher
        // aussparen; `edge` kommt aus der unabhaengigen touchdown_v2-Pipeline
        // und bleibt unangetastet — hatte die einen echten Touchdown erfasst,
        // ist er weiterhin gueltig auch wenn der Legacy-Pfad in denselben
        // Tick fallback_zero geschrieben hat.
        let is_fabricated_fallback = matches!(
            self.landing_source.as_deref(),
            Some("fallback_zero") | Some("other_fallback")
        );
        let legacy_candidates: [Option<f32>; 2] = if is_fabricated_fallback {
            [None, None]
        } else {
            [self.landing_peak_vs_fpm, self.landing_rate_fpm]
        };
        [edge, legacy_candidates[0], legacy_candidates[1]]
            .into_iter()
            .flatten()
            .find(|&v| landing_rate_is_plausible(v))
    }

    /// v0.20 (QS-Fix, fallback_zero-Score-Bug): SSoT-Gate für `landing_peak_
    /// g_force`, analog zu `canonical_landing_rate_fpm`. `landing_peak_g_force`
    /// wird im selben Streamer-Tick gesetzt wie die fallback_zero-Landerate
    /// (lib.rs ~24928-24939: simpler Snapshot-Buffer-Max, kein echter Touchdown-
    /// Impuls) — genauso wenig eine echte Messung. `None` statt einer
    /// erfundenen Zahl, wenn keine echte Quelle je einen Wert lieferte.
    pub fn canonical_peak_g_force(&self) -> Option<f32> {
        // Aus demselben Fenster wie die Sinkrate. Reicht die Aufzeichnung
        // dort nicht, ist auch der Spitzenwert der G-Kraft nur der höchste
        // der wenigen Proben, die zufällig ankamen — kein Messwert.
        // Vorgabe Thomas, 12.09.2026: "Wenn wir keine detaillierten Werte
        // haben, können wir die auch nicht berechnen."
        //
        // ⚠ Hier gilt die MSFS-Ausnahme NICHT: Der Simulator meldet die
        // Aufsetz-SINKRATE selbst, die G-Spitze aber kommt aus unserem
        // Fenster (Codex-Befund 12.09.2026). Eine gültige Sinkrate
        // legitimiert keinen G-Wert.
        if self.landung_abdeckung_fehlt.is_some() {
            return None;
        }
        if matches!(
            self.landing_source.as_deref(),
            Some("fallback_zero") | Some("other_fallback")
        ) {
            return None;
        }
        self.landing_peak_g_force
    }

    /// SSoT fuer die Bank-Streuung (sigma) im Anflug.
    ///
    /// v0.20.0: dieselbe Krankheit wie bei der Sinkrate. Die Bank-Streuung
    /// wird ZWEIMAL gerechnet:
    ///   * legacy `approach_bank_stddev_deg` — ungefiltert, Fenster bis
    ///     1500 ft AGL. Zaehlt absichtliche Anflugkurven als "Instabilitaet"
    ///     und ueberschaetzt sie dadurch.
    ///   * v2 `approach_bank_stddev_filtered_deg` — vektor-gefiltert, Fenster
    ///     bis 1000 ft HAT. Das ist der physikalisch gemeinte Wert.
    ///
    /// Bisher gewann je nach Oberflaeche ein anderer: das Stabilitaets-Gate
    /// (Banner) rechnete aus v2, der Sub-Score und die Client-Kachel aus
    /// legacy, die Webapp aus v2 (nur der ging an MQTT). Ergebnis: Banner
    /// "instabil" neben einer Kachel, die harmlos aussah — und die Webapp
    /// zeigte fuer dieselbe Landung eine dritte Zahl.
    ///
    /// v2 gewinnt, legacy bleibt Fallback fuer Alt-Datensaetze ohne v2-Feld.
    pub fn canonical_bank_stddev_deg(&self) -> Option<f32> {
        // Score-Version 23: beim spaeten Final zaehlt nur die Messung ab dem
        // Ausrichten. Die alte Streuung enthaelt die Kurven davor — als
        // Rueckfall wuerde sie genau das werten, was ausgenommen ist.
        if self.approach_spaetes_final.is_some() {
            return self.approach_bank_stddev_filtered_deg;
        }
        self.approach_bank_stddev_filtered_deg
            .or(self.approach_bank_stddev_deg)
    }

    /// SSoT fuer die V/S-Streuung im Anflug — das Gegenstueck zu
    /// `canonical_bank_stddev_deg()`.
    ///
    /// v0.20.1: Beide Achsen fliessen in DENSELBEN Stabilitaets-Sub-Score. Bis
    /// v0.20.0 kam die Bank-Achse aus dem v2-Fenster (gefiltert, <= 1000 ft HAT)
    /// und die V/S-Achse aus dem Legacy-Fenster (ungefiltert, <= 1500 ft AGL) —
    /// eine Note, zusammengesetzt aus zwei nicht vergleichbaren Messungen.
    ///
    /// Gemessen am VPS-Korpus (224 rekonstruierte Anfluege): die Vereinheitlichung
    /// verschiebt die V/S-Achse im Mittel um -1,4 Punkte, 83 % der Landungen
    /// bleiben im selben Band. Es ist also KEIN Bugfix an der Bewertung — die
    /// Bewertung war schon richtig. Es beseitigt, dass eine einzige Formel aus
    /// zwei Wirklichkeiten rechnet.
    pub fn canonical_vs_stddev_fpm(&self) -> Option<f32> {
        // Score-Version 23: beim spaeten Final zaehlt nur die Messung ab dem
        // Ausrichten. Die alte Streuung enthaelt die Kurven davor — als
        // Rueckfall wuerde sie genau das werten, was ausgenommen ist.
        if self.approach_spaetes_final.is_some() {
            return self.approach_vs_stddev_filtered_fpm;
        }
        self.approach_vs_stddev_filtered_fpm
            .or(self.approach_vs_stddev_fpm)
    }
}

/// v0.7.17 → v0.7.21 Shim: leitet auf `FlightStats::canonical_landing_
/// rate_fpm()` um. Neue Anzeige-Pfade sollen direkt die Methode auf
/// `FlightStats` aufrufen — dieser freistehende Helper wird beim
/// naechsten Major-Bump entfernt.
#[inline]
fn score_basis_vs_fpm(stats: &FlightStats) -> Option<f32> {
    stats.canonical_landing_rate_fpm()
}

/// v0.7.18 (B-012) — Resolution-Source.
///
/// Spec docs/spec/v0.7.18-orphan-flight-cleanup.md §B-012. Wire-Format
/// per Serde snake_case damit Frontend + Webapp das Enum als String
/// vergleichen können.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TouchdownAirportSource {
    /// Touchdown-Edge konnte einer Runway zugeordnet werden — autoritativ.
    RunwayMatch,
    /// Kein Runway-Match, aber Airport innerhalb 25 nmi. `nearest_distance_nm` gesetzt.
    Nearest25Nm,
    /// Total off-airport — fallback auf `flight.arr_airport`.
    PlannedFallback,
}

/// v0.7.18 (B-012) — Resolution-Result für den Touchdown-Airport.
#[derive(Debug, Clone)]
pub struct TouchdownAirportResolution {
    pub icao: String,
    pub source: TouchdownAirportSource,
    pub distance_to_destination_nm: Option<f32>,
    pub nearest_distance_nm: Option<f32>,
}

impl TouchdownAirportSource {
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::RunwayMatch => "runway_match",
            Self::Nearest25Nm => "nearest_25nm",
            Self::PlannedFallback => "planned_fallback",
        }
    }
}

/// v0.8.0: Resultat aller Pure-Function-Assessments für einen Touchdown.
/// Wird sowohl von `record_landing_for_filed_flight` (= LandingRecord)
/// als auch vom Streamer-Tick (= TouchdownPayload) gebraucht — einmal
/// rechnen, an beide Orte feeden.
///
/// TDZ + Aim werden für JEDE runway-match Quelle berechnet (length +
/// td_distance reichen, beides ist auch in OurAirports). DDS + TCH
/// brauchen `displaced_threshold_ft` / `tch_ft` aus der Navigraph-
/// Geometrie und sind deshalb nur bei Navigraph-Source verfügbar.
/// TCH-actual wird im Streamer-Tick beim Touchdown-Edge aus dem 50-Hz-
/// Buffer extrahiert (siehe `stats.runway_tch_actual_ft`); diese
/// Funktion klassifiziert nur den vorberechneten Wert.
#[derive(Debug, Clone, Default)]
struct AssessedTouchdown {
    td_distance_from_threshold_m: Option<f64>,
    tdz: Option<runway_assessment::TdzResult>,
    aim: Option<runway_assessment::AimResult>,
    tch: Option<runway_assessment::TchResult>,
    dds: Option<runway_assessment::DisplacedResult>,
}

/// Die Bahn-Herkunft fuer die Leitung — aus EINER Ableitung.
///
/// # Warum das eine Funktion ist
///
/// Zwei Ereignisse tragen diese Gruppe: `touchdown_complete` neun
/// Sekunden nach dem Aufsetzen und `touchdown_rollout_finalized`, sobald
/// das Ausrollen steht. Trifft die Szenerie des Simulators erst nach dem
/// Aufsetzen ein, holt der Client die Bahnzuordnung nach — dann sind die
/// Werte des ersten Ereignisses ueberholt, und nur das zweite kann sie
/// beim Recorder richtigstellen.
///
/// Bis v1.7.14 zaehlten beide Stellen ihre Felder je einzeln auf. Das
/// Ergebnis: vier gemeinsame Felder von achtundzwanzig. Bahnlaenge,
/// versetzte Schwelle, Herkunft und die ganze Aufsetzpunkt-Einordnung
/// blieben auf dem Stand vor der Szenerie stehen.
///
/// ⚠ Die Funktion nimmt NUR `stats`. Alles, was sie braucht, steht dort —
/// insbesondere `assess_touchdown`, das seinerseits eine reine Funktion
/// von `stats` ist. Ein zweiter Parameter waere die Tuer, durch die eine
/// der beiden Aufrufstellen wieder etwas anderes einsetzt.
fn bahn_herkunft(stats: &FlightStats) -> aeroacars_mqtt::BahnHerkunftWire {
    let rwy_match = stats.runway_match.as_ref();
    let payload_assessed = assess_touchdown(stats);
    aeroacars_mqtt::BahnHerkunftWire {
        // ⚠ Die persistierte Revision, NICHT
        // `korrelierter_szenerie_stand`. Siehe `bahn_revision`.
        bahn_revision: Some(stats.bahn_revision),
        // Runde 6, Befund 3: Sagt dem Recorder, dass die dort liegende
        // Spur gegen eine fruehere Bahn projiziert ist.
        bahn_spur_veraltet: Some(stats.bahn_spur_bezug_veraltet),
        // v1.7.8: Woher die Bahngeometrie kam.
        // Steht auch am PirepPayload — der
        // Recorder propagiert die Teilnoten aus
        // dem PIREP auf diese Zeile, und dann
        // muss die Herkunft an beiden haengen.
        bahn_geometrie_quelle: Some(
            match stats.szenerie_uebernahme.as_ref() {
                Some(b) if !b.uebernommen.is_empty() => "szenerie",
                _ => "navdaten",
            }
            .to_string(),
        ),
        bahn_szenerie_status: Some(szenerie_status(stats)),
        sim_kennung: stats.sim_kennung.clone(),
        bahn_kurs_korrektur_grad: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_kursabweichung_grad),
        bahn_breiten_korrektur_m: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_breitenabweichung_m),
        bahn_schwellen_korrektur_m: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_schwellenabweichung_m),
        runway_match_icao: rwy_match.map(|m| m.airport_ident.clone()),
        runway_match_ident: rwy_match.map(|m| m.runway_ident.clone()),
        runway_match_distance_m: rwy_match
            .map(|m| (m.touchdown_distance_from_threshold_ft as f32) * 0.3048),
        runway_match_centerline_offset_m: rwy_match.map(|m| m.centerline_distance_m as f32),
        // v0.5.22: feeds the live-monitor's "Bahn-
        // Auslastung"-sub-score so it matches the
        // in-app PIREP value 1:1.
        runway_length_m: rwy_match.map(|m| m.length_ft * 0.3048),
        runway_surface: rwy_match
            .map(|m| m.surface.trim().to_string())
            .filter(|s| !s.is_empty()),
        // v0.8.0: Pure-Function-Assessment in den
        // Live-MQTT-Payload. Identische Werte wie
        // im LandingRecord (record_landing_for_filed_
        // flight) — Spec verlangt Field-Symmetrie.
        navdata_source: stats
            .runway_source
            .map(|s| runway_source_wire(s).to_string()),
        navdata_cycle: stats.runway_nav_cycle.clone(),
        runway_true_course_deg: stats.runway_nav_geometry.as_ref().map(|g| g.true_course),
        runway_displaced_threshold_ft: wire_displaced_threshold_ft(stats.runway_match.as_ref()),
        runway_tch_expected_ft: stats.runway_nav_geometry.as_ref().map(|g| g.tch_ft),
        runway_glideslope_angle_deg: stats
            .runway_nav_geometry
            .as_ref()
            .map(|g| g.glideslope_angle),
        td_distance_from_threshold_m: payload_assessed.td_distance_from_threshold_m,
        td_in_tdz: payload_assessed.tdz.map(|t| t.in_tdz),
        td_third: payload_assessed.tdz.map(|t| t.third),
        td_tdz_length_m: payload_assessed.tdz.map(|t| t.tdz_length_m),
        aim_delta_m: payload_assessed.aim.as_ref().map(|a| a.delta_m),
        aim_class: payload_assessed
            .aim
            .as_ref()
            .map(|a| aim_class_wire(a.class).to_string()),
        aim_point_m: payload_assessed.aim.as_ref().map(|a| a.aim_point_m),
        tch_actual_ft: payload_assessed.tch.as_ref().map(|t| t.actual_ft),
        tch_delta_ft: payload_assessed.tch.as_ref().map(|t| t.delta_ft),
        tch_class: payload_assessed
            .tch
            .as_ref()
            .map(|t| tch_class_wire(t.class).to_string()),
        tch_rad_ft: payload_assessed.tch.as_ref().map(|t| t.rad_ft),
        tch_hoehengruppe: payload_assessed.tch.as_ref().map(|t| t.gruppe.nummer()),
        pre_displaced_threshold: payload_assessed.dds.map(|d| d.in_pre_threshold_zone),
    }
}

fn assess_touchdown(stats: &FlightStats) -> AssessedTouchdown {
    let Some(rm) = stats.runway_match.as_ref() else {
        return AssessedTouchdown::default();
    };
    let Bahnmasse {
        td_raw_m,
        abzuziehen_ft,
        td_m,
        nutzbare_laenge_m,
    } = bahnmasse(rm);
    let tdz = runway_assessment::classify_tdz(td_m, nutzbare_laenge_m);
    let aim = Some(runway_assessment::classify_aim(td_m, nutzbare_laenge_m));
    // Dieselbe Groesse wie oben: was in der Geometrie steckt, ist kein
    // Sperrbereich mehr vor der gemessenen Distanz. Sonst gaelte auf
    // einer solchen Bahn jedes Aufsetzen im ersten Stueck als Landung
    // vor der Schwelle.
    let dds = Some(runway_assessment::classify_displaced(
        td_raw_m,
        abzuziehen_ft as f64,
    ));
    // F5 TCH: nur klassifizieren wenn BOTH der nav-geometric tch_ft
    // AND der actual-measured tch_ft im Streamer-Tick vorhanden sind.
    // Die erwartete Schwellenueberflughoehe muss auch WIRKLICH da sein.
    //
    // `tch_ft` ist in **9.804 der 85.058 Navdaten-Bahnen null** (11,5 %,
    // am 24.08.2026 auf dem Live-Server gezaehlt). `classify_tch` rechnet
    // `actual - expected`; gegen 0 wird aus einem sauberen Anflug mit
    // 47 ft ein Delta von +47 ft und die Einstufung „viel zu hoch".
    //
    // Dieselbe Fehlerklasse wie beim Belag (`nav_runways.surface_code`,
    // 0 von 85.058): ein Feld, das oft leer ist, wird ungeprueft
    // weiterverwendet und behauptet dann etwas Falsches, statt zu
    // schweigen. Ohne Erwartungswert gibt es keine Einstufung.
    let tch = match (
        stats.runway_nav_geometry.as_ref(),
        stats.runway_tch_actual_ft,
    ) {
        (Some(g), Some(actual)) if g.tch_ft > 0 => Some(runway_assessment::classify_tch(
            actual as f64,
            g.tch_ft as f64,
            // v1.8.1: Einstufung nach Raederhoehe — braucht die Hoehengruppe
            // des geflogenen Musters (FAA Order 8260.58D, Tab. 1-3-1).
            runway_assessment::hoehengruppe(stats.aufgeloestes_muster.as_deref()),
        )),
        _ => None,
    };
    AssessedTouchdown {
        td_distance_from_threshold_m: Some(td_m),
        tdz,
        aim,
        tch,
        dds,
    }
}

fn tch_class_wire(class: runway_assessment::TchClass) -> &'static str {
    use runway_assessment::TchClass::*;
    match class {
        OnProfile => "on_profile",
        SlightlyLow => "slightly_low",
        SlightlyHigh => "slightly_high",
        High => "high",
        BelowProfile => "below_profile",
    }
}

fn aim_class_wire(class: runway_assessment::AimClass) -> &'static str {
    use runway_assessment::AimClass::*;
    match class {
        Perfect => "perfect",
        ShortOfAim => "short_of_aim",
        PastAim => "past_aim",
        LongLanding => "long_landing",
        Severe => "severe",
    }
}

fn runway_source_wire(s: runway::RunwaySource) -> &'static str {
    match s {
        runway::RunwaySource::Navigraph => "navigraph",
        runway::RunwaySource::OurAirportsFallback => "ourairports_fallback",
    }
}

/// v0.7.18 (B-012) — Touchdown-Airport-Resolution.
///
/// Cascade:
///   1. `runway_match.airport_ident` — wenn TD-Edge einer Runway zugeordnet wurde.
///   2. Nearest Airport innerhalb 25 nmi.
///   3. `planned_arr_airport` als letzter Fallback.
///
/// `airport_lookup` ist eine Closure die für gegebene ICAO die Koordinaten
/// liefert. Caller wählt die Quelle (AppState.airports HashMap, Cache, etc.) —
/// die Spec gibt keinen konkreten Cache-Typ vor.
fn resolve_touchdown_airport<F>(
    runway_match: Option<&runway::RunwayMatch>,
    landing_lat: Option<f64>,
    landing_lon: Option<f64>,
    planned_arr_airport: &str,
    airport_lookup: F,
) -> TouchdownAirportResolution
where
    F: Fn(&str) -> Option<(f64, f64)>,
{
    let planned_coords = airport_lookup(planned_arr_airport);
    let dist_to_planned = |lat: f64, lon: f64| -> Option<f32> {
        planned_coords.map(|(plat, plon)| (geo::distance_m(lat, lon, plat, plon) / 1852.0) as f32)
    };

    if let Some(rw) = runway_match {
        let dist = match (landing_lat, landing_lon) {
            (Some(la), Some(lo)) => dist_to_planned(la, lo),
            _ => None,
        };
        return TouchdownAirportResolution {
            icao: rw.airport_ident.clone(),
            source: TouchdownAirportSource::RunwayMatch,
            distance_to_destination_nm: dist,
            nearest_distance_nm: None,
        };
    }

    if let (Some(lat), Some(lon)) = (landing_lat, landing_lon) {
        // Wert seit v3: bundle/grenzwerte/ausrollen.json -> td_nearest_radius_nm (v2.1.24: 25.0)
        let nearest = runway::find_nearest_airports(lat, lon, crate::grenzwerte::grenzwerte().ausrollen.td_nearest_radius_nm.wert * 1852.0, 1);
        if let Some(closest) = nearest.first() {
            return TouchdownAirportResolution {
                icao: closest.icao.clone(),
                source: TouchdownAirportSource::Nearest25Nm,
                distance_to_destination_nm: dist_to_planned(lat, lon),
                nearest_distance_nm: Some((closest.distance_m / 1852.0) as f32),
            };
        }
        return TouchdownAirportResolution {
            icao: planned_arr_airport.to_string(),
            source: TouchdownAirportSource::PlannedFallback,
            distance_to_destination_nm: dist_to_planned(lat, lon),
            nearest_distance_nm: None,
        };
    }

    TouchdownAirportResolution {
        icao: planned_arr_airport.to_string(),
        source: TouchdownAirportSource::PlannedFallback,
        distance_to_destination_nm: None,
        nearest_distance_nm: None,
    }
}

/// Divert markers for the MQTT PIREP payload. Pure function — testable
/// without a full `ActiveFlight`.
///
/// The distinction this function exists to hold:
///
///   * **fact** — `effective_arr_icao != planned_arr_icao`: the pilot
///     confirmed a divert and the PIREP is filed against a different arrival
///     airport. `divert = Some(true)`, `diverted_to = Some(effective)`.
///   * **suspicion** — the FSM raised a `DivertHint` but the pilot filed as
///     planned. `divert_suspected = Some(true)` (+ the suspected field), and
///     `divert = None`.
///
/// v0.19.3: these used to be the same thing. A hint alone set `divert = true`
/// in the payload, so the recorder's Discord bot announced "🔀 DIVERT filed"
/// for a flight phpVMS had recorded as a perfectly normal arrival — the two
/// systems permanently disagreed about the same flight (observed once in
/// production: DLH859 ENGM→EDDF, 2026-07-12). A suspicion the pilot declined
/// to act on is a diagnostic, not an event; only the pilot's confirmation
/// makes a divert real.
fn divert_payload_markers(
    effective_arr_icao: &str,
    planned_arr_icao: &str,
    divert_hint: Option<&DivertHint>,
) -> DivertMarkers {
    if effective_arr_icao != planned_arr_icao {
        DivertMarkers {
            divert: Some(true),
            diverted_to: Some(effective_arr_icao.to_string()),
            divert_suspected: None,
            divert_suspected_icao: None,
        }
    } else {
        DivertMarkers {
            divert: None,
            diverted_to: None,
            divert_suspected: divert_hint.map(|_| true),
            divert_suspected_icao: divert_hint.and_then(|h| h.actual_icao.clone()),
        }
    }
}

/// The four divert-related payload fields, as one value — so a caller cannot
/// wire the "suspected" pair into the "filed" pair by mixing up tuple slots.
struct DivertMarkers {
    divert: Option<bool>,
    diverted_to: Option<String>,
    divert_suspected: Option<bool>,
    divert_suspected_icao: Option<String>,
}

/// v0.20 (QS-Fix, DLH2064 2026-07-23 — root-cause version): `stats.
/// touchdown_events` only gets its `FinalLanding` entry appended if at
/// least one streamer tick observes `FlightPhase::Landing` (the push site
/// is nested inside that match arm, ~line 25486). A fast deceleration can
/// roll the FSM straight through to TaxiIn in the gap between two ticks,
/// so that arm's body never runs for this landing even though the
/// independent touchdown_v2/edge pipeline captured and scored it fine —
/// the audit vec is then incomplete (or entirely empty) despite a real
/// landing having happened.
///
/// EVERY PIREP-facing consumer that counts or lists `touchdown_events`
/// (the payload's `touchdown_count`, and both "Touchdowns" notes/custom-
/// field sections) must go through this helper instead of reading the raw
/// vec directly — fixing only one call site (as an earlier version of
/// this fix did) leaves the other consumers exposed to the exact same
/// incomplete-vec bug.
///
/// Deliberately gated on `canonical_landing_rate_fpm().is_some()`, NOT on
/// `landing_at.is_some()` — the latter is also set for a fallback_zero
/// flight (no real touchdown), and backfilling there would mask exactly
/// the signal the fallback_zero fix (Joel EWG3552) exists to surface.
fn effective_touchdown_events(stats: &FlightStats) -> std::borrow::Cow<'_, [TouchdownEvent]> {
    // Gated on "is there already a FinalLanding entry", NOT "is the vec
    // empty" — a flight with a real touch-and-go followed by a raced-out
    // final landing has a NON-empty vec (the T&G is in there) but is still
    // missing exactly the entry this whole helper exists to backfill.
    let has_final = stats
        .touchdown_events
        .iter()
        .any(|e| matches!(e.kind, TouchdownKind::FinalLanding));
    if has_final {
        return std::borrow::Cow::Borrowed(&stats.touchdown_events);
    }
    let Some(peak_vs_fpm) = stats.canonical_landing_rate_fpm() else {
        // Either no landing at all, or a fallback_zero one — stays as-is
        // (empty, or T&Gs only), which is the whole point of that fix.
        return std::borrow::Cow::Borrowed(&stats.touchdown_events);
    };
    // v0.20 (QS-Fix, gefunden im Adversarial-Review): `landing_at` wird
    // NUR im FSM-Tick-Pfad gesetzt (Approach/Final on_ground-Kante) — der
    // Sampler-Pfad (Bush/VFR-Kurzhops, siehe lib.rs ~19467) stampt ihn
    // nie, setzt aber `sampler_touchdown_at`. Ohne diesen Fallback haette
    // ein rein-Sampler-erfasster Touchdown hier die PIREP-Filing-Zeit
    // statt der echten Touchdown-Zeit bekommen — sichtbar wuerde das nur
    // im seltenen Fall T&G+gerastete Sampler-Landung, aber ehrlicher ist's
    // so oder so.
    let timestamp = stats
        .landing_at
        .or(stats.sampler_touchdown_at)
        .unwrap_or_else(Utc::now);
    let mut events = stats.touchdown_events.clone();
    events.push(TouchdownEvent {
        timestamp,
        kind: TouchdownKind::FinalLanding,
        peak_vs_fpm,
        peak_g: stats.canonical_peak_g_force().unwrap_or(0.0),
        lat: stats.landing_lat.unwrap_or_default(),
        lon: stats.landing_lon.unwrap_or_default(),
        sub_bounces: stats.bounce_count,
    });
    std::borrow::Cow::Owned(events)
}

/// Die Aufsetzer fuer den PIREP (20.10.2026, Stück 3) — und daraus der
/// haerteste. Nur bei mehr als einem Aufsetzer; bei einer einzigen Landung
/// steht alles schon in den `landing_*`-Feldern. Ungemessene Aufsetzer
/// (Sinkrate 0) zaehlen fuer den haertesten nicht.
fn aufsetzer_fuer_pirep(
    stats: &FlightStats,
) -> (Vec<aeroacars_mqtt::AufsetzerEintrag>, Option<TouchdownEvent>) {
    let events = effective_touchdown_events(stats);
    if events.len() < 2 {
        return (Vec::new(), None);
    }
    let eintraege = events
        .iter()
        .map(|e| aeroacars_mqtt::AufsetzerEintrag {
            ts_ms: e.timestamp.timestamp_millis(),
            art: match e.kind {
                TouchdownKind::TouchAndGo => "touch_and_go",
                TouchdownKind::FinalLanding => "final_landing",
            },
            vs_fpm: e.peak_vs_fpm,
            g: e.peak_g,
            lat: e.lat,
            lon: e.lon,
        })
        .collect();
    let haertester = events
        .iter()
        .filter(|e| e.peak_vs_fpm < 0.0)
        .min_by(|a, b| a.peak_vs_fpm.total_cmp(&b.peak_vs_fpm))
        .cloned();
    (eintraege, haertester)
}

/// Weicht der PIREP von dem ab, was als Landung an live ging (20.10.2026,
/// Stück 3)? Verglichen werden Aufsetz-Zeitpunkt (±2 s, wie der Recorder
/// zuordnet) und Sinkrate (ganze fpm, wie `landing_vs_fpm`). `None`, wenn
/// alles passt oder nichts gemeldet wurde (keine Leitung — dann holt das
/// Flugprotokoll die Landung nach).
///
/// Entscheid Thomas: Bei Abweichung geht der PIREP trotzdem mit dem EINEN
/// kanonischen Wert raus, der Recorder zieht live darauf und schlaegt Alarm.
/// Hier: laut ins Protokoll.
fn landung_widerspricht_meldung(stats: &FlightStats) -> Option<String> {
    let (gemeldet_ts, gemeldet_vs) = stats.gemeldete_landung?;
    let aufsetzen = stats.landing_at.or(stats.sampler_touchdown_at);
    let vs = stats.canonical_landing_rate_fpm().map(|v| v.round() as i32);
    match (aufsetzen, vs) {
        (Some(ts), _) if (ts.timestamp_millis() - gemeldet_ts).abs() > 2_000 => Some(format!(
            "live meldete den Aufsetzer {gemeldet_ts}, der PIREP traegt {}",
            ts.timestamp_millis()
        )),
        (None, _) => Some(format!("live meldete den Aufsetzer {gemeldet_ts}, der PIREP hat keinen")),
        (Some(_), Some(v)) if v != gemeldet_vs => Some(format!(
            "live meldete {gemeldet_vs} fpm, der PIREP traegt {v} fpm"
        )),
        (Some(_), None) => Some(format!(
            "live meldete {gemeldet_vs} fpm, der PIREP hat keine Sinkrate"
        )),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests_lib/effective_touchdown_events_tests.rs"]
mod effective_touchdown_events_tests;

/// v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE1): zentraler
/// MQTT-PirepPayload-Builder — eine Stelle für alle 4 Filing-Pfade
/// (normal /file, Divert, manuell, Queue-Worker). Vorher war dieser
/// ~200-Zeilen-Block inline im `flight_end`-Command dupliziert; Divert-
/// und Manual-Pfad publizierten gar kein MQTT-PIREP → Reports verschluckt.
///
/// `effective_arr_icao` = der tatsächliche Ankunftsflughafen (bei Divert
/// das Ausweich-ICAO, sonst == `planned_arr_icao`). `planned_arr_icao` =
/// der ursprünglich geplante Zielflughafen. Ist `effective != planned`,
/// werden `divert`/`diverted_to` gesetzt; Rollout-/Runway-Geometrie wird
/// gegen `effective_arr_icao` aufgelöst.
fn build_pirep_payload(
    flight: &ActiveFlight,
    body: &api_client::FileBody,
    effective_arr_icao: &str,
    planned_arr_icao: &str,
) -> aeroacars_mqtt::PirepPayload {
    let mut stats = flight.stats.lock().expect("flight stats");
    // v1.7.35: einmal rechnen, dann eingefroren — egal welcher Thread zuerst
    // kommt und egal, wer fragt.
    let sprit_auswertung = sprit_auswertung_einmal(&mut stats);
    let touchdown_count = effective_touchdown_events(&stats).len() as u32;
    let (aufsetzer, haertester_aufsetzer) = aufsetzer_fuer_pirep(&stats);
    if let Some(widerspruch) = landung_widerspricht_meldung(&stats) {
        tracing::error!(
            pirep_id = %flight.pirep_id,
            %widerspruch,
            "PIREP weicht von der Live-Meldung der Landung ab — der Recorder gleicht live an"
        );
    }
    // Divert-Marker via pure Helper: bestätigter Divert vs. bloßer Verdacht.
    let divert_markers = divert_payload_markers(
        effective_arr_icao,
        planned_arr_icao,
        stats.divert_hint.as_ref(),
    );
    // v0.7.1 P1.3-Fix: Sub-Scores einmal berechnen,
    // dann sowohl als sub_scores ins Payload als auch
    // als landing_score (gewichteter Aggregate) nutzen.
    let actual_burn = actual_burn_for_record(&stats);
    // Codex-Befund zu P-4.40: die Rechnung dieser Einreichung, nicht eine
    // eigene (siehe `einreichungs_rechnung`).
    let rechnung = einreichungs_rechnung(flight, &stats, effective_arr_icao);
    let bewertet = rechnung.ergebnis.clone();
    let payload_sub_scores = bewertet.as_ref().map(|(a, _)| a.teilnoten.clone()).unwrap_or_default();
    let aggregate_master = bewertet.as_ref().and_then(|(a, _)| a.gesamt);
    // Score-Version 19: kein Rueckfall auf die Touchdown-Klasse — sie kennt
    // keinen Deckel. Ohne Gesamtnote (keine Sinkrate) gibt es keine Note.
    let payload_landing_score = aggregate_master.map(|m| m as i32);
    let (eingabe_seq, eingabe_sha256) = pirep_eingabe_kennung(flight, &stats, &rechnung);
    aeroacars_mqtt::PirepPayload {
        ts: Utc::now().timestamp_millis(),
        // v0.11.1: Pilot-Client-Version mitsenden
        // damit Reports-Uebersicht die Pill ohne
        // Touchdown-Join zeigen kann.
        client_version: Some(env!("CARGO_PKG_VERSION")),
        // v3 (P-6.03): Paketstand dieses Flugs (Flug-Pin, P-3.31).
        runtime_bundle_id: laufzeit_paket::flug_pin(&flight.pirep_id).map(|id| id.to_string()),
        pirep_id: flight.pirep_id.clone(),
        flight_number: format_callsign(&flight.airline_icao, &flight.flight_number),
        dep: flight.dpt_airport.clone(),
        arr: effective_arr_icao.to_string(),
        block_time_min: body.flight_time,
        flugzeiten: Some(flugzeiten(&stats)),
        flight_time_min: body.flight_time,
        distance_nm: body.distance.map(|d| d as f32),
        // v0.12.5 (LE4, QS-R2-P2): `body.fuel_used` ist die phpVMS-Site-
        // Unit = POUND. Das MQTT-Feld heißt `fuel_used_kg` — also explizit
        // lb→kg zurückrechnen. Vorher floss der lb-Wert ungewandelt in ein
        // `_kg`-Feld (Recorder/Web sah ~2,2× zu viel „kg").
        fuel_used_kg: body.fuel_used.map(|lb| (lb / KG_TO_LB) as f32),
        planned_burn_kg: stats.planned_burn_kg,
        block_fuel_kg: stats.block_fuel_kg,
        takeoff_fuel_kg: stats.takeoff_fuel_kg,
        landing_fuel_kg: stats.landing_fuel_kg,
        // v0.7.6 P1-1: Trip-Burn als SSoT fuer OFP-Vergleich.
        actual_trip_burn_kg: actual_burn,
        takeoff_weight_kg: stats.takeoff_weight_kg.map(|w| w as f32),
        landing_weight_kg: stats.landing_weight_kg.map(|w| w as f32),
        planned_tow_kg: stats.planned_tow_kg,
        planned_ldw_kg: stats.planned_ldw_kg,
        planned_block_fuel_kg: stats.planned_block_fuel_kg,
        // QS 06.10.2026: wie `build_landing_record` — damit die Webapp
        // dieselben Zahlen zeigt (siehe Feld-Doku in aeroacars-mqtt).
        flight_ident: Some(resolve_flight_ident(
            &flight.flight_number,
            flight.bid_callsign.as_deref(),
        ))
        .filter(|s| !s.trim().is_empty()),
        // Dieselbe Quelle wie `build_landing_record`.
        aircraft_registration: Some(flight.planned_registration.clone()).filter(|s| !s.is_empty()),
        landing_scored_g_force: score_g_for_stats(&stats).map(|s| s.scored_g),
        landing_peak_vs_fpm: stats.landing_peak_vs_fpm,
        landing_bank_deg: aufsetz_bank_deg(&stats),
        fenster_unzureichend: stats.landung_abdeckung_fehlt.is_some().then_some(true),
        peak_altitude_ft: stats.peak_altitude_ft.map(|v| v.round() as i32),
        // P1/9 (22.10.2026, GlitchTip 209): RUNDEN wie Anzeige, MQTT-Touchdown
        // und phpVMS-Feld. Vorher `body.landing_rate.map(|r| r as i32)` —
        // abgeschnitten, bei jeder zweiten Landung 1 fpm anders als der Client.
        landing_vs_fpm: stats.canonical_landing_rate_fpm().map(|v| v.round() as i32),
        // v0.7.1 P1.3-Fix: gewichteter Aggregate-Score
        // (Fuel/Loadsheet/Stability/Rollout fliessen ein),
        // mit Fallback auf Touchdown-Klassifikation.
        landing_score: payload_landing_score,
        // v0.20.0: Klasse und Note EINGEFROREN zum Score mitgeben — dieselben
        // beiden Funktionen, die auch der Landungs-Tab und die PIREP-Notizen
        // benutzen. Die Webapp leitete das Wort bisher selbst aus der Zahl ab,
        // mit einer eigenen Leiter. Zwei Kopien einer Regel driften; hier
        // gibt es nur noch eine.
        landing_score_label: payload_landing_score
            .and(bewertet.as_ref().and_then(|(a, _)| a.gesamt_wort.clone())),
        // v3 (P-4.21): Wort und Buchstabe aus der Modul-Ausgabe.
        landing_score_grade: bewertet.as_ref().and_then(|(a, _)| a.gesamt_buchstabe.clone()),
        // Nur wenn es eine Gesamtnote gibt (seit Score-Version 19 ohne
        // Touchdown-Rückfall) und der Deckel sie wirklich gesenkt hat.
        landing_score_deckel: aggregate_master
            .and_then(|_| bewertet.as_ref().and_then(|(a, _)| a.deckel.clone())),
        go_around_count: Some(stats.go_around_count),
        touchdown_count: Some(touchdown_count),
        aufsetzer,
        haertester_aufsetzer_fpm: haertester_aufsetzer.as_ref().map(|e| e.peak_vs_fpm),
        haertester_aufsetzer_g: haertester_aufsetzer.as_ref().map(|e| e.peak_g),
        dep_gate: stats.dep_gate.clone(),
        // Nur der Standplatz des Flughafens, gegen den wirklich eingereicht
        // wird — siehe `arr_gate_for`.
        arr_gate: arr_gate_for(&stats, effective_arr_icao),
        approach_runway: stats.approach_runway.clone(),
        // Bestätigter Divert (Tatsache) vs. FSM-Verdacht (Diagnose) — strikt
        // getrennt, siehe `divert_payload_markers`.
        divert: divert_markers.divert,
        diverted_to: divert_markers.diverted_to,
        divert_suspected: divert_markers.divert_suspected,
        divert_suspected_icao: divert_markers.divert_suspected_icao,
        notes: None,
        // v0.7.0 — Touchdown-Forensik v2 marker (P2 fix)
        forensics_version: touchdown_v2::FORENSICS_VERSION,

        // ─── v0.7.1 Felder (Spec §5.1) ────────────────
        ux_version: 1,
        landing_confidence: stats.landing_confidence.clone(),
        landing_source: stats.landing_source.clone(),
        landung_nicht_bewertbar: landung_nicht_bewertbar_json(&stats),
        sampler_diagnose: stats.sampler_diagnose.clone(),
        // F6: Flare-Felder aus dem 50-Hz-Sampler-Buffer
        // (landing_analysis ist Option<Value>)
        flare_detected: stats
            .landing_analysis
            .as_ref()
            .and_then(|v| v.get("flare_detected"))
            .and_then(|v| v.as_bool()),
        flare_reduction_fpm: stats
            .landing_analysis
            .as_ref()
            .and_then(|v| v.get("flare_reduction_fpm"))
            .and_then(|v| v.as_f64())
            .map(|x| x as f32),
        flare_quality_score: stats
            .landing_analysis
            .as_ref()
            .and_then(|v| v.get("flare_quality_score"))
            .and_then(|v| v.as_i64())
            .map(|x| x.clamp(0, 100) as u8),
        // F7: Stability-v2-Felder (P2.1-A: bestehende
        // Backend-Felder exponieren)
        approach_vs_stddev_fpm: stats.canonical_vs_stddev_fpm(),
        approach_bank_stddev_deg: stats.canonical_bank_stddev_deg(),
        approach_vs_jerk_fpm: stats.approach_vs_jerk_fpm,
        approach_ias_stddev_kt: stats.approach_ias_stddev_kt,
        approach_stable_config: stats.approach_stable_config,
        approach_excessive_sink: stats.approach_excessive_sink,
        // v0.7.1 Round-2 P2-Fix: gate_window aus echten
        // Approach-Samples + TD-Timestamp gerechnet.
        gate_window: build_mqtt_gate_window_from_stats(&stats),
        // P1.5 + Phase 2 (F1/F2/F3) + P1.3-Fix:
        // bereits oben berechnet, hier nur durchreichen.
        sub_scores: payload_sub_scores,
        sprit: sprit_auswertung,
        // v0.7.6 P1-3: Runway-Geometry-Trust. Pure-
        // function Check + reason-string ins Payload.
        // v1.7.8: Woher die Bahngeometrie kam. Ohne diese drei Felder
        // waere die Umstellung im Bestand unsichtbar — und ein
        // Quellenwechsel, den niemand nachmessen kann, ist ein Glaube.
        bahn_geometrie_quelle: Some(
            match stats.szenerie_uebernahme.as_ref() {
                Some(b) if !b.uebernommen.is_empty() => "szenerie",
                _ => "navdaten",
            }
            .to_string(),
        ),
        bahn_szenerie_status: Some(szenerie_status(&stats)),
        sim_kennung: stats.sim_kennung.clone(),
        bahn_kurs_korrektur_grad: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_kursabweichung_grad),
        bahn_breiten_korrektur_m: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_breitenabweichung_m),
        bahn_schwellen_korrektur_m: stats
            .szenerie_uebernahme
            .as_ref()
            .filter(|b| !b.uebernommen.is_empty())
            .map(|b| b.groesste_schwellenabweichung_m),
        runway_geometry_trusted: {
            let (trusted, _) = runway_geometry_trust_check(
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.airport_ident.as_str()),
                effective_arr_icao,
                stats
                    .divert_hint
                    .as_ref()
                    .and_then(|h| h.actual_icao.as_deref()),
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.centerline_distance_m as f32),
                stats.landing_float_distance_m,
            );
            Some(trusted)
        },
        runway_geometry_reason: {
            let (_, reason) = runway_geometry_trust_check(
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.airport_ident.as_str()),
                effective_arr_icao,
                stats
                    .divert_hint
                    .as_ref()
                    .and_then(|h| h.actual_icao.as_deref()),
                stats
                    .runway_match
                    .as_ref()
                    .map(|m| m.centerline_distance_m as f32),
                stats.landing_float_distance_m,
            );
            reason.map(String::from)
        },
        // v0.7.19 GAF-707 Accident-Detection: Accident-Felder
        // gelatcht; Sentinel wird IMMER gesetzt.
        accident_classifier_version: Some(accident::ACCIDENT_CLASSIFIER_VERSION.into()),
        accident: if stats.accident_detected {
            Some(true)
        } else {
            None
        },
        accident_kind: stats.accident_kind.clone(),
        accident_confidence: stats.accident_confidence.clone(),
        accident_reasons: if stats.accident_reasons.is_empty() {
            None
        } else {
            Some(stats.accident_reasons.clone())
        },
        accident_at: stats.accident_at.map(|t| t.timestamp_millis()),
        // v0.10.0 (#runway-utilization-score): markiert
        // dass die sub_scores oben vom LDA-basierten
        // Bahn-Auslastungs-Algorithmus stammen.
        // v0.16.21: bump 3→4 — MSFS touchdown V/S SimVar-lag corrected
        // (g-force-gated AGL de-lag; MSFS only, X-Plane unchanged).
        // v0.20.x: bump 4→5 — Bahnauslastung-QS (Float-Toleranz +
        // Banding grosszuegiger) + Sinkraten-Ziel-Korridor.
        // v1.6.2: bump 5→6 — achte Achse „Ausrichtung" (Versatz zur
        // Mittellinie an der Bahnbreite gemessen + Kursabweichung).
        // v1.6.3: bump 6→7 — Sinkrate kommt aus der Hoehenkurve statt aus
        // dem gedaempften Sim-Messwert, plus Seitenwind-Kompensation der
        // Ausrichtungs-Achse. Beides veraendert Punkte.
        // Forward-only wie alle Vorgaenger: alte Fluege behalten ihren
        // gespeicherten Score, es wird nichts nachgerechnet.
        // v1.6.8: bump 8→9 — die nutzbare Bahnlaenge endet an der
        // versetzten Schwelle; Aufsetzzone und Aufsetzpunkt-Markierung
        // folgen ihr ebenfalls (ICAO Annex 14).
        // v1.6.7: bump 7→8 — zwei Achsen. Sprit: Minderverbrauch bis 15 %
        // unter Plan gibt volle Punkte (vorher 95 ab 5 %). Bahn-Auslastung:
        // gewertet wird die tatsaechlich genutzte Bahnstrecke gegen die LDA
        // (Baender 60/70/80/90 %), keine toleranzbereinigte Zweitgroesse
        // mehr — siehe `sub_rollout_v2`.
        score_algorithm_version: Some(SCORE_ALGORITHMUS_VERSION),
        bewertung_status: Some(crate::bewertung::status_text(&bewertet).to_string()),
        // v3 (P-5.10): Nummer und Hash des Eingabe-Dokuments der Landung.
        seq: eingabe_seq,
        input_sha256: eingabe_sha256,
        client_health: build_client_health_report(&stats),
        // Lernpaket AP4/AP5 (Entscheid Thomas 29.09.2026): Forensik ohne
        // Note, im finalen Stand — `apply_finalized_runway_correlation` lief
        // beim Bau des Bodys, ein Bahnwechsel ist damit schon nachgezogen.
        anflug_gleitpfad: stats.anflug_forensik.gleitpfad.clone(),
        anflug_ruhe: stats.anflug_forensik.ruhe.clone(),
        abfangen: abfangen_aus_analyse(&stats),
    }
}

/// v0.20 (Process-Integrity): assembles `ClientHealthReport` from whatever
/// `disconnect_sim_liveness` / `app_restart_was_unclean` / `resume_discontinuity`
/// this flight accumulated (Phases 1-3). Returns `None` when NONE of them
/// are set — the overwhelming majority of flights — so `client_health`
/// itself is omitted from the wire via `skip_serializing_if`.
fn build_client_health_report(stats: &FlightStats) -> Option<aeroacars_mqtt::ClientHealthReport> {
    let disconnect_sim_liveness = stats
        .disconnect_sim_liveness
        .map(|l| l.as_wire_str().to_string());
    let impossible_resume_jump = stats.resume_discontinuity.map(|_| true);
    // Bei einer blossen Bestaetigung („Trotzdem fortsetzen") sind Sprit-
    // und Hoehendelta konstruktionsbedingt 0 — keine Messwerte, also nicht
    // melden (Cloud-QS 23.09.2026, dritte Runde).
    let gemessen = stats.resume_discontinuity.filter(|d| !d.nur_bestaetigt);
    let resume_fuel_delta_kg = gemessen.map(|d| d.fuel_delta_kg as f32);
    let resume_altitude_delta_ft = gemessen.map(|d| d.altitude_delta_ft as f32);

    // v0.20 (QS-Fix, Finding 3): `Some(false)` (a perfectly CLEAN app
    // restart) must count as "nothing to report", same as `None` — only
    // `Some(true)` (an actually unclean restart) is worth shipping.
    // `stats.app_restart_was_unclean.is_none()` alone would have let
    // `Some(false)` defeat this guard, so EVERY resumed-but-uneventful
    // flight shipped a non-`None` `client_health`.
    let app_restart_unclean_flag = stats.app_restart_was_unclean == Some(true);
    // Ein Muster ohne Eintrag in der Grenzen-Tabelle ist meldenswert, auch
    // wenn der Flug sonst voellig unauffaellig war: die Bewertung lief auf
    // Ersatzwerten. Ohne diesen Zweig bliebe die Luecke stumm — genau der
    // Zustand, der 470 Fluege lang niemandem aufgefallen ist.
    let unknown_aircraft_icao = stats.unbekanntes_muster.clone();

    if disconnect_sim_liveness.is_none()
        && !app_restart_unclean_flag
        && impossible_resume_jump.is_none()
        && unknown_aircraft_icao.is_none()
    {
        return None;
    }

    Some(aeroacars_mqtt::ClientHealthReport {
        unknown_aircraft_icao,
        disconnect_sim_liveness,
        app_restart_unclean: stats.app_restart_was_unclean,
        impossible_resume_jump,
        resume_fuel_delta_kg,
        resume_altitude_delta_ft,
        resume_gap_minutes: stats.resume_gap_minutes,
    })
}

#[cfg(test)]
#[path = "tests_lib/client_health_report_tests.rs"]
mod client_health_report_tests;

/// v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE1): zentraler
/// Filing-Abschluss — schreibt das PIREP-Forensik-Event ins JSONL und
/// publisht das PIREP via MQTT. Arbeitet bewusst auf der bereits
/// serialisierten JSON-Form (`serde_json::Value`), damit der PIREP-Queue-
/// Worker — der nur die persistierte JSON-Form besitzt — exakt denselben
/// Abschluss-Pfad nutzt wie die Live-Pfade. Vorher publizierten der
/// Divert-, der Manual- und der Queue-Pfad GAR KEIN MQTT-PIREP →
/// Reports/Logfile verschluckt (Bug A/B/C der Spec).
///
/// QS-P1: `handle` ist **`Option`**. Das `PirepFiled`-JSONL-Event MUSS
/// immer geschrieben werden — auch ohne MQTT-Verbindung — sonst kann der
/// Recorder-Importer den PIREP nicht aus dem hochgeladenen JSONL nachlegen
/// (`importer.ts` legt `pireps` nur bei `pirep_filed` an). MQTT-Publish ist
/// best-effort und entfällt sauber, wenn kein Handle da ist.
/// Legt den offenen Bahn-Nachtrag ab, BEVOR ein Flugbericht eingereicht
/// wird — auf JEDEM Einreichweg.
///
/// # Warum vor dem Einreichen
///
/// Nach einem erfolgreichen `/file` loescht der Client den gespeicherten
/// Flug. Stirbt er zwischen beidem, gibt es ohne diese Ablage nichts mehr,
/// das die Korrektur je schicken koennte: Der Flug ist weg, der Trichter
/// lief nie (Codex, 03.09.2026, Runde 14 und 15). Runde 14 hatte das nur
/// in `flight_end` eingebaut; der manuelle Weg (auch der manuelle Divert)
/// behielt das Fenster.
///
/// # Warum ein Fehler das Einreichen NICHT aufhaelt
///
/// Thomas' Entscheidung (03.09.2026): Der Flugbericht des Piloten ist
/// wichtiger als die Bahnkorrektur. Ist die Platte voll oder die Ablage
/// nicht beschreibbar, wird trotzdem eingereicht und der Verlust laut
/// gemeldet — statt einen Piloten mit vollem Datentraeger seinen Flug
/// nicht einreichen zu lassen.
///
/// Doppelt abgelegt schadet nicht: `senden` schreibt dieselbe Datei
/// (Kennung + Aufsetzzeit + Revision) noch einmal, atomar und mit
/// gleichem Inhalt, und der Recorder riegelt ueber die Revision.
///
/// Bis v1.7.40 ging die Landung blind raus. Riss die Leitung im
/// Aufsetzmoment — und genau dann riss sie bei drei Flügen am 20.09.2026,
/// rund zehn Sekunden nach dem Aufsetzen —, war sie weg. Der Server sah
/// einen Flugbericht ohne Landung, hielt ihn für unvollständig und stellte
/// ihn in die Prüfliste, während der Pilot eine Note sah, die nie ankam.
/// Wie lange auf die Bestätigung der Landung gewartet wird, bevor der
/// Flugbericht trotzdem rausgeht. Der Bericht ist wichtiger als seine
/// Zustellmeldung — aber die Reihenfolge soll stimmen, wenn es geht.
const LANDUNG_ACK_FRIST: std::time::Duration = std::time::Duration::from_secs(15);

/// Hält fest, ob der Broker die Landung bestätigt hat.
///
/// Bis v1.7.40 ging die Landung blind raus. Riss die Leitung im
/// Aufsetzmoment, war sie weg, und niemand merkte es.
fn landung_zustellung_vermerken(app: &AppHandle, pirep_id: &str, ok: bool) {
    let state = app.state::<AppState>();
    let guard = state.active_flight.lock().expect("active_flight lock");
    let Some(flight) = guard.as_ref() else {
        return;
    };
    if flight.pirep_id != pirep_id {
        return;
    }
    let mut stats = flight.stats.lock().expect("flight stats lock");
    stats.landung_live_bestaetigt = ok;
    if ok {
        // Angekommen — die Kopie für den zweiten Versuch wird nicht mehr
        // gebraucht.
        stats.landung_payload = None;
        tracing::info!(pirep_id = %pirep_id, "Landung vom Server bestaetigt");
    } else {
        tracing::warn!(
            pirep_id = %pirep_id,
            "Landung NICHT bestaetigt — zweiter Versuch beim Einreichen"
        );
    }
}

/// Schickt die Landung vor dem Einreichen erneut, wenn sie nicht
/// bestätigt wurde — und sagt es dem Piloten, wenn auch das nicht klappt.
///
/// Reihenfolge ist wichtig: erst die Landung, dann der Flugbericht. Trifft
/// der Bericht ohne Landung ein, fällt das Urteil „keine Landung
/// aufgezeichnet", und das lässt sich später nicht mehr umdrehen, sobald
/// die Webseite es abgeholt hat (AAL 1331).
///
/// Die Wiederholung ist ungefährlich: Der Recorder erkennt dieselbe
/// Landung an Pilot und Zeitstempel und legt keine zweite Zeile an.
async fn landung_vor_dem_einreichen_nachschicken(app: &AppHandle, flight: &ActiveFlight) {
    let payload = {
        let stats = flight.stats.lock().expect("flight stats lock");
        if stats.landung_live_bestaetigt {
            return;
        }
        stats.landung_payload.as_ref().map(|p| (**p).clone())
    };
    // Kein Aufsetzen aufgezeichnet (Abbruch, Ausweichflug ohne Landung):
    // Dann gibt es nichts nachzuschicken.
    let Some(payload) = payload else {
        return;
    };
    let ack = {
        let state = app.state::<AppState>();
        let mqtt = state.mqtt.lock().await;
        match mqtt.as_ref() {
            Some(handle) => Some(handle.touchdown_bestaetigt(payload)),
            None => None,
        }
    };
    let ok = match ack {
        Some(rx) => tokio::time::timeout(std::time::Duration::from_secs(15), rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(false),
        None => false,
    };
    landung_zustellung_vermerken(app, &flight.pirep_id, ok);
    if ok {
        tracing::info!(
            pirep_id = %flight.pirep_id,
            "Landung beim Einreichen nachgeschickt und bestaetigt"
        );
        return;
    }
    // Ehrlich bleiben: Der Pilot erfährt, dass seine Landung nicht durchkam.
    // Verloren ist sie nicht — das hochgeladene Flugprotokoll trägt sie nach,
    // nur dauert die Bewertung dann länger.
    log_activity_handle(
        app,
        ActivityLevel::Warn,
        "Landung konnte nicht an den Server gemeldet werden",
        Some(
            "Die Verbindung stand beim Aufsetzen nicht. Der Flugbericht wird trotzdem \
             eingereicht; die Landung reicht das Flugprotokoll nach. Die Bewertung auf \
             der Webseite kann dadurch spaeter erscheinen."
                .to_string(),
        ),
    );
}

fn nachtrag_vor_dem_einreichen_sichern(app: &AppHandle, flight: &ActiveFlight) {
    let offener = {
        let st = flight.stats.lock().expect("flight stats");
        if st.bahn_nachtrag_offen {
            bahn_nachtrag_bauen(flight, &st)
        } else {
            None
        }
    };
    let Some(n) = offener else {
        return;
    };
    match nachtrag_queue::enqueue_offline(app, &n) {
        Ok(p) => tracing::info!(
            pirep_id = %flight.pirep_id,
            pfad = %p.display(),
            "Bahn-Nachtrag vor dem Einreichen abgelegt"
        ),
        Err(e) => tracing::error!(
            pirep_id = %flight.pirep_id,
            error = %e,
            "Bahn-Nachtrag vor dem Einreichen NICHT abgelegt — Einreichen laeuft trotzdem \
             (der Flugbericht geht vor), die Korrektur geht verloren"
        ),
    }
}

fn finalize_filed_pirep(
    app: &AppHandle,
    handle: Option<&aeroacars_mqtt::Handle>,
    pirep_id: &str,
    pirep_payload_json: serde_json::Value,
    // ⚠ Der Flug, falls es ihn noch gibt — fuer den offenen
    // Bahn-Nachtrag. `None` beim Nachreichen aus der Warteschlange: Dort
    // gibt es keinen laufenden Flug mehr; sein Nachtrag kommt dann ueber
    // `gequeuter_nachtrag`.
    flight: Option<&ActiveFlight>,
    // ⚠ Der Nachtrag aus der PIREP-Warteschlange (N6): Beim Offline-
    // Einreichen wird der Flug verworfen, bevor der Worker sendet. Der
    // Nachtrag wird deshalb beim Einreihen gebaut und hier mitgegeben.
    gequeuter_nachtrag: Option<serde_json::Value>,
) {
    // ⚠ ZUERST der Bahn-Nachtrag, dann das PIREP.
    //
    // Das Navdaten-Upgrade beim Einreichen ersetzt dieselben Bahnfelder
    // wie das Nachholen: Ist die Abfrage eines Ausweichplatzes inzwischen
    // zurueckgekommen, steht hier die bessere Geometrie. Der Streamer ist
    // an dieser Stelle schon beendet — ohne diese Zeilen bliebe die
    // Touchdown-Zeile beim Recorder auf der vorlaeufigen
    // OurAirports-Geometrie, denn vom PIREP uebernimmt er nur Punkte und
    // Noten.
    if let Some(flight) = flight {
        // ⚠ Die Fahne faellt erst, wenn es einen Handle gibt — dieselbe
        // Regel wie im Streamer. Vorher fiel sie beim Bauen, und ohne
        // MQTT ging die Korrektur ohne jede Logzeile verloren (externe
        // QS, 02.09.2026, N4).
        let nachtrag = {
            let mut stats = flight.stats.lock().expect("flight stats");
            match (stats.bahn_nachtrag_offen, handle.is_some()) {
                (true, true) => {
                    // Ablage VOR der Fahne (Runde 14, High 1): Der Flug ist
                    // eingereicht, einen naechsten Tick gibt es nicht — was
                    // hier nicht auf der Platte liegt, ist weg. Die Fahne
                    // faellt nur, wenn die Datei liegt; `senden` schreibt
                    // dieselbe Datei danach nochmal (atomar, gleicher Inhalt).
                    match bahn_nachtrag_bauen(flight, &stats) {
                        Some(n) => {
                            let s = handle.as_ref().expect("Handle geprueft").nachtrag_sender();
                            match nachtrag_queue::enqueue(app, &s.va_prefix, &s.pilot_id, &n) {
                                Ok(_) => {
                                    stats.bahn_nachtrag_offen = false;
                                    Some(n)
                                }
                                Err(e) => {
                                    tracing::error!(
                                        pirep_id,
                                        error = %e,
                                        "Bahnkorrektur beim Einreichen: Ablage nicht beschreibbar — \
                                         die Korrektur geht verloren (Flug ist eingereicht)"
                                    );
                                    None
                                }
                            }
                        }
                        None => None,
                    }
                }
                (true, false) => {
                    // Kein Handle — in die Ablage, nicht verlieren. Die Fahne
                    // faellt nur, wenn der Nachtrag DAUERHAFT liegt (Codex,
                    // 03.09.2026, P1).
                    match bahn_nachtrag_bauen(flight, &stats) {
                        Some(n) => match nachtrag_queue::enqueue_offline(app, &n) {
                            Ok(_) => {
                                stats.bahn_nachtrag_offen = false;
                                tracing::warn!(
                                    pirep_id,
                                    revision = stats.bahn_revision,
                                    "Bahnkorrektur beim Einreichen: keine MQTT-Verbindung — \
                                     in der Ablage, wird nachgeschickt"
                                );
                            }
                            Err(e) => tracing::error!(
                                pirep_id,
                                error = %e,
                                "Bahnkorrektur beim Einreichen: keine MQTT-Verbindung UND \
                                 Ablage nicht beschreibbar — die Korrektur geht verloren"
                            ),
                        },
                        None => tracing::warn!(
                            pirep_id,
                            "Bahnkorrektur ohne Aufsetzzeit — nichts zu schicken"
                        ),
                    }
                    None
                }
                (false, _) => None,
            }
        };
        if let (Some(handle), Some(nachtrag)) = (handle, nachtrag) {
            tracing::info!(
                pirep_id,
                revision = ?nachtrag.herkunft.bahn_revision,
                "Bahnkorrektur beim Einreichen nachgeschickt"
            );
            // Ohne Ablage kein Senden; der Flug ist eingereicht, ein zweites
            // Fahrzeug fuer den Nachtrag gibt es nicht — laut sagen.
            if let Err(e) = nachtrag_queue::senden(app, &handle.nachtrag_sender(), nachtrag) {
                tracing::error!(pirep_id, error = %e, "Bahn-Nachtrag beim Einreichen nicht abgelegt — verloren");
            }
        }
    }
    if let Some(json) = gequeuter_nachtrag {
        match (
            handle,
            aeroacars_mqtt::TouchdownRolloutFinalizedPayload::aus_json(json),
        ) {
            (Some(handle), Ok(nachtrag)) => {
                tracing::info!(
                    pirep_id,
                    revision = ?nachtrag.herkunft.bahn_revision,
                    "Bahnkorrektur aus der Warteschlange nachgeschickt"
                );
                // Ohne Ablage kein Senden; der Flug ist eingereicht, ein zweites
                // Fahrzeug fuer den Nachtrag gibt es nicht — laut sagen.
                if let Err(e) = nachtrag_queue::senden(app, &handle.nachtrag_sender(), nachtrag) {
                    tracing::error!(pirep_id, error = %e, "Bahn-Nachtrag beim Einreichen nicht abgelegt — verloren");
                }
            }
            (None, Ok(nachtrag)) => match nachtrag_queue::enqueue_offline(app, &nachtrag) {
                Ok(_) => tracing::warn!(
                    pirep_id,
                    "Bahnkorrektur aus der Warteschlange: keine MQTT-Verbindung — in der Ablage"
                ),
                Err(e) => tracing::error!(
                    pirep_id,
                    error = %e,
                    "Bahnkorrektur aus der Warteschlange: keine MQTT-Verbindung UND Ablage \
                     nicht beschreibbar"
                ),
            },
            (_, Err(e)) => tracing::warn!(
                pirep_id,
                error = %e,
                "Bahnkorrektur aus der Warteschlange ist nicht lesbar"
            ),
        }
    }
    // v0.5.34: PIREP-Forensik ins JSONL — damit das Recovery-Tool
    // Pireps + abhaengige Stats rekonstruieren kann. IMMER, unabhängig
    // vom MQTT-Status (QS-P1: JSONL-Gap-Fill-Durabilität).
    record_event(
        app,
        pirep_id,
        &FlightLogEvent::PirepFiled {
            timestamp: Utc::now(),
            payload: pirep_payload_json.clone(),
        },
    );
    // MQTT-Publish nur wenn eine Verbindung steht — best-effort.
    if let Some(handle) = handle {
        handle.pirep_json(pirep_payload_json);
    }
}

/// v0.12.5 (LE7-QS): emittiert das `LandingFinalized`-JSONL-Event mit dem
/// finalen V/S + Score (Spec touchdown-forensics-v2 §7.2). Gerufen vom
/// Normal-, Divert- UND Manual-Filing-Pfad — alle drei haben eine lebende
/// `ActiveFlight`. Der PIREP-Queue-Worker kann es nicht: dort ist die
/// `ActiveFlight` beim Queueing bereits verworfen.
fn emit_landing_finalized(app: &AppHandle, flight: &ActiveFlight) {
    // Lernpaket AP4/AP5: Hat ein Bahnwechsel die Anflug-Forensik nach dem
    // `landing_analysis`-Ereignis neu gerechnet, den korrigierten Stand ins
    // Log schreiben — VOR dem Abschluss-Ereignis.
    anflug_forensik_nachtrag_schreiben(app, flight);
    // Aktuell: nimmt den letzten validated TD-Score (single-shot, bis
    // Multi-TD-Episodes voll integriert sind).
    let (final_vs, final_score_label) = {
        let s = flight.stats.lock().expect("flight stats");
        (
            // v0.20.0: kanonisch — dieses Forensik-Event ist die Zahl, gegen
            // die wir spaeter Pilot-Reports rekonstruieren. Sie MUSS die
            // gleiche sein wie in Log, Karte, Score und PIREP.
            s.canonical_landing_rate_fpm(),
            s.landing_score.map(|sc| format!("{:?}", sc)),
        )
    };
    record_event(
        app,
        &flight.pirep_id,
        &FlightLogEvent::LandingFinalized {
            timestamp: Utc::now(),
            forensics_version: touchdown_v2::FORENSICS_VERSION,
            final_vs_fpm: final_vs,
            final_score: final_score_label,
        },
    );
}
