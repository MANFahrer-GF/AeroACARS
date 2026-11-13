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
      /** `null`/fehlend, wo die Webapp keine Rate zeigt: Anzeige `--`, nie 0. */
      landing_rate_fpm?: number | null;
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
/** Abstand der Nachfragen, solange „gelandet“ noch ohne Note ist (live setzt
 *  Note und angeglichene Sinkrate erst mit dem PIREP des Gastgebers). */
export const SC_NACHFRAGE_NOTE_MS = 60_000;
/** Höchstens so viele Abfragen ohne Note (60 min). */
export const SC_HOECHSTENS_NOTE = 60;
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

/**
 * Welcher Stand gilt nach einer neuen Antwort? Dieselbe Regel wie
 * `sc_ersetzt` in Rust: „ausstehend“/„keine Crew“ ersetzt nie ein
 * „gelandet“; dieselbe Landung mit Note wird nie durch eine Antwort ohne
 * Note ersetzt; sonst gilt die neuere (Note und angeglichene Sinkrate kommen
 * mit dem PIREP des Gastgebers, nach einem Touch-and-Go eine neue
 * `touchdown_id`).
 */
export function scUebernehmen(alt: ScLandung | null, neu: ScLandung): ScLandung {
  if (alt?.status !== "gelandet") return neu;
  if (neu.status !== "gelandet") return alt;
  if (alt.touchdown_id === neu.touchdown_id && alt.landing_score != null && neu.landing_score == null) return alt;
  return neu;
}

/** Gastgeber gelandet, live führt aber noch keine Note. */
function ohneNote(l: ScLandung | null): boolean {
  return l?.status === "gelandet" && l.landing_score == null;
}

/** Partner einer Crew: gelandet oder ausstehend. */
export function istPartner(l: ScLandung | null): l is ScPartnerLandung {
  return l?.status === "gelandet" || l?.status === "ausstehend";
}

/**
 * Fragt live für den eigenen Flug `pirepId`. Mit `nachfragen` alle
 * {@link SC_NACHFRAGE_MS}, solange „ausstehend“ (höchstens
 * {@link SC_HOECHSTENS} Abfragen), und alle {@link SC_NACHFRAGE_NOTE_MS},
 * solange „gelandet“ noch ohne Note ist (höchstens {@link SC_HOECHSTENS_NOTE});
 * „gelandet“ mit Note und „keine Crew“ beenden.
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
    let ohneNoteAufrufe = 0;
    let letzte: ScLandung | null = null;
    const frage = async () => {
      aufrufe++;
      try {
        const neu = await invoke<ScLandung>("sc_landung_gastgeber", { pirepId });
        if (weg) return;
        const l = scUebernehmen(letzte ?? bekannt.get(pirepId) ?? null, neu);
        letzte = l;
        bekannt.set(pirepId, l);
        setStand({ id: pirepId, l });
      } catch {
        if (weg) return;
        fehler++;
      }
      if (!nachfragen) return;
      let takt: number | null = null;
      if (ohneNote(letzte)) {
        if (ohneNoteAufrufe++ < SC_HOECHSTENS_NOTE) takt = SC_NACHFRAGE_NOTE_MS;
      } else if (
        aufrufe < SC_HOECHSTENS &&
        (letzte?.status === "ausstehend" || (letzte === null && fehler < SC_FEHLVERSUCHE))
      ) {
        takt = SC_NACHFRAGE_MS;
      }
      if (takt != null) timer = window.setTimeout(() => void frage(), takt);
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

// ── Landungsübersicht (wie die Webapp-Historie) ──────────────────────────
// Der Partnerflug trägt das Kennzeichen am Datensatz (`shared_cockpit`). Die
// Zeile zeigt die Landung des Gastgebers mit „SC“; weder sie noch die eigene
// Messung zählt in Durchschnitt, Kurve, Kategorien oder Bestwert.

/** Kennzeichen am Datensatz, nur für Partnerflüge. */
export function scAmDatensatz(r: { shared_cockpit?: ScPartnerLandung | null }): ScPartnerLandung | null {
  return istPartner(r.shared_cockpit ?? null) ? (r.shared_cockpit as ScPartnerLandung) : null;
}

/** Nur die eigenen Landungen (ohne Partnerflüge) — Grundlage aller Kennzahlen. */
export function eigeneLandungen<T extends { shared_cockpit?: ScPartnerLandung | null }>(records: readonly T[]): T[] {
  return records.filter((r) => scAmDatensatz(r) == null);
}

/** Anzahl der Partnerflüge (für „3 + 1 Shared Cockpit“). */
export function scAnzahl(records: readonly { shared_cockpit?: ScPartnerLandung | null }[]): number {
  return records.length - eigeneLandungen(records).length;
}
