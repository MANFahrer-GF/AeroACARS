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
    /** Wie oft dieselbe Auffälligkeit in derselben Phase auftrat. Der
     *  Server fasst Gleichlautendes seit dem 12.08.2026 zusammen, statt es
     *  zu stapeln — fehlt das Feld (älterer Server), zählt der Eintrag
     *  einfach als einer. */
    anzahl?: number;
  };
}

export interface IntegrityState {
  /** Aktuelle Session-Severity (= max(effective_severity) aller Flags). */
  sessionSeverity: IntegritySeverity;
  /** Liste der zuletzt empfangenen Flags (max 50; FIFO). */
  recentFlags: IntegrityFlagPayload[];
  /**
   * Der schwerste bisher gemeldete Fall.
   *
   * Feldbefund 12.08.2026: Das Banner zeigte immer den ZULETZT
   * eingegangenen Flag. Bei kritischer Sitzung stand deshalb im roten
   * Kasten ein harmloser Hinweis ("Tankvorgang beim Boarding") — die
   * Überschrift schrie, der Text passte nicht dazu, und der eigentliche
   * Grund war nirgends zu sehen.
   */
  schwersterFlag: IntegrityFlagPayload | null;
  /**
   * Wie oft insgesamt etwas aufgetreten ist, Serverzähler eingerechnet.
   *
   * QS-Runde 2 (12.08.2026): Hier wurde zunächst schlicht aufaddiert
   * (`prev + anzahl`). Das war falsch, sobald der Server gleichlautende
   * Funde zusammenfasst: er schickt bei jedem neuen Vorkommen den
   * VOLLSTÄNDIGEN Eintrag mit dem inzwischen gewachsenen Zähler. Drei
   * gleiche Funde kamen also als 1, dann 2, dann 3 herein — addiert
   * ergab das 6 statt 3, und im Cockpit stand die doppelte Zahl.
   * Deshalb: je Merkmal den höchsten gemeldeten Stand halten und diese
   * Stände summieren.
   */
  meldungenGesamt: number;
  /** Wahr wenn der Benutzer den Banner dismissed hat (UI-only, kein
   *  persistenter State; Reset auf neue critical). */
  dismissed: boolean;
  /**
   * Zu welchem Flug die gesammelten Meldungen gehören.
   *
   * QS 12.08.2026: Der Hook sammelte über die gesamte Laufzeit der App —
   * `clear()` existiert, wird aber von niemandem gerufen (per Mutation
   * geprüft: Löschen der Funktion fiel nirgends auf). Beim zweiten Flug
   * einer Sitzung stand deshalb die Meldung des ersten noch im Zustand,
   * und der Text "x-mal in diesem Flug" zählte quer über Flüge. Mit dem
   * Umbau auf "zeige den schwersten Fall" wäre daraus ein echter Fehler
   * geworden: der schwerste Fall des VORIGEN Fluges hätte den aktuellen
   * überstrahlt. Jetzt hängt der Reset an der Flugsitzung selbst — kein
   * Aufrufer, der vergessen werden kann.
   */
  sessionId: number | null;
}

const INITIAL: IntegrityState = {
  sessionSeverity: "info",
  recentFlags: [],
  schwersterFlag: null,
  meldungenGesamt: 0,
  dismissed: false,
  sessionId: null,
};

const severityOrder: Record<IntegritySeverity, number> = {
  info: 1, anomaly: 2, critical: 3,
};

/**
 * Wie oft ist etwas aufgetreten? Je Merkmal (Art + Phase) zählt der
 * höchste vom Server gemeldete Stand; die Stände werden summiert.
 * Siehe die Erklärung an `meldungenGesamt`.
 */
function zaehleMeldungen(alle: IntegrityFlagPayload[]): number {
  const hoechster = new Map<string, number>();
  for (const p of alle) {
    const schluessel = `${p.flag.type}|${p.flag.phase}`;
    const stand = p.flag.anzahl ?? 1;
    hoechster.set(schluessel, Math.max(hoechster.get(schluessel) ?? 0, stand));
  }
  let summe = 0;
  for (const n of hoechster.values()) summe += n;
  return summe;
}

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
        const fn = await listen<IntegrityFlagPayload>("integrity-flag", (event) => {
          if (!mounted) return;
          const p = event.payload;
          setState((prevRoh) => {
            // Neuer Flug → bei null anfangen.
            const prev = prevRoh.sessionId != null && prevRoh.sessionId !== p.session_id
              ? { ...INITIAL, sessionId: p.session_id }
              : prevRoh;
            const newSev = severityOrder[p.session_effective_severity] > severityOrder[prev.sessionSeverity]
              ? p.session_effective_severity
              : prev.sessionSeverity;
            const bisher = prev.schwersterFlag;
            const neuerIstSchwerer =
              !bisher ||
              severityOrder[p.flag.effective_severity] >
                severityOrder[bisher.flag.effective_severity];
            const alle = [p, ...prev.recentFlags].slice(0, 50);
            const next: IntegrityState = {
              sessionId: p.session_id,
              sessionSeverity: newSev,
              recentFlags: alle,
              schwersterFlag: neuerIstSchwerer ? p : bisher,
              meldungenGesamt: zaehleMeldungen(alle),
              // Re-show banner if a new CRITICAL arrives — even if previously
              // dismissed (don't let pilot hide a sim-state-reset).
              dismissed: p.session_effective_severity === "critical" ? false : prev.dismissed,
            };
            return next;
          });
        });
        // v0.19.x FIX: on a fast unmount (guaranteed every mount in React
        // StrictMode dev), the cleanup below can run BEFORE this await
        // resolves — `unlisten` would still be undefined when cleanup
        // checks it, and the assignment below would then set it AFTER
        // cleanup already ran, so nothing would ever call it: the Tauri
        // listener leaked permanently. If we're already unmounted by the
        // time the promise resolves, unregister immediately instead of
        // stashing it in a variable cleanup already passed.
        if (!mounted) {
          fn();
        } else {
          unlisten = fn;
        }
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
