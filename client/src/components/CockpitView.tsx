import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen, openExternal as openUrl } from "../lib/ipc";
import type {
  ActiveFlightInfo,
  FlightEndOutcome,
  LoginResult,
  SimSnapshot,
} from "../types";
import { ResumeFlightBanner } from "./ResumeFlightBanner";
import { ActiveFlightPanel } from "./ActiveFlightPanel";
import { StableApproachBanner } from "./StableApproachBanner";

// v0.12.12-dev: GSG-Wetter-Briefing-Seite. Login-basiert — wenn der Pilot
// in seinem Standard-Browser bei phpVMS eingeloggt ist, zieht die Seite
// den aktiven Bid automatisch.
const WEATHER_BRIEFING_URL = "https://german-sky-group.eu/weatherbriefing";
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
  approachAdvisoriesEnabled,
}: Props) {
  const { t } = useTranslation();
  // v0.12.5 (LE7): how the last flight concluded — drives the post-flight
  // banner. A *green* success banner ONLY for a genuine filing; a neutral
  // notice for a discard. Replaces the old `filedFlightInfo` which blindly
  // showed "PIREP filed" for cancel/forget/resume too (Bug F).
  const [endNotice, setEndNotice] = useState<FlightEndOutcome | null>(null);
  /** v0.12.12-dev: Wetter-Briefing-Lade-Hinweis. Erscheint beim Klick auf
   *  den 🌦-Button (5 s sichtbar) statt als permanenter Schild. */
  const [weatherLoadHint, setWeatherLoadHint] = useState(false);
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

  // v0.12.6: Auto-File läuft jetzt im **Backend** (Position-Streamer),
  // window-unabhängig. Vorher war es ein Frontend-`useEffect` — der lief
  // nur, wenn das AeroACARS-Fenster im Vordergrund UND der Cockpit-Tab
  // aktiv war (sonst drosselt die WebView die JS-Timer). Pilot-Befund:
  // der PIREP ging erst raus, nachdem der Pilot AeroACARS in den
  // Vordergrund holte. Das Backend filet beim FSM-Latch auf `Arrived`
  // selbst und emittiert `pirep_auto_filed` — wir zeigen darauf nur noch
  // das LE7-Erfolgs-Banner.
  useEffect(() => {
    const unlisten = listen<{ callsign: string; dpt: string; arr: string }>(
      "pirep_auto_filed",
      (e) => {
        setEndNotice({ kind: "filed", ...e.payload });
        setActiveFlight(null);
      },
    );
    return () => {
      void unlisten.then((f) => f());
    };
  }, [setActiveFlight]);

  // v0.13.7: Backend emittiert `pirep_cancelled_remotely` wenn phpVMS einen
  // 404 auf einen laufenden PIREP liefert (= Inaktivitäts-Timeout oder
  // VA-Admin hat den PIREP entfernt). Activity-Log enthält den Eintrag
  // bereits durchgehend; hier räumen wir zusätzlich den lokalen
  // Flugzustand auf, damit der Pilot nicht weiterklickt auf einem PIREP
  // der serverseitig nicht mehr existiert.
  useEffect(() => {
    const unlisten = listen<{ pirep_id: string; source: string }>(
      "pirep_cancelled_remotely",
      () => {
        setEndNotice({ kind: "cancelled_remotely" });
        setActiveFlight(null);
      },
    );
    return () => {
      void unlisten.then((f) => f());
    };
  }, [setActiveFlight]);

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
        {endNotice.kind === "cancelled_remotely" && (
          <>
            <strong>{t("cockpit.cancelled_remotely_title")}</strong>
            <span>{t("cockpit.cancelled_remotely_detail")}</span>
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

  // v0.12.12-dev: Wetter-Briefing-Knopf öffnet die GSG-Briefing-Seite im
  // System-Browser. Login-basiert, die Seite zieht den aktiven Bid auto-
  // matisch. Der Lade-Hinweis erscheint per Toast beim Klick (5 s sichtbar)
  // statt als permanenter Schild — der Pilot soll bemerken dass die Seite
  // ihre Daten live holt (METAR/TAF/NOTAMs/Runway), Ladezeit bis zu 30 s.
  const quickActionRow = (
    <div className="cockpit-actions">
      <button
        type="button"
        className="button button--ghost cockpit-actions__weather"
        onClick={() => {
          setWeatherLoadHint(true);
          window.setTimeout(() => setWeatherLoadHint(false), 5000);
          void openUrl(WEATHER_BRIEFING_URL).catch(() => {});
        }}
        title={t("cockpit.weather_briefing_hint")}
      >
        🌦 {t("cockpit.weather_briefing")}
      </button>
    </div>
  );

  const weatherLoadToast = weatherLoadHint && (
    <div className="cockpit-weather-toast" role="status">
      🌦 {t("cockpit.weather_briefing_load_hint")}
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
        {quickActionRow}
        {weatherLoadToast}
      </>
    );
  }

  return (
    <>
      {noticeBanner}
      {quickActionRow}
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
