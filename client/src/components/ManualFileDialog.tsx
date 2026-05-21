import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo } from "../types";

interface Props {
  /** Active flight — used to seed the form (planned arrival, current FSM
   *  values shown as placeholders so the pilot knows what AeroACARS would
   *  ship if they leave a field blank). */
  info: ActiveFlightInfo;
  /** List of i18n field keys reported by the backend's validation. */
  missing: string[];
  /** Called after the manual PIREP was filed successfully. */
  onFiled: () => void;
  /** Called when the user wants to cancel the flight (PIREP discarded server-side). */
  onCancelFlight: () => void;
  /** Called when the user dismisses the dialog without taking action. */
  onClose: () => void;
}

type Stage = "options" | "manual_form";

/** Convert "" / undefined → null; trims and rejects pure-whitespace. */
function strOrNull(v: string): string | null {
  const t = v.trim();
  return t.length > 0 ? t : null;
}

/** Parse a numeric string (with comma OR dot decimal) to f64.
 *  Returns null on empty / invalid input. Negative numbers OK so the pilot
 *  can enter a landing rate like -180. */
function numOrNull(v: string): number | null {
  const t = v.trim().replace(",", ".");
  if (t.length === 0) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/** Convert a `<input type="datetime-local">` value (local time, no zone)
 *  to an RFC-3339 UTC string. Returns null on empty input. */
function localToUtcIso(v: string): string | null {
  if (!v) return null;
  // datetime-local is "YYYY-MM-DDTHH:MM" without seconds/zone — Date()
  // interprets that as LOCAL time, then toISOString() converts to UTC Z.
  const d = new Date(v);
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString();
}

/** v0.12.5 (LE3): Convert an RFC-3339 UTC string back to a
 *  `<input type="datetime-local">` value (LOCAL time, "YYYY-MM-DDTHH:MM").
 *  Used to pre-fill the block-off/on fields from FSM-captured times. */
function utcIsoToLocal(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}`
  );
}

/** Format an optional number to a string for an input value (rounded). */
function numToInput(v: number | null): string {
  return v != null && Number.isFinite(v) ? String(Math.round(v)) : "";
}

export function ManualFileDialog({
  info,
  missing,
  onFiled,
  onCancelFlight,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("options");

  // v0.12.5 (LE3): die Felder werden aus den FSM-Defaults vorbefüllt —
  // der Pilot korrigiert nur Abweichungen statt alles blank einzutippen.
  const d = info.manual_filing_defaults;
  const ftMin = d.flight_time_minutes;

  // Routing
  const [divert, setDivert] = useState(d.divert_to ?? "");
  const [reason, setReason] = useState("");

  // Performance overrides
  const [distanceNm, setDistanceNm] = useState(numToInput(d.distance_nm));
  const [cruiseLevelFt, setCruiseLevelFt] = useState(
    numToInput(d.cruise_level_ft),
  );
  const [flightHours, setFlightHours] = useState(
    ftMin != null ? String(Math.floor(ftMin / 60)) : "",
  );
  const [flightMinutes, setFlightMinutes] = useState(
    ftMin != null ? String(ftMin % 60) : "",
  );
  const [landingRate, setLandingRate] = useState(
    numToInput(d.landing_rate_fpm),
  );
  const [blockOffLocal, setBlockOffLocal] = useState(
    utcIsoToLocal(d.block_off_at),
  );
  const [blockOnLocal, setBlockOnLocal] = useState(
    utcIsoToLocal(d.block_on_at),
  );

  // Fuel — LE4: der Pilot gibt die Restmenge im Tank ein, nicht den
  // Verbrauch. Das Backend rechnet used = block − remaining.
  const [blockFuelKg, setBlockFuelKg] = useState(numToInput(d.block_fuel_kg));
  const [remainingFuelKg, setRemainingFuelKg] = useState(
    numToInput(d.remaining_fuel_kg),
  );

  // Notes
  const [notes, setNotes] = useState("");

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submitManual() {
    const trimmedDivert = divert.trim().toUpperCase();
    const trimmedReason = reason.trim();
    // Divert without a reason is meaningless — the admin needs context.
    if (trimmedDivert && !trimmedReason) {
      setError(t("active_flight.validation.reason_required_for_divert"));
      return;
    }
    // Combine hours + minutes into total minutes when EITHER is filled.
    const h = numOrNull(flightHours);
    const m = numOrNull(flightMinutes);
    const flightTimeMinutes =
      h != null || m != null ? Math.max(0, (h ?? 0) * 60 + (m ?? 0)) : null;

    setBusy(true);
    setError(null);
    try {
      await invoke("flight_end_manual", {
        notesOverride: strOrNull(notes),
        divertTo: trimmedDivert || null,
        reason: trimmedReason || null,
        flightTimeMinutes,
        blockFuelKg: numOrNull(blockFuelKg),
        remainingFuelKg: numOrNull(remainingFuelKg),
        distanceNm: numOrNull(distanceNm),
        cruiseLevelFt:
          numOrNull(cruiseLevelFt) != null
            ? Math.round(numOrNull(cruiseLevelFt)!)
            : null,
        landingRateFpm: numOrNull(landingRate),
        blockOffAtIso: localToUtcIso(blockOffLocal),
        blockOnAtIso: localToUtcIso(blockOnLocal),
      });
      onFiled();
    } catch (err: unknown) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      setError(`${t("active_flight.validation.manual_failed")}\n\n${msg}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="manual-dialog-backdrop" role="dialog" aria-modal="true">
      <div className="manual-dialog manual-dialog--wide">
        {stage === "options" && (
          <>
            <h2 className="manual-dialog__title">
              {t("active_flight.validation.title")}
            </h2>
            <p className="manual-dialog__intro">
              {t("active_flight.validation.intro")}
            </p>
            <ul className="manual-dialog__missing">
              {missing.map((key) => (
                <li key={key}>
                  {t(`active_flight.validation.fields.${key}`, {
                    defaultValue: key,
                  })}
                </li>
              ))}
            </ul>

            <h3 className="manual-dialog__subtitle">
              {t("active_flight.validation.options_title")}
            </h3>
            <div className="manual-dialog__options">
              <button
                type="button"
                className="button button--primary"
                onClick={() => setStage("manual_form")}
                disabled={busy}
              >
                {t("active_flight.validation.option_manual")}
              </button>
              <span className="manual-dialog__hint">
                {t("active_flight.validation.option_manual_hint")}
              </span>

              <button
                type="button"
                className="manual-dialog__danger"
                onClick={onCancelFlight}
                disabled={busy}
              >
                {t("active_flight.validation.option_cancel")}
              </button>
              <span className="manual-dialog__hint">
                {t("active_flight.validation.option_cancel_hint")}
              </span>

              <button
                type="button"
                className="manual-dialog__secondary"
                onClick={onClose}
                disabled={busy}
              >
                {t("active_flight.validation.option_back")}
              </button>
            </div>
          </>
        )}

        {stage === "manual_form" && (
          <>
            <h2 className="manual-dialog__title">
              {t("active_flight.validation.manual_form_title")}
            </h2>
            <p className="manual-dialog__intro">
              {t("active_flight.validation.manual_form_intro")}
            </p>

            <div className="manual-dialog__summary">
              <span>
                <strong>{info.flight_number}</strong>
              </span>
              <span>
                {info.dpt_airport} → {info.arr_airport}
              </span>
              {info.distance_nm > 0 && (
                <span>{info.distance_nm.toFixed(0)} nm</span>
              )}
            </div>

            {/* ---- Routing ---- */}
            <h3 className="manual-form__section-title">
              {t("active_flight.validation.section_routing")}
            </h3>
            <div className="manual-form__grid">
              <label className="manual-form__field">
                <span>{t("active_flight.validation.divert_label")}</span>
                <input
                  type="text"
                  value={divert}
                  onChange={(e) => setDivert(e.target.value.toUpperCase())}
                  maxLength={4}
                  placeholder="EDDV"
                  disabled={busy}
                />
                <small>
                  {t("active_flight.validation.divert_hint", {
                    planned: info.arr_airport,
                  })}
                </small>
              </label>
              <label className="manual-form__field manual-form__field--wide">
                <span>{t("active_flight.validation.reason_label")}</span>
                <textarea
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                  rows={2}
                  placeholder={t(
                    "active_flight.validation.reason_placeholder",
                  )}
                  disabled={busy}
                />
              </label>
            </div>

            {/* ---- Performance ---- */}
            <h3 className="manual-form__section-title">
              {t("active_flight.validation.section_performance")}
            </h3>
            <div className="manual-form__grid">
              <label className="manual-form__field">
                <span>{t("active_flight.validation.distance_label")}</span>
                <input
                  type="text"
                  inputMode="decimal"
                  value={distanceNm}
                  onChange={(e) => setDistanceNm(e.target.value)}
                  placeholder={
                    info.distance_nm > 0 ? info.distance_nm.toFixed(0) : "0"
                  }
                  disabled={busy}
                />
                <small>{t("active_flight.validation.distance_hint")}</small>
              </label>
              <label className="manual-form__field">
                <span>{t("active_flight.validation.cruise_level_label")}</span>
                <input
                  type="text"
                  inputMode="numeric"
                  value={cruiseLevelFt}
                  onChange={(e) => setCruiseLevelFt(e.target.value)}
                  placeholder="35000"
                  disabled={busy}
                />
                <small>
                  {t("active_flight.validation.cruise_level_hint")}
                </small>
              </label>
              <label className="manual-form__field">
                <span>{t("active_flight.validation.flight_time_label")}</span>
                <div className="manual-form__row">
                  <input
                    type="text"
                    inputMode="numeric"
                    value={flightHours}
                    onChange={(e) => setFlightHours(e.target.value)}
                    placeholder="0"
                    style={{ width: "4rem" }}
                    disabled={busy}
                  />
                  <span>{t("active_flight.validation.hours_short")}</span>
                  <input
                    type="text"
                    inputMode="numeric"
                    value={flightMinutes}
                    onChange={(e) => setFlightMinutes(e.target.value)}
                    placeholder="0"
                    style={{ width: "4rem" }}
                    disabled={busy}
                  />
                  <span>{t("active_flight.validation.minutes_short")}</span>
                </div>
                <small>{t("active_flight.validation.flight_time_hint")}</small>
              </label>
              <label className="manual-form__field">
                <span>{t("active_flight.validation.landing_rate_label")}</span>
                <input
                  type="text"
                  inputMode="decimal"
                  value={landingRate}
                  onChange={(e) => setLandingRate(e.target.value)}
                  placeholder="-180"
                  disabled={busy}
                />
                <small>
                  {t("active_flight.validation.landing_rate_hint")}
                </small>
              </label>
              <label className="manual-form__field">
                <span>{t("active_flight.validation.block_off_label")}</span>
                <input
                  type="datetime-local"
                  value={blockOffLocal}
                  onChange={(e) => setBlockOffLocal(e.target.value)}
                  disabled={busy}
                />
                <small>{t("active_flight.validation.block_off_hint")}</small>
              </label>
              <label className="manual-form__field">
                <span>{t("active_flight.validation.block_on_label")}</span>
                <input
                  type="datetime-local"
                  value={blockOnLocal}
                  onChange={(e) => setBlockOnLocal(e.target.value)}
                  disabled={busy}
                />
                <small>{t("active_flight.validation.block_on_hint")}</small>
              </label>
            </div>

            {/* ---- Fuel ---- */}
            <h3 className="manual-form__section-title">
              {t("active_flight.validation.section_fuel")}
            </h3>
            <div className="manual-form__grid">
              <label className="manual-form__field">
                <span>{t("active_flight.validation.block_fuel_label")}</span>
                <input
                  type="text"
                  inputMode="decimal"
                  value={blockFuelKg}
                  onChange={(e) => setBlockFuelKg(e.target.value)}
                  placeholder="0"
                  disabled={busy}
                />
                <small>{t("active_flight.validation.block_fuel_hint")}</small>
              </label>
              <label className="manual-form__field">
                <span>
                  {t("active_flight.validation.remaining_fuel_label")}
                </span>
                <input
                  type="text"
                  inputMode="decimal"
                  value={remainingFuelKg}
                  onChange={(e) => setRemainingFuelKg(e.target.value)}
                  placeholder="0"
                  disabled={busy}
                />
                <small>
                  {t("active_flight.validation.remaining_fuel_hint")}
                </small>
              </label>
            </div>

            {/* ---- Notes ---- */}
            <h3 className="manual-form__section-title">
              {t("active_flight.validation.section_notes")}
            </h3>
            <label className="manual-form__field manual-form__field--wide">
              <textarea
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                rows={3}
                placeholder={t("active_flight.validation.notes_placeholder")}
                disabled={busy}
              />
            </label>

            {error && (
              <p className="manual-dialog__error" role="alert">
                {error}
              </p>
            )}

            <div className="manual-dialog__options">
              <button
                type="button"
                className="button button--primary"
                onClick={() => void submitManual()}
                disabled={busy}
              >
                {busy
                  ? t("active_flight.validation.submitting_manual")
                  : t("active_flight.validation.submit_manual")}
              </button>
              <button
                type="button"
                className="manual-dialog__secondary"
                onClick={() => setStage("options")}
                disabled={busy}
              >
                {t("active_flight.validation.option_back")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
