import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, FlightEndOutcome, SimSnapshot } from "../types";
import { formatRefreshError } from "../lib/refreshErrorFormatter";
import { useConfirm } from "./ConfirmDialog";
import { InfoStrip } from "./InfoStrip";
import { LiveTapes } from "./LiveTapes";
import { LoadsheetMonitor } from "./LoadsheetMonitor";
import { ManualFileDialog } from "./ManualFileDialog";
import { RouteMap } from "./RouteMap";
import { WeatherBriefing } from "./WeatherBriefing";

interface Props {
  /** Active-flight info, owned by Dashboard. Pure display. */
  info: ActiveFlightInfo | null;
  /** Live sim telemetry — fed into the live-tapes strip. */
  simSnapshot?: SimSnapshot | null;
  /**
   * v0.12.5 (LE7): a real PIREP was concluded — normal flight-end, manual
   * file, or a cancel that resolved to filed/queued/cancelled. The parent
   * shows the matching banner. Replaces the overloaded `onEnded`.
   */
  onFiledSuccess: (outcome: FlightEndOutcome) => void;
  /**
   * v0.12.5 (LE7): just reload the active flight — used for `flight_forget`
   * and the disconnect-resume. No PIREP was filed → no success banner.
   */
  onRefreshActiveFlight: () => void;
}

function fmtDistance(nm: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
    nm,
  )} nmi`;
}

export function ActiveFlightPanel({
  info,
  simSnapshot,
  onFiledSuccess,
  onRefreshActiveFlight,
}: Props) {
  const { t, i18n } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [busy, setBusy] = useState<"end" | "cancel" | "forget" | "refresh" | null>(null);
  const [error, setError] = useState<string | null>(null);
  // v0.3.2: short-lived inline message after a successful OFP refresh
  // ("Plan-Werte aktualisiert"). Cleared on the next action so it
  // doesn't linger forever.
  const [refreshMsg, setRefreshMsg] = useState<string | null>(null);
  /**
   * When `flight_end` fails with `flight_validation_failed`, the backend
   * sends back a list of i18n-keyed missing-field codes. We surface the
   * ManualFileDialog so the pilot can either cancel the flight or file it
   * as a manual PIREP (with optional divert + reason). Null = no dialog.
   */
  const [validationMissing, setValidationMissing] = useState<string[] | null>(
    null,
  );
  // Tick once a second so the elapsed-time display refreshes between polls.
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, []);

  if (!info) return null;

  /**
   * v0.7.19 GAF-707 (QS-R1 Finding 3): wenn der aktive Flug einen
   * Accident-Latch hat, MUSS vor dem File-Versuch der Pilot bestaetigen
   * oder widersprechen. Spec §Active Flight / Flight End "War das ein
   * Absturz?". Drei Auswahlmoeglichkeiten plus Zurueck:
   *
   *   1. "Ja, Unfall einreichen"         → flight_end ohne Override.
   *   2. "Nein, als harte Landung filen" → flight_end mit
   *      accident_decision="as_hard_landing". Backend clearet den
   *      Accident-Latch und filed regulaer; Notes enthalten den
   *      Override-Eintrag fuer die VA-Admin-Spur.
   *   3. "Flug verwerfen & Cleanup"      → flight_cancel mit force=true.
   *   4. "Zurueck"                       → kein State-Change.
   */
  function isAccidentDetected(): boolean {
    if (!info) return false;
    return info.accident_detected === true
      || info.accident_confidence === "medium";
  }

  /** v0.12.5 (LE7): build the "filed" outcome from the current flight. */
  function filedOutcome(): FlightEndOutcome {
    return {
      kind: "filed",
      callsign: info!.airline_icao
        ? `${info!.airline_icao} ${info!.flight_number}`
        : info!.flight_number,
      dpt: info!.dpt_airport,
      arr: info!.arr_airport,
    };
  }

  async function handleEndConfirmed(decision: "as_accident" | "as_hard_landing" | null) {
    setBusy("end");
    setError(null);
    try {
      // Tauri's #[tauri::command] looks up args in camelCase, so the Rust
      // `accident_decision` param is read as `accidentDecision`. Sending
      // snake_case here silently drops the pilot's override (it would file
      // as an accident regardless of the "harte Landung" choice).
      const payload = decision ? { accidentDecision: decision } : undefined;
      await invoke("flight_end", payload);
      onFiledSuccess(filedOutcome());
    } catch (err: unknown) {
      const e = err as {
        code?: string;
        message?: string;
        details?: { missing?: string[] };
      };
      if (e?.code === "flight_validation_failed") {
        setValidationMissing(e.details?.missing ?? []);
      } else {
        const msg =
          typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: string }).message)
            : String(err);
        setError(msg);
      }
    } finally {
      setBusy(null);
    }
  }

  async function handleEnd() {
    if (busy) return;

    // v0.7.19 GAF-707 (QS-R1 Finding 3): bei aktivem Accident-Latch erst
    // den 4-Optionen-Dialog zeigen, sonst direkt filen wie bisher.
    if (isAccidentDetected()) {
      const isConfirmed = info?.accident_detected === true;
      // Schritt 1: "War das wirklich ein Absturz?" (oder fuer suspected:
      // "Moeglicher Absturz erkannt — wie filen?")
      const reasonsText = (info?.accident_reasons ?? []).join("\n");
      const yes = await confirm({
        title: isConfirmed
          ? t("active_flight.accident.confirm_title")
          : t("active_flight.accident.suspected_title"),
        message: t("active_flight.accident.confirm_body", {
          reasons: reasonsText || "—",
        }),
        confirmLabel: t("active_flight.accident.file_as_accident"),
        cancelLabel: t("active_flight.accident.other_action"),
        destructive: true,
      });
      if (yes) {
        await handleEndConfirmed("as_accident");
        return;
      }

      // Schritt 2: "Andere Aktion" → was genau?
      const fileAsHard = await confirm({
        title: t("active_flight.accident.other_title"),
        message: t("active_flight.accident.other_body"),
        confirmLabel: t("active_flight.accident.file_as_hard"),
        cancelLabel: t("active_flight.accident.back_or_cancel"),
      });
      if (fileAsHard) {
        await handleEndConfirmed("as_hard_landing");
        return;
      }

      // Schritt 3: Pilot hat "Zurueck oder Cancel" gewaehlt — den
      // bestehenden Cancel-Flow anbieten.
      const reallyCancel = await confirm({
        title: t("active_flight.confirm_cancel_force_title"),
        message: t("active_flight.confirm_cancel_force_body"),
        confirmLabel: t("active_flight.confirm_cancel_force_yes"),
        cancelLabel: t("active_flight.confirm_cancel_force_back"),
        destructive: true,
      });
      if (reallyCancel) {
        await invokeCancelOrForce(true);
      }
      return;
    }

    setBusy("end");
    setError(null);
    try {
      await invoke("flight_end");
      onFiledSuccess(filedOutcome());
    } catch (err: unknown) {
      // Backend's UiError shape: { code, message, details? }. The validation
      // path puts `{ missing: ["distance", ...] }` into details so we can
      // render the dialog with the exact reasons the file was rejected.
      const e = err as {
        code?: string;
        message?: string;
        details?: { missing?: string[] };
      };
      if (e?.code === "flight_validation_failed") {
        setValidationMissing(e.details?.missing ?? []);
      } else {
        const msg =
          typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: string }).message)
            : String(err);
        setError(msg);
      }
    } finally {
      setBusy(null);
    }
  }

  // v0.7.18 (B-014): is_finalizable-Check fuer File-First-Logik.
  // Spec §B-014 — wenn der Flug fast fertig ist (LANDING/TaxiIn/
  // BLOCKS_ON/Arrived + valider TD), darf Cancel nicht direkt
  // verwerfen. Dann zeigen wir 3-Button-Confirm:
  //   - „Lieber filen versuchen" → flight_cancel ohne force
  //   - „Abbrechen" → kein Cancel, Dialog zu
  //   - „Trotzdem verwerfen" → flight_cancel mit force=true
  function isFinalizable(): boolean {
    if (!info) return false;
    const isTdPhase =
      info.phase === "landing" ||
      info.phase === "taxi_in" ||
      info.phase === "blocks_on" ||
      info.phase === "arrived";
    return isTdPhase && info.landing_at !== null;
  }

  /** User accepted the cancel option from the validation dialog. */
  async function handleCancelFromDialog() {
    setValidationMissing(null);
    // Dieser Pfad ist „flight_end hat Validation-Failure geworfen,
    // Pilot wählt Cancel statt Korrektur". File-First wurde schon
    // implizit gemacht (via flight_end), also hier force=true setzen
    // damit der Backend nicht nochmal versucht zu filen.
    await invokeCancelOrForce(true);
  }

  async function invokeCancelOrForce(force: boolean) {
    setBusy("cancel");
    setError(null);
    try {
      const outcome = (await invoke("flight_cancel", { force })) as
        | { kind: "filed_instead"; pirep_id: string }
        | { kind: "queued"; pirep_id: string }
        | { kind: "cancelled"; pirep_id: string };
      // v0.12.5 (LE7): Outcome an den Parent durchreichen — CockpitView
      // entscheidet, welches Banner es zeigt:
      //   - filed_instead: PIREP direkt eingereicht (Erfolg).
      //   - queued:        Transient-Fehler, PIREP wartet in der Queue.
      //   - cancelled:     regulärer Cancel — KEIN Erfolgs-Banner.
      if (outcome.kind === "filed_instead") {
        onFiledSuccess({ kind: "filed_instead", pirep_id: outcome.pirep_id });
      } else if (outcome.kind === "queued") {
        onFiledSuccess({ kind: "queued", pirep_id: outcome.pirep_id });
      } else {
        onFiledSuccess({ kind: "cancelled" });
      }
    } catch (err: unknown) {
      const code =
        typeof err === "object" && err !== null && "code" in err
          ? String((err as { code: string }).code)
          : null;
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      if (code === "blocked") {
        setError(t("active_flight.cancel_blocked"));
      } else if (code === "file_first_failed") {
        // v0.7.18 (R2-1): File-First-Versuch ist hart fehlgeschlagen.
        // Backend hat NICHT automatisch gecancelt — Pilot hatte „filen
        // versuchen" gewaehlt, nicht „bei Fehler trotzdem verwerfen".
        // Wir zeigen jetzt explizit den zweiten Confirm: „Filen ist
        // gescheitert (Grund). Trotzdem verwerfen?"
        const really = await confirm({
          title: t("active_flight.confirm_cancel_after_file_failed_title"),
          message: t("active_flight.confirm_cancel_after_file_failed_body", {
            reason: msg,
          }),
          confirmLabel: t("active_flight.confirm_cancel_force_yes"),
          cancelLabel: t("active_flight.confirm_cancel_force_back"),
          destructive: true,
        });
        if (really) {
          // force=true bypasst File-First → direkter Cancel.
          await invokeCancelOrForce(true);
        }
      } else {
        setError(msg);
      }
    } finally {
      setBusy(null);
    }
  }

  async function handleCancel() {
    if (busy) return;

    if (isFinalizable()) {
      // 3-Button-Dialog: filen / abbrechen / trotzdem verwerfen.
      // useConfirm liefert nur 2 Buttons → wir machen es seriell:
      //   1. „Flug eigentlich fast fertig — lieber filen versuchen?"
      //      [Filen versuchen] vs [Abbrechen]
      //   2. Wenn „Abbrechen": zweiter Dialog „Wirklich verwerfen?"
      //      [Trotzdem verwerfen] vs [Zurück]
      const tryFile = await confirm({
        title: t("active_flight.confirm_cancel_finalizable_title"),
        message: t("active_flight.confirm_cancel_finalizable_body"),
        confirmLabel: t("active_flight.confirm_cancel_finalizable_file"),
        cancelLabel: t("active_flight.confirm_cancel_finalizable_other"),
      });
      if (tryFile) {
        // File-First: force=false. Backend versucht erst zu filen.
        // Outcomes:
        //   - Ok(filed_instead | queued | cancelled) → kein weiterer Dialog.
        //   - Err(blocked)            → Account-Sperre, Fehlertext.
        //   - Err(file_first_failed)  → invokeCancelOrForce zeigt
        //     zweiten Confirm-Dialog (R2-1). Kein Auto-Cancel mehr.
        await invokeCancelOrForce(false);
        return;
      }
      // Pilot will nicht filen — fragen ob „verwerfen" oder „doch zurueck".
      const really = await confirm({
        title: t("active_flight.confirm_cancel_force_title"),
        message: t("active_flight.confirm_cancel_force_body"),
        confirmLabel: t("active_flight.confirm_cancel_force_yes"),
        cancelLabel: t("active_flight.confirm_cancel_force_back"),
        destructive: true,
      });
      if (!really) return;
      await invokeCancelOrForce(true);
      return;
    }

    // Nicht finalisierbar → klassischer Cancel-Dialog mit single confirm.
    const ok = await confirm({
      message: t("active_flight.confirm_cancel"),
      destructive: true,
    });
    if (!ok) return;
    await invokeCancelOrForce(false);
  }

  /**
   * v0.3.2: Refresh the SimBrief OFP for the running flight without
   * having to discard & restart. Real-pilot workflow: pilot regenerates
   * the OFP on simbrief.com after AeroACARS already cached the previous
   * one at flight-start (e.g. pax/cargo/reserve changed). Click → backend
   * re-pulls the bid (which carries the latest OFP id), fetches the OFP,
   * and overwrites planned_block / planned_tow / planned_zfw / etc. on
   * the active flight. The Loadsheet then compares against the new plan.
   */
  async function handleRefreshOfp() {
    if (busy) return;
    setBusy("refresh");
    setError(null);
    setRefreshMsg(null);
    try {
      await invoke("flight_refresh_simbrief");
      setRefreshMsg(t("active_flight.refresh_ofp_done"));
    } catch (err: unknown) {
      // v0.7.8 v1.5.2: shared Helper formattiert Mismatch-JSON +
      // bekannte Error-Codes in lesbare Notices (Spec §8).
      // v1.5.3 (Thomas-QS): context="cockpit" damit phase_locked
      // + no_simbrief_link lesbare Texte bekommen (statt null →
      // String(err) → "[object Object]").
      const formatted = formatRefreshError(
        err as { code?: string; message?: string } | null,
        t,
        "cockpit",
      );
      setError(formatted?.text ?? String(err));
    } finally {
      setBusy(null);
    }
  }

  /**
   * Force-discard local active-flight state without touching phpVMS. Useful
   * when the cancel call fails because the PIREP is already gone server-side
   * but our local state still thinks a flight is active.
   */
  async function handleForget() {
    if (busy) return;
    if (
      !(await confirm({
        message: t("active_flight.confirm_forget"),
        destructive: true,
      }))
    )
      return;
    setBusy("forget");
    setError(null);
    try {
      await invoke("flight_forget");
      onRefreshActiveFlight();
    } catch (err: unknown) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      setError(msg);
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="active-flight">
      {confirmDialog}
      {/* v0.4.1: Sim-Disconnect-Pause-Banner. Sichtbar nur wenn der
          Streamer einen Disconnect detektiert hat — sonst null.
          Pilot sieht die letzten Sim-Werte zum Repositionieren und
          klickt „Flug wiederaufnehmen" sobald er den Sim wieder
          aufgesetzt hat. */}
      {info.paused_since && info.paused_last_known && (
        <DisconnectBanner
          pausedSince={info.paused_since}
          lastKnown={info.paused_last_known}
          onResumed={() => {
            // v0.12.5 (LE7): nur den aktiven Flug neu laden — der Flug
            // läuft weiter, es wurde nichts gefilt → kein Banner.
            onRefreshActiveFlight();
          }}
        />
      )}
      <header className="active-flight__header">
        <div className="active-flight__title-block">
          <span className="active-flight__label">
            {t("active_flight.title")}
          </span>
          <div className="active-flight__heading">
            <h2 className="active-flight__callsign">
              {info.airline_icao
                ? `${info.airline_icao} ${info.flight_number}`
                : info.flight_number}
            </h2>
            <span
              className={`active-flight__phase active-flight__phase--${info.phase}`}
            >
              {t(`active_flight.phase.${info.phase}`, {
                defaultValue: info.phase,
              })}
            </span>
          </div>
        </div>
        <div className="active-flight__route">
          <span className="active-flight__icao">{info.dpt_airport}</span>
          <span className="active-flight__route-arrow">
            <span className="active-flight__arrow">→</span>
            <span className="active-flight__route-distance">
              {fmtDistance(info.distance_nm, i18n.language)}
            </span>
          </span>
          <span className="active-flight__icao">{info.arr_airport}</span>
        </div>
        <div className="active-flight__actions">
          <button
            type="button"
            className="button button--primary"
            onClick={handleEnd}
            disabled={busy !== null}
          >
            {busy === "end" ? t("active_flight.filing") : t("active_flight.end")}
          </button>
          <button type="button" onClick={handleCancel} disabled={busy !== null}>
            {busy === "cancel"
              ? t("active_flight.cancelling")
              : t("active_flight.cancel")}
          </button>
          {/* OFP refresh — pre-takeoff only. After takeoff the plan
              shouldn't change anyway, and we don't want pilots
              accidentally clobbering the loadsheet baseline mid-flight. */}
          {/* v0.7.7: Phase-Gate inkl. Pushback (Spec §6.2) — Plan-Werte sind
              dort noch nutzbar, Score noch nicht festgenagelt. Backend hat
              denselben Gate. */}
          {(info.phase === "preflight" ||
            info.phase === "boarding" ||
            info.phase === "pushback" ||
            info.phase === "taxi_out") && (
            <button
              type="button"
              className="active-flight__refresh-ofp"
              onClick={handleRefreshOfp}
              disabled={busy !== null}
              title={t("active_flight.refresh_ofp_hint")}
            >
              {busy === "refresh"
                ? t("active_flight.refresh_ofp_busy")
                : t("active_flight.refresh_ofp")}
            </button>
          )}
          <button
            type="button"
            className="active-flight__forget"
            onClick={handleForget}
            disabled={busy !== null}
            title={t("active_flight.forget_hint")}
          >
            {busy === "forget"
              ? t("active_flight.forgetting")
              : t("active_flight.forget")}
          </button>
        </div>
        {refreshMsg && (
          <div className="active-flight__refresh-msg" role="status">
            ✓ {refreshMsg}
          </div>
        )}
      </header>

      {/* v0.3.0: RouteMap (Progress-Bar EDDW [✈] EGSS 0%) erst ab
          Pushback einblenden. Vor Pushback ist 0 % Strecke logisch
          unsinnig und verschwendet vertikalen Platz. Tachos bleiben
          dagegen drin (User-Wunsch — nur 10 % kleiner). */}
      {info.phase !== "preflight" && info.phase !== "boarding" && (
        <RouteMap
          dptIcao={info.dpt_airport}
          arrIcao={info.arr_airport}
          currentLat={simSnapshot?.lat ?? null}
          currentLon={simSnapshot?.lon ?? null}
          dptGate={info.dep_gate}
          arrGate={info.arr_gate}
        />
      )}

      <LiveTapes snapshot={simSnapshot ?? null} />

      <InfoStrip
        info={info}
        snapshot={simSnapshot ?? null}
        elapsedMinutes={Math.max(
          0,
          Math.floor((Date.now() - new Date(info.started_at).getTime()) / 60000),
        )}
      />

      {/* v0.3.0: Loadsheet direkt unter dem InfoStrip — gehört zum
          aktiven Flug, deshalb im selben Container. Verschwindet von
          alleine ab TaxiOut/Pushback (siehe LoadsheetMonitor). */}
      <LoadsheetMonitor info={info} />

      <WeatherBriefing dptIcao={info.dpt_airport} arrIcao={info.arr_airport} />

      {error && (
        <p className="active-flight__error" role="alert">
          {error}
        </p>
      )}

      {validationMissing !== null && (
        <ManualFileDialog
          info={info}
          missing={validationMissing}
          onFiled={() => {
            setValidationMissing(null);
            onFiledSuccess(filedOutcome());
          }}
          onCancelFlight={() => void handleCancelFromDialog()}
          onClose={() => setValidationMissing(null)}
        />
      )}
    </section>
  );
}

// ===========================================================================
// v0.4.1: Sim-Disconnect-Pause-Banner
// ===========================================================================
//
// Wenn der Streamer im Backend `paused_since` setzt (Sim wegbrach >30 s),
// rendert ActiveFlightPanel diese Component an oberster Stelle. Pilot
// sieht die letzten bekannten Werte (LAT/LON/HDG/ALT/Fuel/ZFW), kann
// damit das Flugzeug nach Sim-Restart auf die richtige Position setzen,
// und klickt dann „Flug wiederaufnehmen" — der Streamer macht weiter.
// Bewusst KEIN Auto-Resume — selbst wenn der Sim plötzlich wieder
// Daten liefert, wartet das Backend auf den expliziten Klick (siehe
// `flight_resume_after_disconnect` in lib.rs).

interface DisconnectBannerProps {
  pausedSince: string;
  lastKnown: import("../types").PausedSnapshot;
  onResumed: () => void;
}

function DisconnectBanner({
  pausedSince,
  lastKnown,
  onResumed,
}: DisconnectBannerProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pausedDate = new Date(pausedSince);
  const pausedTime = `${pausedDate.getHours().toString().padStart(2, "0")}:${pausedDate.getMinutes().toString().padStart(2, "0")}`;

  const fmtCoord = (val: number, isLat: boolean): string => {
    const hemi = isLat ? (val >= 0 ? "N" : "S") : val >= 0 ? "E" : "W";
    return `${Math.abs(val).toFixed(4)}° ${hemi}`;
  };

  async function handleResume() {
    setBusy(true);
    setError(null);
    try {
      await invoke("flight_resume_after_disconnect");
      onResumed();
    } catch (err: unknown) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      setError(msg);
      setBusy(false);
    }
  }

  return (
    <div className="active-flight__paused-banner" role="alert">
      <div className="active-flight__paused-header">
        <span className="active-flight__paused-icon">⏸</span>
        <div>
          <strong>{t("active_flight.paused.title")}</strong>
          <span className="active-flight__paused-since">
            {t("active_flight.paused.since", { time: pausedTime })}
          </span>
        </div>
      </div>
      <p className="active-flight__paused-instructions">
        {t("active_flight.paused.instructions")}
      </p>
      <div className="active-flight__paused-grid">
        <div>
          <span className="active-flight__paused-label">
            {t("active_flight.paused.position")}
          </span>
          <code>
            {fmtCoord(lastKnown.lat, true)} · {fmtCoord(lastKnown.lon, false)}
          </code>
        </div>
        <div>
          <span className="active-flight__paused-label">
            {t("active_flight.paused.heading_alt")}
          </span>
          <code>
            HDG {Math.round(lastKnown.heading_deg)}° · ALT{" "}
            {Math.round(lastKnown.altitude_ft).toLocaleString()} ft
          </code>
        </div>
        <div>
          <span className="active-flight__paused-label">
            {t("active_flight.paused.fuel")}
          </span>
          <code>
            {Math.round(lastKnown.fuel_total_kg).toLocaleString()} kg
          </code>
        </div>
        <div>
          <span className="active-flight__paused-label">
            {t("active_flight.paused.zfw")}
          </span>
          <code>
            {lastKnown.zfw_kg !== null
              ? `${Math.round(lastKnown.zfw_kg).toLocaleString()} kg`
              : "—"}
          </code>
        </div>
      </div>
      <div className="active-flight__paused-actions">
        <button
          type="button"
          className="button button--primary"
          onClick={() => void handleResume()}
          disabled={busy}
        >
          {busy
            ? t("active_flight.paused.resuming")
            : t("active_flight.paused.resume")}
        </button>
      </div>
      {error && (
        <p className="active-flight__paused-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
