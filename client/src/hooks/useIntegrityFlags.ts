// v0.13.0 Slice 6 — Mid-Session-Integrity-Banner Hook
//
// Lauscht auf das Tauri-Event "integrity-flag" (= published vom Rust-
// MQTT-Subscriber im aeroacars-mqtt-Crate sobald der Recorder ein
// neues Integrity-Flag auf aeroacars/<va>/<pilot>/integrity_flag
// gepublisht hat).
//
// Stream F (Resume-Policy) Slice 6 ist hier MVP: wir akkumulieren die
// jüngsten Flags + die aktuelle session_effective_severity in einem
// kleinen State, plus ein dismiss-Mechanismus. Die volle Resume-Tier-
// Klassifikation (LE23–LE26) kommt in einem späteren Slice.
//
// Spec: docs/spec/v0.13.0-mid-session-integrity-and-resume-policy.md

import { useEffect, useState, useCallback } from "react";
import { listen, type UnlistenFn } from "../lib/ipc";

export type IntegritySeverity = "info" | "anomaly" | "critical";

export interface IntegrityFlagPayload {
  session_id: number;
  session_effective_severity: IntegritySeverity;
  flag: {
    type: string;
    base_severity: IntegritySeverity;
    effective_severity: IntegritySeverity;
    ts: number;
    phase: string;
    detail: Record<string, unknown>;
    mode: "continuous" | "gap_edge";
    detector: string;
    ruleset_version?: string;
  };
}

export interface IntegrityState {
  /** Aktuelle Session-Severity (= max(effective_severity) aller Flags). */
  sessionSeverity: IntegritySeverity;
  /** Liste der zuletzt empfangenen Flags (max 50; FIFO). */
  recentFlags: IntegrityFlagPayload[];
  /** Wahr wenn der Benutzer den Banner dismissed hat (UI-only, kein
   *  persistenter State; Reset auf neue critical). */
  dismissed: boolean;
}

const INITIAL: IntegrityState = {
  sessionSeverity: "info",
  recentFlags: [],
  dismissed: false,
};

const severityOrder: Record<IntegritySeverity, number> = {
  info: 1, anomaly: 2, critical: 3,
};

export function useIntegrityFlags(): {
  state: IntegrityState;
  dismiss: () => void;
  clear: () => void;
} {
  const [state, setState] = useState<IntegrityState>(INITIAL);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let mounted = true;

    (async () => {
      try {
        unlisten = await listen<IntegrityFlagPayload>("integrity-flag", (event) => {
          if (!mounted) return;
          const p = event.payload;
          setState((prev) => {
            const newSev = severityOrder[p.session_effective_severity] > severityOrder[prev.sessionSeverity]
              ? p.session_effective_severity
              : prev.sessionSeverity;
            const next: IntegrityState = {
              sessionSeverity: newSev,
              recentFlags: [p, ...prev.recentFlags].slice(0, 50),
              // Re-show banner if a new CRITICAL arrives — even if previously
              // dismissed (don't let pilot hide a sim-state-reset).
              dismissed: p.session_effective_severity === "critical" ? false : prev.dismissed,
            };
            return next;
          });
        });
      } catch (err) {
        console.warn("[integrity] failed to listen for integrity-flag events:", err);
      }
    })();

    return () => {
      mounted = false;
      if (unlisten) unlisten();
    };
  }, []);

  const dismiss = useCallback(() => {
    setState((prev) => ({ ...prev, dismissed: true }));
  }, []);

  // Reset for new session — caller invokes when a fresh flight begins.
  const clear = useCallback(() => {
    setState(INITIAL);
  }, []);

  return { state, dismiss, clear };
}
