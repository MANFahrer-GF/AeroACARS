import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import type { SimSnapshot, SimStatus } from "../types";

/**
 * Centralised simulator polling. Owned at the App level so every tab
 * (Cockpit, Briefing, Settings) sees the same state without duplicate
 * IPC. The polling cadence (500 ms) matches what `SimPanel` used to do
 * directly — feel free to tighten if a panel ever needs a faster refresh
 * (the bottleneck is the SimConnect adapter, not the IPC).
 */
export function useSimSession(): {
  status: SimStatus | null;
  snapshot: SimSnapshot | null;
} {
  const [status, setStatus] = useState<SimStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    async function poll() {
      try {
        const next = await invoke<SimStatus>("sim_status");
        if (cancelled) return;
        setStatus(next);
      } catch {
        // ignore — IPC errors are transient on dev rebuilds
      }
    }
    void poll();
    timer = setInterval(poll, 500);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, []);

  return { status, snapshot: status?.snapshot ?? null };
}
