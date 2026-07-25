// v1.3.0 (#Hoppie-PDC-CPDLC) — App-level CPDLC attention signal.
//
// Lives at the App root, not inside CpdlcPanel, because a pilot on the
// Cockpit tab (or with the window in the background entirely) still has
// to hear and see that ATC called. The panel-scoped hook could only ever
// alert someone already looking at the messages.
//
// Two distinct counts, because they answer different questions:
//   - `unseenCount` drives the banner: messages that arrived since the
//     pilot last opened the tab. Opening the tab clears it.
//   - `pendingCount` drives the tab badge: uplinks still awaiting a
//     reply. Only actually replying clears it.
// Collapsing them into one number is what made the banner stick around
// after "open" — it was reporting unanswered, not unseen.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/ipc";
import notifySoundUrl from "../assets/sounds/cpdlc-alert.mp3";

interface HoppieSettings {
  enabled: boolean;
  notify_sound: boolean;
}

interface HoppieStatus {
  connected: boolean;
  /** Uplinks the pilot still owes ATC an answer for. Deliberately NOT
   *  `pending_response_count`, which also counts our own outstanding
   *  requests — those need no reply from us. */
  pending_uplink_count: number;
}

interface ThreadEntry {
  direction: "sent" | "received";
}

const POLL_MS = 5000;

export function useHoppieAttention(active: boolean): {
  pendingCount: number;
  unseenCount: number;
  markSeen: () => void;
} {
  const [enabled, setEnabled] = useState(false);
  const [notifySound, setNotifySound] = useState(true);
  const [pendingCount, setPendingCount] = useState(0);
  const [unseenCount, setUnseenCount] = useState(0);
  const receivedSeen = useRef<number | null>(null);
  const chime = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    if (!active) return;
    void invoke<HoppieSettings>("hoppie_get_settings").then((s) => {
      setEnabled(s.enabled);
      setNotifySound(s.notify_sound);
    });
  }, [active]);

  const markSeen = useCallback(() => setUnseenCount(0), []);

  useEffect(() => {
    if (!active || !enabled) {
      setPendingCount(0);
      setUnseenCount(0);
      receivedSeen.current = null;
      return;
    }
    const poll = () => {
      void invoke<HoppieStatus>("hoppie_status")
        .then((s) => {
          setPendingCount(s.pending_uplink_count);
          // Reception dropped: the thread is gone with it, so the
          // baseline has to go too. Keeping the old count meant the
          // first N messages after reconnecting were silent — N being
          // however many had arrived before — because the count came
          // back lower than the stale baseline.
          if (!s.connected) receivedSeen.current = null;
        })
        .catch(() => undefined);
      void invoke<ThreadEntry[]>("hoppie_get_thread")
        .then((entries) => {
          const received = entries.filter((e) => e.direction === "received").length;
          // First poll of a session establishes the baseline instead of
          // alerting for the entire backlog at once.
          if (receivedSeen.current === null) {
            receivedSeen.current = received;
            return;
          }
          // Defensive: a shorter thread than the baseline can only mean
          // it was reset underneath us. Re-baseline instead of going
          // negative and swallowing the next N alerts.
          if (received < receivedSeen.current) {
            receivedSeen.current = received;
            return;
          }
          // EVERY inbound message alerts — including a logon accept. The
          // pilot is entitled to be told about anything that arrives, and
          // silently filtering "unimportant" traffic is not our call.
          const fresh = received - receivedSeen.current;
          if (fresh > 0) {
            receivedSeen.current = received;
            setUnseenCount((n) => n + fresh);
            // One chime per poll, however many messages arrived, and
            // reusing a single element so a burst can't stack several
            // overlapping playbacks on top of each other.
            if (notifySound) {
              const audio = (chime.current ??= new Audio(notifySoundUrl));
              audio.currentTime = 0;
              void audio.play().catch(() => undefined);
            }
          }
        })
        .catch(() => undefined);
    };
    poll();
    const id = window.setInterval(poll, POLL_MS);
    return () => window.clearInterval(id);
  }, [active, enabled, notifySound]);

  return { pendingCount, unseenCount, markSeen };
}
