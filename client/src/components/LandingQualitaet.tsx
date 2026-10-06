// Aufsetz-Qualität: Wing-Strike-Risiko, Aufsetz-Drittel, Distanz hinter der
// Schwelle, Vref-Abweichung, Gierrate, Bremsenergie — zur Einordnung, KEINE
// Note.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 gab es die Karte nur in der Webapp (_LandingQualityCard.tsx);
// der Client hatte die Werte gar nicht in seiner Aufzeichnung. Die Grenzen
// der Farben stehen nur hier.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { distanzHinterSchwelle, fmtSigned } from "../lib/landungsFormat";
import "./landungForensik.css";
import { InfoBadge } from "./InfoBadge";

type Ton = "good" | "neutral" | "warn" | "err";

export const tonWingStrike = (p: number): Ton => (p < 40 ? "good" : p < 60 ? "neutral" : p < 80 ? "warn" : "err");
export const tonDistanz = (m: number): Ton =>
  // Negativ = VOR der Schwelle aufgesetzt — keine kurze Landung, sondern
  // eine Landung vor der Bahn.
  m < 0 ? "err" : m < 300 ? "good" : m < 600 ? "neutral" : m < 1000 ? "warn" : "err";
/** Bei der generischen ICAO-Vref ist die Quelle selbst ±5 kt ungenau — das
 *  Band ist deshalb um 5 kt breiter. */
export const tonVref = (kt: number, quelle: string | null | undefined): Ton => {
  const a = Math.abs(kt);
  return quelle === "icao_default"
    ? a < 10 ? "good" : a < 15 ? "neutral" : a < 20 ? "warn" : "err"
    : a < 5 ? "good" : a < 10 ? "neutral" : a < 15 ? "warn" : "err";
};
export const tonGierrate = (g: number): Ton => (g < 2 ? "good" : g < 5 ? "neutral" : g < 8 ? "warn" : "err");
export const tonBremsenergie = (e: number): Ton => (e < 200 ? "good" : e < 500 ? "neutral" : e < 1000 ? "warn" : "err");

function Kachel({
  label,
  wert,
  einheit,
  ton,
  hinweis,
}: {
  label: string;
  wert: string | null;
  einheit?: string;
  ton?: Ton;
  hinweis: string;
}) {
  const { t } = useTranslation();
  return (
    <div className={`sinkrate-tile sinkrate-tile--${wert == null ? "na" : (ton ?? "neutral")}`}>
      <div className="sinkrate-tile__label">
        {label} <InfoBadge explanation={hinweis} />
      </div>
      <div className="sinkrate-tile__value">
        {wert ?? t("landing.quality.na")}
        {wert != null && einheit && <span className="sinkrate-tile__unit">{einheit}</span>}
      </div>
    </div>
  );
}

export function LandingQualitaet({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const wing = record.landing_wing_strike_severity_pct ?? null;
  const zone = record.landing_touchdown_zone ?? null;
  const distanz = distanzHinterSchwelle(record);
  const vref = record.landing_vref_deviation_kt ?? null;
  const vrefQuelle = record.landing_vref_source ?? null;
  const gier = record.landing_yaw_rate_deg_per_sec ?? null;
  const bremse = record.landing_brake_energy_proxy ?? null;

  // Bahnbezug nicht vertrauenswürdig → Drittel und Distanz weglassen statt
  // Werte auf falscher Geometrie zu zeigen. Ohne Feld (alte Flüge) gilt er
  // als vertrauenswürdig. „no_runway_match" (Privatplatz) ohne Hinweis.
  const bahnOk = record.runway_geometry_trusted ?? true;
  const grund = record.runway_geometry_reason;
  const grundText =
    !bahnOk && (grund === "icao_mismatch" || grund === "centerline_offset_too_large" || grund === "negative_float_distance")
      ? t(`landing.quality.trust.${grund}`)
      : null;

  const hatDaten =
    wing != null ||
    (bahnOk && (distanz != null || zone != null)) ||
    vref != null ||
    gier != null ||
    bremse != null ||
    grundText != null;
  if (!hatDaten) return null;

  const marke =
    wing != null && wing >= 80
      ? { text: t("landing.quality.flag_wing_strike"), ton: "err" }
      : bahnOk && zone === 3
        ? { text: t("landing.quality.flag_long_landing"), ton: "warn" }
        : grundText != null
          ? { text: `⚠ ${grundText}`, ton: "warn" }
          : null;

  const vrefLabel =
    vrefQuelle === "pmdg" || vrefQuelle === "fbw" || vrefQuelle === "icao_default"
      ? t(`landing.quality.vref_label.${vrefQuelle}`)
      : t("landing.quality.vref_label.unbekannt");
  const vrefHinweis =
    vrefQuelle === "pmdg" || vrefQuelle === "fbw" || vrefQuelle === "icao_default"
      ? t(`landing.quality.vref_hint.${vrefQuelle}`)
      : t("landing.quality.vref_hint.unbekannt");

  return (
    <section className="landing-section landing-section--quality">
      <h3>
        {t("landing.quality.title")}
        {marke && (
          <span className={`landing-quality__flag landing-quality__flag--${marke.ton}`}>{marke.text}</span>
        )}
      </h3>
      <div className="sinkrate-forensik-tiles">
        <Kachel
          label={t("landing.quality.wing_strike")}
          wert={wing != null ? `${wing.toFixed(0)}%` : null}
          ton={wing != null ? tonWingStrike(wing) : undefined}
          hinweis={t("landing.quality.wing_strike_hint")}
        />
        {bahnOk && (
          <Kachel
            label={t("landing.quality.zone")}
            wert={zone == null ? null : t(`landing.quality.zone_wert.${zone === 1 ? "1" : zone === 2 ? "2" : "3"}`)}
            ton={zone == null ? undefined : zone === 1 ? "good" : zone === 2 ? "warn" : "err"}
            hinweis={t("landing.quality.zone_hint")}
          />
        )}
        {bahnOk && (
          <Kachel
            label={t("landing.quality.distanz")}
            wert={distanz != null ? String(Math.round(distanz)) : null}
            einheit="m"
            ton={distanz != null ? tonDistanz(distanz) : undefined}
            hinweis={
              distanz != null && distanz < 0
                ? t("landing.quality.distanz_hint_vor")
                : t("landing.quality.distanz_hint")
            }
          />
        )}
        <Kachel
          label={vrefLabel}
          wert={vref != null ? fmtSigned(vref, 0) : null}
          einheit="kt"
          ton={vref != null ? tonVref(vref, vrefQuelle) : undefined}
          hinweis={vrefHinweis}
        />
        <Kachel
          label={t("landing.quality.gierrate")}
          wert={gier != null ? gier.toFixed(1) : null}
          einheit="°/s"
          ton={gier != null ? tonGierrate(gier) : undefined}
          hinweis={t("landing.quality.gierrate_hint")}
        />
        <Kachel
          label={t("landing.quality.bremsenergie")}
          wert={bremse != null ? String(Math.round(bremse)) : null}
          einheit="kJ/m"
          ton={bremse != null ? tonBremsenergie(bremse) : undefined}
          hinweis={t("landing.quality.bremsenergie_hint")}
        />
      </div>
    </section>
  );
}
