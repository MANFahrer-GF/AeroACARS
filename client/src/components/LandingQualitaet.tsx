// Aufsetz-Qualität: Wing-Strike-Risiko, Aufsetz-Drittel, Distanz hinter der
// Schwelle, Vref-Abweichung, Gierrate, Verzögerung — zur Einordnung, KEINE
// Note.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 gab es die Karte nur in der Webapp (_LandingQualityCard.tsx);
// der Client hatte die Werte gar nicht in seiner Aufzeichnung. Die Grenzen
// der Farben stehen nur hier.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { distanzHinterSchwelle, fmtNumber, fmtSigned } from "../lib/landungsFormat";
import "./landungForensik.css";
import { InfoBadge } from "./InfoBadge";

type Ton = "good" | "neutral" | "warn" | "err" | "ohne";

export const tonWingStrike = (p: number): Ton => (p < 40 ? "good" : p < 60 ? "neutral" : p < 80 ? "warn" : "err");
export const tonDistanz = (m: number): Ton =>
  // Negativ = VOR der Schwelle aufgesetzt — keine kurze Landung, sondern
  // eine Landung vor der Bahn.
  m < 0 ? "err" : m < 300 ? "good" : m < 600 ? "neutral" : m < 1000 ? "warn" : "err";
/** Vref-Quellen seit 2.0.6 (`landing_scoring::vref`). `icao_default` (der
 *  frühere Pauschalwert je Muster) gehört nicht dazu — solche Werte zeigt die
 *  Kachel nicht mehr. */
const VREF_QUELLEN = ["pmdg", "fbw", "fmc", "kalibriert", "faa", "faa_ungeprueft"] as const;
type VrefQuelle = (typeof VREF_QUELLEN)[number];
const istVrefQuelle = (q: string | null | undefined): q is VrefQuelle =>
  (VREF_QUELLEN as readonly (string | null | undefined)[]).includes(q);
/** Um wie viele kt das Band breiter wird; `null` = kein Urteil. Gemessene
 *  Quellen aus Fassungen vor 2.0.6 tragen kein Feld und zählen als 0. */
export const vrefToleranz = (quelle: string | null | undefined, toleranz: number | null | undefined): number | null =>
  toleranz ?? (quelle === "pmdg" || quelle === "fbw" || quelle === "fmc" ? 0 : null);
export const tonVref = (kt: number, toleranz: number | null): Ton => {
  if (toleranz == null) return "ohne";
  const a = Math.abs(kt) - toleranz;
  return a < 5 ? "good" : a < 10 ? "neutral" : a < 15 ? "warn" : "err";
};
export const tonGierrate = (g: number): Ton => (g < 2 ? "good" : g < 5 ? "neutral" : g < 8 ? "warn" : "err");
// Mittlere Verzögerung beim Ausrollen (m/s²). Zum Vergleich: 737 Autobrake 2
// ≈ 1,5, 3 ≈ 2,2, MAX ≈ 4,3; A320 LO ≈ 1,7, MED ≈ 3. Live 30 Tage (408
// Landungen): Median 1,6, p95 2,6. Gewichtsunabhängig — die frühere
// Bremsenergie (kJ/m) färbte ab 150 t fast jede zweite Landung gelb/rot.
export const tonVerzoegerung = (a: number): Ton => (a < 2.5 ? "good" : a < 3.5 ? "neutral" : a < 4.5 ? "warn" : "err");

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
  const vrefQuelle = istVrefQuelle(record.landing_vref_source) ? record.landing_vref_source : null;
  const vref = vrefQuelle != null ? (record.landing_vref_deviation_kt ?? null) : null;
  const gier = record.landing_yaw_rate_deg_per_sec ?? null;
  const verz = record.landing_decel_mps2 ?? null;

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
    verz != null ||
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

  const vrefLabel = vrefQuelle != null ? t(`landing.quality.vref_label.${vrefQuelle}`) : t("landing.quality.vref_label.unbekannt");
  const vrefHinweis = vrefQuelle != null ? t(`landing.quality.vref_hint.${vrefQuelle}`) : t("landing.quality.vref_hint.unbekannt");
  const vrefBand = vrefToleranz(vrefQuelle, record.landing_vref_toleranz_kt);

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
        {vref != null && (
        <Kachel
          label={vrefLabel}
          wert={fmtSigned(vref, 0)}
          einheit="kt"
          ton={tonVref(vref, vrefBand)}
          hinweis={vrefBand == null ? `${vrefHinweis} ${t("landing.quality.vref_ohne_urteil")}` : vrefHinweis}
        />
        )}
        <Kachel
          label={t("landing.quality.gierrate")}
          wert={gier != null ? gier.toFixed(1) : null}
          einheit="°/s"
          ton={gier != null ? tonGierrate(gier) : undefined}
          hinweis={t("landing.quality.gierrate_hint")}
        />
        <Kachel
          label={t("landing.quality.verzoegerung")}
          wert={verz != null ? fmtNumber(verz, 1) : null}
          einheit="m/s²"
          ton={verz != null ? tonVerzoegerung(verz) : undefined}
          hinweis={t("landing.quality.verzoegerung_hint")}
        />
      </div>
    </section>
  );
}
