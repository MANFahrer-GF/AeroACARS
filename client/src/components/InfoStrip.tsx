import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, SimSnapshot } from "../types";

interface Props {
  info: ActiveFlightInfo;
  snapshot: SimSnapshot | null;
  /** Elapsed minutes since flight start — passed in so the parent
   *  ticking already drives this without a duplicate timer here. */
  elapsedMinutes: number;
}

/**
 * Three cards in the Cockpit grid4 (Stage E redesign) — Weights, Air
 * Data, Trip — replacing the single 3-Group .info-strip panel. Same
 * data, same i18n keys; only the markup changed (from one strip with
 * inline groups to three standalone .card elements so they sit next to
 * the Loadsheet card in the grid).
 *
 * Design notes:
 *   * Mono-numerics with tabular-nums so columns line up.
 *   * Gross weight / the Air-Data reference speed / (Trip has no single
 *     headline — see TripCard) get the bold "row--sum" treatment,
 *     matching the Weights/Air-Data cards' one dispatch-relevant figure.
 */
export function InfoStrip({ info, snapshot, elapsedMinutes }: Props) {
  return (
    <>
      <WeightsCard snapshot={snapshot} />
      <AirDataCard snapshot={snapshot} />
      <TripCard info={info} elapsedMinutes={elapsedMinutes} />
    </>
  );
}

function WeightsCard({ snapshot }: { snapshot: SimSnapshot | null }) {
  const { t, i18n } = useTranslation();
  const fob = snapshot && snapshot.fuel_total_kg > 0 ? snapshot.fuel_total_kg : null;
  const gw =
    snapshot && snapshot.total_weight_kg !== null && snapshot.total_weight_kg > 0
      ? snapshot.total_weight_kg
      : null;
  const zfw =
    snapshot && snapshot.zfw_kg !== null && snapshot.zfw_kg > 0
      ? snapshot.zfw_kg
      : null;
  const dow = computeDow(snapshot);
  const payload = dow !== null && zfw !== null && zfw > dow ? zfw - dow : null;

  return (
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("mass_panel.title")}</span>
      </div>
      <div className="card__body">
        <Row label={t("mass_panel.dow")} value={fmtKgShort(dow, i18n.language)} />
        <Row label={t("mass_panel.payload")} value={fmtKgShort(payload, i18n.language)} />
        <Row label={t("mass_panel.zfw")} value={fmtKgShort(zfw, i18n.language)} />
        <Row label={t("mass_panel.fob")} value={fmtKgShort(fob, i18n.language)} />
        <Row label={t("mass_panel.gross_weight")} value={fmtKgShort(gw, i18n.language)} sum />
      </div>
    </div>
  );
}

/**
 * v0.16.x hysteresis (Stage E redesign spec): the reference-speed
 * headline row shows Mach once cruise speed reaches M0.50, and reverts
 * to TAS-in-knots only once it drops below M0.45 — a dead band so the
 * label doesn't flicker back and forth around M0.50 the way a naive
 * single-threshold switch would (same idea as a real PFD's speed-tape
 * Mach/IAS crossover).
 */
function AirDataCard({ snapshot }: { snapshot: SimSnapshot | null }) {
  const { t, i18n } = useTranslation();
  const mach = snapshot?.mach ?? null;
  const [showMach, setShowMach] = useState(false);
  const showMachRef = useRef(showMach);
  showMachRef.current = showMach;
  useEffect(() => {
    if (mach == null) return;
    if (!showMachRef.current && mach >= 0.5) setShowMach(true);
    else if (showMachRef.current && mach < 0.45) setShowMach(false);
  }, [mach]);

  const tas = snapshot ? Math.round(snapshot.true_airspeed_kt) : null;
  const refLabel = showMach && mach != null ? t("flight_info.mach") : t("flight_info.tas");
  const refValue =
    showMach && mach != null ? fmtMach(mach) : fmtTas(tas, i18n.language);

  return (
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("flight_info.title")}</span>
      </div>
      <div className="card__body">
        <Row label={t("flight_info.wind")} value={fmtWind(snapshot)} />
        <Row label={t("flight_info.qnh")} value={fmtQnh(snapshot, i18n.language)} />
        <Row label={t("flight_info.oat")} value={fmtTemp(snapshot?.outside_air_temp_c ?? null)} />
        <Row label={t("flight_info.tat")} value={fmtTemp(snapshot?.total_air_temp_c ?? null)} />
        <Row label={refLabel} value={refValue} sum />
      </div>
    </div>
  );
}

function TripCard({
  info,
  elapsedMinutes,
}: {
  info: ActiveFlightInfo;
  elapsedMinutes: number;
}) {
  const { t, i18n } = useTranslation();
  return (
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("info_strip.trip")}</span>
      </div>
      <div className="card__body">
        <Row
          label={t("active_flight.elapsed")}
          value={fmtDuration(elapsedMinutes, i18n.language)}
        />
        <Row
          label={t("active_flight.distance")}
          value={fmtDistance(info.distance_nm, i18n.language)}
        />
        <Row label={t("active_flight.positions")} value={String(info.position_count)} sum />
        {/* Touch-and-go + go-around counters: hidden until they fire, so
            a routine A→B keeps the Trip card at three rows. The moment
            a T&G or GA happens the row appears and stays for the rest
            of the flight as a quiet running tally. */}
        {info.touch_and_go_count > 0 && (
          <Row
            label={t("active_flight.touch_and_go")}
            value={String(info.touch_and_go_count)}
          />
        )}
        {info.go_around_count > 0 && (
          <Row label={t("active_flight.go_around")} value={String(info.go_around_count)} />
        )}
      </div>
    </div>
  );
}

function Row({ label, value, sum }: { label: string; value: string; sum?: boolean }) {
  return (
    <div className={`row ${sum ? "row--sum" : ""}`}>
      <span className="row__label">{label}</span>
      <span className="row__value">{value}</span>
    </div>
  );
}

// ---- Format helpers ----

/**
 * Same DOW heuristic as the standalone MassPanel: only emit when
 * EMPTY WEIGHT is at least 3 t (filters Asobo's bogus default-airliner
 * values, accepts real OEW from FBW/Fenix/PMDG).
 */
function computeDow(snap: SimSnapshot | null): number | null {
  if (!snap) return null;
  const ew = snap.empty_weight_kg;
  if (ew === null || ew < 3000) return null;
  return ew;
}

function fmtKgShort(kg: number | null, locale: string): string {
  if (kg === null) return "—";
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(
    kg,
  )} kg`;
}

function fmtWind(snap: SimSnapshot | null): string {
  if (!snap || (snap.wind_direction_deg === null && snap.wind_speed_kt === null)) {
    return "—";
  }
  const dir = snap.wind_direction_deg ?? 0;
  const spd = snap.wind_speed_kt ?? 0;
  const dirNorm = ((dir % 360) + 360) % 360;
  return `${Math.round(dirNorm).toString().padStart(3, "0")}/${Math.round(spd)
    .toString()
    .padStart(2, "0")}`;
}

/** 1 hPa = 0.0295299830714 inHg. US-Piloten lesen QNH in inHg. */
function fmtQnh(snap: SimSnapshot | null, locale: string): string {
  if (!snap || snap.qnh_hpa === null) return "—";
  const hpaLabel = new Intl.NumberFormat(locale, {
    maximumFractionDigits: 0,
  }).format(snap.qnh_hpa);
  const inHgLabel = new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(snap.qnh_hpa * 0.0295299830714);
  return `${hpaLabel} hPa / ${inHgLabel} inHg`;
}

function fmtTemp(c: number | null): string {
  if (c === null) return "—";
  const sign = c >= 0 ? "+" : "";
  return `${sign}${Math.round(c)}°C`;
}

function fmtMach(m: number | null): string {
  if (m === null || m <= 0) return "—";
  return `M ${m.toFixed(2)}`;
}

function fmtTas(kt: number | null, locale: string): string {
  if (kt === null) return "—";
  return `${new Intl.NumberFormat(locale).format(kt)} kt`;
}

export function fmtDuration(minutes: number, locale: string): string {
  const m = Math.max(0, Math.floor(minutes));
  const h = Math.floor(m / 60);
  const mm = m % 60;
  if (h === 0) return `${mm}m`;
  return locale.startsWith("de")
    ? `${h}h ${mm.toString().padStart(2, "0")}m`
    : `${h}h ${mm}m`;
}

export function fmtDistance(nm: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
    nm,
  )} nm`;
}
