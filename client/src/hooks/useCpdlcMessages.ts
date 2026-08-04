// v1.3.0 (#Hoppie-PDC-CPDLC) — message-history hook for the CPDLC tab.
//
// Phase 2: polls `hoppie_get_thread` while mounted. Phase 3 upgrades this
// to real backend push (`listen("cpdlc-message", ...)`, mirroring
// useIntegrityFlags.ts) once the poller actually emits that event — no
// call-site changes needed then, this hook's return shape stays the same.
//
// Cadence: README §6 says 15-20s. Was 5s (an earlier, faster cadence
// from before the Datalink-3a spec existed, never reconciled with it).
// Note useHoppieAttention.ts polls the same `hoppie_get_thread` endpoint
// independently at its own 5s cadence — that one drives the app-wide
// notification banner/sound, not this screen's history, and is out of
// this audit's scope; the two together still mean Hoppie sees requests
// at roughly the faster of the two rates whenever this tab is open.
//
// This hook itself stays silent — see the `refresh` comment below for
// where the sound alert actually lives and why it isn't here.

import { useEffect, useState, useCallback } from "react";
import { invoke } from "../lib/ipc";

export interface ThreadEntry {
  /** "telex" is PDC traffic; "cpdlc" is datalink. A CPDLC packet that
   *  arrived without a parseable /data2/ header is reported as
   *  kind "cpdlc" with no MIN, so it shows up in the CPDLC log where
   *  the pilot was working — not in the PDC tab. */
  kind: "telex" | "cpdlc";
  direction: "sent" | "received";
  text: string;
  at: string;
  /// Only populated for kind === "cpdlc".
  min: number | null;
  mrn: number | null;
  response: "WU" | "AN" | "R" | "Y" | "N" | "NE" | null;
  element_id: string | null;
  closed: boolean | null;
  /** Already deferred with STANDBY — the key is then hidden. */
  deferred: boolean | null;
  /** This uplink was still open when a handover happened — the station
   *  that sent it is no longer talking to the aircraft. Must render as
   *  greyed-out/inactive and hide its reply buttons: the backend also
   *  refuses to send a reply for it, but the UI should never offer one
   *  in the first place. */
  superseded: boolean | null;
}

const POLL_MS = 15000;

export function useCpdlcMessages(active: boolean): {
  messages: ThreadEntry[];
  refresh: () => void;
  /** When the last poll actually completed (`Date.now()`), for the
   *  status bar's "letzte Abfrage vor {n}s" readout — `null` before the
   *  first one lands. */
  lastFetchedAt: number | null;
} {
  const [messages, setMessages] = useState<ThreadEntry[]>([]);
  const [lastFetchedAt, setLastFetchedAt] = useState<number | null>(null);

  // Deliberately silent. The alert lives in `useHoppieAttention`, which
  // runs at the App root whether or not this tab is mounted. Playing it
  // here too meant two overlapping chimes whenever the tab was open.
  const refresh = useCallback(() => {
    void invoke<ThreadEntry[]>("hoppie_get_thread")
      .then((entries) => {
        setMessages(entries);
        setLastFetchedAt(Date.now());
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!active) return;
    refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [active, refresh]);

  return { messages, refresh, lastFetchedAt };
}
