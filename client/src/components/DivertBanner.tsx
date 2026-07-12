import { useEffect, useState } from "react";
import { useTranslation, Trans } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, FlightEndOutcome } from "../types";

interface Props {
  activeFlight: ActiveFlightInfo;
  /**
   * v0.12.5 (LE7-QS-P2): a real PIREP was filed via the divert banner
   * (submit-as-planned or submit-as-divert). The parent shows the green
   * success banner and clears `activeFlight` — same contract as
   * `ActiveFlightPanel.onFiledSuccess`.
   */
  onFiledSuccess: (outcome: FlightEndOutcome) => void;
}

/**
 * Banner shown in the cockpit when the FSM detected the aircraft
 * landed somewhere other than the planned `arr_airport`. Three
 * actions:
 *
 *   1. Submit as divert to <actual>     → divert-confirm modal (LE2)
 *   2. Submit as planned (no override)  → flight_end()
 *   3. Override: pick another airport   → manual modal → divert-confirm
 *
 * v0.12.5 (LE2): filing as a divert never calls `flight_end` directly
 * anymore — it always routes through `DivertConfirmModal`, which makes
 * the pilot enter a mandatory reason. Only "submit as planned" (= not a
 * divert) still files straight away.
 *
 * Hidden when `activeFlight.divert_hint` is null. Hidden also when
 * the flight is still in resume-banner-pending state (was_just_resumed)
 * — we want the resume choice to settle first before piling another
 * decision on the pilot.
 */
export function DivertBanner({ activeFlight, onFiledSuccess }: Props) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showOverride, setShowOverride] = useState(false);
  /** ICAO the pilot chose to file as a divert — drives the confirm modal. */
  const [confirmDivertTo, setConfirmDivertTo] = useState<string | null>(null);

  const hint = activeFlight.divert_hint;
  if (!hint) return null;
  if (activeFlight.was_just_resumed) return null;

  /** v0.12.5 (LE7-QS-P2): build the "filed" outcome for the success banner. */
  const filedOutcome = (arr: string): FlightEndOutcome => ({
    kind: "filed",
    callsign: activeFlight.airline_icao
      ? `${activeFlight.airline_icao} ${activeFlight.flight_number}`
      : activeFlight.flight_number,
    dpt: activeFlight.dpt_airport,
    arr,
  });

  // Skip the banner during early phases — only meaningful once the
  // FSM has actually settled at Arrived (or the universal fallback
  // promoted us). Otherwise a brief mid-flight excursion outside the
  // 2 nmi planned-arrival circle could paint a divert banner during
  // climb-out, which would be nonsensical.
  if (activeFlight.phase !== "arrived") return null;

  const titleKey =
    hint.kind === "alternate"
      ? "divert.title_alternate"
      : hint.kind === "nearest"
      ? "divert.title_nearest"
      : "divert.title_unknown";
  const bodyKey =
    hint.kind === "alternate"
      ? "divert.body_alternate"
      : hint.kind === "nearest"
      ? "divert.body_nearest"
      : "divert.body_unknown";

  /** "Submit as planned" — NOT a divert, files immediately. */
  const fileAsPlanned = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("flight_end", {});
      onFiledSuccess(filedOutcome(hint.planned_arr_icao));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const distanceLabel = Math.round(hint.distance_to_planned_nmi).toString();

  return (
    <>
      <section className="divert-banner" role="alert" aria-live="polite">
        <header className="divert-banner__header">
          <span className="divert-banner__icon" aria-hidden="true">
            ⚠
          </span>
          <h2 className="divert-banner__title">{t(titleKey)}</h2>
        </header>
        <p className="divert-banner__body">
          <Trans
            i18nKey={bodyKey}
            values={{
              actual: hint.actual_icao ?? "—",
              planned: hint.planned_arr_icao,
              distance: distanceLabel,
            }}
            components={{ strong: <strong /> }}
          />
        </p>
        {error && <p className="divert-banner__error">{error}</p>}
        <div className="divert-banner__actions">
          {hint.actual_icao && (
            <button
              type="button"
              className="button button--primary"
              disabled={busy}
              onClick={() => setConfirmDivertTo(hint.actual_icao)}
            >
              {t("divert.submit_as_divert", { actual: hint.actual_icao })}
            </button>
          )}
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => void fileAsPlanned()}
          >
            {busy
              ? t("divert.submitting")
              : t("divert.submit_as_planned", { planned: hint.planned_arr_icao })}
          </button>
          <button
            type="button"
            className="button button--ghost"
            disabled={busy}
            onClick={() => setShowOverride(true)}
          >
            {t("divert.manual_override")}
          </button>
        </div>
      </section>
      {showOverride && (
        <ManualDivertModal
          activeFlight={activeFlight}
          onClose={() => setShowOverride(false)}
          onPicked={(icao) => {
            setShowOverride(false);
            setConfirmDivertTo(icao);
          }}
        />
      )}
      {confirmDivertTo && (
        <DivertConfirmModal
          divertTo={confirmDivertTo}
          plannedIcao={hint.planned_arr_icao}
          onClose={() => setConfirmDivertTo(null)}
          onFiled={() => onFiledSuccess(filedOutcome(confirmDivertTo))}
        />
      )}
    </>
  );
}

interface ConfirmProps {
  /** Actual landing ICAO the flight will be filed as a divert to. */
  divertTo: string;
  /** The originally planned arrival ICAO — shown for context. */
  plannedIcao: string;
  onClose: () => void;
  /** Called after the divert PIREP was filed successfully. */
  onFiled: () => void;
}

/**
 * v0.12.5 (LE2): confirmation modal for filing a flight as a divert.
 * Pre-fills the actual landing airport and forces the pilot to enter a
 * mandatory reason — without a reason the divert cannot be filed
 * (frontend guard here, backend guard in `flight_end`). The reason is
 * passed to `flight_end` as `divertReason` and ends up both in the
 * phpVMS notes and the MQTT PIREP payload for audit.
 */
function DivertConfirmModal({
  divertTo,
  plannedIcao,
  onClose,
  onFiled,
}: ConfirmProps) {
  const { t } = useTranslation();
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmedReason = reason.trim();
  const canSubmit = trimmedReason.length > 0 && !busy;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("flight_end", {
        divertTo,
        divertReason: trimmedReason,
      });
      onFiled();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2 className="modal__title">{t("divert.confirm_title")}</h2>
        <p className="modal__hint">
          <Trans
            i18nKey="divert.confirm_body"
            values={{ actual: divertTo, planned: plannedIcao }}
            components={{ strong: <strong /> }}
          />
        </p>

        <div className="divert-confirm__reason">
          <label htmlFor="divert-reason">
            {t("divert.confirm_reason_label")}
          </label>
          <textarea
            id="divert-reason"
            rows={3}
            placeholder={t("divert.confirm_reason_placeholder")}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            disabled={busy}
            autoFocus
          />
          {trimmedReason.length === 0 && (
            <p className="modal__hint modal__hint--muted">
              {t("divert.confirm_reason_required")}
            </p>
          )}
        </div>

        {error && <p className="modal__error">{error}</p>}

        <div className="modal__footer">
          <button
            type="button"
            className="button"
            onClick={onClose}
            disabled={busy}
          >
            {t("divert.confirm_cancel")}
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={!canSubmit}
            onClick={() => void submit()}
          >
            {busy ? t("divert.submitting") : t("divert.confirm_submit")}
          </button>
        </div>
      </div>
    </div>
  );
}

interface NearestAirport {
  icao: string;
  lat: number;
  lon: number;
  distance_m: number;
  longest_runway_ft: number;
}

interface ManualProps {
  activeFlight: ActiveFlightInfo;
  onClose: () => void;
  /** Called with the chosen ICAO — parent opens the divert-confirm modal. */
  onPicked: (icao: string) => void;
}

/**
 * Modal that opens from the divert banner's "override" button.
 * Loads the 5 nearest airports from the local DB (via the
 * `divert_nearest_airports` Tauri command, which queries
 * runway::find_nearest_airports against the touchdown coords — v0.19.3;
 * before that it searched from the *current* position despite this comment,
 * so a long taxi-in could offer the wrong field at an airport cluster) and
 * lets the pilot pick one — or type a custom ICAO if their actual
 * landing field isn't in the runways table.
 *
 * v0.12.5 (LE2): picking an airport no longer files directly — it
 * hands the ICAO back to `DivertBanner`, which then opens the
 * mandatory-reason confirmation modal.
 */
function ManualDivertModal({ activeFlight, onClose, onPicked }: ManualProps) {
  const { t } = useTranslation();
  const [nearby, setNearby] = useState<NearestAirport[] | null>(null);
  const [custom, setCustom] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const r = await invoke<NearestAirport[]>(
          "divert_nearest_airports",
          { limit: 5 },
        );
        setNearby(r);
      } catch (e) {
        setError(String(e));
        setNearby([]);
      }
    })();
  }, []);

  const pick = (icao: string) => {
    onPicked(icao.trim().toUpperCase());
  };

  const fmtKm = (m: number): string => `${(m / 1000).toFixed(1)} km`;
  const fmtRunway = (ft: number): string =>
    ft > 0 ? `${(ft * 0.3048).toFixed(0)} m` : "—";

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      onClick={onClose}
    >
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2 className="modal__title">{t("divert.manual_modal_title")}</h2>
        <p className="modal__hint">{t("divert.manual_modal_hint")}</p>

        {nearby === null ? (
          <p className="modal__loading">…</p>
        ) : nearby.length === 0 ? (
          <p className="modal__empty">—</p>
        ) : (
          <ul className="divert-modal__list">
            {nearby.map((a) => {
              const isPlanned = a.icao === activeFlight.arr_airport;
              return (
                <li key={a.icao}>
                  <button
                    type="button"
                    className="divert-modal__candidate"
                    onClick={() => pick(a.icao)}
                  >
                    <span className="divert-modal__icao">{a.icao}</span>
                    <span className="divert-modal__meta">
                      {fmtKm(a.distance_m)} · runway {fmtRunway(a.longest_runway_ft)}
                      {isPlanned && " · planned"}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}

        <div className="divert-modal__custom">
          <label htmlFor="divert-icao">
            {t("divert.manual_modal_custom_label")}
          </label>
          <input
            id="divert-icao"
            type="text"
            maxLength={4}
            placeholder={t("divert.manual_modal_custom_placeholder")}
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
          />
          <button
            type="button"
            className="button button--primary"
            disabled={custom.trim().length < 3}
            onClick={() => pick(custom)}
          >
            {t("divert.manual_modal_submit")}
          </button>
        </div>

        {error && <p className="modal__error">{error}</p>}

        <div className="modal__footer">
          <button type="button" className="button" onClick={onClose}>
            {t("divert.manual_modal_cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
