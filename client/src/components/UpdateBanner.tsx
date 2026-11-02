import { useTranslation } from "react-i18next";
import type { FlightPhase } from "../types";
import type { UseUpdateCheckerResult } from "../hooks/useUpdateChecker";

/**
 * v0.5.48: Großes Update-Banner oben in der App.
 *
 * **Wann sichtbar?** Drei Bedingungen müssen ALLE erfüllt sein:
 *
 * 1. Hook meldet `stage === "banner"` (Update ≥ 3 Tage gesehen + nicht
 *    gerade snoozed)
 * 2. Pilot ist NICHT in einer aktiv-fliegenden Phase (siehe
 *    `ACTIVE_FLIGHT_PHASES` unten — Banner während Pushback / Cruise /
 *    Approach würde den Pilot stören und ist absoluter no-go)
 * 3. Pilot hat das Banner nicht für 4 h weggeklickt (`bannerSnoozed`)
 *
 * **Layout:** voll-breit oben, Akzent-Farbe (Cyan), kompakt, drei
 * Buttons (Install / Später / Was ist neu). Dismissible aber kommt
 * nach 4 h wieder bis Pilot installiert oder eine neue Version
 * erscheint die zurück auf `fresh` setzt.
 *
 * Renders nichts wenn nicht alle drei Bedingungen erfüllt sind —
 * sicher zu unbedingt zu mounten in App.tsx.
 */

/** Phasen in denen wir den Pilot NICHT mit einem Banner stören. Pilot
 *  ist gerade aktiv am Fliegen / Rollen / Boarding-Endphase. Update-
 *  Hinweis bleibt dann nur am Header-Button (passive Variante). */
export const ACTIVE_FLIGHT_PHASES: ReadonlySet<FlightPhase> = new Set([
  "pushback",
  "taxi_out",
  "takeoff_roll",
  "takeoff",
  "climb",
  "cruise",
  "holding",
  "descent",
  "approach",
  "final",
  "landing",
  "taxi_in",
  "blocks_on",
] as FlightPhase[]);

interface Props {
  checker: UseUpdateCheckerResult;
  /** Aktuelle Flug-Phase wenn ein Flug aktiv ist, sonst null. Banner
   *  wird in aktiven Phasen unterdrückt. */
  activePhase: FlightPhase | null;
}

export function UpdateBanner({ checker, activePhase }: Props) {
  const { t } = useTranslation();
  const {
    update,
    stage,
    installing,
    progress,
    snoozeBanner,
    installAndRelaunch,
  } = checker;

  // Bedingung 1: Stage muss "banner" sein.
  if (!update || stage !== "banner") return null;

  // Bedingung 2: nicht in aktiver Flug-Phase.
  if (activePhase != null && ACTIVE_FLIGHT_PHASES.has(activePhase)) return null;

  return (
    <div className="update-banner" role="status" aria-live="polite">
      <div className="update-banner__icon" aria-hidden="true">
        ⬇
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">
          {t("update.banner_title", { version: update.version })}
        </div>
        <div className="update-banner__subtitle">
          {t("update.banner_subtitle")}
        </div>
        {progress && (
          <div className="update-banner__progress">{progress}</div>
        )}
      </div>
      <div className="update-banner__actions">
        <button
          type="button"
          className="button button--primary"
          onClick={() => void installAndRelaunch()}
          disabled={installing}
        >
          {installing ? "…" : t("update.install_now")}
        </button>
        <button
          type="button"
          className="button button--ghost"
          onClick={snoozeBanner}
          disabled={installing}
          title={t("update.snooze_hint")}
        >
          {t("update.later")}
        </button>
      </div>
    </div>
  );
}
