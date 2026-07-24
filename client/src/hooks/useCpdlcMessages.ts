// v1.3.0 (#Hoppie-PDC-CPDLC) — message-history hook for the CPDLC tab.
//
// Phase 2: polls `hoppie_get_thread` every 5s while mounted (same cadence
// DiscordRpcPanel already uses for its status poll). Phase 3 upgrades this
// to real backend push (`listen("cpdlc-message", ...)`, mirroring
// useIntegrityFlags.ts) once the poller actually emits that event — no
// call-site changes needed then, this hook's return shape stays the same.
//
// The in-app sound alert is pulled forward from Phase 3 here (asset +
// setting already existed) since it's cheap and self-contained — it just
// plays `notify_sound_url` when a poll surfaces a new "received" entry.
// The OS-native toast (tauri-plugin-notification, needs its own
// permission flow) stays Phase 3.

import { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "../lib/ipc";
import notifySoundUrl from "../assets/sounds/cpdlc-alert.mp3";

export interface ThreadEntry {
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
}

const POLL_MS = 5000;

export function useCpdlcMessages(
  active: boolean,
  notifySound: boolean,
): {
  messages: ThreadEntry[];
  refresh: () => void;
} {
  const [messages, setMessages] = useState<ThreadEntry[]>([]);
  const seenCount = useRef(0);
  const notifySoundRef = useRef(notifySound);
  notifySoundRef.current = notifySound;

  const refresh = useCallback(() => {
    void invoke<ThreadEntry[]>("hoppie_get_thread")
      .then((next) => {
        const hasNewReceived =
          next.length > seenCount.current &&
          next.slice(seenCount.current).some((m) => m.direction === "received");
        seenCount.current = next.length;
        setMessages(next);
        if (hasNewReceived && notifySoundRef.current) {
          void new Audio(notifySoundUrl).play().catch(() => undefined);
        }
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!active) {
      seenCount.current = 0;
      return;
    }
    refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [active, refresh]);

  return { messages, refresh };
}
