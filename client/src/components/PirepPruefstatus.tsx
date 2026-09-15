// Prüfstatus eines PIREPs beim Live-Server — Anzeige im Landungs-Tab.
//
// Anlass: DLH 880 (Sven M, 15.09.2026). Der Flug hing im Integritäts-Gate,
// weil der Server beim Einreichen keine Landung kannte. Der Pilot sah davon
// im Client nichts und erfuhr es erst über Discord; auch die spätere
// Freigabe blieb unsichtbar. Der Server bewertet solche Flüge inzwischen
// selbst neu, sobald die Landung nachgereicht ist — dieser Baustein zeigt
// den jeweils aktuellen Stand.
// Über die gemeinsame IPC-Schicht, damit es auch auf dem Tablet über die
// LAN-Fernbedienung funktioniert (siehe lib/ipc.ts).
import { invoke } from "../lib/ipc";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

export interface PirepPruefstatus {
  pirep_id: string;
  known: boolean;
  score_trust_level: string | null;
  requires_review: boolean;
  review_state: string | null;
  review_decision: string | null;
  reason_codes: string[];
  reviewed_at: number | null;
}

export type PruefArt =
  | "unbekannt"
  | "ok"
  | "in_pruefung"
  | "freigegeben_hinweis"
  | "fehlalarm"
  | "abgelehnt";

/** Eine Entscheidung des Admins geht vor dem automatischen Stand. */
export function pruefArt(s: PirepPruefstatus | null | undefined): PruefArt {
  if (!s || !s.known) return "unbekannt";
  const entscheidung = s.review_decision ?? s.review_state;
  if (entscheidung === "rejected") return "abgelehnt";
  if (entscheidung === "dismissed_false_positive") return "fehlalarm";
  if (entscheidung === "accepted_with_warning") return "freigegeben_hinweis";
  if (s.requires_review) return "in_pruefung";
  return "ok";
}

/** Bekannte Gründe haben einen eigenen Text, alles andere einen allgemeinen. */
export const BEKANNTE_GRUENDE = [
  "no_touchdown_recorded",
  "sim_crash_signature_at_touchdown",
] as const;

export function grundSchluessel(code: string): string {
  return (BEKANNTE_GRUENDE as readonly string[]).includes(code)
    ? `landing.pruefstatus.grund.${code}`
    : "landing.pruefstatus.grund.sonstiges";
}

/** Nachfrage-Takt, solange ein Flug in der Prüfung steht bzw. sonst. */
export const TAKT_OFFEN_MS = 60_000;
export const TAKT_RUHIG_MS = 10 * 60_000;

/**
 * Lädt den Prüfstatus für die übergebenen PIREPs und hält ihn aktuell.
 *
 * `ids` ändert sich im Landungs-Tab alle 5 s als neues Array — der Effekt
 * hängt deshalb am zusammengesetzten Schlüssel, nicht an der Identität.
 * Fehler (offline, nicht provisioniert) lassen den letzten Stand stehen.
 */
export function usePirepPruefstatus(ids: string[]): Record<string, PirepPruefstatus> {
  const [stand, setStand] = useState<Record<string, PirepPruefstatus>>({});
  const schluessel = ids.join(",");
  const standRef = useRef(stand);
  standRef.current = stand;

  useEffect(() => {
    if (schluessel === "") return;
    let aktiv = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const liste = schluessel.split(",");

    const laden = async () => {
      let offen = false;
      try {
        const roh = await invoke<PirepPruefstatus[]>("pirep_pruefstatus", { pirepIds: liste });
        if (!aktiv) return;
        const antwort = Array.isArray(roh) ? roh : [];
        const neu: Record<string, PirepPruefstatus> = {};
        for (const s of antwort) neu[s.pirep_id] = s;
        setStand(neu);
        offen = antwort.some((s) => pruefArt(s) === "in_pruefung");
      } catch (e) {
        console.warn("pirep_pruefstatus failed", e);
        offen = Object.values(standRef.current).some((s) => pruefArt(s) === "in_pruefung");
      }
      if (aktiv) timer = setTimeout(laden, offen ? TAKT_OFFEN_MS : TAKT_RUHIG_MS);
    };
    void laden();
    return () => {
      aktiv = false;
      if (timer) clearTimeout(timer);
    };
  }, [schluessel]);

  return stand;
}

/** Kleines Kennzeichen für die Übersichtszeile — nur wenn es etwas zu sagen gibt. */
export function PruefstatusMarke({ status }: { status: PirepPruefstatus | undefined }) {
  const { t } = useTranslation();
  const art = pruefArt(status);
  if (art === "unbekannt" || art === "ok") return null;
  return (
    <span className={`pruefstatus-marke pruefstatus-marke--${art}`} data-testid="pruefstatus-marke">
      {t(`landing.pruefstatus.kurz.${art}`)}
    </span>
  );
}

/** Kasten in der Detailansicht. */
export function PruefstatusKasten({ status }: { status: PirepPruefstatus | undefined }) {
  const { t } = useTranslation();
  const art = pruefArt(status);
  if (art === "unbekannt" || !status) return null;
  const gruende = status.reason_codes.filter((c) => c !== "");
  return (
    <div className={`pruefstatus-kasten pruefstatus-kasten--${art}`} role="status" data-testid="pruefstatus-kasten">
      <div className="pruefstatus-kasten__titel">{t(`landing.pruefstatus.titel.${art}`)}</div>
      <div className="pruefstatus-kasten__text">{t(`landing.pruefstatus.text.${art}`)}</div>
      {art !== "ok" && gruende.length > 0 && (
        <ul className="pruefstatus-kasten__gruende">
          {[...new Set(gruende.map(grundSchluessel))].map((k) => (
            <li key={k}>{t(k)}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
