import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import {
  abflugSchaetzung,
  abweichungTon,
  cockpitSchaetzung,
  datumUhrzeit,
  hatPlan,
  planPasst,
  uhrzeit,
} from "../lib/flugzeiten";
import { kurzMinuten, useJetztSek } from "./FahrplanBand";
import "./fahrplanBand.css";

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
 *   * Gross weight / the Air-Data reference speed / the Trip card's time
 *     against plan (elapsed time without a plan) get the bold "row--sum"
 *     treatment, matching each card's one dispatch-relevant figure.
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
  const jetzt = useJetztSek();
  const z = info.flugzeiten;
  // Mit passendem Plan trägt die fette Zeile die Zeit gegen Plan (Trip-Uhr,
  // 15.10.2026); ohne Plan bleibt die verstrichene Zeit die Hauptzahl. Die
  // Zahl der Positionsmeldungen steht nicht mehr hier — sie interessiert den
  // Piloten nicht (Thomas), geht aber weiter mit dem Flug an den Server und
  // steht im Diagnose-Log.
  const mitPlan = !!z && hatPlan(z) && planPasst(z, jetzt);
  return (
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("info_strip.trip")}</span>
      </div>
      <div className="card__body">
        <Row
          label={t("active_flight.elapsed")}
          value={fmtDuration(elapsedMinutes, i18n.language)}
          sum={!mitPlan}
        />
        <Row
          label={t("active_flight.distance")}
          value={fmtDistance(info.distance_nm, i18n.language)}
        />
        {/* Touch-and-go + go-around counters: hidden until they fire, so
            a routine A→B keeps the Trip card short. The moment a T&G or GA
            happens the row appears and stays for the rest of the flight as
            a quiet running tally. */}
        {info.touch_and_go_count > 0 && (
          <Row
            label={t("active_flight.touch_and_go")}
            value={String(info.touch_and_go_count)}
          />
        )}
        {info.go_around_count > 0 && (
          <Row label={t("active_flight.go_around")} value={String(info.go_around_count)} />
        )}
        <PlanZeitenZeilen info={info} jetzt={jetzt} />
      </div>
    </div>
  );
}

/** Abweichung neben einer Uhrzeit — Farbe nach `abweichungTon`, nie Rot. */
function Delta({ min }: { min: number }) {
  const ton = abweichungTon(min);
  return <span className={`trip-uhr__delta${ton ? ` trip-uhr__delta--${ton}` : ""}`}>{kurzMinuten(min)}</span>;
}

/** „Zeiten gegen Flugplan" live — die Trip-Uhr (15.10.2026).
 *
 *  Die fette Zeile ist die eine Zahl, die zählt: am Gate der Countdown bis
 *  zum geplanten Off-Block (oder wie lange er überfällig ist), danach das
 *  erwartete On-Block mit der Abweichung. Den Ablauf als Bild zeigt das
 *  Fahrplan-Band darüber. Passt der Plan nicht zum Flug (OFP von gestern),
 *  wird nicht verglichen. Keine Note. */
function PlanZeitenZeilen({ info, jetzt }: { info: ActiveFlightInfo; jetzt: number }) {
  const { t } = useTranslation();
  const z = info.flugzeiten;
  if (!z || !hatPlan(z)) return null;
  const fuss = (inhalt: ReactNode) => <div className="trip-uhr__fuss">{inhalt}</div>;
  if (!planPasst(z, jetzt)) {
    return (
      <div data-testid="planzeiten-cockpit">
        {fuss(
          z.ofp_erstellt
            ? t("flugzeiten.plan_passt_nicht_vom", { wann: datumUhrzeit(z.ofp_erstellt) })
            : t("flugzeiten.plan_passt_nicht"),
        )}
      </div>
    );
  }
  const planVom = z.ofp_erstellt ? t("flugzeiten.cockpit.plan_vom", { wann: datumUhrzeit(z.ofp_erstellt) }) : null;
  const abflug = abflugSchaetzung(z, jetzt);
  if (abflug) {
    return (
      <div data-testid="planzeiten-cockpit">
        <Row label={t("flugzeiten.cockpit.off_block_plan")} value={uhrzeit(abflug.plan)} />
        <Row label={t("flugzeiten.cockpit.on_block_plan")} value={uhrzeit(z.plan_on_block)} />
        {abflug.noch_min > 0 ? (
          <Row
            label={t("flugzeiten.cockpit.off_block_in")}
            value={t("flugzeiten.cockpit.min", { min: abflug.noch_min })}
            sum
          />
        ) : (
          <Row
            label={t("flugzeiten.cockpit.off_block_ueberfaellig")}
            value={<Delta min={abflug.gegen_plan_min} />}
            sum
          />
        )}
        {planVom && fuss(planVom)}
      </div>
    );
  }
  const s = cockpitSchaetzung(z, info.landung_erwartet);
  if (!s) return null;
  const a = s.aufgeholt_min;
  const seitAbheben =
    a == null
      ? null
      : a > 0
        ? t("flugzeiten.cockpit.aufgeholt", { min: a })
        : a < 0
          ? t("flugzeiten.cockpit.verloren", { min: -a })
          : t("flugzeiten.cockpit.gehalten");
  return (
    <div data-testid="planzeiten-cockpit">
      <Row label={t("flugzeiten.cockpit.on_block_plan")} value={uhrzeit(z.plan_on_block)} />
      <Row
        label={s.quelle === "on_block" ? t("flugzeiten.cockpit.on_block_ist") : t("flugzeiten.cockpit.on_block_erwartet")}
        value={
          <>
            {uhrzeit(s.erwartet)} <Delta min={s.gegen_plan_min} />
          </>
        }
        sum
      />
      {fuss(
        <>
          {seitAbheben && <>{seitAbheben} · </>}
          {t(`flugzeiten.cockpit.quelle.${s.quelle}`)}
        </>,
      )}
    </div>
  );
}

function Row({ label, value, sum }: { label: string; value: ReactNode; sum?: boolean }) {
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
