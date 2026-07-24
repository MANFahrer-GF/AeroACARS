// v1.3.0 (#Hoppie-PDC-CPDLC) — lightweight "does CPDLC need attention"
// signal for App.tsx (tab badge + top-level banner), independent of
// whether the CPDLC tab itself is open. Deliberately only polls
// `hoppie_status` (cheap, already has `pending_response_count`) rather
// than the full message thread — no need to duplicate `useCpdlcMessages`'
// heavier poll at the App level.

import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";

interface HoppieSettings {
  enabled: boolean;
}

interface HoppieStatus {
  connected: boolean;
  pending_response_count: number;
}

const POLL_MS = 5000;

export function useHoppieAttention(active: boolean): { pendingCount: number } {
  const [enabled, setEnabled] = useState(false);
  const [pendingCount, setPendingCount] = useState(0);

  useEffect(() => {
    if (!active) return;
    void invoke<HoppieSettings>("hoppie_get_settings").then((s) => setEnabled(s.enabled));
  }, [active]);

  useEffect(() => {
    if (!active || !enabled) {
      setPendingCount(0);
      return;
    }
    const poll = () => {
      void invoke<HoppieStatus>("hoppie_status")
        .then((s) => setPendingCount(s.pending_response_count))
        .catch(() => undefined);
    };
    poll();
    const id = window.setInterval(poll, POLL_MS);
    return () => window.clearInterval(id);
  }, [active, enabled]);

  return { pendingCount };
}
