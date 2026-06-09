import { useEffect, useRef, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, ResumableFlight, SimStatus } from "../types";
import { useConfirm } from "./ConfirmDialog";

const COUNTDOWN_SECONDS = 30;

/** Toleranzen für die "grün/rot"-Färbung der Δ-Spalte im Vergleichsblock. */
const TOL_POSITION_NM = 5.0;
const TOL_ALT_FT = 500;
const TOL_FUEL_KG = 500;
const TOL_WEIGHT_KG = 1000;

interface Props {
  /** Already-attached active flight (e.g. restored from disk). */
  activeFlight: ActiveFlightInfo | null;
  /** Notify the dashboard when adoption succeeded. */
  onAdopted: (info: ActiveFlightInfo) => void;
  /** Notify the dashboard when the flight was cancelled. */
  onCancelled: () => void;
}

type Mode =
  | { kind: "idle" }
  | {
      kind: "auto_resumed";
      flight: ActiveFlightInfo;
      secondsLeft: number;
      busy: boolean;
    }
  | {
      kind: "discovered";
      flight: ResumableFlight;
      secondsLeft: number;
      busy: boolean;
    };

export function ResumeFlightBanner({
  activeFlight,
  onAdopted,
  onCancelled,
}: Props) {
  const { t } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [mode, setMode] = useState<Mode>({ kind: "idle" });
  const consumedRef = useRef(false);
  const confirmingRef = useRef(false);
  // v0.13.12 (Michel-Befund): positionSuspect als reactive Value statt Ref.
  // Vorher: positionSuspectRef wurde via useEffect aktualisiert — React-Refs
  // triggern KEINEN Re-Render und keine erneute Auswertung von dependent
  // useEffects. Wenn der Backend-Status flippte (Pilot positioniert nach
  // Sim-Crash zurueck → on_ground=false → positionSuspect false), blieb der
  // Countdown-Effect bei positionSuspectRef.current=true haengen und der
  // Countdown lief nie an. Mit reactive Value triggert der Countdown-Effect
  // ueber die [mode, positionSuspect] dep automatisch erneut.
  const positionSuspect = activeFlight?.resume_position_suspect === true;

  // v0.13.10 (QS-Round-1 Fix): consumedRef zuruecksetzen sobald
  // was_just_resumed im Backend auf false transitioniert (Pilot hat den
  // Banner via Re-Check, Force-Resume oder Cancel resolved). Ohne diesen
  // Reset wuerde ein zweiter Mid-Session-Sim-Crash NICHT erneut den Banner
  // anzeigen — exakt das Pattern was wir bei Pilot Michel (3x XP12-Crash
  // pro Flug) beobachtet haben. Mit Reset triggert jeder Crash neu.
  useEffect(() => {
    if (activeFlight && !activeFlight.was_just_resumed) {
      consumedRef.current = false;
    }
  }, [activeFlight?.was_just_resumed]);

  // v0.13.12 (Michel-Befund): Wenn das Backend was_just_resumed cleared
  // (z.B. spawn_resume_sim_gate hat einen frischen Sim-Snapshot bekommen
  // und schaltet den Flug scharf, oder flight_resume_check_position ist
  // clean durchgelaufen) MUSS das Banner verschwinden. Vorher: mode blieb
  // in "auto_resumed" haengen, Banner war weiter sichtbar obwohl der
  // Resume bereits durchgelaufen war. Pilot konnte zwar Resume-Now
  // druecken, aber das Banner sollte sich selbst schliessen sobald der
  // Backend-Status sagt "fertig".
  useEffect(() => {
    if (
      mode.kind === "auto_resumed" &&
      activeFlight &&
      !activeFlight.was_just_resumed
    ) {
      setMode({ kind: "idle" });
    }
  }, [activeFlight?.was_just_resumed, mode.kind]);

  // Disk-resume Banner
  useEffect(() => {
    if (
      activeFlight &&
      activeFlight.was_just_resumed &&
      mode.kind === "idle" &&
      !consumedRef.current
    ) {
      consumedRef.current = true;
      setMode({
        kind: "auto_resumed",
        flight: activeFlight,
        secondsLeft: COUNTDOWN_SECONDS,
        busy: false,
      });
    }
  }, [activeFlight, mode.kind]);

  // phpVMS-discovered
  useEffect(() => {
    if (activeFlight) return;
    if (mode.kind !== "idle") return;
    let cancelled = false;
    void (async () => {
      try {
        const list = await invoke<ResumableFlight[]>(
          "flight_discover_resumable",
        );
        if (cancelled) return;
        if (list.length > 0) {
          consumedRef.current = true;
          setMode({
            kind: "discovered",
            flight: list[0]!,
            secondsLeft: COUNTDOWN_SECONDS,
            busy: false,
          });
        }
      } catch {
        // ignore
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight, mode.kind]);

  // Countdown ticker
  // v0.13.12 (Michel-Befund): positionSuspect zur dep-Liste hinzugefuegt,
  // damit der Effect erneut laeuft sobald der Backend-Flag flippt (z.B.
  // Pilot positioniert nach Sim-Crash zurueck — Hard-Stop weicht dem
  // normalen Countdown). Vorher blieb der Countdown eingefroren weil
  // positionSuspectRef.current als Ref kein Re-Render ausloest.
  useEffect(() => {
    if (mode.kind !== "auto_resumed" && mode.kind !== "discovered") return;
    if (mode.busy) return;
    if (mode.kind === "auto_resumed" && positionSuspect) return;
    if (mode.secondsLeft <= 0) {
      if (confirmingRef.current) return;
      confirmingRef.current = true;
      void doConfirm();
      return;
    }
    const timer = setTimeout(() => {
      setMode((prev) =>
        prev.kind === "auto_resumed" || prev.kind === "discovered"
          ? { ...prev, secondsLeft: prev.secondsLeft - 1 }
          : prev,
      );
    }, 1000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, positionSuspect]);

  async function doConfirm() {
    if (mode.kind === "auto_resumed") {
      setMode((prev) =>
        prev.kind === "auto_resumed" ? { ...prev, busy: true } : prev,
      );
      try {
        await invoke("flight_resume_confirm");
        setMode({ kind: "idle" });
      } catch (err) {
        const msg = errMsg(err);
        alert(`${t("resume.confirm_failed")}\n\n${msg}`);
        setMode({ kind: "idle" });
      }
      return;
    }
    if (mode.kind === "discovered") {
      const pirepId = mode.flight.pirep_id;
      setMode((prev) =>
        prev.kind === "discovered" ? { ...prev, busy: true } : prev,
      );
      try {
        const info = await invoke<ActiveFlightInfo>("flight_adopt", {
          pirepId,
        });
        await invoke("flight_resume_confirm");
        onAdopted(info);
        setMode({ kind: "idle" });
      } catch (err) {
        const msg = errMsg(err);
        alert(`${t("resume.adopt_failed")}\n\n${msg}`);
        setMode({ kind: "idle" });
      }
    }
  }

  async function doCancel() {
    if (mode.kind !== "auto_resumed" && mode.kind !== "discovered") return;
    if (
      !(await confirm({
        message: t("resume.confirm_cancel"),
        destructive: true,
      }))
    )
      return;
    setMode((prev) =>
      prev.kind === "auto_resumed" || prev.kind === "discovered"
        ? { ...prev, busy: true }
        : prev,
    );
    try {
      if (mode.kind === "discovered") {
        await invoke<ActiveFlightInfo>("flight_adopt", {
          pirepId: mode.flight.pirep_id,
        });
      }
      await invoke("flight_cancel");
      onCancelled();
      setMode({ kind: "idle" });
    } catch (err) {
      const msg = errMsg(err);
      alert(`${t("resume.cancel_failed")}\n\n${msg}`);
      setMode({ kind: "idle" });
    }
  }

  if (mode.kind === "idle") return null;

  // v0.13.12: lokale JSX-Variable umbenannt — `positionSuspect` ist jetzt
  // oben als reactive Value definiert. Diese hier kombiniert sie mit dem
  // Mode-Check fuer die JSX-Variantenwahl (Hard-Stop vs Normal-Countdown).
  const showHardStop = mode.kind === "auto_resumed" && positionSuspect;

  const flight =
    mode.kind === "auto_resumed"
      ? {
          callsign: mode.flight.airline_icao
            ? `${mode.flight.airline_icao} ${mode.flight.flight_number}`
            : mode.flight.flight_number,
          dpt_airport: mode.flight.dpt_airport,
          arr_airport: mode.flight.arr_airport,
        }
      : {
          callsign: mode.flight.flight_number,
          dpt_airport: mode.flight.dpt_airport,
          arr_airport: mode.flight.arr_airport,
        };

  // ─────────────────────────────────────────────────────────────────
  // v0.13.0 Stream F: Hard-Stop-Variante — Banner im Pause-Banner-
  // Look (rote Variante), Vergleichs-Grid statt Pause-Grid, 3-Button-
  // Recheck-Workflow statt Single-Resume-Button.
  // ─────────────────────────────────────────────────────────────────
  if (showHardStop) {
    return (
      <section
        className="active-flight__paused-banner active-flight__paused-banner--hard"
        role="alert"
        aria-live="polite"
      >
        {confirmDialog}
        <div className="active-flight__paused-header">
          <span className="active-flight__paused-icon" aria-hidden="true">
            ⚠
          </span>
          <div>
            <strong>{t("resume.hard_stop_title")}</strong>
            <span className="active-flight__paused-since">
              {flight.callsign} · {flight.dpt_airport} → {flight.arr_airport}
            </span>
          </div>
        </div>
        <p className="active-flight__paused-instructions">
          {t("resume.hard_stop_body")}
        </p>
        <ComparisonGrid activeFlight={activeFlight} />
        <RecheckActions
          busy={mode.busy}
          onConfirm={() => {
            if (confirmingRef.current) return;
            confirmingRef.current = true;
            void doConfirm();
          }}
          onCancel={() => void doCancel()}
        />
      </section>
    );
  }

  // ─── Standard-Variante: normaler Resume mit Countdown ────────────
  return (
    <section className="resume-modal" role="status" aria-live="polite">
      {confirmDialog}
      <div className="resume-modal__header">
        <span className="resume-modal__icon" aria-hidden="true">
          ✈
        </span>
        <h2 className="resume-modal__title">{t("resume.title")}</h2>
      </div>

      <div className="resume-modal__route">
        <div className="resume-modal__icao">{flight.dpt_airport}</div>
        <div className="resume-modal__arrow">→</div>
        <div className="resume-modal__icao">{flight.arr_airport}</div>
      </div>

      <div className="resume-modal__callsign">{flight.callsign}</div>

      <div className="resume-modal__countdown">
        <div
          className="resume-modal__countdown-bar"
          style={{
            width: `${(mode.secondsLeft / COUNTDOWN_SECONDS) * 100}%`,
          }}
        />
        <span className="resume-modal__countdown-text">
          {t("resume.countdown", { seconds: mode.secondsLeft })}
        </span>
      </div>

      <div className="resume-modal__actions">
        <button
          type="button"
          className="button button--primary resume-modal__primary"
          onClick={() => {
            if (confirmingRef.current) return;
            confirmingRef.current = true;
            void doConfirm();
          }}
          disabled={mode.busy}
        >
          {mode.busy ? t("resume.adopting") : t("resume.adopt_now")}
        </button>
        <button
          type="button"
          className="resume-modal__danger"
          onClick={() => void doCancel()}
          disabled={mode.busy}
        >
          {t("resume.cancel_flight")}
        </button>
      </div>
    </section>
  );
}

function errMsg(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    return String((err as { message: string }).message);
  }
  return String(err);
}

// ─── v0.13.0 Stream F: Live-Vergleichs-Grid ──────────────────────────
//
// Drei Spalten pro Parameter:
//   GESPEICHERT  |  SIM JETZT  |  Δ
//
// Sim-Werte werden via Polling von `sim_status` Tauri-Command live
// abgerufen (1 s Intervall, nur solange das Banner sichtbar ist).
// Δ-Spalte ist grün wenn innerhalb der Toleranz (TOL_* Konstanten),
// rot wenn drüber, grau wenn nicht vergleichbar (z. B. ZFW null im Sim).

function ComparisonGrid({
  activeFlight,
}: {
  activeFlight: ActiveFlightInfo | null;
}) {
  const [sim, setSim] = useState<SimStatus | null>(null);

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    async function poll() {
      try {
        const next = await invoke<SimStatus>("sim_status");
        if (!alive) return;
        setSim(next);
      } catch {
        // ignore — Sim noch nicht hochgefahren, Grid zeigt "—" für Sim-Spalte
      }
      if (alive) timer = setTimeout(() => void poll(), 1000);
    }
    void poll();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  }, []);

  const snap = sim?.snapshot ?? null;

  // ── Saved-Werte
  const sLat = activeFlight?.last_known_lat;
  const sLon = activeFlight?.last_known_lon;
  const sAlt = activeFlight?.last_known_alt_ft;
  const sFuel = activeFlight?.last_known_fuel_kg;
  const sZfw = activeFlight?.last_known_zfw_kg;
  const sTow = activeFlight?.last_known_total_weight_kg;
  const sAircraft = activeFlight?.last_known_aircraft_icao;

  // ── Sim-Werte
  const cLat = snap?.lat;
  const cLon = snap?.lon;
  const cAlt = snap?.altitude_msl_ft;
  const cHdg = snap?.heading_deg_true;
  const cFuel = snap?.fuel_total_kg;
  const cZfw = snap?.zfw_kg ?? null;
  const cTow = snap?.total_weight_kg ?? null;
  const cAircraft = snap?.aircraft_icao ?? null;

  // ── Δ-Berechnungen
  const posDriftNm =
    sLat !== undefined &&
    sLon !== undefined &&
    cLat !== undefined &&
    cLon !== undefined
      ? haversineNm(sLat, sLon, cLat, cLon)
      : null;
  const fuelDelta =
    sFuel !== undefined && cFuel !== undefined ? cFuel - sFuel : null;
  const zfwDelta =
    sZfw !== undefined && cZfw !== null && cZfw !== undefined
      ? cZfw - sZfw
      : null;
  const towDelta =
    sTow !== undefined && cTow !== null && cTow !== undefined
      ? cTow - sTow
      : null;
  const altDelta =
    sAlt !== undefined && cAlt !== undefined ? cAlt - sAlt : null;
  const aircraftMatch =
    sAircraft && cAircraft
      ? sAircraft.toUpperCase() === cAircraft.toUpperCase()
      : null;

  return (
    <div className="resume-compare" role="table">
      <div className="resume-compare__head">&nbsp;</div>
      <div className="resume-compare__head">Gespeichert</div>
      <div className="resume-compare__head">Sim jetzt</div>
      <div className="resume-compare__head">Δ</div>

      {/* Position */}
      <div className="resume-compare__label">Position</div>
      <Val>{sLat !== undefined && sLon !== undefined ? fmtPos(sLat, sLon) : null}</Val>
      <Val>{cLat !== undefined && cLon !== undefined ? fmtPos(cLat, cLon) : null}</Val>
      <Delta
        value={posDriftNm}
        format={(v) => `${v.toFixed(1)} nm`}
        ok={(v) => v < TOL_POSITION_NM}
        signed={false}
      />

      {/* Altitude */}
      <div className="resume-compare__label">Altitude</div>
      <Val>{sAlt !== undefined ? `${sAlt.toLocaleString()} ft` : null}</Val>
      <Val>{cAlt !== undefined ? `${Math.round(cAlt).toLocaleString()} ft` : null}</Val>
      <Delta
        value={altDelta}
        format={(v) => `${Math.round(v).toLocaleString()} ft`}
        ok={(v) => Math.abs(v) < TOL_ALT_FT}
        signed
      />

      {/* Heading (nur Sim-Anzeige, kein Saved-Wert) */}
      <div className="resume-compare__label">Heading</div>
      <Val>{null}</Val>
      <Val>{cHdg !== undefined ? `${Math.round(cHdg)}°` : null}</Val>
      <div className="resume-compare__delta resume-compare__delta--neutral">
        —
      </div>

      {/* Aircraft */}
      <div className="resume-compare__label">Aircraft</div>
      <Val>{sAircraft ?? null}</Val>
      <Val>{cAircraft ?? null}</Val>
      <div
        className={`resume-compare__delta ${
          aircraftMatch === null
            ? "resume-compare__delta--neutral"
            : aircraftMatch
            ? "resume-compare__delta--ok"
            : "resume-compare__delta--bad"
        }`}
      >
        {aircraftMatch === null ? "—" : aircraftMatch ? "✓ match" : "⚠ mismatch"}
      </div>

      {/* Fuel */}
      <div className="resume-compare__label">Fuel</div>
      <Val>{sFuel !== undefined ? fmtKg(sFuel) : null}</Val>
      <Val>{cFuel !== undefined ? fmtKg(cFuel) : null}</Val>
      <Delta
        value={fuelDelta}
        format={(v) => fmtKg(v)}
        ok={(v) => Math.abs(v) < TOL_FUEL_KG}
        signed
      />

      {/* ZFW */}
      <div className="resume-compare__label">ZFW</div>
      <Val>{sZfw !== undefined ? fmtKg(sZfw) : null}</Val>
      <Val>{cZfw !== null && cZfw !== undefined ? fmtKg(cZfw) : null}</Val>
      <Delta
        value={zfwDelta}
        format={(v) => fmtKg(v)}
        ok={(v) => Math.abs(v) < TOL_WEIGHT_KG}
        signed
      />

      {/* Total Weight */}
      <div className="resume-compare__label">Total Weight</div>
      <Val>{sTow !== undefined ? fmtKg(sTow) : null}</Val>
      <Val>{cTow !== null && cTow !== undefined ? fmtKg(cTow) : null}</Val>
      <Delta
        value={towDelta}
        format={(v) => fmtKg(v)}
        ok={(v) => Math.abs(v) < TOL_WEIGHT_KG}
        signed
      />
    </div>
  );
}

function Val({ children }: { children: string | null | undefined }) {
  if (children === null || children === undefined || children === "") {
    return <div className="resume-compare__val resume-compare__val--missing">—</div>;
  }
  return <div className="resume-compare__val">{children}</div>;
}

function Delta({
  value,
  format,
  ok,
  signed,
}: {
  value: number | null;
  format: (v: number) => string;
  ok: (v: number) => boolean;
  signed: boolean;
}) {
  if (value === null) {
    return <div className="resume-compare__delta resume-compare__delta--neutral">—</div>;
  }
  const cls = ok(value)
    ? "resume-compare__delta--ok"
    : "resume-compare__delta--bad";
  const sign = signed ? (value > 0 ? "+" : value < 0 ? "−" : "±") : "";
  const txt = signed ? sign + format(Math.abs(value)) : format(value);
  const icon = ok(value) ? "✓" : "⚠";
  return (
    <div className={`resume-compare__delta ${cls}`}>
      {icon} {txt}
    </div>
  );
}

// Formatter
function fmtPos(lat: number, lon: number): string {
  const latH = lat >= 0 ? "N" : "S";
  const lonH = lon >= 0 ? "E" : "W";
  return `${Math.abs(lat).toFixed(4)}°${latH} · ${Math.abs(lon).toFixed(4)}°${lonH}`;
}

function fmtKg(kg: number): string {
  return `${Math.round(kg).toLocaleString()} kg`;
}

function haversineNm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const R_NM = 3440.065;
  const toRad = (d: number) => (d * Math.PI) / 180;
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return R_NM * c;
}

// ─── RecheckActions (im Pause-Banner-Action-Look) ────────────────────

function RecheckActions({
  busy,
  onConfirm,
  onCancel,
}: {
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [checking, setChecking] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);
  const [showForce, setShowForce] = useState(false);

  async function doRecheck() {
    setChecking(true);
    setLastError(null);
    try {
      const r = await invoke<{ ok: boolean; detail: string }>(
        "flight_resume_check_position",
      );
      if (r.ok) {
        onConfirm();
      } else {
        setLastError(r.detail);
      }
    } catch (err) {
      setLastError(errMsg(err));
    } finally {
      setChecking(false);
    }
  }

  return (
    <>
      {lastError && (
        <p className="active-flight__paused-error" role="alert">
          ⚠ {lastError}
        </p>
      )}
      <div className="active-flight__paused-actions">
        {!showForce ? (
          <button
            type="button"
            className="button"
            style={{ opacity: 0.7 }}
            onClick={() => setShowForce(true)}
            disabled={busy || checking}
          >
            {t("resume.recheck_show_force")}
          </button>
        ) : (
          <button
            type="button"
            className="button"
            style={{
              background: "rgba(251,191,36,0.15)",
              borderColor: "rgba(251,191,36,0.5)",
              color: "#fbbf24",
            }}
            onClick={onConfirm}
            disabled={busy || checking}
          >
            ⚠ {t("resume.hard_stop_force_resume")}
          </button>
        )}
        <button
          type="button"
          className="resume-modal__danger"
          onClick={onCancel}
          disabled={busy || checking}
        >
          {t("resume.hard_stop_discard")}
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => void doRecheck()}
          disabled={busy || checking}
        >
          {checking ? t("resume.recheck_checking") : t("resume.recheck_check_now")}
        </button>
      </div>
    </>
  );
}
