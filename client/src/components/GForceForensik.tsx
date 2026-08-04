// v0.7.17 — G-Force-Forensik (Pilot-Client)
// Bug: B-009 — siehe docs/qs/v0.7.16-fenix-beta-bugs.md
//
// 1:1-Spiegel der SinkrateForensik fuer die G-Force-Dimension. Erklaert
// dem Piloten warum ein scheinbarer „Severe G"-Peak (z.B. 2.30 G bei
// butterweicher -116 fpm Landung) durch Sim-Strut-Compression entsteht
// und keinen Pilot-Performance-Downgrade rechtfertigt.
//
// Block-Struktur 1:1 wie Sinkrate-Forensik:
//   [1] Aufklaerungs-Block (cyan border)
//   [2] 4 G-Tiles: Edge / 250ms / 500ms-Peak / 1000ms-Peak
//   [3] Sample-Distribution (G-Buckets analog VS-Buckets)
//   [4] Score-Basis-Tile (Peak + Strut-Force + V/S-fuehrt-Note)
//   [5] Coaching-Tipp (5 priorisierte Trigger)
//
// Design-Konsistenz: gleiche CSS-Klassen-Prefixes wie sinkrate-* (nur
// gforce-* statt sinkrate-*) damit die Webapp-Komponente identisch
// gepflegt werden kann.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "./LandingPanel";
import {
  T_G_SMOOTH,
  T_G_FIRM,
  T_G_HARD,
  T_G_SEVERE,
  T_VS_SMOOTH_FPM,
  T_VS_FIRM_FPM,
} from "../lib/landingScoring";

// ───────────────────────────────────────────────────────────────────────────
// Pure functions — testbar isoliert
// ───────────────────────────────────────────────────────────────────────────

/// Render-Gate: rendert die Sektion wenn IRGENDEIN G-Forensik-Feld da ist.
export function hasGForensics(record: Pick<LandingRecord,
  | "g_at_edge"
  | "g_smoothed_250ms_post"
  | "g_median_post_500ms"
  | "g_p95_post_500ms"
  | "peak_g_post_500ms"
  | "peak_g_post_1000ms"
  | "max_gear_force_n"
>): boolean {
  return record.g_at_edge != null
    || record.g_smoothed_250ms_post != null
    || record.g_median_post_500ms != null
    || record.g_p95_post_500ms != null
    || record.peak_g_post_500ms != null
    || record.peak_g_post_1000ms != null
    || record.max_gear_force_n != null;
}

/// G-Force-Tone-Bands (matched landing-scoring T_G_*-Konstanten).
/// Spec docs/qs/v0.7.16-fenix-beta-bugs.md B-009.
export type Tone = "good" | "neutral" | "warn" | "err" | "err-severe";

export function gTone(g: number | null | undefined): Tone | null {
  if (g == null) return null;
  if (g < T_G_SMOOTH) return "good";
  if (g < T_G_FIRM) return "neutral";
  if (g < T_G_HARD) return "warn";
  if (g < T_G_SEVERE) return "err";
  return "err-severe";
}

/// v0.19.x FIX: whether the "V/S leads, master score not dragged down by
/// G" reassurance note is accurate. Was hardcoded at 1.40 (T_G_FIRM) —
/// `classify_landing`'s actual B-009 guarantee (V/S smooth ⇒ the
/// classification stays exactly what V/S alone produces) only holds up
/// to (not including) T_G_HARD (1.70); at/above that, G can drop the
/// category by one stage even with smooth V/S. Extracted as its own pure
/// function so the threshold is directly testable, matching this file's
/// other gate functions above.
export function showsVsLeadsNote(
  scoredG: number | null | undefined,
  vsAtEdgeFpm: number | null | undefined,
): boolean {
  const vsSmooth = vsAtEdgeFpm != null && Math.abs(vsAtEdgeFpm) < T_VS_SMOOTH_FPM;
  const gHigh = scoredG != null && scoredG >= T_G_HARD;
  return vsSmooth && gHigh;
}

export type GCoachingTipKey =
  | "sim_strut"
  | "real_impact"
  | "clean"
  | "airborne_spike"
  | "outlier_filtered";

/// Priorisierte Coaching-Tipps. Erster Match gewinnt.
export function pickGCoachingTip(args: {
  vsAtEdgeFpm: number | null | undefined;
  gPeak: number | null | undefined;
  gMedian: number | null | undefined;
  gP95: number | null | undefined;
  maxGearForceN: number | null | undefined;
}): GCoachingTipKey {
  const vs = args.vsAtEdgeFpm;
  const peak = args.gPeak;
  const median = args.gMedian;
  const p95 = args.gP95;
  const force = args.maxGearForceN;

  // 1. real_impact: V/S Hard (< -400 fpm) + G high
  if (vs != null && Math.abs(vs) > T_VS_FIRM_FPM && peak != null && peak >= T_G_FIRM) {
    return "real_impact";
  }
  // 2. sim_strut: V/S smooth (>= -200 fpm) but G high + Strut-Force-Anstieg
  //    → klassische Sim-Strut-Compression-Spike
  if (vs != null && Math.abs(vs) < T_VS_SMOOTH_FPM && peak != null && peak >= T_G_HARD
      && force != null && force > 5000) {
    return "sim_strut";
  }
  // 3. outlier_filtered: Peak >> P95 → einzelner Spike der durch
  //    P95/Median verschluckt wurde
  if (peak != null && p95 != null && peak - p95 > 0.5) {
    return "outlier_filtered";
  }
  // 4. airborne_spike: G hoch aber Strut-Force unauffaellig
  //    → Frame-Stutter oder Sample-Noise, kein echter Impact
  if (peak != null && peak >= T_G_HARD
      && (force == null || force < 2000)
      && median != null && median < T_G_SMOOTH) {
    return "airborne_spike";
  }
  // 5. clean: alles smooth
  return "clean";
}

// ───────────────────────────────────────────────────────────────────────────
// Haupt-Component
// ───────────────────────────────────────────────────────────────────────────

export function GForceForensik({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  if (!hasGForensics(record)) return null;

  const peakG = record.peak_g_post_500ms ?? null;
  // v0.12.3 (LE4): the value the landing is SCORED on — the EMA-smoothed
  // window peak (FOQA method). `peakG` above stays the raw 50 Hz peak,
  // shown only as forensic detail.
  const scoredG = record.landing_scored_g_force ?? peakG;
  const tipKey = pickGCoachingTip({
    vsAtEdgeFpm: record.vs_at_edge_fpm,
    gPeak: peakG,
    gMedian: record.g_median_post_500ms,
    gP95: record.g_p95_post_500ms,
    maxGearForceN: record.max_gear_force_n,
  });

  // (Distribution-Tile haben wir nicht — Sample-Array steht hier nicht
  // zur Verfuegung. Stattdessen zeigen die 4 Tiles oben den G-Verlauf
  // Edge → 250ms → 500ms-Peak → 1000ms-Peak, und Block 3 die robusten
  // Statistiken (Median / 95p) als Spike-Diagnose.)

  return (
    <section className="landing-section landing-section--gforce-forensik">
      <h3>{t("landing.gforce_forensik.title")}</h3>

      {/* Block [1] — Aufklaerungs-Block (cyan border, gleich wie Sinkrate) */}
      <div className="sinkrate-forensik-intro">
        <div className="sinkrate-forensik-intro__header">
          📊 {t("landing.gforce_forensik.intro_header")}
        </div>
        <div className="sinkrate-forensik-intro__body">
          {t("landing.gforce_forensik.intro_body")}
        </div>
      </div>

      {/* Block [2] — G-Tile-Verlauf: Edge / 250ms / 500ms-Peak / 1000ms-Peak */}
      <div className="sinkrate-forensik-section">
        <div className="sinkrate-forensik-section__title">
          📺 {t("landing.gforce_forensik.tool_section_title")}
        </div>
        <div className="sinkrate-forensik-section__subtitle">
          {t("landing.gforce_forensik.tool_section_subtitle")}
        </div>
        <div className="sinkrate-forensik-tiles">
          <GTile
            label={t("landing.gforce_forensik.tile_edge")}
            value={record.g_at_edge ?? null}
          />
          <GTile
            label={t("landing.gforce_forensik.tile_250ms")}
            value={record.g_smoothed_250ms_post ?? null}
            volantaStyle
          />
          <GTile
            label={t("landing.gforce_forensik.tile_500ms")}
            value={record.peak_g_post_500ms ?? null}
            peakTile
          />
          <GTile
            label={t("landing.gforce_forensik.tile_1000ms")}
            value={record.peak_g_post_1000ms ?? null}
            peakTile
          />
        </div>
      </div>

      {/* Block [3] — Robustheits-Statistiken (Median / P95) */}
      {(record.g_median_post_500ms != null || record.g_p95_post_500ms != null) && (
        <div className="sinkrate-forensik-section">
          <div className="sinkrate-forensik-section__title">
            🎯 {t("landing.gforce_forensik.robust_section_title")}
          </div>
          <div className="sinkrate-forensik-section__subtitle">
            {t("landing.gforce_forensik.robust_section_subtitle")}
          </div>
          <div className="sinkrate-forensik-tiles">
            <GTile
              label={t("landing.gforce_forensik.tile_median")}
              value={record.g_median_post_500ms ?? null}
            />
            <GTile
              label={t("landing.gforce_forensik.tile_p95")}
              value={record.g_p95_post_500ms ?? null}
            />
          </div>
          {peakG != null && record.g_p95_post_500ms != null
            && peakG - record.g_p95_post_500ms > 0.5 && (
              <div className="sinkrate-forensik-trend-note">
                ⚠️ {t("landing.gforce_forensik.spike_detected", {
                  peak: peakG.toFixed(2),
                  p95: record.g_p95_post_500ms.toFixed(2),
                })}
              </div>
          )}
        </div>
      )}

      {/* Block [4] — Score-Basis-Tile */}
      <div className="sinkrate-forensik-section">
        <div className="sinkrate-forensik-section__title">
          ⭐ {t("landing.gforce_forensik.score_section_title")}
        </div>
        <div className="sinkrate-forensik-section__subtitle">
          {t("landing.gforce_forensik.score_section_subtitle")}
        </div>
        <GScoreBasisTile
          scoredG={scoredG}
          rawPeakG={peakG}
          gearForceN={record.max_gear_force_n ?? null}
          vsAtEdge={record.vs_at_edge_fpm ?? null}
        />
      </div>

      {/* Block [5] — Coaching-Tipp */}
      <div className="sinkrate-forensik-coach">
        💡 {t(`landing.gforce_forensik.tip.${tipKey}`)}
      </div>
    </section>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Lokale Sub-Komponenten — Pattern wie SinkrateForensik
// ───────────────────────────────────────────────────────────────────────────

function GTile({
  label,
  value,
  volantaStyle,
  peakTile,
}: {
  label: string;
  value: number | null;
  volantaStyle?: boolean;
  peakTile?: boolean;
}) {
  const { t } = useTranslation();
  const tone = gTone(value);
  const valueText = value != null ? value.toFixed(2) : "—";
  const naTooltip = value == null ? t("landing.gforce_forensik.tile_na_tooltip") : undefined;
  return (
    <div
      className={`sinkrate-tile ${tone ? `sinkrate-tile--${tone}` : "sinkrate-tile--na"}`}
      title={naTooltip}
      aria-label={naTooltip ? `${label}: ${naTooltip}` : undefined}
    >
      <div className="sinkrate-tile__label">
        {label}
        {volantaStyle && (
          <span className="sinkrate-tile__hint">
            {" · "}{t("landing.gforce_forensik.tile_volanta_hint")}
          </span>
        )}
        {peakTile && (
          <span className="sinkrate-tile__hint">
            {" · "}{t("landing.gforce_forensik.tile_peak_hint")}
          </span>
        )}
      </div>
      <div className="sinkrate-tile__value">
        {valueText}
        {value != null && <span className="sinkrate-tile__unit"> G</span>}
      </div>
    </div>
  );
}

function GScoreBasisTile({
  scoredG,
  rawPeakG,
  gearForceN,
  vsAtEdge,
}: {
  scoredG: number | null;
  rawPeakG: number | null;
  gearForceN: number | null;
  vsAtEdge: number | null;
}) {
  const { t } = useTranslation();
  // v0.12.3 (LE4): Tone + Wert kommen vom gescorten (EMA) G — nicht vom
  // rohen Peak. Der Roh-Peak steht unten als Forensik-Detail.
  const tone = gTone(scoredG);
  // V/S-fuehrt-Hinweis: wenn V/S smooth (< 200 fpm) und G hoch → Master-
  // Score wird trotzdem als Smooth/Acceptable klassifiziert (B-009 Score-
  // Logik). Erklaer das hier dem Piloten. Threshold-Logik siehe
  // `showsVsLeadsNote` oben (v0.19.x FIX: T_G_HARD statt T_G_FIRM).
  const showVsLeadsNote = showsVsLeadsNote(scoredG, vsAtEdge);
  return (
    <div className={`sinkrate-score-tile ${tone ? `sinkrate-score-tile--${tone}` : ""}`}>
      <div className="sinkrate-score-tile__heading">
        {t("landing.gforce_forensik.score_basis_label")}
      </div>
      <div className="sinkrate-score-tile__sublabel">
        {t("landing.gforce_forensik.score_basis_sublabel")}
      </div>
      <div className="sinkrate-score-tile__value">
        {scoredG != null ? scoredG.toFixed(2) : "—"}
        <span className="sinkrate-score-tile__unit"> G</span>
      </div>
      {/* v0.12.3 (LE4): roher 50-Hz-Peak als Forensik-Detail — NICHT
          der gescorte Wert. */}
      {rawPeakG != null && (
        <div className="sinkrate-score-tile__source">
          {t("landing.gforce_forensik.raw_peak_detail", {
            g: rawPeakG.toFixed(2),
          })}
        </div>
      )}
      {gearForceN != null && gearForceN > 0 && (
        <div className="sinkrate-score-tile__source">
          {t("landing.gforce_forensik.score_strut_force", {
            force: Math.round(gearForceN).toLocaleString("de-DE"),
          })}
        </div>
      )}
      {showVsLeadsNote && (
        <div className="sinkrate-score-tile__hint">
          🟢 {t("landing.gforce_forensik.score_vs_leads")}
        </div>
      )}
    </div>
  );
}
