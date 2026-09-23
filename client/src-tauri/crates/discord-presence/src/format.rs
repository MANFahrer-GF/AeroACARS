//! v0.9.0 (#Discord-RPC) — Pure-Fn-Format-Helpers fuer die Discord-Presence.
//!
//! Spec: docs/spec/v0.9.0-discord-rich-presence.md (Datenmodell + Phase-Mapping)
//!
//! Hier liegt die gesamte Stringbau-Logik damit sie OHNE Discord-IPC testbar ist:
//!   - `build_details()` baut die erste Zeile ("GSG3184 · EDDB → KMRH")
//!   - `build_state()`   baut die zweite Zeile ("CRUISE · A320 · FL360")
//!   - `format_altitude()` macht FL360 vs 2500 ft
//!   - `phase_to_label()` deckt ALLE 18 kanonischen Phase-Strings ab (= Vermeidung
//!     A5.4 aus dem Konflikt-Audit, kein Fallback-String-Pfad noetig)
//!   - `phase_to_asset_key()` mapped auf die 6 registrierten Discord-Assets

use crate::{FlightPhase, PresenceInput, SimKind};

/// Discord-Image-Asset-Keys die im Developer-Portal hochgeladen werden muessen.
/// Falls ein Asset fehlt, ignoriert Discord es stillschweigend (= small-image
/// wird weggelassen, der Text bleibt sichtbar).
pub const ASSET_LOGO: &str = "aeroacars_logo";

/// Sim-Icons: kleiner Badge unten-rechts am `large_image`.
pub fn sim_to_asset_key(sim: SimKind) -> Option<&'static str> {
    match sim {
        SimKind::Msfs2024 => Some("sim_msfs2024"),
        SimKind::Msfs2020 => Some("sim_msfs2020"),
        SimKind::Xplane11 => Some("sim_xplane11"),
        SimKind::Xplane12 => Some("sim_xplane12"),
        SimKind::Prepar3d => Some("sim_p3d"),
        SimKind::Unknown => None,
    }
}

/// Sim-Tooltip-Text der erscheint wenn der Pilot mit der Maus ueber dem kleinen Badge hovert.
pub fn sim_to_tooltip(sim: SimKind) -> &'static str {
    match sim {
        SimKind::Msfs2024 => "MSFS 2024",
        SimKind::Msfs2020 => "MSFS 2020",
        SimKind::Xplane11 => "X-Plane 11",
        SimKind::Xplane12 => "X-Plane 12",
        SimKind::Prepar3d => "Prepar3D",
        SimKind::Unknown => "Simulator",
    }
}

/// 6 registrierte Phase-Asset-Keys mit semantisch-passendem Fallback fuer die
/// restlichen 12 Phasen. Wird derzeit NICHT direkt im Image-Slot gerendert
/// (Logo bleibt im large_image fuer Brand-Konsistenz), aber bleibt im
/// Datenmodell fuer evtl. spaetere Layout-Variante (siehe Spec, Offene Fragen).
pub fn phase_to_asset_key(phase: FlightPhase) -> &'static str {
    use FlightPhase::*;
    match phase {
        Preflight | Boarding | Pushback | TaxiOut | TaxiIn | RejectedTakeoff => "phase_taxi",
        TakeoffRoll | Takeoff | Climb | GoAround => "phase_climb",
        // Holding ist hoehen-stabil → semantisch wie Cruise (auch wenn's manchmal
        // im Approach passiert; gemeinsamer Cruise-Asset ist trotzdem sauberer
        // als Approach-Asset weil Holding immer "stable level" bedeutet).
        Cruise | Holding => "phase_cruise",
        Descent => "phase_descent",
        Approach | Final => "phase_approach",
        // PirepSubmitted = nach Block-On, semantisch landed.
        Landing | Arrived | BlocksOn | Deboarding | PirepSubmitted => "phase_landed",
    }
}

/// VOLLSTÄNDIGE Mapping-Tabelle: 18 kanonische Phase-Strings → Discord-State-Text.
/// Spec-Pflicht: KEINE Phase darf in einen Fallback-Code-Pfad laufen.
///
/// Substitutionen werden vom Caller eingesetzt:
///   {aircraft}      → meist 4-stellig ICAO oder "Aircraft" wenn unbekannt
///   {altitude_text} → "FL360" >= 18000 ft, sonst "2500 ft" (nur bei FL-Phasen)
///   {arr_icao}      → "KMRH" oder "—" wenn unbekannt
///
/// `warn`-Tone (RTO + GA) bekommen ein „⚠"-Prefix damit diese Sonder-Situationen
/// im Discord-Server visuell hervorstehen.
pub fn phase_to_label(phase: FlightPhase) -> &'static str {
    use FlightPhase::*;
    match phase {
        Preflight => "PREFLIGHT",
        Boarding => "BOARDING",
        Pushback => "PUSHBACK",
        TaxiOut => "TAXI OUT",
        TakeoffRoll => "TAKEOFF ROLL",
        Takeoff => "TAKEOFF",
        RejectedTakeoff => "⚠ REJECTED TAKE-OFF",
        Climb => "CLIMB",
        Cruise => "CRUISE",
        Holding => "HOLDING",
        Descent => "DESCENT",
        Approach => "APPROACH",
        Final => "FINAL",
        Landing => "LANDING",
        GoAround => "⚠ GO-AROUND",
        TaxiIn => "TAXI IN",
        Arrived => "ARRIVED",
        BlocksOn => "SHUTDOWN",
        Deboarding => "DEBOARDING",
        PirepSubmitted => "PIREP FILED",
    }
}

/// Welche Phasen zeigen die Altitude im State-Text? Boden-Phasen nicht.
/// Holding zeigt Altitude (= stable level macht den Wert besonders aussagekraeftig).
fn phase_uses_altitude(phase: FlightPhase) -> bool {
    use FlightPhase::*;
    matches!(
        phase,
        Climb | Cruise | Holding | Descent | Approach | Final | GoAround
    )
}

/// Altitude-Display-Regel aus Spec:
///   >= 18000 ft → "FL360" (Flight-Level, ohne Hunderter)
///   <  18000 ft → "2500 ft" (volle Zahl mit Einheit)
///
/// Hinweis: ICAO definiert die Transition-Altitude regional (USA: 18000, EU: variabel
/// 3000-13000, je nach Land). Wir nutzen die US-Konvention 18000 als pragmatischer
/// Wert — Pilots sehen ihren tatsaechlichen Wert in der Cockpit-Anzeige sowieso,
/// das Discord-Display ist nur ein grober Status-Indikator.
pub fn format_altitude(altitude_ft: i32) -> String {
    if altitude_ft >= 18000 {
        // Auf naechste 100 runden + durch 100 teilen
        let fl = ((altitude_ft + 50) / 100).max(180);
        format!("FL{:03}", fl)
    } else {
        format!("{} ft", altitude_ft)
    }
}

/// Anonymisiert das Callsign: "GSG3184" → "GSG-Flight".
/// Wenn `anonymize=false`, bleibt das Callsign 1:1.
/// Wenn das Callsign keinen Buchstaben-Prefix hat, wird "Flight" zurueckgegeben.
pub fn maybe_anonymize_callsign(callsign: &str, anonymize: bool) -> String {
    if !anonymize {
        return callsign.to_string();
    }
    // ICAO-Prefix extrahieren (= fuehrende Buchstaben), z.B. "GSG" aus "GSG3184"
    let prefix: String = callsign.chars().take_while(|c| c.is_alphabetic()).collect();
    if prefix.is_empty() {
        "Flight".to_string()
    } else {
        format!("{}-Flight", prefix.to_uppercase())
    }
}

/// Erste Zeile der Presence: "GSG3184 · EDDB → KMRH" (oder anonymisiert).
/// Fehlende Felder werden mit "—" ersetzt damit die Zeile nie leer ist.
pub fn build_details(input: &PresenceInput, anonymize: bool) -> String {
    let callsign = maybe_anonymize_callsign(&input.callsign, anonymize);
    let dep = if input.dep_icao.is_empty() {
        "—".to_string()
    } else {
        input.dep_icao.clone()
    };
    let arr = if input.arr_icao.is_empty() {
        "—".to_string()
    } else {
        input.arr_icao.clone()
    };
    format!("{} · {} → {}", callsign, dep, arr)
}

/// Zweite Zeile der Presence: phase-spezifisch.
///   `CRUISE · A320 · FL360`     (FL-Phasen)
///   `TAXI OUT · A320`            (Boden-Phasen)
///   `ARRIVED · A320 · KMRH`      (Sonderfall: Arrived zeigt arr_icao)
///   `⚠ REJECTED TAKE-OFF · A320` (Warn-Phasen mit Prefix)
///
/// `sim_lost=true` haengt " · ⚠ Sim getrennt" hinten an (LE8).
pub fn build_state(input: &PresenceInput, sim_lost: bool) -> String {
    let aircraft = if input.aircraft.is_empty() {
        "Aircraft".to_string()
    } else {
        input.aircraft.clone()
    };
    let label = phase_to_label(input.phase);

    let mut text = if input.phase == FlightPhase::Arrived {
        let arr = if input.arr_icao.is_empty() {
            "—".to_string()
        } else {
            input.arr_icao.clone()
        };
        format!("{} · {} · {}", label, aircraft, arr)
    } else if phase_uses_altitude(input.phase) {
        let alt_text = input
            .altitude_ft
            .map(format_altitude)
            .unwrap_or_else(|| "—".to_string());
        format!("{} · {} · {}", label, aircraft, alt_text)
    } else {
        format!("{} · {}", label, aircraft)
    };

    if sim_lost {
        text.push_str(" · ⚠ Sim getrennt");
    }
    text
}
