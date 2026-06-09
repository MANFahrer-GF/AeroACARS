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
 * Mini route map: departure pin on the left, arrival pin on the right,
 * a soft accent line between them, and a small plane glyph that
 * tracks the live position along the route.
 *
 * Implementation: HTML/CSS layout (not SVG) so the pin circles and
 * plane glyph stay perfectly round regardless of container width.
 * The previous SVG version with preserveAspectRatio="none" stretched
 * everything horizontally, producing oval pins on wide screens.
 *
 * Projection is intentionally simple — straight-line interpolation
 * between the two endpoints in lat/lon space. Correct enough for
 * short/medium-haul; a true Mercator great circle is a follow-up if
 * VAs need accurate transatlantic shapes.
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
    <div className="route-map">
      <div className="route-map__track">
        <div className="route-map__line" />
        <div
          className="route-map__line-fill"
          style={{ width: `${trackProgressPct}%` }}
        />

        <div className="route-map__pin route-map__pin--dpt">
          <span className="route-map__icao">{dpt.icao}</span>
          {dptGate && <span className="route-map__gate">{dptGate}</span>}
          <span className="route-map__dot" />
        </div>

        <div className="route-map__pin route-map__pin--arr">
          <span className="route-map__icao">{arr.icao}</span>
          {arrGate && <span className="route-map__gate">{arrGate}</span>}
          <span className="route-map__dot" />
        </div>

        <div
          className="route-map__plane"
          style={{ left: `${trackProgressPct}%` }}
          aria-hidden="true"
        >
          {/* Rotate the plane glyph 90° so the nose points along the
              track toward the arrival pin instead of straight up. */}
          <svg
            viewBox="0 0 24 24"
            width="20"
            height="20"
            style={{ transform: "rotate(90deg)" }}
          >
            <path
              fill="currentColor"
              d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z"
            />
          </svg>
        </div>
        <div
          className="route-map__progress"
          style={{ left: `${trackProgressPct}%` }}
          aria-hidden="true"
        >
          {progressLabel}
        </div>
      </div>
    </div>
  );
}
