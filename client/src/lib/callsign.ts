// Callsign/flight-number precedence shared across the dashboard, briefing,
// divert banner, resume banner and live-map components.
//
// phpVMS's own `Flight::atc()` accessor prefers `callsign` over
// `flight_number` when building the ATC callsign — Personal/Free-Flight
// bookings (DisposableSpecial module) often carry `flight_number: 0` (no
// fixable formula value, unlike e.g. block times) and put the real per-flight
// identifier in `callsign` instead. Every place in this client that
// concatenates `${airline_icao}${flight_number}` needs the same fallback,
// or it renders e.g. "CFG0" instead of "CFG7ME" (pilot report Ralf T.,
// GSG0016, 2026-07-28) — this single function is the one place that
// precedence lives, so it can't drift out of sync again between components.
export function resolveFlightIdent(
  flightNumber: string,
  callsign?: string | null,
): string {
  const trimmed = callsign?.trim();
  return trimmed ? trimmed : flightNumber;
}
