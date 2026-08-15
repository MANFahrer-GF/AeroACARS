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

/** Das anzuzeigende Rufzeichen: Airline-Code plus Bezeichner, ohne Doppelung.
 *
 *  Manche VAs legen im `callsign`-Feld nur den Bezeichner ab ("7ME"), andere
 *  das volle Rufzeichen ("GEC4TK"). Wer stur `airline_icao + ident`
 *  zusammensetzt, macht aus dem zweiten Fall "GECGEC4TK". Der Panel-Server
 *  fängt das seit v1.5.6 ab (`with_display_callsign` in panel_server.rs,
 *  Feldbefund Thomas), die Oberflaeche tat es nicht — dieselbe Regel lief an
 *  zwei Orten auseinander.
 *
 *  Betroffen sind auch die Landungs-Ansichten: dort traegt `flight_number`
 *  bereits den aufgeloesten Bezeichner (`build_landing_record` ruft
 *  `resolve_flight_ident`), also greift die Doppelung dort genauso.
 *
 *  Bewusst dieselbe Praefix-Pruefung wie in Rust (`ident.starts_with`),
 *  damit die beiden nicht erneut auseinanderlaufen. Der Trenner bleibt leer
 *  wie bisher in der App; das HUD setzt dort ein Leerzeichen.
 */
export function displayCallsign(
  airlineIcao: string | null | undefined,
  flightNumber: string,
  callsign?: string | null,
): string {
  const ident = resolveFlightIdent(flightNumber, callsign);
  const icao = airlineIcao?.trim() ?? "";
  if (!icao) return ident;
  if (!ident) return icao;
  return ident.startsWith(icao) ? ident : `${icao}${ident}`;
}
