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
 * Single compact info-strip that replaces three separate panels
 * (Mass / Flight / Stats) in the Cockpit tab. Same data, ~⅓ the
 * vertical space. Three column groups keep the values logically
 * separated; on narrow screens the groups stack rather than going
 * tiny.
 *
 * Design notes:
 *   * No per-group borders or padding — one container, one neutral
 *     background. The grouping comes from the small column headers.
 *   * Mono-numerics with tabular-nums so columns line up.
 *   * Gross weight is the only "primary" value (it's the dispatch-
 *     relevant figure pilots glance at), highlighted slightly.
 */
export function InfoStrip({ info, snapshot, elapsedMinutes }: Props) {
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
    <section className="info-strip">
      <Group label={t("mass_panel.title")}>
        <Cell label={t("mass_panel.dow")} value={fmtKgShort(dow, i18n.language)} />
        <Cell
          label={t("mass_panel.payload")}
          value={fmtKgShort(payload, i18n.language)}
        />
        <Cell label={t("mass_panel.zfw")} value={fmtKgShort(zfw, i18n.language)} />
        <Cell label={t("mass_panel.fob")} value={fmtKgShort(fob, i18n.language)} />
        <Cell
          label={t("mass_panel.gross_weight")}
          value={fmtKgShort(gw, i18n.language)}
          primary
        />
      </Group>
      <Group label={t("flight_info.title")}>
        <Cell label={t("flight_info.wind")} value={fmtWind(snapshot)} />
        <Cell label={t("flight_info.qnh")} value={fmtQnh(snapshot, i18n.language)} />
        <Cell label={t("flight_info.oat")} value={fmtTemp(snapshot?.outside_air_temp_c ?? null)} />
        <Cell label={t("flight_info.tat")} value={fmtTemp(snapshot?.total_air_temp_c ?? null)} />
        <Cell label={t("flight_info.mach")} value={fmtMach(snapshot?.mach ?? null)} />
      </Group>
      <Group label={t("info_strip.trip")}>
        <Cell
          label={t("active_flight.elapsed")}
          value={fmtDuration(elapsedMinutes, i18n.language)}
        />
        <Cell
          label={t("active_flight.distance")}
          value={fmtDistance(info.distance_nm, i18n.language)}
        />
        <Cell
          label={t("active_flight.positions")}
          value={String(info.position_count)}
        />
        {/* Touch-and-go + go-around counters: hidden until they fire,
            so a routine A→B keeps the Trip group at three cells. The
            moment a T&G or GA happens the cell appears and stays for
            the rest of the flight as a quiet running tally. */}
        {info.touch_and_go_count > 0 && (
          <Cell
            label={t("active_flight.touch_and_go")}
            value={String(info.touch_and_go_count)}
          />
        )}
        {info.go_around_count > 0 && (
          <Cell
            label={t("active_flight.go_around")}
            value={String(info.go_around_count)}
          />
        )}
      </Group>
    </section>
  );
}

function Group({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="info-strip__group">
      <h4 className="info-strip__group-label">{label}</h4>
      <div className="info-strip__cells">{children}</div>
    </div>
  );
}

function Cell({
  label,
  value,
  primary,
}: {
  label: string;
  value: string;
  primary?: boolean;
}) {
  return (
    <div className={`info-strip__cell ${primary ? "info-strip__cell--primary" : ""}`}>
      <span className="info-strip__cell-label">{label}</span>
      <span className="info-strip__cell-value">{value}</span>
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

function fmtDuration(minutes: number, locale: string): string {
  const m = Math.max(0, Math.floor(minutes));
  const h = Math.floor(m / 60);
  const mm = m % 60;
  if (h === 0) return `${mm}m`;
  return locale.startsWith("de")
    ? `${h}h ${mm.toString().padStart(2, "0")}m`
    : `${h}h ${mm}m`;
}

function fmtDistance(nm: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
    nm,
  )} nm`;
}
