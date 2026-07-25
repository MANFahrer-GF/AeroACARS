// v1.2.3 (#Hoppie-PDC-CPDLC) — is anything registered under this callsign?
//
// The protocol gives no delivery or read receipt: `ok` on a send only
// means the message landed in the addressee's mailbox. What we CAN ask
// is whether a callsign is currently registered, via a `ping` the docs
// describe as side-effect-free ("does not register the station that is
// pinging as online", and no callsign lock).
//
// Deliberately checked on ENTRY only, not on a timer — the same design
// FlyByWire uses (AcarsConnector.isStationAvailable, called from the
// logon and departure-request pages). Hoppie is volunteer-run and the
// docs ask repeatedly not to hammer it; a periodic badge refresh would
// have roughly doubled our request count for the whole flight.
//
// Note what this can and cannot tell you: a controller's datalink
// callsign is free-text on their side (vSMR defaults it to "EGKK") and
// is advertised in the ATIS, so "nothing registered under EDDF" does NOT
// mean nobody is working EDDF. The wording reflects that.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/ipc";

export interface StationStatus {
  station: string;
  online: boolean;
  /** Set when the CHECK failed, not when nothing was registered.
   *  "Couldn't ask" must never be shown as "nothing there". */
  reason: string | null;
}

/** Wait for typing to settle before asking about a half-typed ICAO. */
const DEBOUNCE_MS = 700;

/**
 * Look up whether `station` is registered, once the pilot stops typing.
 * Returns `null` while unknown — before the first check, or when there
 * is nothing to check.
 */
export function useStationOnline(station: string, active: boolean): StationStatus | null {
  const [status, setStatus] = useState<StationStatus | null>(null);
  const trimmed = station.trim().toUpperCase();

  // Guards a reply that arrives after the pilot moved on, or after the
  // panel closed. Without it a late failure could wipe the badge of a
  // station it never described.
  const generation = useRef(0);

  const check = useCallback((target: string, mine: number) => {
    void invoke<StationStatus>("hoppie_ping_station", { station: target })
      .then((s) => {
        if (generation.current === mine) setStatus(s);
      })
      .catch(() => {
        if (generation.current === mine) setStatus(null);
      });
  }, []);

  useEffect(() => {
    generation.current += 1;
    const mine = generation.current;

    // "SERVER" is the docs' placeholder addressee and the settings
    // default — it is not a facility, so checking it would announce
    // "nothing registered as SERVER" to a pilot who has simply not
    // picked a centre yet.
    if (!active || trimmed === "" || trimmed === "SERVER") {
      setStatus(null);
      return;
    }
    setStatus(null);
    const id = window.setTimeout(() => check(trimmed, mine), DEBOUNCE_MS);
    return () => {
      window.clearTimeout(id);
      // Bumping the generation invalidates any in-flight reply, which is
      // how an unmount mid-request is handled — `invoke` itself can't be
      // cancelled.
      generation.current += 1;
    };
  }, [trimmed, active, check]);

  return status;
}
