// Shared Cockpit (B4): die Landung des Gastgebers für den Partner.
//
// Eine Quelle: live führt die maßgebliche Landung des Gastgebers (dieselbe,
// die die Webapp zeigt). Der Client rechnet nichts nach; Rust holt sie mit
// dem Pilot-Zugang (`sc_landung_gastgeber`, Fähigkeit `sc-landung-1`).
// Ohne Fähigkeit, bei „keine Crew“ oder bei Fehlern bleibt alles wie bisher.

import { useEffect, useState } from "react";
import { invoke } from "./ipc";

export type ScRolle = "PF" | "PM";

export type ScLandung =
  | {
      status: "gelandet";
      touchdown_id: number;
      fuehrender_pirep_id: string;
      gastgeber_name: string | null;
      rolle_gastgeber: ScRolle | null;
      landing_rate_fpm: number;
      landing_score: number | null;
      g_force: number | null;
      bounce_count: number | null;
      score_text: string | null;
    }
  | {
      status: "ausstehend";
      fuehrender_pirep_id: string | null;
      gastgeber_name: string | null;
      rolle_gastgeber: ScRolle | null;
    }
  | { status: "keine_crew" };

export type ScPartnerLandung = Exclude<ScLandung, { status: "keine_crew" }>;

/** Abstand der Nachfragen, solange der Gastgeber noch nicht gelandet ist. */
export const SC_NACHFRAGE_MS = 20_000;
/** Höchstens so viele Abfragen je Flug und Ansicht (gut 15 min). */
export const SC_HOECHSTENS = 45;
/** Fehlversuche ohne jede Antwort, danach bleibt es still wie bisher. */
const SC_FEHLVERSUCHE = 3;

// Letzter bekannter Stand je eigener Nummer, über Reiterwechsel hinweg: Die
// Ansicht zeigt ihn sofort (die eigene Note blitzt beim Partner nicht auf),
// und die Abgabe-Meldung kennt ihn noch, wenn das Cockpit neu eingehängt wird.
const bekannt = new Map<string, ScLandung>();
let letzterFlug: string | null = null;

/** Das Cockpit merkt sich den laufenden Flug für die Abgabe-Meldung. */
export function merkeFlug(pirepId: string): void {
  letzterFlug = pirepId;
}

/** War der zuletzt laufende Flug der eines Partners? */
export function letzterFlugAlsPartner(): boolean {
  return letzterFlug != null && istPartner(bekannt.get(letzterFlug) ?? null);
}

/** Nur für Tests. */
export function scVergessen(): void {
  bekannt.clear();
  letzterFlug = null;
}

/** Partner einer Crew: gelandet oder ausstehend. */
export function istPartner(l: ScLandung | null): l is ScPartnerLandung {
  return l?.status === "gelandet" || l?.status === "ausstehend";
}

/**
 * Fragt live für den eigenen Flug `pirepId`. Mit `nachfragen` alle
 * {@link SC_NACHFRAGE_MS}, solange „ausstehend“ (höchstens
 * {@link SC_HOECHSTENS} Abfragen); „gelandet“ und „keine Crew“ beenden.
 * `anlass` erzwingt eine neue Abfrage (z. B. das Aufsetzen).
 * `null` = nichts bekannt, Anzeige wie bisher.
 */
export function useScLandung(
  pirepId: string | null,
  { nachfragen, anlass = null }: { nachfragen: boolean; anlass?: string | null },
): ScLandung | null {
  const [stand, setStand] = useState<{ id: string; l: ScLandung } | null>(null);
  useEffect(() => {
    if (!pirepId) return;
    let weg = false;
    let timer: number | undefined;
    let aufrufe = 0;
    let fehler = 0;
    let letzte: ScLandung | null = null;
    const frage = async () => {
      aufrufe++;
      try {
        const l = await invoke<ScLandung>("sc_landung_gastgeber", { pirepId });
        if (weg) return;
        letzte = l;
        bekannt.set(pirepId, l);
        setStand({ id: pirepId, l });
      } catch {
        if (weg) return;
        fehler++;
      }
      const weiter =
        nachfragen &&
        aufrufe < SC_HOECHSTENS &&
        (letzte?.status === "ausstehend" || (letzte === null && fehler < SC_FEHLVERSUCHE));
      if (weiter) timer = window.setTimeout(() => void frage(), SC_NACHFRAGE_MS);
    };
    void Promise.resolve(invoke<{ faehigkeiten?: unknown }>("paket_info"))
      .then((i) => {
        if (!weg && Array.isArray(i?.faehigkeiten) && i.faehigkeiten.includes("sc-landung-1")) void frage();
      })
      .catch(() => {});
    return () => {
      weg = true;
      window.clearTimeout(timer);
    };
  }, [pirepId, nachfragen, anlass]);
  if (stand && stand.id === pirepId) return stand.l;
  return pirepId ? (bekannt.get(pirepId) ?? null) : null;
}
