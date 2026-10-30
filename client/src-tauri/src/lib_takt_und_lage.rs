/// Phase-dependent position-post cadence. Ground manoeuvres (taxi, takeoff,
/// landing rollout) and the approach are where pilots want a precise trail
/// on the live map; cruise is straight-and-level so a sparse sample is fine
/// and saves API budget. Spec §10 ("configurable intervals") — these are
/// the defaults; a future settings panel can override.
/// v0.5.21: MQTT live-tracking publish interval. Constant 3 s
/// regardless of phase — gives the live-tracking server 10x more
/// position points in cruise (was 30 s) for smoother live maps and
/// finer post-flight analytics. Decoupled from phpVMS POST cadence
/// (which stays phase-aware via `position_interval`) so the phpVMS
/// DB doesn't get hammered.
///
/// Per-pilot bandwidth at 3 s: ~1.3 MB/h with WSS+TLS overhead;
/// for a 5 h flight that's less than 1 minute of YouTube traffic.
/// VPS handles it trivially even with 200 concurrent pilots.
const MQTT_PUBLISH_INTERVAL_SECS: u64 = 3;

// v0.5.35: Adaptive Tick-Cadence. Standardmäßig 3s, aber im
// Approach/Final/Landing/Takeoff schneller — sonst verfehlt das
// Sampling den Touchdown bei langsamen GA-Flugzeugen.
//
// v0.5.45: Schwellen verschoben damit der mittlere Approach
// (1000-2000 ft AGL) nicht mehr auf 3s default faellt. Volanta/DLHv
// messen konstant bei 1 Hz oder hoeher — bei -800 fpm und 3s
// Lücke verlieren wir 42 ft Δ AGL zwischen Samples (User-Report
// DLH 1731 / CFG 9746 LDZA→EDDM, Sample-Cadence 3.5s im Final).
//
// Neue Skala:
//   AGL <100 ft   → 500 ms (2 Hz)   — Flare / Wheels-Up
//   AGL <500 ft   → 750 ms (1.3 Hz) — kurz vor TD
//   AGL <1500 ft  → 1000 ms (1 Hz)  — Final-Approach (war: 1000-500ft = 1s)
//   AGL <2000 ft  → 1500 ms         — kurz vor Final
//   sonst         → 3000 ms (default)
// Abtastintervall waehrend des Ausrollens.
//
// # Warum das Ausrollen ein eigenes Intervall braucht
//
// Der Rollweg lief bisher mit denselben 2 Hz wie der Flare. Bei 100 kt
// (51 m/s) ist das ein Messpunkt alle **25 Meter** — und damit eine Spur,
// die aus fuenfzehn bis dreissig Punkten besteht. Fuer die Frage „wie weit
// war das Rad von der Kante" reicht das gerade so; fuer die Frage „wie hat
// sich das Flugzeug auf der Bahn verhalten" nicht. Ein Ausbrechen und
// Zurueckziehen ueber vierzig Meter faellt zwischen zwei Punkte.
//
// Bei 5 Hz sind es zehn Meter je Punkt — genau der Mindestabstand, mit dem
// `bahndisziplin_tick` die Spur ohnehin ausduennt. Damit ist die
// Aufzeichnung so fein, wie die Ablage es zulaesst, und nicht feiner.
//
// # Was das kostet
//
// Der Tick selbst ist billig: Er liest einen Schnappschuss und rechnet.
// Die teuren Wege — MQTT-Publish und der phpVMS-Punktabstand — haengen
// seit v0.6.0 bzw. v1.6.14 an eigenen Kadenzen und werden davon **nicht**
// beruehrt. Das Fenster ist ausserdem kurz: Ausrollen dauert dreissig bis
// sechzig Sekunden, danach steht das Flugzeug auf dem Rollweg und das
// Intervall geht auf 2 Hz zurueck.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> rollout_tick_ms (v2.1.24: 200)

/// Ab dieser Fahrt gilt „am Ausrollen".
///
/// # Warum das die Spur-Schwelle ist und nicht die Bewertungs-Schwelle
///
/// Bis v1.7.0 stand hier `BAHN_MESS_MIN_GS_KT` (60 kt) — dieselbe Schwelle,
/// unter der das **Bewertungs**fenster schliesst. Die Begruendung klang
/// schluessig: Wer langsamer rollt, wird nicht mehr bewertet und braucht
/// keine feine Spur.
///
/// Sie war falsch, und Thomas hat es an der Zeichnung gesehen: Die
/// Ausfahrt wird mit fuenfzehn bis dreissig Knoten gefahren. Genau dort
/// bog das Flugzeug ab, genau dort kruemmt sich die Spur am staerksten —
/// und genau dort fiel der Takt von fuenf Hertz auf zwei. Bei zwanzig
/// Knoten sind das statt 2,1 Metern je Punkt deren 5,1, und die Kurve, um
/// derentwillen die feine Taktung ueberhaupt eingebaut wurde, bekam die
/// wenigsten Punkte von allen.
///
/// Aufgezeichnet wird bis `BAHN_SPUR_STOP_GS_KT`; so lange laeuft jetzt
/// auch der Takt. Das kostet wenig: Zwischen 60 und 5 Knoten vergehen
/// keine dreissig Sekunden, und der Tick liest nur einen Schnappschuss.
// v3: aus einem Grenzwert abgeleitet, deshalb Funktion statt Konstante.
fn rollout_tick_min_gs_kt() -> f64 {
    crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_stop_gs_kt.wert as f64
}

/// Wie `adaptive_tick_interval`, kennt aber zusaetzlich Bodenkontakt und
/// Fahrt — beides braucht es, um das Ausrollen von allem anderen zu
/// unterscheiden.
fn adaptive_tick_interval_v2(
    phase: FlightPhase,
    agl_ft: Option<f64>,
    on_ground: Option<bool>,
    groundspeed_kt: Option<f64>,
    spur_laeuft: bool,
) -> Duration {
    // Ausrollen zuerst: Es ist der einzige Zustand, in dem die seitliche
    // Lage laufend gemessen wird, und der einzige, dessen Aufloesung an der
    // Fahrt haengt statt an der Hoehe.
    //
    // Zwei Wege fuehren hierher, und es braucht beide:
    //
    // * `spur_laeuft` — die Aufzeichnung selbst meldet, dass sie laeuft.
    //   Das deckt die Ausfahrt ab, in der die Phase schon auf `TaxiIn`
    //   gesprungen ist, die Spur aber noch gezeichnet wird.
    // * Die Phasen-Bedingung — sie deckt das offene Messfenster ab, in dem
    //   `spur_fortschreiben` noch gar nicht gerufen wird und das Flag
    //   folglich false ist.
    //
    // Bis v1.7.0 gab es nur den zweiten. Gemessen an neun echten
    // Landungen fiel die Kadenz dadurch bei dreissig Knoten von 0,51 s
    // auf 3,01 s — 34,9 Meter zwischen zwei Punkten, und das genau in der
    // Kurve. Die Ausfahrt bekam die wenigsten Punkte der ganzen Landung.
    if spur_laeuft
        || (matches!(phase, FlightPhase::Landing)
            && on_ground == Some(true)
            && groundspeed_kt.is_some_and(|gs| gs >= rollout_tick_min_gs_kt()))
    {
        return Duration::from_millis(crate::grenzwerte::grenzwerte().ausrollen.rollout_tick_ms.wert);
    }
    if let (Some(agl), true) = (
        agl_ft,
        matches!(
            phase,
            FlightPhase::Approach
                | FlightPhase::Final
                | FlightPhase::Landing
                | FlightPhase::Takeoff
                | FlightPhase::TakeoffRoll
        ),
    ) {
        if agl < 100.0 {
            return Duration::from_millis(500); // 2 Hz im Flare / Wheels-Up
        }
        if agl < 500.0 {
            return Duration::from_millis(750); // 1.3 Hz kurz vor TD
        }
        if agl < 1500.0 {
            return Duration::from_secs(1); // 1 Hz im Final-Approach
        }
        if agl < 2000.0 {
            return Duration::from_millis(1500); // 0.67 Hz im spaeten Approach
        }
    }
    Duration::from_secs(MQTT_PUBLISH_INTERVAL_SECS)
}

/// v0.5.x/v0.6.1 — phase-aware POST-Cadence an phpVMS.
///
/// **Wichtig:** Die *Streamer-Tick*-Cadence ist seit v0.6.0 davon
/// entkoppelt — der Streamer tickt adaptiv nach AGL (500ms-3s) und
/// pusht jede Position in die Memory-Outbox. Diese Funktion steuert
/// nur wie oft der **phpVMS-Worker** seine gepufferten Positions als
/// Batch an phpVMS abliefert.
///
/// Warum phase-aware: im Cruise ist 1 POST alle 30s genug (langer
/// gerader Leg, sparse samples reichen für die Live-Map). Im Pushback
/// muss alle 4s gepostet werden weil die Phase in 8-15s vorbei sein
/// kann — sonst springt das Dashboard direkt von Boarding auf Taxi
/// ohne Pushback-Snapshot. Im Approach/Final 8s damit der inbound-
/// Track präzise ist.
/// v1.6.14 — Punktabstand fuer **phpVMS**, getrennt von der POST-Kadenz.
///
/// Zwei verschiedene Fragen, die bisher dieselbe Antwort hatten:
/// * `position_interval` = wie oft wird gesendet (Netzverkehr, Aktualitaet)
/// * hier = wie fein wird der Weg aufgezeichnet (Kartenaufloesung)
///
/// Am Boden muss der Weg erkennbar bleiben — Rollwege, Startlauf,
/// Landerollen. In der Luft reicht deutlich weniger, im Reiseflug genuegt
/// phpVMS' eigene Vorgabe von 60 s (`acars.update_interval`).
///
/// Zum Vergleich, was ein Intervall an Strecke bedeutet:
/// 5 s beim Rollen (15 kt) = 38 m · 5 s im Startlauf (bis 150 kt) = 390 m ·
/// 8 s im Endanflug (150 kt) = 600 m · 15 s im Steigflug (300 kt) = 2,3 km ·
/// 60 s im Reiseflug (480 kt) = 15 km.
fn phpvms_punkt_intervall(phase: FlightPhase) -> Duration {
    let secs = match phase {
        // Am Boden: der Weg soll auf der Karte nachvollziehbar sein —
        // welches Gate, welcher Rollweg, welche Bahn.
        FlightPhase::Preflight
        | FlightPhase::Boarding
        | FlightPhase::Pushback
        | FlightPhase::TaxiOut
        | FlightPhase::TaxiIn
        | FlightPhase::TakeoffRoll
        | FlightPhase::BlocksOn
        | FlightPhase::Arrived => 5,
        // Abheben und Aufsetzen sind kurz und der interessanteste Teil.
        FlightPhase::Takeoff | FlightPhase::Landing => 5,
        // Anflug: bei 150 kt sind 8 s 600 m — fein genug fuer den
        // Anflugweg, ohne den Track zu fluten.
        FlightPhase::Approach | FlightPhase::Final => 8,
        // Steig- und Sinkflug: grosse Strecken, gleichmaessige Bewegung.
        FlightPhase::Climb | FlightPhase::Descent => 15,
        // Holding: die Kreise sollen als Kreise erkennbar bleiben.
        FlightPhase::Holding => 30,
        // Reiseflug: phpVMS' eigener Richtwert.
        FlightPhase::Cruise => 60,
        FlightPhase::PirepSubmitted => 60,
    };
    Duration::from_secs(secs)
}

/// v1.6.14 — Entscheidet, ob dieser Tick als Punkt an **phpVMS** geht.
///
/// Der VPS-Recorder und die JSONL-Forensik bekommen unabhaengig davon
/// jeden Tick; hier geht es nur um die Zeilen in `phpvmsacars`.
///
/// Regeln:
/// * Ein Phasenwechsel geht immer raus — sonst faellt z. B. der Touchdown
///   in ein laufendes Intervall und fehlt auf der Karte.
/// * Sonst nur, wenn das Phasen-Intervall abgelaufen ist (dieselbe
///   Staffelung, nach der auch gepostet wird).
fn phpvms_punkt_faellig(
    phase: FlightPhase,
    seit_letztem_push: Option<Duration>,
    zuletzt_gepushte_phase: Option<FlightPhase>,
) -> bool {
    if zuletzt_gepushte_phase != Some(phase) {
        return true;
    }
    match seit_letztem_push {
        None => true,
        Some(d) => d >= phpvms_punkt_intervall(phase),
    }
}

fn position_interval(phase: FlightPhase) -> Duration {
    let secs = match phase {
        // Brief, critical events — sample a touch faster so the touchdown
        // point and the actual liftoff don't get smeared between two posts.
        FlightPhase::Takeoff | FlightPhase::Landing => 5,
        // Boarding + Pushback need fast ticks because a real pushback
        // can be over in 8–15 s; if the streamer only fires every 10 s
        // we miss the entire phase between two snapshots and the
        // dashboard jumps straight from Boarding to Taxi.
        FlightPhase::Boarding | FlightPhase::Pushback => 4,
        // On the ground (taxi, takeoff roll) — 10 s is plenty;
        // movements are slow and the live map just needs a clean trail.
        FlightPhase::TaxiOut | FlightPhase::TakeoffRoll | FlightPhase::TaxiIn => 10,
        // Approach / final — pilot wants the inbound track precise (ILS,
        // localizer, glideslope) without overdoing samples.
        FlightPhase::Approach | FlightPhase::Final => 8,
        // Climb / descent: moderate change rate.
        FlightPhase::Climb | FlightPhase::Descent => 10,
        // Cruise: long straight legs, sparse samples are enough.
        // v0.13.8: 30s -> 60s. Eine halbe Stunde Cruise erzeugte vorher
        // ~60 Positions, jetzt ~30 — phpVMS-PIREP-Render-Performance
        // war ein realer Pilot-Befund. Live-Map zeigt jetzt Updates
        // alle 60s im Cruise; bei 480kt GS sind das ~8 nm zwischen
        // Punkten, fuer den Strack-Verlauf voellig ausreichend. VPS-
        // Recorder bekommt weiterhin alle Snapshots (separater MQTT-
        // Stream), nur phpVMS sieht die ausgedünnte Variante.
        FlightPhase::Cruise => 60,
        // v0.5.11: Holding — same cadence as Cruise (slow track update,
        // we're circling at constant altitude).
        FlightPhase::Holding => 60,
        // Parked / pre-/post-flight — keep a 30 s heartbeat so phpVMS
        // sees the PIREP is alive while the pilot files.
        FlightPhase::Preflight
        | FlightPhase::BlocksOn
        | FlightPhase::Arrived
        | FlightPhase::PirepSubmitted => 30,
    };
    Duration::from_secs(secs)
}

// Minimum great-circle distance between two consecutive samples before we
// add it to the running total. Filters out GPS jitter while parked.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> distance_epsilon_m (v2.1.24: 5.0)

// v0.12.1 (Stream A LE1): minimum real position displacement (metres)
// from the Boarding reference point before the Boarding→Pushback/TaxiOut
// transition may fire. A sim-reload velocity glitch can spike
// `groundspeed_kt` but cannot move `lat`/`lon` — so genuine displacement
// is the trustworthy signal. 5 m is small enough not to delay a real
// pushback, large enough to clear GPS jitter (< 1 m parked).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> min_boarding_movement_m (v2.1.24: 5.0)

// v0.12.1 (Stream A LE2): a gap between two processed `step_flight`
// snapshots longer than `RELOAD_GAP_FACTOR × position_interval(phase)`
// is treated as a sim reload/reposition — the first snapshot after the
// gap carries glitch-prone velocity values and must not drive a
// velocity-derived phase transition.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> reload_gap_factor (v2.1.24: 2.5)

/// v0.12.1 (Stream A LE2): true if `gap_secs` between two processed
/// snapshots is long enough to count as a sim reload/reposition.
fn is_reload_gap(gap_secs: f64, interval_secs: f64) -> bool {
    gap_secs > crate::grenzwerte::grenzwerte().phasen_kern.reload_gap_factor.wert * interval_secs
}

// v0.12.1 (Stream A LE2): the Pushback→TaxiOut "powered taxi" signal.
// Gated by `!reload_gap_suspect` so a sim-reload glitch tick (which can
// spike `groundspeed_kt`) cannot drive the velocity-derived TaxiOut
// transition — same rule the Boarding branch already applies.
//
// v0.15.13: zusätzlich VORWÄRTS-Bedingung (`velocity_body_z_fps`). Beim
// Pushback startet der Pilot häufig die Triebwerke MITTEN im Schieben — die
// alte Bedingung (Triebwerk + gs>3) flippte dann sofort auf TaxiOut, obwohl
// der Tug noch RÜCKWÄRTS schob (Live-Log BTX2222: Pushback nur 7 s statt
// ~78 s, weil das Triebwerk bei vbz=−6,5 ansprang). Solange vbz rückwärts/≈0
// ist, bleibt es Pushback. Liefert der Sim kein vbz (`None`), gilt das alte
// Verhalten (sonst blieben solche Aircraft für immer in Pushback hängen).
// Ab dieser Groundspeed rollt das Flugzeug sicher selbst: Ein Schlepper
// schiebt mit 2–5 kt (Joels eigener Pushback: 2,7 kt rückwärts), auch ein
// schneller GSX-Pushback bleibt deutlich darunter.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> pushback_haengt_kt (v2.1.24: 8.0)
// So lange muss das eigene Vorwärtsrollen anhalten, damit ein kurzer
// Ausreißer im Schnappschuss die Phase nicht umwirft.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> pushback_haengt_s (v2.1.24: 15)

/// Rollt das Flugzeug selbst, obwohl der Simulator „Pushback läuft" meldet?
///
/// ⚠ Nur mit bekannter Fahrtrichtung: Ohne `velocity_body_z_fps` gälte
/// `moving_forward` als wahr, und ein schneller Schlepper könnte die Phase
/// umschalten. Dann greift dieser Weg nicht.
fn pushback_haengt_aber_rollt(
    pushback_state: Option<u8>,
    powered_taxi: bool,
    groundspeed_kt: f32,
    velocity_body_z_fps: Option<f32>,
) -> bool {
    pushback_state.is_some_and(|p| p != 3)
        && powered_taxi
        && groundspeed_kt > crate::grenzwerte::grenzwerte().phasen_kern.pushback_haengt_kt.wert
        && velocity_body_z_fps.is_some_and(|v| v > crate::grenzwerte::grenzwerte().phasen_kern.taxi_direction_fps.wert)
}

fn powered_taxi_move(
    reload_gap_suspect: bool,
    on_ground: bool,
    engines_running: bool,
    groundspeed_kt: f32,
    velocity_body_z_fps: Option<f32>,
) -> bool {
    !reload_gap_suspect
        && on_ground
        && engines_running
        && groundspeed_kt > 3.0
        && moving_forward(velocity_body_z_fps)
}

/// v0.12.1 (Stream A LE1): pure decision for the Boarding→Pushback/TaxiOut
/// trigger. Real movement = on the surface, groundspeed over threshold
/// for two consecutive ticks (de-bounce), AND genuine position
/// displacement past `MIN_BOARDING_MOVEMENT_M`. A velocity glitch can
/// satisfy the speed checks but never the displacement one.
fn boarding_real_movement(
    on_surface: bool,
    gs_over_threshold: bool,
    prev_gs_over_threshold: bool,
    moved_m: f64,
) -> bool {
    on_surface && gs_over_threshold && prev_gs_over_threshold && moved_m > crate::grenzwerte::grenzwerte().phasen_kern.min_boarding_movement_m.wert
}

// v0.15.13: Schwelle (fps) ab der die körperfeste Längs-Geschwindigkeit
// `velocity_body_z_fps` als klare Vorwärts-/Rückwärtsbewegung zählt (Rauschen
// am Stand liegt darunter). >0 = vorwärts, <0 = rückwärts. Bestätigt am Live-
// Log BTX2222 (X-Plane/ToLiss): −6,5 fps beim Tug-Push, +9..+14 fps beim
// Vorwärts-Taxi, |vbz| ≈ Groundspeed über ALLE Heading-Werte (= körperfest,
// nicht erdfest). MSFS „VELOCITY BODY Z" ist ohnehin körperfest, vorwärts +.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> taxi_direction_fps (v2.1.24: 1.0)

/// Bewegt sich das Flugzeug RÜCKWÄRTS (Tug-Pushback)? `None` (Sim liefert kein
/// vbz) → `false` (unbekannt nicht als Rückwärts werten).
fn moving_backward(velocity_body_z_fps: Option<f32>) -> bool {
    matches!(velocity_body_z_fps, Some(v) if v < -crate::grenzwerte::grenzwerte().phasen_kern.taxi_direction_fps.wert)
}

/// Bewegt sich das Flugzeug klar VORWÄRTS? `None` → `true` (kein vbz → altes
/// Verhalten beibehalten, damit Sims/Aircraft ohne diesen Wert nicht in
/// Pushback hängen bleiben).
fn moving_forward(velocity_body_z_fps: Option<f32>) -> bool {
    match velocity_body_z_fps {
        Some(v) => v > crate::grenzwerte::grenzwerte().phasen_kern.taxi_direction_fps.wert,
        None => true,
    }
}

#[cfg(test)]
#[path = "tests_lib/quelle_gesamt.rs"]
mod quelle_gesamt;

#[cfg(test)]
#[path = "tests_lib/v0_15_13_pushback_direction_tests.rs"]
mod v0_15_13_pushback_direction_tests;

/// Der Punkt, mit dem eine Wiederaufnahme die Sim-Position vergleicht.
///
/// `last_known_*` zuerst: `last_lat/lon` ist die Wegstrecken-Basis und wird
/// beim Resume absichtlich geleert — damit lief die Pruefung nach einem
/// Absturz vor dem naechsten Speicherpunkt ohne Bezug und liess jede Position
/// durch (Codex-Befund 27.09.2026, SIA 375).
fn gespeicherter_punkt(stats: &FlightStats) -> Option<(f64, f64)> {
    stats
        .last_known_lat
        .zip(stats.last_known_lon)
        .or(stats.last_lat.zip(stats.last_lon))
}

#[cfg(test)]
#[path = "tests_lib/gespeicherter_punkt_tests.rs"]
mod gespeicherter_punkt_tests;

/// Warum die Lage im Sim nach einer Unterbrechung nicht zum Flug passt.
/// Siehe `lage_abweichung`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LageAbweichung {
    /// Der Sim liefert noch keinen echten Messwert (Ladebild, Menue).
    KeinEchterWert,
    /// Vorher in der Luft, jetzt am Boden — oder knapp ueber dem Boden nach
    /// einem grossen Hoehenverlust.
    AmBoden,
    /// Sprit in der Luft gestiegen: neu geladener Zustand.
    SpritGestiegen,
    /// Weiter weg, als der Flieger in der Luecke kommen konnte.
    ZuWeitWeg { drift_nm: f64, erlaubt_nm: f64 },
}

/// Der Stand vor der Unterbrechung, gegen den die Lage geprueft wird.
#[derive(Debug, Clone, Copy)]
struct LageReferenz {
    phase: FlightPhase,
    lat: f64,
    lon: f64,
    alt_ft: Option<f64>,
    fuel_kg: Option<f32>,
    on_ground: bool,
    gs_kt: Option<f32>,
    /// Wann der Punkt zuletzt galt — daraus folgt die Luecke.
    at: Option<DateTime<Utc>>,
    /// Vom Abtaster bestaetigtes Aufsetzen, das zu dieser Luecke gehoert
    /// (siehe `aufsetzen_der_luecke`). `None`: keins.
    aufgesetzt: Option<(f64, f64)>,
}

// Wie lange vor dem letzten Punkt ein bestaetigtes Aufsetzen noch zur
// Luecke zaehlt. Ein Durchstart-Aufsetzer Minuten davor zaehlt nicht.
// Wert seit v3: bundle/grenzwerte/landung.json -> aufsetzen_vor_luecke_max_s (v2.1.24: 30)
// Wie lange NACH dem letzten Punkt das Aufsetzen hoechstens liegen darf:
// ein Abtast-Takt im Endanflug, mit Luft. Ein Aufsetzen, das erst nach
// einem Neuladen entsteht, liegt weit dahinter (Codex-Pruefung 26.10.2026).
// Wert seit v3: bundle/grenzwerte/landung.json -> aufsetzen_nach_letztem_punkt_max_s (v2.1.24: 3)
// Wie weit der Aufsetzpunkt hoechstens vom letzten Punkt vor der Luecke
// liegt. Ein Aufsetzen ist keine Reise: Bei LTU 154 lagen 30 m dazwischen.
// Wert seit v3: bundle/grenzwerte/landung.json -> aufsetzen_vor_luecke_max_nm (v2.1.24: 1.0)

/// Gehoert das bestaetigte Aufsetzen des Abtasters zu dieser Luecke?
///
/// LTU 154 (Michael K, 26.10.2026, EDDF 25R): MSFS flackerte 150–250 ms
/// nach dem Aufsetzen kurz auf „Pause". Der Abtaster bestaetigte das
/// Aufsetzen eine Sekunde spaeter, die Lage-Pruefung aber sah nur „vorher
/// in der Luft, jetzt am Boden" und hielt den Flug als Sim-Absturz an
/// (am Vortag derselbe Ablauf in EDDK 31R).
///
/// Ein neu geladener Flieger (GAF 278) bringt kein solches Aufsetzen mit:
/// Der Sprung auf den Boden faellt beim Abtaster als FALSE_EDGE durch, und
/// ein frueherer Aufsetzer liegt zeitlich oder raeumlich zu weit weg.
fn aufsetzen_der_luecke(stats: &FlightStats, lat: f64, lon: f64) -> Option<(f64, f64)> {
    let (Some(td_at), Some(la), Some(lo), Some(at)) = (
        stats.sampler_touchdown_at,
        stats.landing_lat,
        stats.landing_lon,
        stats.last_known_at,
    ) else {
        return None;
    };
    let zeitlich = td_at >= at - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().landung.aufsetzen_vor_luecke_max_s.wert)
        && td_at <= at + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().landung.aufsetzen_nach_letztem_punkt_max_s.wert);
    let raeumlich = ::geo::distance_m(la, lo, lat, lon) / 1852.0 <= crate::grenzwerte::grenzwerte().landung.aufsetzen_vor_luecke_max_nm.wert;
    (zeitlich && raeumlich).then_some((la, lo))
}

// Laenger als die Pruef-Frist des Abtasters (1,1 s). Ein haengender
// Kandidat sperrt das Fortsetzen nicht fuer immer.
// Wert seit v3: bundle/grenzwerte/landung.json -> aufsetz_pruefung_abwarten_s (v2.1.24: 5)

/// Prueft der Abtaster gerade ein Aufsetzen? Dann noch nicht ueber die Lage
/// urteilen: Bei LTU 154 kam die Bestaetigung nur fuenf Sekunden vor dem
/// Urteil — ein Takt frueher, und „am Boden" haette ohne Aufsetzen dagestanden.
fn aufsetzen_wird_geprueft(stats: &FlightStats, jetzt: DateTime<Utc>) -> bool {
    stats
        .pending_td_at
        .is_some_and(|t| jetzt - t < chrono::Duration::seconds(crate::grenzwerte::grenzwerte().landung.aufsetz_pruefung_abwarten_s.wert))
}

#[cfg(test)]
#[path = "tests_lib/landung_in_der_luecke_tests.rs"]
mod landung_in_der_luecke_tests;

/// Referenz aus dem Flugstand: der dauerhafte letzte Punkt, dazu Sprit und
/// Bodenlage aus dem Pausen- bzw. Ablage-Snapshot.
fn lage_referenz(stats: &FlightStats) -> Option<LageReferenz> {
    let snap = stats
        .paused_last_known
        .as_ref()
        .or(stats.last_persisted_snapshot.as_ref());
    let (lat, lon) = gespeicherter_punkt(stats).or(snap.map(|s| (s.lat, s.lon)))?;
    Some(LageReferenz {
        phase: stats.phase,
        lat,
        lon,
        alt_ft: stats.last_known_alt_ft.or(snap.map(|s| s.altitude_ft)),
        fuel_kg: snap.map(|s| s.fuel_total_kg),
        on_ground: snap.is_some_and(|s| s.on_ground),
        gs_kt: stats.last_known_gs_kt,
        at: stats.last_known_at,
        aufgesetzt: aufsetzen_der_luecke(stats, lat, lon),
    })
}

/// Durfte der Flieger in der Unterbrechung weiterfliegen? Nein bei einer
/// Sim-Pause und wenn der Sim-Prozess beim Ausfall weg war (abgestuerzt) —
/// dann muss er dort stehen, wo der Flug abbrach.
fn darf_weitergeflogen_sein(stats: &FlightStats) -> bool {
    match stats.current_pause_reason {
        Some(PauseReason::SimPause) => false,
        _ => !matches!(
            stats.disconnect_sim_liveness,
            Some(sim_core::process_probe::ProcessLiveness::Gone)
        ),
    }
}

/// Passt die Lage im Sim nach einer Unterbrechung (Sim-Absturz, Sim-Pause,
/// App-Neustart) zum Flug? `None` heisst: ja, der Flug darf von selbst
/// weiterlaufen. Sonst muss der Pilot im Resume-Banner bestaetigen.
///
/// ⚠ GAF 278 (Adrian, 24.10.2026): Im Sinkflug FL323 stuerzte der Sim ab, 16
/// Minuten spaeter stand der A321 in Zagreb am Boden, Tank voll. Der Client
/// setzte von selbst fort und meldete eine Minute spaeter ARRIVED — in
/// Zagreb, 112 nm vor Wien. Der erste Wert nach dem Laden meldete dabei noch
/// `on_ground=false` auf 354 ft (Platzhoehe 353 ft); darum zaehlt knapp ueber
/// dem Boden nach grossem Hoehenverlust ebenfalls als „am Boden".
///
/// Weite: Durfte der Flieger weiterfliegen (nur die Datenverbindung war weg),
/// gilt die in der Luecke erreichbare Strecke; sonst `RESUME_DRIFT_CLEAN_NM`.
/// Bewusst keine Hochrechnung entlang des Kurses — Kurven und Holdings
/// loesten sonst Fehlalarme aus (Codex-Pruefung 24.10.2026).
fn lage_abweichung(
    vorher: &LageReferenz,
    jetzt: &SimSnapshot,
    zeitpunkt: DateTime<Utc>,
    darf_weitergeflogen: bool,
) -> Option<LageAbweichung> {
    if !snapshot_plausibel(jetzt) || !snapshot_position_is_usable(jetzt) {
        return Some(LageAbweichung::KeinEchterWert);
    }
    let vorher_in_der_luft = matches!(
        vorher.phase,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Holding
            | FlightPhase::Descent
            | FlightPhase::Approach
            | FlightPhase::Final
    );
    let hoehe_verloren_ft = vorher
        .alt_ft
        .map_or(0.0, |alt| alt - jetzt.altitude_msl_ft);
    let jetzt_am_boden =
        jetzt.on_ground || (jetzt.altitude_agl_ft < 100.0 && hoehe_verloren_ft > 1_000.0);
    // In der Luecke gelandet (LTU 154): bestaetigtes Aufsetzen, ohne
    // Hoehensturz, und der Flieger steht in Rollweite davon. Sprit und
    // Weite pruefen die Schritte unten wie sonst auch.
    let in_der_luecke_gelandet = vorher.aufgesetzt.is_some_and(|(la, lo)| {
        hoehe_verloren_ft < 1_000.0
            && ::geo::distance_m(la, lo, jetzt.lat, jetzt.lon) / 1852.0 <= crate::grenzwerte::grenzwerte().abgabe.resume_drift_clean_nm.wert
    });
    if vorher_in_der_luft && jetzt_am_boden && !in_der_luecke_gelandet {
        return Some(LageAbweichung::AmBoden);
    }
    if let Some(fuel) = vorher.fuel_kg {
        let durchgehend_am_boden = vorher.on_ground && jetzt.on_ground;
        if !durchgehend_am_boden
            && (jetzt.fuel_total_kg - fuel) as f64 > crate::grenzwerte::grenzwerte().abgabe.resume_fuel_jump_impossible_kg.wert
        {
            return Some(LageAbweichung::SpritGestiegen);
        }
    }
    let drift_nm = ::geo::distance_m(vorher.lat, vorher.lon, jetzt.lat, jetzt.lon) / 1852.0;
    let erlaubt_nm = if !darf_weitergeflogen {
        crate::grenzwerte::grenzwerte().abgabe.resume_drift_clean_nm.wert
    } else {
        match (vorher.gs_kt, vorher.at) {
            (Some(gs), Some(at)) => {
                let luecke_h = (zeitpunkt - at).num_seconds().max(0) as f64 / 3600.0;
                // 25 % Luft fuer Wind und Beschleunigung.
                crate::grenzwerte::grenzwerte().abgabe.resume_drift_clean_nm.wert + gs.max(0.0) as f64 * 1.25 * luecke_h
            }
            // Altstand ohne Zeit/Tempo: die fruehere Grenze.
            _ => crate::grenzwerte::grenzwerte().abgabe.resume_drift_warn_nm.wert,
        }
    };
    if drift_nm > erlaubt_nm {
        return Some(LageAbweichung::ZuWeitWeg {
            drift_nm,
            erlaubt_nm,
        });
    }
    None
}

/// „Vom Boden weiterfliegen" (GAF 278): MSFS kann viele Fluege nicht in der
/// Luft neu laden, der Pilot laedt deshalb auf einem nahen Platz am Boden und
/// fliegt von dort zum Ziel weiter. Gilt nur fuer „Trotzdem fortsetzen", nur
/// aus einer Luftphase ohne angefangene Landung, und nicht am geplanten Ziel
/// (dort bleibt es bei Arrived samt Ziel-Sperre).
fn bodenneustart_moeglich(stats: &FlightStats, abweichung: Option<LageAbweichung>, am_ziel: bool) -> bool {
    let in_der_luft = matches!(
        stats.phase,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Holding
            | FlightPhase::Descent
            | FlightPhase::Approach
            | FlightPhase::Final
    );
    in_der_luft
        && stats.landing_at.is_none()
        && abweichung == Some(LageAbweichung::AmBoden)
        && !am_ziel
}

/// Setzt den Flug fuer den Weiterflug vom Boden auf „Rollen zum Start".
/// Abflugwerte, Block-off, `was_airborne`, Strecke und der Sprung-Vermerk
/// bleiben; geraeumt wird, was zur abgebrochenen Luftphase gehoert.
fn bodenneustart_vorbereiten(stats: &mut FlightStats, jetzt: DateTime<Utc>) {
    stats.phase = FlightPhase::TaxiOut;
    stats.transitions.push((jetzt, FlightPhase::TaxiOut));
    stats.weiterflug_ab_boden_seit = Some(jetzt);
    // Der Tank wurde neu geladen: Abhebe- minus Landesprit ergaebe einen
    // erfundenen Verbrauch (Claude-Pruefer 24.10.2026). Ohne Abhebe-Sprit
    // gilt der Sprit ueberall als „nicht bewertbar".
    stats.takeoff_fuel_kg = None;
    // Die Schatten-Engine kennt nur den Sinkflug — frisch, sonst zieht der
    // Versoehner die Phase zurueck (Codex-Pruefung 24.10.2026).
    stats.shadow_engine = phase_v2::ShadowPhaseEngine::default();
    stats.shadow_phase = None;
    stats.enroute_divergence_since = None;
    stats.arrived_fallback_pending_since = None;
    stats.takeoff_roll_slow_since = None;
    stats.takeoff_roll_stall_since = None;
    stats.takeoff_roll_peak_kt = 0.0;
    stats.was_on_ground = Some(true);
    stats.last_step_at = None;
    stats.divert_hint = None;
    // Was zum abgebrochenen Anflug gehoert.
    stats.pending_td_at = None;
    stats.snapshot_buffer.clear();
    clear_approach_stability_and_rollout(stats);
    stats.lowest_agl_during_approach_ft = None;
    stats.tiefster_anflugpunkt = None;
    stats.go_around_climb_pending_since = None;
    stats.climb_peak_msl = None;
}

/// `lage_abweichung` fuer den laufenden Flug. Ohne gespeicherten Punkt gibt
/// es nichts zu vergleichen — dann passt die Lage (wie bisher).
fn lage_abweichung_fuer(flight: &ActiveFlight, jetzt: &SimSnapshot) -> Option<LageAbweichung> {
    let (referenz, darf_weitergeflogen) = {
        let stats = flight.stats.lock().expect("flight stats");
        (lage_referenz(&stats), darf_weitergeflogen_sein(&stats))
    };
    lage_abweichung(&referenz?, jetzt, Utc::now(), darf_weitergeflogen)
}

#[cfg(test)]
#[path = "tests_lib/lage_nach_luecke_tests.rs"]
mod lage_nach_luecke_tests;

/// Fuer Protokoll und Banner: was nicht passt, in einem Satz.
fn lage_abweichung_text(a: &LageAbweichung) -> String {
    match a {
        LageAbweichung::KeinEchterWert => {
            "Der Simulator liefert noch keine echte Position (Ladebildschirm/Menü).".to_string()
        }
        LageAbweichung::AmBoden => {
            "Das Flugzeug steht im Sim am Boden, der Flug war in der Luft.".to_string()
        }
        LageAbweichung::SpritGestiegen => {
            "Im Sim ist mehr Sprit an Bord als vor der Unterbrechung — neu geladener Zustand."
                .to_string()
        }
        LageAbweichung::ZuWeitWeg {
            drift_nm,
            erlaubt_nm,
        } => format!(
            "Das Flugzeug steht {drift_nm:.1} nm von der letzten Position weg (erlaubt {erlaubt_nm:.1} nm)."
        ),
    }
}

/// „Am Boden" fuer `is_mid_session_sim_crash_recovery`: Der erste Wert nach
/// dem Laden meldet oft noch „in der Luft", steht aber auf Platzhoehe (GAF
/// 278: 354 ft, on_ground=false). Der 5000-ft-Sprung bleibt dort Bedingung.
fn am_boden_fuer_absturzerkennung(snap: &SimSnapshot) -> bool {
    snap.on_ground || snap.altitude_agl_ft < 100.0
}

/// v0.13.9 Stream F (LE27): Mid-Session Sim-Crash-Recovery Detection.
///
/// Pure Funktion — verglich zwei aufeinanderfolgende Snapshots im Streamer-
/// Tick und gibt `true` zurueck wenn die Kombination ein Sim-Reload-Artefakt
/// anzeigt:
///
/// 1. Aktuelle FlightPhase ist airborne-Set (Climb/Cruise/Holding/
///    Descent/Approach/Final).
/// 2. Vorheriger Snapshot war `on_ground=false`.
/// 3. Neuer Snapshot ist `on_ground=true`.
/// 4. |Altitude-Differenz| > Schwelle (default 5000 ft).
///
/// Der 5000-ft-Wert ist konservativ: Max realer Descent (Airbus 'Open
/// Descent' / Boeing 'VS-Mode') liegt bei ~4000 fpm, also ~4000 ft pro
/// Minute. Im laengsten Tick-Intervall (30 s Cruise) waeren das maximal
/// 2000 ft realer Drop. Ein 5000-ft-Drop in einem Tick ist physikalisch
/// unmoeglich = eindeutig Reload-Artefakt.
///
/// Position-Distance wird NICHT geprueft weil XP12-Crash-Recovery den
/// Aircraft am naechstgelegenen Airport spawnt (typisch 10-20 km vom
/// Cruise-Punkt, < `RESUME_DRIFT_WARN_NM` von 50 nm) — Distance-Trigger
/// wuerde Faelle wie Michel 2026-05-25 verpassen.
fn is_mid_session_sim_crash_recovery(
    current_phase: FlightPhase,
    prev_on_ground: bool,
    prev_altitude_msl_ft: f64,
    curr_on_ground: bool,
    curr_altitude_msl_ft: f64,
) -> bool {
    let was_airborne_phase = matches!(
        current_phase,
        FlightPhase::Climb
            | FlightPhase::Cruise
            | FlightPhase::Holding
            | FlightPhase::Descent
            | FlightPhase::Approach
            | FlightPhase::Final
    );
    if !was_airborne_phase {
        return false;
    }
    if prev_on_ground || !curr_on_ground {
        return false;
    }
    let alt_drop_ft = (prev_altitude_msl_ft - curr_altitude_msl_ft).abs();
    alt_drop_ft > 5000.0
}

/// Kilograms → pounds. We collect fuel in kg internally because every
/// SimConnect adapter normalises to SI, but phpVMS-Core's `acars` table
/// and the PIREP `file` endpoint expect pounds — convert at the boundary.
const KG_TO_LB: f64 = 2.20462262;

// The on-airport radius (formerly ARRIVED_FALLBACK_RADIUS_NM and its twin
// DIVERT_DETECT_RADIUS_NM — two constants for one concept, kept in sync by
// nothing but a doc comment) now lives at `arrival::ON_FIELD_RADIUS_NM`,
// together with the geometry it belongs to. The nearest-field search radius
// lives at `arrival::NEAREST_SEARCH_RADIUS_NM`.
//
// Universal "we're done here" fallback for the FSM. Catches helicopters
// (no taxi-out / taxi-in / parking-brake convention), short hops,
// emergency landings near the destination, and anything else where the
// normal Pushback → Cruise → BlocksOn chain doesn't fire cleanly.
//
// When the aircraft is on-ground with engines off, sitting on the
// destination field (see `arrival::locate`), the FSM jumps straight to
// `Arrived` regardless of what phase it's currently in (apart from
// pre-block-off phases — we won't accidentally end a flight that hasn't
// started yet).
// Wall-clock dwell the engines-off condition
// (`arrived_fallback_conditions_basic`) must hold, uninterrupted, before
// the fallback fires. `engines_running` is an unsmoothed per-tick SimVar
// read (`GENERAL ENG COMBUSTION:N` / N1-fallback) with no read-side
// debouncing, so a lone stray sample shouldn't end the flight — but once
// it has genuinely held for a few seconds there is nothing left to wait
// for (the shutdown-pacing concern the old 30 s dwell was also covering
// is already handled by requiring `engines_running == 0` in the first
// place, not by waiting longer after it's confirmed).
//
// Deliberately wall-clock, NOT a tick count: `adaptive_tick_interval`
// drops the streamer's tick cadence to 500 ms whenever the phase
// is Approach/Final/Landing/Takeoff/TakeoffRoll below 100 ft AGL — which
// is exactly the "stuck on Final while already on the ground" state this
// fallback exists to rescue. A fixed *tick-count* threshold would confirm
// up to ~6x faster than intended in that window (an earlier version of
// this fix made exactly that mistake — caught in code review before
// release); wall-clock time is cadence-independent by construction.
//
// Lowered from 30 s (set at v0.1.20, never re-tuned) after a field report
// (GSG22 EDLN→EDDL, PIREP XrRp6eMQYvJJlExP): the pilot perceived the app
// as frozen on "Final" for 3-4 min after touchdown. Flight-log analysis
// showed the vast majority of that was real taxi-in + the pilot's own
// shutdown pacing; the only software-attributable wait was this dwell.
// 9 s still comfortably rules out a single bad sample (≥3 ticks at the
// normal 3 s ground cadence; far more during the sub-second flare
// cadence).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> arrived_fallback_dwell_secs (v2.1.24: 9)
// Wie lange ein Bremshalt auf einer Rollweg-Mittellinie (bei laufenden
// Triebwerken) halten muss, bevor er als Block-On zählt. Die drei falschen
// Block-Ons aus 30 Tagen Feldflügen (27.10.2026) hielten 27, 27 und 84 s.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> rollweg_halt_bestaetigt_secs (v2.1.24: 120)

/// Zählt ein Bremshalt auf der Rollweg-Mittellinie schon als Block-On?
/// Ja, sobald die Triebwerke aus sind (wer auf dem Rollweg abstellt, ist
/// angekommen) oder die Frist seit Beginn des Halts verstrichen ist.
fn rollweg_halt_bestaetigt(seit: DateTime<Utc>, now: DateTime<Utc>, engines_running: u8) -> bool {
    engines_running == 0 || (now - seit).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.rollweg_halt_bestaetigt_secs.wert
}

/// Gehört der gemerkte Haltbeginn zu DIESEM Halt? Nur, wenn das Flugzeug
/// noch dort steht (Codex-QS 27.10.2026: Slew/GSX bei gesetzter Bremse oder
/// ein erneutes Abheben ließen sonst eine fremde Zeit als Block-On stehen).
fn kandidat_hier(kandidat: &(DateTime<Utc>, f64, f64), lat: f64, lon: f64) -> bool {
    ::geo::distance_m(kandidat.1, kandidat.2, lat, lon) <= 10.0
}
// v0.17.x (#Premium, Aircraft-Scan): längerer Dwell für den engines-
// UNABHÄNGIGEN Stillstands-Fallback (Groundspeed < 1 kt + Parkbremse am
// Ziel). Fängt Addons ab, deren `engines_running`-SimVar nach dem Shutdown
// klemmt: der Contrail-FA50-Trijet meldet dauerhaft 3 laufende Triebwerke,
// sodass der engines-off-Pfad NIE feuert und Auto-File hängt (Michel D.,
// D-BETI).
//
// v0.18.x QA-Härtung (Einwand Thomas: lange Rollhalte an großen Hubs, z. B.
// Warten auf weitere Rollfreigabe, wo Piloten realistisch die Parkbremse
// ziehen): 2 Minuten reichten dafür nicht als Sicherheitsmarge. ZWEI
// unabhängige Gegenmaßnahmen, nicht nur eine:
//   1. Dieser Pfad läuft nur noch für Flugzeuge mit bestätigt kaputtem
//      Triebwerkszähler (`AircraftProfile::engine_count_unreliable`), NICHT
//      mehr fleet-weit — ein Rollhalt in einem Fenix/PMDG/FBW/etc. kann
//      diesen Pfad gar nicht mehr erreichen, weil der harte engines-off-Test
//      (`ARRIVED_FALLBACK_DWELL_SECS`) für die die einzige Rolle spielt.
//   2. Für die verbleibenden, eng begrenzten Fälle (aktuell nur FA50) ist der
//      Dwell auf 5 Minuten verlängert — deutlich länger als ein typischer
//      Rollhalt, aber am echten Zielflughafen nach Blockzeit ohne
//      spürbaren Nachteil für den Piloten.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> arrived_standstill_dwell_secs (v2.1.24: 300)

// ---- Touch-and-Go / Go-Around detection (Stage 3) ----
//
// These thresholds were chosen by working backwards from real-world
// pilot intent rather than from sim-specific telemetry shapes:
//
// * 100 ft AGL threshold for T&G — comfortably above gear-strut /
//   bounce-rebound height (which is at most ~40 ft on the worst
//   bounces), but low enough that a deliberate climb-out from a
//   training landing crosses it quickly.
// * KEIN Beobachtungsfenster mehr (20.10.2026, EWG 1599): bis 2.1.4
//   zaehlte ein Wegsteigen nur innerhalb von 30 s ab dem Aufsetzen. Nach
//   26 s Bodenlauf kamen die 100 ft erst nach 35,9 s — die Phase blieb
//   15 Minuten in der Luft auf `Landing`. Siehe
//   [`landung_beim_wegsteigen_abschliessen`].
// * 200 ft recovery for GA — a missed approach typically initiates
//   at ≥200 ft above the lowest observed altitude, and 200 ft is
//   the standard Cat I missed-approach decision height. Sustained
//   8 s prevents the detector firing on a single climb-correction.
// * +500 fpm V/S filter on GA — pilots really go around with positive
//   V/S, no false positives from a flare round-out.

// Nach so vielen Sekunden am Boden wird ein Aufsetzer vorlaeufig als
// `FinalLanding` in `touchdown_events` verbucht (spaetestens beim Wechsel
// nach `TaxiIn`). Steigt das Flugzeug danach doch noch weg, macht
// [`landung_beim_wegsteigen_abschliessen`] aus genau diesem Eintrag einen
// `TouchAndGo` — die Zeit begrenzt das Erkennen des Wegsteigens nicht mehr.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> final_landing_verbuchen_nach_secs (v2.1.24: 30)
// Aircraft must climb above this AGL after a touchdown to count
// as a touch-and-go (not a tall bounce or runway-end pop-up).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> touch_and_go_agl_threshold_ft (v2.1.24: 100.0)
// Aircraft must be above the AGL threshold for this long for the
// T&G to be confirmed — short spike doesn't count.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> touch_and_go_dwell_secs (v2.1.24: 1)
// AGL increase from `lowest_agl_during_approach_ft` that signals a
// possible go-around in progress.
//
// v0.5.35: 200 → 150 ft. Reason: bei sparse Sampling kann der erste
// Frame nach der lowest_agl-Marke schon 60+ Sekunden später sein —
// in der Zeit klettert ein C152 leicht 200+ ft. Niedrigerer Threshold
// fängt mehr GA-Versuche ein. Dwell-Check filtert noch False Positives.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> go_around_agl_recovery_ft (v2.1.24: 150.0)

// ---- Holding-pattern detection (v0.5.11) ------------------------------
//
// A holding pattern is a circle at constant altitude. We detect it via
// sustained turn (banked) + level flight (low VS). Real ICAO holds:
//   * 1-min legs at <FL140, 1.5-min legs above → 4-6 min full circuit
//   * Standard 3°/sec turn rate = full 360° in 2 min
//   * Constant altitude (±100 ft)
//
// Entry threshold: 90 s of sustained bank > 15° + |VS| < 200 fpm.
// This filters out:
//   * 90° heading change at standard rate (~30 s) — won't trigger
//   * Procedure turn / teardrop entry (~60 s) — won't trigger
//   * Long ATC vector with sustained 30° bank (~2 min) — WILL trigger,
//     which is fine: the FSM simply shows "Holding" briefly and then
//     exits when bank levels off
//
// Exit threshold: 30 s of sustained bank < 5° (level flight). Or
// active descent (< -300 fpm + AGL < 5000) which means ATC cleared
// us out of the hold for the approach.
//
// 17.10.2026 — ersetzt durch die DREHUNG statt der Querneigungs-Dauer:
// Die 30-s-Ausgangsschwelle war kürzer als ein Schenkel (60–90 s), jede
// echte Warteschleife endete am ersten Schenkel und begann in der nächsten
// Kurve neu (P16, FL205: drei 45-s-Bruchstücke). Und 90 s Querneigung
// fingen lange Kurven auf FL388. Korpus 447 Flüge: echte Warteschleifen
// drehen in 6 min ≥ 300° gleichsinnig, Flüge ohne Warteschleife im Level
// zu 99 % ≤ 244°.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_kurs_fenster_secs (v2.1.24: 360)
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_eintritt_drehung_deg (v2.1.24: 300.0)
// Ende: so lange (fast) geradeaus — länger als jeder Schenkel.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_austritt_fenster_secs (v2.1.24: 180)
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_austritt_drehung_deg (v2.1.24: 45.0)
// Gezählt wird nur im Geradeaus-Höhenflug über dieser Höhe — keine
// Platzrunden, keine Steig-/Sinkkurven.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_min_agl_ft (v2.1.24: 1500.0)
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> holding_level_vs_fpm (v2.1.24: 300.0)
// Sustained climb time required before we classify a Go-Around.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> go_around_dwell_secs (v2.1.24: 8)
// Seit 17.10.2026 („Stück 2“, docs/spec/phase-reiseband-v2.md) nur noch für
// v1 im Anflug/Final und für v1 im Reiseband bei v2 im Anflug/Final/Landung;
// innerhalb des Reisebands übernimmt v1 v2s Phase direkt (`reiseband_phase`).
// Die folgende Beschreibung ist die ursprüngliche Mechanik.
//
// En-Route-Versöhner (Korpus-Audit 2026-07, 632 Flüge): Sicherheitsnetz
// für die GANZE Fehlerklasse "alte FSM hängt auf einer Luft-Phase fest",
// nicht für einen einzelnen Übergangs-Arm. Historie derselben Klasse,
// jedes Mal als Einzelfix hinterher: v0.5.40 (Pushback klemmt,
// pushback_state konstant 3), v0.13.17 (Boden-Phasen klemmen, tote
// Engine-SimVars), v0.19.1 (Final→Landing, Anzeige-only), GSG-105
// (Descent-Falle, es gibt keinen Descent→Climb-Weg zurück), zuletzt
// GSG22 EDLN→EDDL + eine C172 (Climb→Descent mathematisch unerreichbar:
// `lost_from_peak > 500..1200 ft` kann bei < 1000 ft Climb-Peak nie
// eintreten — Ansatz-Problem, kein Schwellenwert-Problem).
//
// Mechanik: weicht v1 in einer Luft-Phase (Climb/Cruise/Descent/
// Approach/Final — NICHT Holding, das v2 bewusst nicht modelliert) so
// lange von v2s aufgelöster Phase ab, ohne sich selbst zu bewegen, wird
// v1s echter Zustand auf v2s Phase gezogen — über den normalen
// `next_phase`-Commit, also mit echtem `PhaseChanged`-Event (Treibstoff-
// Snapshot, METAR-Abruf, ACARS-Log, Auto-File-Kette ziehen alle nach;
// das unterscheidet ihn von der reinen Anzeige-Korrektur aus v0.19.1).
// Es wird KEINE neue Heuristik erfunden: Ziel ist v2s bereits
// review-gehärtete Resolver-Phase (phase_v2.rs), keine Roh-Segmente.
//
// Der PRIMÄRE Schutz vor Fehl-Zügen ist NICHT der Dwell, sondern das
// Kinematik-Widerspruchs-Gate [`segment_contradicts_v1`]: gefeuert wird
// ausschließlich, wenn die Fenster-Evidenz v1s aktuelle Phase aktiv
// WIDERLEGT (Climb während anhaltenden Sinkens/am Boden, Descent während
// anhaltenden Steigens, …). Bloßes v1≠v2 genügt NICHT — QS-Runde 1
// bewies empirisch, dass v1 und v2 bei Level-Flug GEWOLLT und dauerhaft
// auseinanderliegen (v2 promotet Climb→Cruise ref-los erst nach 240 s
// Level; v2 hält bei Zwischen-Level-Offs bewusst Descent, wo v1s B-003
// sofort Cruise recovert; v1 latcht Cruise bei Climb-Restrictions
// single-tick, wo v2 korrekt Climb hält) — ein rein v1≠v2-basierter
// Versöhner oszillierte auf JEDEM gesunden Normalflug (Zug → v1s
// Primär-Arm latcht zurück → Zug → …). `Level`/`Insufficient`
// widersprechen deshalb NIE einer Luft-Phase.
//
// Der Dwell obendrauf ist ASYMMETRISCH nach Zug-Richtung im natürlichen
// Flugbogen (Climb→Cruise→Descent→Approach→Final→Landing, s.
// `enroute_phase_rank`):
//
// * **Rückwärts** (v2 hinter v1) — 90 s: die vorsichtigere Richtung
//   (Defense-in-Depth über das Widerspruchs-Gate hinaus); länger als
//   v2s komplettes Evidenz-Fenster (60 s) + Hysterese-Reserve, damit
//   ein nachhinkendes v2 eine gesunde, frisch transitionierte v1 nie
//   zurückzieht (die Uhr resettet bei jeder v1-Transition ohnehin).
// * **Vorwärts** (v2 vor v1) — 45 s: ein v2, das VORAUS ist, hat MEHR
//   bestätigte Evidenz, nicht weniger. Replay-Befund BE24 (der Feld-
//   Report-Flug): die ehrliche Divergenz-Spanne vor dem Touchdown
//   betrug nur 82 s — mit einem symmetrischen 90-s-Dwell käme die
//   Rettung erst NACH dem Aufsetzen, mit 45 s noch in der Luft (v1
//   erlebt Final/Landing als echte Phasen und erfasst den Touchdown
//   über die eigene Landing-Kante statt Sampler-Carry-Over).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> enroute_reconcile_dwell_secs (v2.1.24: 90)
// Vorwärts-Dwell des En-Route-Versöhners — siehe
// [`ENROUTE_RECONCILE_DWELL_SECS`].
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> enroute_reconcile_forward_dwell_secs (v2.1.24: 45)
// Minimum positive V/S at which the GA detector accepts a sample
// as "actually climbing" (filters out flare round-out / level-off).
//
// v0.5.35: 500 → 300 fpm. Slow GA aircraft (Cessna 152, Piper Cub,
// taildraggers) klettern bei einem GA selten >500 fpm — der frühere
// Threshold übersah ihre Go-Arounds komplett.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> go_around_min_vs_fpm (v2.1.24: 300.0)
// Durchstarter zählen nur mit einer Landeschwelle höchstens so weit voraus
// vom tiefsten Anflugpunkt (17.10.2026, siehe `runway::schwelle_voraus`).
// 5 NM: Ein Durchstarter am Stabilitäts-Gate (1000 ft) liegt auf einem
// 3°-Gleitpfad 3,1 NM vor der Schwelle, bei 1500 ft 4,7 NM. Im Korpus
// lagen die falschen quer zur Bahn oder 5,3–25 NM entfernt.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> go_around_schwelle_nm (v2.1.24: 5.0)
// … und höchstens so viel Abweichung zwischen Flugzeug- und Bahnkurs.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> go_around_schwelle_kurs_deg (v2.1.24: 45.0)
// AGL threshold above which we mark the flight as having actually
// flown. Powers the `was_airborne` gate that prevents both the
// Arrived-fallback and the divert-detection from firing pre-flight
// (GSX repositioning, ground-handling wackeln, etc.). 50 ft is well
// above gear-strut oscillation and runway-end short-hop noise.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> was_airborne_agl_ft (v2.1.24: 50.0)
// Upper bound on AGL we'll trust for the `was_airborne` mark. Live bug
// 2026-05-03 (PMDG B738, GSX pushback): MSFS reported AGL=53819 ft
// for ~60 s right after the SimConnect handshake while the terrain
// engine was still loading — the aircraft was sitting on a stand
// at EDDH but the SimVar said it was at FL538 with on_ground=false.
// `was_airborne` flipped true from that loading glitch, and 30 min
// later the universal Arrived-fallback (gated by was_airborne)
// fired during the GSX pushback because pushback motion happens to
// match the conditions. The fix is to ignore obvious garbage AGL
// values: real pre-flight terrain data is bounded; nothing in the
// real world makes a parked aircraft suddenly "airborne at FL538".
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> was_airborne_agl_max_ft (v2.1.24: 30000.0)
// Minimum number of consecutive ticks the airborne conditions must
// hold before we flip `was_airborne`. Gates against single-tick
// glitches where MSFS briefly reports !on_ground during scenery
// load even with sane AGL. 2 ticks ≈ 5-10 s at the streamer cadence.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> was_airborne_dwell_ticks (v2.1.24: 2)

// "Is the aircraft on the planned field, or on some other one?" is answered
// exclusively by `arrival::locate` — including the radii it uses. There is no
// separate divert-detection radius any more: a divert is, by definition, "not
// on the planned field", and that is one decision made in one place.

// ---- v0.16.24: approach-phase divert navdata pre-fetch -----------------
//
// During a descending approach we watch whether the nearest airport has
// diverged from the planned `arr_airport`. Once the aircraft is on a
// stable, committed approach to a DIFFERENT field (descending, low, near
// that field, stable for a few seconds) we pre-fetch its Navigraph navdata
// so the cache is warm at touchdown — instead of falling back to the
// coarse OurAirports CSV. Bounded (only fires on divergence, idempotent per
// ICAO) and debounced (stability dwell) so an airport merely overflown
// mid-descent doesn't trigger a fetch.

// AGL ceiling (ft) below which we consider the aircraft committed enough
// to an approach to bother pre-fetching. Above this the nearest field is
// still ambiguous (en-route over a busy area). 5000 ft AGL is the same
// gate the FSM uses to enter Approach.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> divert_prefetch_max_agl_ft (v2.1.24: 5000.0)
// Horizontal radius (nm) within which the diverged nearest field must lie
// before we treat the approach as committed to it. 10 nm ≈ a typical
// final-approach intercept distance; tighter than the 50 nm divert search
// so a field merely passed at altitude doesn't qualify.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> divert_prefetch_max_dist_nm (v2.1.24: 10.0)
// The aircraft must be descending at least this fast (negative fpm) — a
// level overflight of an airport mid-cruise never qualifies.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> divert_prefetch_min_sink_fpm (v2.1.24: -200.0)
// How long the diverged nearest field must remain the stable candidate
// before the pre-fetch fires. Debounces an airport that is briefly nearest
// while the aircraft tracks past it on a vector.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> divert_prefetch_stable_secs (v2.1.24: 10)
// The diverged field must be at least this far from the planned
// `arr_airport` to count as a real divert (same threshold the
// parked-Arrived divert detection uses). Below this it's the same airport
// area and the planned navdata already covers it.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> divert_prefetch_planned_diverge_nm (v2.1.24: 5.0)
