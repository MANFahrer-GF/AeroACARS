import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import type {
  ActiveFlightInfo,
  FlightEndOutcome,
  LoginResult,
  SimSnapshot,
} from "../types";
import { ResumeFlightBanner } from "./ResumeFlightBanner";
import { ActiveFlightPanel } from "./ActiveFlightPanel";
import { StableApproachBanner } from "./StableApproachBanner";
// v0.3.0: LoadsheetMonitor wird jetzt direkt im ActiveFlightPanel
// gerendert (zwischen InfoStrip und WeatherBriefing), damit das
// Loadsheet visuell zum aktiven Flug gehört statt als getrennte
// Section unter dem WeatherBriefing zu hängen.
import { DivertBanner } from "./DivertBanner";

interface Props {
  session: LoginResult;
  activeFlight: ActiveFlightInfo | null;
  setActiveFlight: (info: ActiveFlightInfo | null) => void;
  simSnapshot: SimSnapshot | null;
  /** Called when there's no active flight and the user wants to pick
   *  one — UI nudges them to the briefing tab. */
  onSwitchToBriefing: () => void;
  /** Auto-file the PIREP once the FSM reaches `Arrived`. Toggle in
   *  Settings → Filing. When false the pilot has to click
   *  "Flug beenden" themselves. */
  autoFile: boolean;
  /** v0.5.38: Stable-Approach-Banner anzeigen. Default ON. */
  approachAdvisoriesEnabled: boolean;
}

/**
 * Cockpit tab — the in-flight pilot view. Shows the resume banner
 * (when a stale flight is detected on startup), the active-flight
 * panel with weather briefing and PIREP actions, and a friendly empty
 * state when no flight is running.
 *
 * Deliberately no SimPanel here — sim telemetry lives in Settings →
 * Debug. The pilot during a flight cares about phase, route and
 * weather, not floating-point lat/lon.
 */
export function CockpitView({
  activeFlight,
  setActiveFlight,
  simSnapshot,
  onSwitchToBriefing,
  autoFile,
  approachAdvisoriesEnabled,
}: Props) {
  const { t } = useTranslation();
  // v0.12.5 (LE7): how the last flight concluded — drives the post-flight
  // banner. A *green* success banner ONLY for a genuine filing; a neutral
  // notice for a discard. Replaces the old `filedFlightInfo` which blindly
  // showed "PIREP filed" for cancel/forget/resume too (Bug F).
  const [endNotice, setEndNotice] = useState<FlightEndOutcome | null>(null);
  useEffect(() => {
    if (!endNotice) return;
    const id = window.setTimeout(() => setEndNotice(null), 8000);
    return () => window.clearTimeout(id);
  }, [endNotice]);

  /** v0.12.5 (LE7): reload the active flight without claiming a PIREP was
   *  filed. `flight_forget` → backend returns null → cockpit collapses;
   *  disconnect-resume → backend keeps the flight → it stays. */
  const refreshActiveFlight = () => {
    void invoke<ActiveFlightInfo | null>("flight_status")
      .then(setActiveFlight)
      .catch(() => {});
  };

  /** v0.12.5 (LE7): a real PIREP concluded — show the outcome banner and
   *  clear the flight. Shared by `ActiveFlightPanel` and `DivertBanner`. */
  const handleFiledSuccess = (outcome: FlightEndOutcome) => {
    setEndNotice(outcome);
    setActiveFlight(null);
  };

  // Auto-file the PIREP once the FSM marks the flight as Arrived
  // (BlocksOn + 30 s + engines off + parking brake set). Most pilots
  // wouldn't manually click "Flug beenden" if the app could just
  // submit on its own — and with all the pre-flight validation in
  // flight_end the worst case is a soft fail (the manual file dialog
  // surfaces, same as today). We attempt it exactly once per flight
  // via the ref, so a transient phase flutter back to TaxiIn doesn't
  // re-trigger.
  const autoFiledRef = useRef<string | null>(null);
  useEffect(() => {
    // v0.12.5 (LE6, Bug D): autoFiledRef NICHT mehr auf null zurücksetzen
    // wenn `activeFlight` kurz null wird. Der 2-s-Status-Poll hat ein
    // Race-Fenster, in dem ein veralteter Poll „kein Flug" liefert und der
    // nächste den Flug zurückbringt — beim Reset wäre der Auto-File-Guard
    // weg und „Auto-File fehlgeschlagen" feuerte ein zweites Mal. Der Ref
    // hält jetzt dauerhaft die zuletzt auto-gefilte pirep_id; ein echter
    // neuer Flug hat eine andere pirep_id und löst regulär aus.
    if (!activeFlight) return;
    if (!autoFile) return;
    if (activeFlight.phase !== "arrived") return;
    // Suppress auto-file when we've detected a divert. The pilot must
    // explicitly choose "submit as divert to X" / "submit as planned"
    // / "override" via the DivertBanner — silently filing with the
    // wrong arr_airport_id would defeat the whole point.
    if (activeFlight.divert_hint) return;
    if (autoFiledRef.current === activeFlight.pirep_id) return;
    autoFiledRef.current = activeFlight.pirep_id;
    // Snapshot the flight identity for the success banner before the
    // async call clears `activeFlight`.
    const filedNotice: FlightEndOutcome = {
      kind: "filed",
      callsign: activeFlight.airline_icao
        ? `${activeFlight.airline_icao} ${activeFlight.flight_number}`
        : activeFlight.flight_number,
      dpt: activeFlight.dpt_airport,
      arr: activeFlight.arr_airport,
    };
    void (async () => {
      try {
        await invoke("flight_end");
        // Clear the active flight in the React tree *immediately*
        // instead of waiting for the next 2 s status poll to notice.
        // Without this, pilots reported the cockpit panel sticking
        // around after the auto-file completed; the polling-only
        // path had a race window where a stale poll could overwrite
        // a "no flight" reading and bring it back briefly.
        setActiveFlight(null);
        // v0.12.5 (LE7): auto-file is a genuine filing → success banner.
        setEndNotice(filedNotice);
      } catch (err: unknown) {
        // v0.7.17 (B-006): Auto-File-Failure war vorher KOMPLETT
        // stumm — catch{} schluckte alles, Pilot dachte „auto-filed"
        // aber tatsaechlich war der PIREP noch lokal nicht gefilt
        // (z.B. „not_at_arrival" weil Pilot inzwischen vom Gate weg,
        // oder „fuel" weil Block-Fuel fehlt). Pilot lief in den
        // Stale-Stream-Zustand (B-004).
        //
        // Jetzt: Activity-Log-Warning + UI-Toast damit der Pilot weiss
        // dass Auto-File scheiterte und er manuell „Flug beenden"
        // klicken muss. activeFlight bleibt erhalten damit der
        // manuelle Button weiter funktioniert. autoFiledRef bleibt
        // gesetzt → kein Retry-Loop.
        const errObj = err as { code?: string; message?: string } | undefined;
        const errCode = errObj?.code ?? "unknown";
        const errMsg = errObj?.message ?? String(err);
        void invoke("activity_log_add", {
          level: "warn",
          message: 'Auto-File fehlgeschlagen — bitte manuell „Flug beenden" klicken',
          detail: `${errCode}: ${errMsg}`,
        }).catch(() => null);
      }
    })();
  }, [activeFlight, autoFile]);

  // v0.12.5 (LE7): post-flight notice banner — green ✅ for a real filing,
  // neutral for a discard. Rendered above both the empty state and the
  // active-flight panel so it's visible whichever way the tree resolves.
  const noticeBanner = endNotice && (
    <div
      className={
        endNotice.kind === "filed" || endNotice.kind === "filed_instead"
          ? "cockpit-pirep-success"
          : "cockpit-pirep-success cockpit-pirep-success--neutral"
      }
      role="status"
    >
      <div className="cockpit-pirep-success__icon">
        {endNotice.kind === "filed" || endNotice.kind === "filed_instead"
          ? "✅"
          : endNotice.kind === "queued"
          ? "⏳"
          : "ℹ"}
      </div>
      <div className="cockpit-pirep-success__text">
        {endNotice.kind === "filed" && (
          <>
            <strong>{t("cockpit.pirep_filed_title")}</strong>
            <span>
              {t("cockpit.pirep_filed_detail", {
                callsign: endNotice.callsign,
                dpt: endNotice.dpt,
                arr: endNotice.arr,
              })}
            </span>
          </>
        )}
        {endNotice.kind === "filed_instead" && (
          <>
            <strong>{t("cockpit.filed_instead_title")}</strong>
            <span>
              {t("cockpit.filed_instead_detail", {
                pirep_id: endNotice.pirep_id,
              })}
            </span>
          </>
        )}
        {endNotice.kind === "queued" && (
          <>
            <strong>{t("cockpit.queued_title")}</strong>
            <span>
              {t("cockpit.queued_detail", { pirep_id: endNotice.pirep_id })}
            </span>
          </>
        )}
        {endNotice.kind === "cancelled" && (
          <>
            <strong>{t("cockpit.cancelled_title")}</strong>
            <span>{t("cockpit.cancelled_detail")}</span>
          </>
        )}
      </div>
      <button
        type="button"
        className="cockpit-pirep-success__close"
        onClick={() => setEndNotice(null)}
        aria-label={t("cockpit.pirep_filed_dismiss")}
      >
        ✕
      </button>
    </div>
  );

  if (!activeFlight) {
    return (
      <>
        {/* v0.12.5 (LE7): post-flight notice — 8 s auto-dismiss. */}
        {noticeBanner}
        <section className="cockpit-empty">
          <div className="cockpit-empty__icon" aria-hidden="true">
            ✈
          </div>
          <h2 className="cockpit-empty__title">{t("cockpit.empty_title")}</h2>
          <p className="cockpit-empty__hint">{t("cockpit.empty_hint")}</p>
          <button
            type="button"
            className="button button--primary"
            onClick={onSwitchToBriefing}
          >
            {t("cockpit.go_briefing")}
          </button>
        </section>
      </>
    );
  }

  return (
    <>
      {noticeBanner}
      <ResumeFlightBanner
        activeFlight={activeFlight}
        onAdopted={setActiveFlight}
        onCancelled={() => setActiveFlight(null)}
      />

      {!activeFlight.was_just_resumed && (
        <DivertBanner
          activeFlight={activeFlight}
          onFiledSuccess={handleFiledSuccess}
        />
      )}

      {/* v0.5.38: Visual Stable-Approach-Advisory. Steht ÜBER dem
          ActiveFlightPanel sodass es bei jedem Flugzustand sichtbar
          ist. Banner blendet sich selbst aus wenn der Anflug stabil
          ist — null Visual-Footprint im Normal-Fall. */}
      <StableApproachBanner
        activeFlight={activeFlight}
        simSnapshot={simSnapshot}
        enabled={approachAdvisoriesEnabled}
      />

      {!activeFlight.was_just_resumed && (
        <ActiveFlightPanel
          info={activeFlight}
          simSnapshot={simSnapshot}
          onFiledSuccess={handleFiledSuccess}
          onRefreshActiveFlight={refreshActiveFlight}
        />
      )}

      {/* Live-Loadsheet wird seit v0.3.0 direkt im ActiveFlightPanel
          gerendert — siehe Import-Kommentar oben. */}
    </>
  );
}
