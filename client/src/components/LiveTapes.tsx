import { useTranslation } from "react-i18next";
import type { SimSnapshot } from "../types";

interface Props {
  snapshot: SimSnapshot | null;
}

/**
 * Compact live-telemetry strip shown in the Cockpit tab while a flight
 * is active. Five glanceable tiles: IAS, GS, altitude (MSL+AGL), V/S
 * with a tiny climb/descent arrow, and heading. Numbers come straight
 * from the active sim snapshot — when the sim is disconnected we
 * render the tiles muted so the layout doesn't shift.
 */
export function LiveTapes({ snapshot }: Props) {
  const { t, i18n } = useTranslation();
  const has = snapshot !== null;

  const ias = has ? Math.round(snapshot.indicated_airspeed_kt) : null;
  const gs = has ? Math.round(snapshot.groundspeed_kt) : null;
  const altMsl = has ? Math.round(snapshot.altitude_msl_ft) : null;
  // v0.16.15: Piloten vergleichen mit dem Höhenmesser (PFD) — die
  // angezeigte Höhe ist primär; die geometrische GPS-Höhe wandert in
  // die Sub-Zeile (Differenz = Baro + Temperatur-Effekt, kein Fehler).
  const altInd =
    has && snapshot.altitude_indicated_ft != null
      ? Math.round(snapshot.altitude_indicated_ft)
      : null;
  const altPrimary = altInd ?? altMsl;
  const altAgl = has ? Math.round(snapshot.altitude_agl_ft) : null;
  const vs = has ? Math.round(snapshot.vertical_speed_fpm) : null;
  const heading = has
    ? ((snapshot.heading_deg_magnetic % 360) + 360) % 360
    : null;

  function fmtInt(n: number | null): string {
    if (n == null) return "—";
    return new Intl.NumberFormat(i18n.language).format(n);
  }
  function fmtSigned(n: number | null): string {
    if (n == null) return "—";
    const sign = n > 0 ? "+" : "";
    return `${sign}${new Intl.NumberFormat(i18n.language).format(n)}`;
  }

  const vsArrow = vs == null ? "" : vs > 50 ? "▲" : vs < -50 ? "▼" : "•";
  const vsClass =
    vs == null
      ? "live-tape--idle"
      : vs > 50
        ? "live-tape--up"
        : vs < -50
          ? "live-tape--down"
          : "live-tape--level";

  return (
    <section className={`live-tapes ${has ? "" : "live-tapes--idle"}`}>
      <Tape label={t("tapes.ias")} value={fmtInt(ias)} unit="kt" />
      <Tape label={t("tapes.gs")} value={fmtInt(gs)} unit="kt" />
      <Tape
        label={t("tapes.altitude")}
        value={fmtInt(altPrimary)}
        unit="ft"
        sub={[
          altAgl != null ? `AGL ${fmtInt(altAgl)} ft` : null,
          altInd != null && altMsl != null && Math.abs(altInd - altMsl) >= 100
            ? `GPS ${fmtInt(altMsl)} ft`
            : null,
        ]
          .filter(Boolean)
          .join(" · ") || undefined}
      />
      <Tape
        label={t("tapes.vs")}
        value={fmtSigned(vs)}
        unit="fpm"
        modifier={vsClass}
        prefix={vsArrow}
      />
      <Tape
        label={t("tapes.heading")}
        value={
          heading == null
            ? "—"
            : Math.round(heading).toString().padStart(3, "0") + "°"
        }
      />
    </section>
  );
}

function Tape({
  label,
  value,
  unit,
  sub,
  modifier,
  prefix,
}: {
  label: string;
  value: string;
  unit?: string;
  sub?: string;
  modifier?: string;
  prefix?: string;
}) {
  return (
    <div className={`live-tape ${modifier ?? ""}`}>
      <div className="live-tape__label">{label}</div>
      <div className="live-tape__value">
        {prefix && <span className="live-tape__prefix">{prefix}</span>}
        {value}
        {unit && <span className="live-tape__unit">{unit}</span>}
      </div>
      {sub && <div className="live-tape__sub">{sub}</div>}
    </div>
  );
}
