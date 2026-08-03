import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import { RouteMap } from "./RouteMap";
import { fmtDistance, fmtDuration } from "./InfoStrip";

interface Props {
  info: ActiveFlightInfo;
  snapshot: SimSnapshot | null;
  phaseLabel: string;
  elapsedMinutes: number;
}

/**
 * The centre element of the Cockpit instrument band (Stage E redesign):
 * phase dot + name, a 7-segment progress axis, the route track
 * (RouteMap, unchanged logic), and a 4-tile row (V/S, HDG, Elapsed,
 * Distance flown). Replaces the plain phase badge that used to sit in
 * the header — same underlying `info.phase`, just a richer glance view.
 *
 * The axis buckets the app's fine-grained phase values (preflight,
 * boarding, pushback, taxi_out, takeoff_roll, takeoff, climb, level,
 * cruise, descent, approach, final, landing, taxi_in, blocks_on,
 * arrived, pirep_submitted) into the 7 segments a pilot actually
 * thinks in. "Level" (an ATC-restriction level-off during climb/
 * descent) buckets with whichever of the two it interrupts — the axis
 * is a coarse progress bar, not the precise phase badge.
 */
export function PhaseCard({ info, snapshot, phaseLabel, elapsedMinutes }: Props) {
  const { t, i18n } = useTranslation();

  const vs = snapshot ? Math.round(snapshot.vertical_speed_fpm) : null;
  const heading = snapshot
    ? ((snapshot.heading_deg_magnetic % 360) + 360) % 360
    : null;

  const vsSign = vs == null ? "" : vs > 0 ? "+" : "";
  const vsClass =
    vs == null ? "" : vs > 50 ? "tile__value--up" : vs < -50 ? "tile__value--down" : "";

  const axisIndex = phaseAxisIndex(info.phase);
  const dotClass = phaseDotClass(info.phase);

  return (
    <div className="center">
      <div className="center__phase">
        <span className={`dot ${dotClass}`} aria-hidden="true" />
        <span className="center__phase-name">{phaseLabel}</span>
      </div>
      <div className="center__axis" aria-hidden="true">
        {AXIS_LABELS.map((_, i) => (
          <span
            key={i}
            data-done={i < axisIndex ? "" : undefined}
            data-now={i === axisIndex ? "" : undefined}
          />
        ))}
      </div>
      <div className="center__axis-labels" aria-hidden="true">
        {AXIS_LABELS.map((key) => (
          <span key={key}>{t(`cockpit.phase_axis.${key}`)}</span>
        ))}
      </div>

      <RouteMap
        dptIcao={info.dpt_airport}
        arrIcao={info.arr_airport}
        currentLat={snapshot?.lat ?? null}
        currentLon={snapshot?.lon ?? null}
        dptGate={info.dep_gate}
        arrGate={info.arr_gate}
      />

      <div className="tiles">
        <div className="tile">
          <div className="tile__label">{t("tapes.vs")}</div>
          <div className={`tile__value ${vsClass}`}>
            {vs == null ? "—" : `${vsSign}${new Intl.NumberFormat(i18n.language).format(vs)}`}
          </div>
        </div>
        <div className="tile">
          <div className="tile__label">{t("tapes.heading")}</div>
          <div className="tile__value">
            {heading == null ? "—" : `${Math.round(heading).toString().padStart(3, "0")}°`}
          </div>
        </div>
        <div className="tile">
          <div className="tile__label">{t("active_flight.elapsed")}</div>
          <div className="tile__value">{fmtDuration(elapsedMinutes, i18n.language)}</div>
        </div>
        <div className="tile">
          <div className="tile__label">{t("active_flight.distance")}</div>
          <div className="tile__value">{fmtDistance(info.distance_nm, i18n.language)}</div>
        </div>
      </div>
    </div>
  );
}

const AXIS_LABELS = [
  "taxi",
  "takeoff",
  "climb",
  "cruise",
  "descent",
  "approach",
  "landing",
] as const;

function phaseAxisIndex(phase: string): number {
  switch (phase) {
    case "preflight":
    case "boarding":
    case "pushback":
    case "taxi_out":
      return 0;
    case "takeoff_roll":
    case "takeoff":
      return 1;
    case "climb":
      return 2;
    case "cruise":
      return 3;
    case "descent":
      return 4;
    case "approach":
    case "final":
      return 5;
    case "landing":
    case "taxi_in":
    case "blocks_on":
    case "arrived":
    case "pirep_submitted":
      return 6;
    default:
      return 0;
  }
}

function phaseDotClass(phase: string): string {
  switch (phase) {
    case "climb":
    case "cruise":
    case "descent":
    case "blocks_on":
    case "arrived":
    case "pirep_submitted":
      return "dot--ok";
    case "takeoff_roll":
    case "takeoff":
    case "approach":
    case "final":
    case "landing":
      return "dot--warn";
    default:
      return "";
  }
}
