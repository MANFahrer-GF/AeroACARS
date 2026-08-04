import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, FlightEndOutcome, SimSnapshot } from "../types";
import { formatRefreshError } from "../lib/refreshErrorFormatter";
import { resolveFlightIdent } from "../lib/callsign";
import { useConfirm } from "./ConfirmDialog";
// QS 2026-08-04: `fmtDistance` war hier lokal nochmal definiert — mit der
// Einheit "nmi", während die exportierte Variante (und damit PhaseCard +
// TripCard, direkt darunter auf demselben Bildschirm) "nm" schreibt. Zwei
// Schreibweisen derselben Einheit nebeneinander. Jetzt eine gemeinsame Quelle.
import { InfoStrip, fmtDistance } from "./InfoStrip";
import { LiveTapes } from "./LiveTapes";
import { LoadsheetMonitor } from "./LoadsheetMonitor";
import { ManualFileDialog } from "./ManualFileDialog";
import { PhaseCard } from "./PhaseCard";
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
  /** Field feedback (2026-08-03): the Wetter-Briefing button used to float
   *  in its own row above the whole Cockpit tab — visually disconnected
   *  from (and read as a confusing duplicate of) this panel's own action
   *  row. Owned by CockpitView (needed there too for the no-active-flight
   *  empty state); rendered here as a normal sibling of Route-Sync/OFP-
   *  Refresh instead. */
  onOpenWeatherBriefing: () => void;
  weatherLoadHint: boolean;
}

const EARTH_RADIUS_NM = 3440.065; // nautical miles

/** Great-circle distance in nautical miles between two lat/lon points —
 *  same formula/constant RouteMap.tsx uses for its own progress-bar math,
 *  computed independently here (see the header-distance fix below). */
function haversineNm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const toRad = (deg: number) => (deg * Math.PI) / 180;
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_NM * c;
}

/**
 * #phase-v2 Cutover: bestimmt Label-Key + CSS-Klassen-Suffix des Phasen-Badges.
 * `phase` ist die (v2-)Flugphase; `shadowSegment` ist das ROHE Kinematik-Segment
 * der v2-Engine (`ground|climbing|level|descending|insufficient`).
 *
 * „Level" ist eine RESTRIKTION: ein Level-Off UNTER der Reiseflughöhe während
 * Steig- oder Sinkflug (ATC-Zwischenhöhe). Deshalb greift der „Level"-Override
 * NUR bei `climb`/`descent`. Im Reiseflug (`cruise`) fliegt der Flieger normal
 * level → `shadowSegment` ist dort dauerhaft `"level"`, aber das bleibt „Cruise",
 * NICHT „Level" (sonst zeigte das Badge den ganzen Reiseflug fälschlich „Level").
 *
 * v0.19.1: "Final" blieb nach dem Aufsetzen früher minutenlang stehen
 * (Rollout/Taxi-in/Shutdown), weil die v2-Engine im Boden-/Terminal-Band rein
 * auf die alte FSM 1:1 sync-te und die bei manchen Flügen selbst hängen blieb
 * (Field-Report GSG22 EDLN→EDDL). Behoben an der Quelle in `phase_v2.rs`
 * (`Final` promotet sich jetzt selbst auf `Landing`, sobald der Kinematik-
 * Segmenter `Ground` meldet) — `phase` hier ist dadurch bereits `"landing"`,
 * kein zusätzliches Label-Override in dieser rein UI-seitigen Funktion nötig.
 */
export function phaseBadgeDisplay(
  phase: string,
  shadowSegment: string | undefined | null,
): { labelKey: string; className: string } {
  const inRestrictable = phase === "climb" || phase === "descent";
  const showLevel = shadowSegment === "level" && inRestrictable;
  const key = showLevel ? "level" : phase;
  return { labelKey: key, className: key };
}

/**
 * v0.7.18 (B-014): is_finalizable check for the file-first Cancel logic.
 * Spec §B-014 — once the flight is essentially done (LANDING/TaxiIn/
 * BLOCKS_ON/Arrived + a valid touchdown), Cancel must not discard directly;
 * a 3-button confirm offers "try filing instead" first. Pure + exported so
 * it's regression-testable without mounting the component (same reasoning
 * as `phaseBadgeDisplay`).
 */
export function isFlightFinalizable(phase: string, landingAt: string | null): boolean {
  const isTdPhase =
    phase === "landing" ||
    phase === "taxi_in" ||
    phase === "blocks_on" ||
    phase === "arrived";
  return isTdPhase && landingAt !== null;
}

export function ActiveFlightPanel({
  info,
  simSnapshot,
  onFiledSuccess,
  onRefreshActiveFlight,
  onOpenWeatherBriefing,
  weatherLoadHint,
}: Props) {
  const { t, i18n } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [busy, setBusy] = useState<
    "end" | "cancel" | "forget" | "refresh" | "sync_route" | null
  >(null);
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

  // Field feedback (2026-08-03): the header's route distance used to reuse
  // `info.distance_nm` — that field is the CUMULATIVE distance FLOWN so far
  // (ticks up from 0 server-side), correct for TripCard's own "Strecke"
  // cell (InfoStrip.tsx, left untouched), but wrong here: the header wants
  // the FIXED planned distance for the whole route, dep→arr, same
  // dpt/arr-airport-coords + haversine pattern RouteMap.tsx already uses
  // for its own progress-bar math — computed independently here so this
  // header doesn't depend on RouteMap/PhaseCard's internal state.
  const [routeDistanceNm, setRouteDistanceNm] = useState<number | null>(null);
  useEffect(() => {
    if (!info?.dpt_airport || !info?.arr_airport) return;
    let cancelled = false;
    void (async () => {
      try {
        const [dpt, arr] = await Promise.all([
          invoke<{ lat: number; lon: number } | null>("airport_get", { icao: info.dpt_airport }),
          invoke<{ lat: number; lon: number } | null>("airport_get", { icao: info.arr_airport }),
        ]);
        if (!cancelled && dpt?.lat != null && dpt?.lon != null && arr?.lat != null && arr?.lon != null) {
          setRouteDistanceNm(haversineNm(dpt.lat, dpt.lon, arr.lat, arr.lon));
        }
      } catch {
        // stays null — header falls back to the flown-so-far value below
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [info?.dpt_airport, info?.arr_airport]);

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
        ? `${info!.airline_icao} ${resolveFlightIdent(info!.flight_number, info!.callsign)}`
        : resolveFlightIdent(info!.flight_number, info!.callsign),
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
  // See `isFlightFinalizable` (pure, exported, unit-tested) for the condition.
  function isFinalizable(): boolean {
    if (!info) return false;
    return isFlightFinalizable(info.phase, info.landing_at);
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
   * v0.16.23: Sync ONLY the planned route from the latest SimBrief OFP
   * and redraw it on the map — available in EVERY flight phase (unlike
   * the full OFP refresh above, which stays Preflight–TaxiOut to protect
   * the fuel/weight loadsheet baseline). Real-pilot workflow: ATC reroute
   * mid-flight, pilot regenerates the SimBrief route, clicks "Sync route".
   * The backend writes only planned_route / planned_waypoints / alternate
   * and re-posts the route to phpVMS — no scored field is touched.
   */
  async function handleSyncRoute() {
    if (busy) return;
    setBusy("sync_route");
    setError(null);
    setRefreshMsg(null);
    try {
      const res = (await invoke("flight_refresh_route_only")) as {
        waypoint_count: number;
        route_posted: boolean;
      };
      // Differenzierte Erfolgsmeldung: Route lokal aktualisiert immer,
      // aber das phpVMS-Upload kann fehlschlagen (Warning, kein Error)
      // oder es gab keine Wegpunkte zu syncen.
      if (res.waypoint_count === 0) {
        setRefreshMsg(t("active_flight.sync_route_no_waypoints"));
      } else if (res.route_posted) {
        setRefreshMsg(t("active_flight.sync_route_done"));
      } else {
        setRefreshMsg(t("active_flight.sync_route_done_local"));
      }
    } catch (err: unknown) {
      // Gleicher shared Formatter wie der OFP-Refresh — rendert
      // no_simbrief_identifier (actionable) + den DEP/ARR-Mismatch-
      // Hard-Block lesbar.
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

  // #phase-v2 Cutover: `info.phase` ist jetzt die v2-Phase. Die Badge-
  // Entscheidung (inkl. „Level" bei Höhen-Restriktion) steckt in der puren
  // `phaseBadgeDisplay`-Helper (unten, exportiert + unit-getestet).
  const { labelKey } = phaseBadgeDisplay(
    info.phase,
    info.shadow_segment,
  );
  const phaseLabel = t(`active_flight.phase.${labelKey}`, {
    defaultValue: info.phase,
  });

  const elapsedMinutes = Math.max(
    0,
    Math.floor((Date.now() - new Date(info.started_at).getTime()) / 60000),
  );

  // v0.3.0: Loadsheet nur in Preflight/Boarding sichtbar (siehe
  // LoadsheetMonitor) — die grid4-Karten-Reihe hat dann 4 statt 3
  // Spalten, sonst würde die letzte Spalte als Lücke stehen bleiben.
  const showLoadsheet = info.phase === "preflight" || info.phase === "boarding";

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
                ? `${info.airline_icao} ${resolveFlightIdent(info.flight_number, info.callsign)}`
                : resolveFlightIdent(info.flight_number, info.callsign)}
            </h2>
            {/* Field feedback (2026-08-03): this badge duplicated the phase
                that's already shown big in the PhaseCard band right below —
                removed here, PhaseCard stays the single source for it. */}
          </div>
        </div>
        <div className="active-flight__route">
          <span className="active-flight__icao">{info.dpt_airport}</span>
          <span className="active-flight__route-arrow">
            <span className="active-flight__arrow">→</span>
            <span className="active-flight__route-distance">
              {fmtDistance(routeDistanceNm ?? info.distance_nm, i18n.language)}
            </span>
          </span>
          <span className="active-flight__icao">{info.arr_airport}</span>
        </div>
      </header>

      {/* Stage E redesign: instrument band (IAS tape · phase card ·
          altitude tape) replaces the old flat live-tapes strip + the
          RouteMap that used to sit between header and InfoStrip —
          RouteMap now renders inside PhaseCard's route track. */}
      <div className="band">
        <LiveTapes snapshot={simSnapshot ?? null} />
        <PhaseCard
          info={info}
          snapshot={simSnapshot ?? null}
          phaseLabel={phaseLabel}
          elapsedMinutes={elapsedMinutes}
        />
      </div>

      <div
        className="grid4"
        style={{ gridTemplateColumns: showLoadsheet ? undefined : "repeat(3, 1fr)" }}
      >
        <InfoStrip
          info={info}
          snapshot={simSnapshot ?? null}
          elapsedMinutes={elapsedMinutes}
        />
        {/* v0.3.0: Loadsheet als 4. Karte in derselben grid4-Reihe —
            gehört zum aktiven Flug, deshalb im selben Container.
            Verschwindet von alleine ab TaxiOut/Pushback (siehe
            LoadsheetMonitor). */}
        <LoadsheetMonitor info={info} />
      </div>

      <WeatherBriefing dptIcao={info.dpt_airport} arrIcao={info.arr_airport} />

      {refreshMsg && (
        <div className="active-flight__refresh-msg" role="status">
          ✓ {refreshMsg}
        </div>
      )}
      {error && (
        <p className="active-flight__error" role="alert">
          {error}
        </p>
      )}

      {weatherLoadHint && (
        <div className="cockpit-weather-toast" role="status">
          🌦 {t("cockpit.weather_briefing_load_hint")}
        </div>
      )}

      <div className="active-flight__actions">
        <button
          type="button"
          className="button button--primary"
          onClick={handleEnd}
          disabled={busy !== null}
        >
          {busy === "end" ? t("active_flight.filing") : t("active_flight.end")}
        </button>
        {/* v0.16.23: Route-Sync — in JEDER Phase verfügbar (im
            Gegensatz zum OFP-Refresh unten). Aktualisiert NUR die
            Karten-Route aus dem aktuellen SimBrief-OFP, kein
            Fuel/Gewicht/Score wird angefasst. Nützlich nach einem
            ATC-Reroute mitten im Flug. */}
        <button
          type="button"
          className="active-flight__sync-route"
          onClick={handleSyncRoute}
          disabled={busy !== null}
          title={t("active_flight.sync_route_hint")}
        >
          {busy === "sync_route"
            ? t("active_flight.sync_route_busy")
            : t("active_flight.sync_route")}
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
          className="cockpit-actions__weather"
          onClick={onOpenWeatherBriefing}
          title={t("cockpit.weather_briefing_hint")}
        >
          🌦 {t("cockpit.weather_briefing")}
        </button>
        <span className="actions__spacer" />
        <button type="button" onClick={handleCancel} disabled={busy !== null}>
          {busy === "cancel"
            ? t("active_flight.cancelling")
            : t("active_flight.cancel")}
        </button>
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
