// v1.2.3 (#Hoppie-PDC-CPDLC) — is anybody actually there?
//
// The protocol gives no delivery or read receipt: `ok` on a send only
// means the message landed in the addressee's mailbox. Whether a
// controller is even logged on is the one thing we can establish, via a
// side-effect-free `ping`.
//
// This matters because the failure is otherwise completely silent: a
// clearance request to an airport with no controller online just sits
// there, and the pilot waits for an answer that was never coming.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/ipc";

export interface StationStatus {
  station: string;
  online: boolean;
  /** Set when the CHECK failed, not when the station is merely absent.
   *  "Couldn't ask" must never be shown as "offline". */
  reason: string | null;
}

/** Re-check this often while the panel is open. Generous: `ping` is
 *  free of side effects but still a request to a volunteer-run service,
 *  and controllers don't come and go by the second. */
const RECHECK_MS = 60_000;

/** Wait for typing to settle before asking about a half-typed ICAO. */
const DEBOUNCE_MS = 600;

/**
 * Track whether `station` is logged on. Returns `null` while unknown —
 * before the first check, or when there is nothing to check.
 */
export function useStationOnline(station: string, active: boolean): StationStatus | null {
  const [status, setStatus] = useState<StationStatus | null>(null);
  const wanted = useRef(station);
  wanted.current = station;

  const check = useCallback(() => {
    const target = wanted.current.trim();
    if (!target) {
      setStatus(null);
      return;
    }
    void invoke<StationStatus>("hoppie_ping_station", { station: target })
      .then((s) => {
        // A reply for a station the pilot has since typed away from is
        // stale — dropping it avoids showing the wrong one's state.
        if (s.station.toUpperCase() === wanted.current.trim().toUpperCase()) {
          setStatus(s);
        }
      })
      .catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    if (!active || station.trim() === "") {
      setStatus(null);
      return;
    }
    setStatus(null);
    const debounce = window.setTimeout(check, DEBOUNCE_MS);
    const interval = window.setInterval(check, RECHECK_MS);
    return () => {
      window.clearTimeout(debounce);
      window.clearInterval(interval);
    };
  }, [station, active, check]);

  return status;
}
