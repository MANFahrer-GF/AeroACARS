import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";

interface AirportInfo {
  icao: string;
  name: string;
  lat: number;
  lon: number;
}

const EARTH_RADIUS_NM = 3440.065; // nautical miles

/** Great-circle distance in nautical miles between two lat/lon points. */
function haversineNm(
  lat1: number,
  lon1: number,
  lat2: number,
  lon2: number,
): number {
  const toRad = (deg: number) => (deg * Math.PI) / 180;
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_NM * c;
}

interface Props {
  dptIcao: string;
  arrIcao: string;
  currentLat?: number | null;
  currentLon?: number | null;
  /** Optional gate label under the departure ICAO (e.g. "GATE A 12"). */
  dptGate?: string | null;
  /** Optional gate label under the arrival ICAO. */
  arrGate?: string | null;
}

/**
 * Route progress track for the Cockpit phase card: a thin filled bar
 * between departure and arrival ICAO/gate labels, with a small plane
 * glyph and a percentage readout that track the live position.
 *
 * Progress math is unchanged from the pre-redesign version (straight-
 * line great-circle interpolation, intentionally simple — correct
 * enough for short/medium-haul; a true Mercator great circle is a
 * follow-up if VAs need accurate transatlantic shapes). Only the
 * markup/classNames changed, from the old .route-map__pin layout to
 * the new .route__track/.route__ends layout used by the phase card.
 */
export function RouteMap({
  dptIcao,
  arrIcao,
  currentLat,
  currentLon,
  dptGate,
  arrGate,
}: Props) {
  const [dpt, setDpt] = useState<AirportInfo | null>(null);
  const [arr, setArr] = useState<AirportInfo | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function fetchOne(icao: string, set: (a: AirportInfo) => void) {
      try {
        const data = await invoke<AirportInfo>("airport_get", { icao });
        if (!cancelled) set(data);
      } catch {
        // Silent — the dashboard already shows the ICAO without coords.
      }
    }
    if (dptIcao) void fetchOne(dptIcao, setDpt);
    if (arrIcao) void fetchOne(arrIcao, setArr);
    return () => {
      cancelled = true;
    };
  }, [dptIcao, arrIcao]);

  if (!dpt || !arr) {
    return null;
  }

  // Progress = 1 − (distance_remaining / total_distance), great-circle.
  // Linear lat/lon projection onto the dpt→arr axis fails for any
  // SID/STAR routing (e.g. EDDP→EDDF westbound, but the SID first
  // takes you south-east — projection goes negative and the plane
  // sticks at 0%). Distance-based progress instead just asks "how
  // much closer to the destination am I", which is what the pilot
  // actually wants to see.
  const totalNm = haversineNm(dpt.lat, dpt.lon, arr.lat, arr.lon);
  let progress = 0;
  if (totalNm > 0 && currentLat != null && currentLon != null) {
    const remainingNm = haversineNm(currentLat, currentLon, arr.lat, arr.lon);
    progress = 1 - remainingNm / totalNm;
    // Clamp to 0..1 — long SIDs that pull you further from the
    // destination than the direct distance shouldn't drive the bar
    // negative, and an overshoot past the airport (rollout, taxi-in
    // past the threshold) shouldn't go > 100%.
    progress = Math.max(0, Math.min(1, progress));
  }

  // Track padding (matches CSS) — the pins sit `--track-padding` from
  // each end, the plane / fill use the same offsets.
  const trackProgressPct = progress * 100;
  const progressLabel = `${Math.round(progress * 100)}%`;

  return (
    <div className="route">
      <div className="route__track">
        <div
          className="route__fill"
          style={{ width: `${trackProgressPct}%` }}
        />
        <div
          className="route__plane"
          style={{ left: `${trackProgressPct}%` }}
          aria-hidden="true"
        >
          {/* Rotate the plane glyph 90° so the nose points along the
              track toward the arrival airport instead of straight up. */}
          <svg viewBox="0 0 24 24" style={{ rotate: "90deg" }}>
            <path
              fill="currentColor"
              d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z"
            />
          </svg>
        </div>
        <div
          className="route__pct"
          style={{ left: `${trackProgressPct}%` }}
          aria-hidden="true"
        >
          {progressLabel}
        </div>
      </div>
      <div className="route__ends">
        <span className="route__end">
          <span className="route__icao">{dpt.icao}</span>
          {dptGate && <span className="route__gate">{dptGate}</span>}
        </span>
        <span className="route__end route__end--arr">
          <span className="route__icao">{arr.icao}</span>
          {arrGate && <span className="route__gate">{arrGate}</span>}
        </span>
      </div>
    </div>
  );
}
