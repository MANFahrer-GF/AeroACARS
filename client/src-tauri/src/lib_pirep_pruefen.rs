/// Names of fields that failed validation, returned to the UI as the
/// `details` payload of the `flight_validation_failed` UiError. The UI looks
/// up `flight.validation.<key>` for the localized message and decides
/// whether the user can still file as a manual PIREP.
fn validate_for_filing(
    flight: &ActiveFlight,
    stats: &FlightStats,
    elapsed_minutes: i32,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if elapsed_minutes < crate::grenzwerte::grenzwerte().abgabe.min_flight_time_min.wert {
        missing.push("flight_time");
    }
    if stats.distance_nm < crate::grenzwerte::grenzwerte().abgabe.min_distance_nm.wert {
        missing.push("distance");
    }
    if stats.position_count < crate::grenzwerte::grenzwerte().abgabe.min_position_count.wert {
        missing.push("position_count");
    }
    if stats.block_fuel_kg.is_none() || stats.last_fuel_kg.is_none() {
        missing.push("fuel");
    }
    // Must have actually arrived at a parking position. Anything before
    // BlocksOn means engines weren't shut down on the ground at the gate.
    if !matches!(
        stats.phase,
        FlightPhase::BlocksOn | FlightPhase::Arrived | FlightPhase::PirepSubmitted
    ) {
        missing.push("not_arrived");
    }
    // Bid is required for a clean ACARS PIREP. `flight_adopt` falls back to
    // bid_id=0 when no matching bid is found server-side, which means the
    // pilot is flying a PIREP that no longer has a booking attached. That
    // can happen legitimately (admin removed the bid) but it's never an
    // ACARS-clean situation — admin must review.
    if flight.bid_id <= 0 {
        missing.push("no_bid");
    }
    missing
}

/// Open (or create) the offline position queue under the OS-appropriate
/// app data directory. None when we can't resolve the dir — caller
/// treats that as "no queue available, drop the offline-resilience
/// feature for this tick" rather than blowing up the streamer.
fn open_position_queue(app: &AppHandle) -> Option<PositionQueue> {
    let dir = app.path().app_data_dir().ok()?;
    PositionQueue::open(dir).ok()
}

// v0.6.0: enqueue_position_offline ist entfallen — der phpVMS-Worker
// pusht failende Positionen zurück in die Memory-Outbox. Persistierung
// passiert durch persist_outbox() bei Worker-Stop / Backlog>500.
// Kein per-Position-File-IO mehr im POST-Path.
