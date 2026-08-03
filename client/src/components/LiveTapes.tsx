import { useTranslation } from "react-i18next";
import type { SimSnapshot } from "../types";

interface Props {
  snapshot: SimSnapshot | null;
}

/**
 * The two lateral instrument tapes flanking the Cockpit phase card: IAS
 * on the left, altitude on the right — each with a tick strip and a
 * foot line of secondary readouts (GS/TAS/Mach under IAS, AGL/GPS under
 * altitude). V/S and heading moved into the phase card's tile row in
 * the Stage E redesign (they read more naturally next to Elapsed/
 * Distance there); this component now owns only the two tapes.
 *
 * Numbers come straight from the active sim snapshot — when the sim is
 * disconnected we render both tapes muted so the layout doesn't shift.
 */
export function LiveTapes({ snapshot }: Props) {
  const { t, i18n } = useTranslation();
  const has = snapshot !== null;

  const ias = has ? Math.round(snapshot.indicated_airspeed_kt) : null;
  const gs = has ? Math.round(snapshot.groundspeed_kt) : null;
  const tas = has ? Math.round(snapshot.true_airspeed_kt) : null;
  const mach = has ? (snapshot.mach ?? null) : null;
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

  function fmtInt(n: number | null): string {
    if (n == null) return "—";
    return new Intl.NumberFormat(i18n.language).format(n);
  }

  const iasFoot = [
    gs != null ? `${t("tapes.gs")} ${fmtInt(gs)}` : null,
    tas != null ? `TAS ${fmtInt(tas)}` : null,
    mach != null && mach > 0 ? `M ${mach.toFixed(2)}` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const altFoot = [
    altAgl != null ? `AGL ${fmtInt(altAgl)} ft` : null,
    altInd != null && altMsl != null && Math.abs(altInd - altMsl) >= 100
      ? `GPS ${fmtInt(altMsl)} ft`
      : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <>
      <div className={`tape tape--left ${has ? "" : "tape--idle"}`}>
        <div className="tape__head">
          <span>{t("tapes.ias")}</span>
          <span>KT</span>
        </div>
        <div className="tape__body">
          <div className="tape__ticks" aria-hidden="true" />
          <div className="tape__value">{fmtInt(ias)}</div>
        </div>
        {iasFoot && <div className="tape__foot">{iasFoot}</div>}
      </div>
      <div className={`tape tape--right ${has ? "" : "tape--idle"}`}>
        <div className="tape__head">
          <span>FT</span>
          <span>{t("tapes.altitude")}</span>
        </div>
        <div className="tape__body">
          <div className="tape__ticks" aria-hidden="true" />
          <div className="tape__value">{fmtInt(altPrimary)}</div>
        </div>
        {altFoot && <div className="tape__foot">{altFoot}</div>}
      </div>
    </>
  );
}
