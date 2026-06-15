// v0.7.8 — Landing-Rate-Explainability (Pilot-Client)
// Spec: docs/spec/v0.7.8-landing-rate-explainability.md (v1.8 APPROVED)
//
// Erklaert dem Piloten WARUM seine Sinkrate-Bewertung so ist wie sie ist:
// 1. Aufklaerungs-Block — "Welche Sinkrate ist die richtige?"
// 2. Tool-Mittel-Tiles (1.5 s / 1.0 s / 0.5 s / 0.25 s) — was Volanta/Cockpit-VSI zeigt
// 3. Bucket-Aufschluesselung — wie sich VS in jeder Phase entwickelte
// 4. Score-Basis-Tile — was AeroACARS scort (landing_peak_vs_fpm ?? landing_rate_fpm)
// 5. Coaching-Tipp — ein Satz nach Prioritaet
// 6. <details> collapsible: Position-Trace + Aufprall-G
//
// Design-Konsistenz (§4.5):
// - landing-section / landing-stability / landing-stability__row CSS-Klassen
// - lokale Sub-Komponenten im selben File (kein MetricTile/Card-Import — die
//   existieren im Pilot-Client nicht, nur in der VPS-Webapp)
// - CSS-Variablen statt Hex-Hardcodes
// - Pattern wie StabilityIndicator/CoachTip/QuickFlags in LandingPanel.tsx

import { useTranslation } from "react-i18next";
import type { LandingRecord, LandingProfilePoint } from "./LandingPanel";

// ───────────────────────────────────────────────────────────────────────────
// Pure functions — gut testbar isoliert von React
// ───────────────────────────────────────────────────────────────────────────

/// Render-Gate: rendert die Sektion wenn IRGENDEIN Forensik-Feld vorhanden.
/// Spec §3.1, §6 A1.
export function hasForensics(record: Pick<LandingRecord,
  | "forensic_sample_count"
  | "vs_smoothed_250ms_fpm"
  | "vs_smoothed_500ms_fpm"
  | "vs_smoothed_1000ms_fpm"
  | "vs_smoothed_1500ms_fpm"
  | "vs_at_edge_fpm"
>): boolean {
  return record.forensic_sample_count != null
    || record.vs_smoothed_250ms_fpm != null
    || record.vs_smoothed_500ms_fpm != null
    || record.vs_smoothed_1000ms_fpm != null
    || record.vs_smoothed_1500ms_fpm != null
    || record.vs_at_edge_fpm != null;
}

/// v0.7.20 (QS-Befund GSG219): Pilot-Aufklaerung wenn der 50-Hz-Buffer-
/// Dump fehlt. Unterscheidung zwischen
///   - "alter Flug, Forensik gab's damals noch nicht" (forensics_version < 2)
///   - "v2-Flug, aber Sampler-Edge-Buffer fehlt fuer diesen Touchdown"
/// Vorher zeigten beide Faelle den irrefuehrenden Legacy-Text.
export function pickForensicsLegacyKey(
  record: Pick<LandingRecord, "forensics_version">,
): "legacy_notice_text" | "no_edge_buffer_text" {
  const isV2 = (record.forensics_version ?? 0) >= 2;
  return isV2 ? "no_edge_buffer_text" : "legacy_notice_text";
}

export interface Bucket {
  /// Anzeige-Label fuer die Phase, z.B. "−1.5 s … −1.0 s"
  label: string;
  /// Berechnete mittlere VS in fpm (negativ = sinkend)
  vs: number;
}

/// Bucket-Aufschluesselung aus den 4 kumulativen Mittelwerten + Edge.
/// Spec §3.2. disjoint-Bucket-Differenz:
///   mean[-1500..-1000] = (1500*m1500 - 1000*m1000) / 500
///   mean[-1000..-500]  = (1000*m1000 - 500*m500) / 500
///   mean[-500..-250]   = (500*m500 - 250*m250) / 250
///   mean[-250..0]      = vs_at_edge_fpm (fallback vs_smoothed_250ms_fpm)
export function computeBuckets(
  vs1500: number | null | undefined,
  vs1000: number | null | undefined,
  vs500: number | null | undefined,
  vs250: number | null | undefined,
  vsEdge: number | null | undefined,
): Bucket[] | null {
  // Alle 5 Werte muessen vorhanden sein (oder edge-Fallback auf 250ms-Mittel).
  // Spec §3.1: Bucket-Sub-Sektion ausgeblendet wenn nicht alle 5 da.
  if (vs1500 == null || vs1000 == null || vs500 == null || vs250 == null) {
    return null;
  }
  const edge = vsEdge ?? vs250;
  return [
    { label: "−1.5 s … −1.0 s", vs: (1500 * vs1500 - 1000 * vs1000) / 500 },
    { label: "−1.0 s … −0.5 s", vs: (1000 * vs1000 - 500 * vs500) / 500 },
    { label: "−0.5 s … −0.25 s", vs: (500 * vs500 - 250 * vs250) / 250 },
    { label: "−0.25 s … TD", vs: edge },
  ];
}

/// Spec §3.2: Trend-Diagnose ueber BETRAG (sonst falsch bei negativen VS).
/// True wenn alle 3 Inter-Bucket-Deltas > 20 fpm (Betrag steigt monoton).
export function isMonotonAccelerating(buckets: Bucket[]): boolean {
  if (buckets.length < 4) return false;
  const deltas = [
    Math.abs(buckets[1]!.vs) - Math.abs(buckets[0]!.vs),
    Math.abs(buckets[2]!.vs) - Math.abs(buckets[1]!.vs),
    Math.abs(buckets[3]!.vs) - Math.abs(buckets[2]!.vs),
  ];
  return deltas.every((d) => d > 20);
}

export type CoachingTipKey =
  | "flare_lost"
  | "hard_g"
  | "no_flare"
  | "late_drop"
  | "clean";

/// Spec §5.1: Trigger-Regeln mit Prioritaet (erster Match gewinnt).
/// Late-Drop nutzt Math.abs (v1.3-Fix, sonst falsch bei negativen Werten).
export function pickCoachingTip(args: {
  buckets: Bucket[] | null;
  peakGPost500ms: number | null | undefined;
  flareReductionFpm: number | null | undefined;
  vsAtEdgeFpm: number | null | undefined;
  vsSmoothed1500ms: number | null | undefined;
}): CoachingTipKey {
  // 1. flare_lost — Bucket-Trend monoton |zunehmend|
  if (args.buckets && isMonotonAccelerating(args.buckets)) {
    return "flare_lost";
  }
  // 2. hard_g — peak_g_post_500ms >= 1.70
  if (args.peakGPost500ms != null && args.peakGPost500ms >= 1.7) {
    return "hard_g";
  }
  // 3. no_flare — flare_reduction_fpm < 50
  if (args.flareReductionFpm != null && args.flareReductionFpm < 50) {
    return "no_flare";
  }
  // 4. late_drop — |edge| - |1500ms-mean| > 100 (BETRAG, sonst Vorzeichen-Bug)
  if (args.vsAtEdgeFpm != null && args.vsSmoothed1500ms != null) {
    const drop = Math.abs(args.vsAtEdgeFpm) - Math.abs(args.vsSmoothed1500ms);
    if (drop > 100) return "late_drop";
  }
  // 5. clean — kein Trigger
  return "clean";
}

/// Tone-Bands aus landingScoring.ts:128-131 (T_VS_*-Konstanten).
/// Spec §4.4. Niemals diese Schwellen verschieben — sind SoT fuer Sub-Score.
export type Tone = "good" | "neutral" | "warn" | "err" | "err-severe";

export function vsTone(vs: number | null | undefined): Tone | null {
  if (vs == null) return null;
  const abs = Math.abs(vs);
  if (abs < 200) return "good";
  if (abs < 400) return "neutral";
  if (abs < 600) return "warn";
  if (abs < 1000) return "err";
  return "err-severe";
}

/// Score-Basis-Chain.
///
/// v0.7.17 (B-015): Bevorzuge `vs_at_edge_fpm` (50-Hz-Buffer-Edge, FAR
/// 25.473 Engineering-Standard). Alte Felder `landing_peak_vs_fpm` /
/// `landing_rate_fpm` kommen vom Streamer-Tick (3-30 s Cadence, je
/// nach Phase) und liegen meist 30-50 fpm vom echten Edge entfernt.
/// Die Webapp nutzt seit v0.7.11 den Edge-Wert konsequent (siehe
/// `webapp/src/lib/displayVs.ts`); ohne diesen Fix zeigt der Pilot-
/// Client einen anderen Wert als die Webapp fuer denselben Touchdown
/// — Pilot kann sich das nicht erklaeren (User-Report 2026-05-12
/// EIN799: Client -311 vs Web -265).
///
/// Negative-Check gegen Float-Noise / Pre-Touchdown-Bumps: nur Werte
/// < 0 zaehlen als gueltiger Edge (im Sim positives V/S beim TD =
/// Ballooning oder Sample-Glitch, nicht der echte Aufsetz-Moment).
export function scoreBasisVs(record: Pick<LandingRecord,
  "vs_at_edge_fpm" | "landing_peak_vs_fpm" | "landing_rate_fpm"
>): number {
  if (record.vs_at_edge_fpm != null && record.vs_at_edge_fpm < 0) {
    return record.vs_at_edge_fpm;
  }
  return record.landing_peak_vs_fpm ?? record.landing_rate_fpm;
}

export interface TraceSample {
  t_ms: number;
  vs_fpm: number;
  agl_ft: number;
}

/// Position-Trace aus touchdown_profile (NICHT approach_samples — die hat
/// nur vs_fpm/bank_deg, keine t_ms/agl_ft im Base-Struct).
/// Filter t_ms ∈ [-3500, 0], reduziert auf ~5 repraesentative Punkte.
export function selectTraceSamples(
  profile: LandingProfilePoint[] | null | undefined,
): TraceSample[] {
  if (!profile || profile.length < 3) return [];
  const inWindow = profile.filter((p) => p.t_ms >= -3500 && p.t_ms <= 0);
  if (inWindow.length < 3) return [];

  // Nearest-Neighbor bei -3000, -2000, -1000, -500, -100 ms
  const targets = [-3000, -2000, -1000, -500, -100];
  const seen = new Set<number>();
  const picked: TraceSample[] = [];
  for (const target of targets) {
    let best: LandingProfilePoint | null = null;
    let bestDist = Infinity;
    for (const p of inWindow) {
      const d = Math.abs(p.t_ms - target);
      if (d < bestDist) {
        bestDist = d;
        best = p;
      }
    }
    if (best && !seen.has(best.t_ms)) {
      seen.add(best.t_ms);
      picked.push({ t_ms: best.t_ms, vs_fpm: best.vs_fpm, agl_ft: best.agl_ft });
    }
  }
  return picked;
}

// ───────────────────────────────────────────────────────────────────────────
// Haupt-Component
// ───────────────────────────────────────────────────────────────────────────

export function SinkrateForensik({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();

  if (!hasForensics(record)) {
    // v0.7.20 (QS-Befund GSG219): zwei verschiedene Texte je nach
    // forensics_version — vorher zeigte selbst ein v2-Flug die
    // "alter Flug"-Notice, was Piloten in die Irre fuehrte. Spec §QS-R
    // Notice-Text-Differenzierung.
    const noticeKey = pickForensicsLegacyKey(record);
    return (
      <section className="landing-section">
        <h3>{t("landing.sinkrate_forensik.title")}</h3>
        <div className="sinkrate-forensik-legacy">
          {t(`landing.sinkrate_forensik.${noticeKey}`)}
        </div>
      </section>
    );
  }

  const buckets = computeBuckets(
    record.vs_smoothed_1500ms_fpm,
    record.vs_smoothed_1000ms_fpm,
    record.vs_smoothed_500ms_fpm,
    record.vs_smoothed_250ms_fpm,
    record.vs_at_edge_fpm,
  );
  const tipKey = pickCoachingTip({
    buckets,
    peakGPost500ms: record.peak_g_post_500ms,
    flareReductionFpm: record.flare_reduction_fpm,
    vsAtEdgeFpm: record.vs_at_edge_fpm,
    vsSmoothed1500ms: record.vs_smoothed_1500ms_fpm,
  });
  const trace = selectTraceSamples(record.touchdown_profile);
  const scoreVs = scoreBasisVs(record);
  // v0.16.22 FIX 3: use the CANONICAL backend `flare_reduction_fpm` directly
  // instead of recomputing `|peak_vs_pre_flare_fpm| − |vs_at_edge_fpm|`. The
  // old recompute mixed the AGL-derived peak with the de-lagged edge, so on
  // MSFS it could go negative / disagree with the backend value and with
  // `flare_detected` (and with LandingPanel.tsx, which already uses the
  // canonical value). The backend computes the reduction from internally-
  // consistent endpoints (AGL on the AGL path, SimVar on fallback); reading
  // it here keeps both panels and the detection flag in lockstep. Positive =
  // the flare reduced the sink rate; null/≤0 = no reduction.
  const flareReduction = record.flare_reduction_fpm != null
    ? Math.round(record.flare_reduction_fpm)
    : null;

  return (
    <section className="landing-section landing-section--sinkrate-forensik">
      <h3>{t("landing.sinkrate_forensik.title")}</h3>

      {/* Block [1] — Aufklaerungs-Block (cyan Border-Left) */}
      <div className="sinkrate-forensik-intro">
        <div className="sinkrate-forensik-intro__header">
          📊 {t("landing.sinkrate_forensik.intro_header")}
        </div>
        <div className="sinkrate-forensik-intro__body">
          {t("landing.sinkrate_forensik.intro_body")}
        </div>
      </div>

      {/* Block [2] — Tool-Mittelwerte (4 Tiles) */}
      <div className="sinkrate-forensik-section">
        <div className="sinkrate-forensik-section__title">
          📺 {t("landing.sinkrate_forensik.tool_section_title")}
        </div>
        <div className="sinkrate-forensik-section__subtitle">
          {t("landing.sinkrate_forensik.tool_section_subtitle")}
        </div>
        <div className="sinkrate-forensik-tiles">
          <SmoothedVsTile
            label={t("landing.sinkrate_forensik.tile_1500ms")}
            value={record.vs_smoothed_1500ms_fpm ?? null}
          />
          <SmoothedVsTile
            label={t("landing.sinkrate_forensik.tile_1000ms")}
            value={record.vs_smoothed_1000ms_fpm ?? null}
          />
          <SmoothedVsTile
            label={t("landing.sinkrate_forensik.tile_500ms")}
            value={record.vs_smoothed_500ms_fpm ?? null}
            volantaStyle
          />
          <SmoothedVsTile
            label={t("landing.sinkrate_forensik.tile_250ms")}
            value={record.vs_smoothed_250ms_fpm ?? null}
          />
        </div>
      </div>

      {/* Block [3] — Bucket-Aufschluesselung */}
      {buckets && (
        <div className="sinkrate-forensik-section">
          <div className="sinkrate-forensik-section__title">
            {t("landing.sinkrate_forensik.bucket_title")}
          </div>
          <div className="sinkrate-forensik-section__subtitle">
            {t("landing.sinkrate_forensik.bucket_subtitle")}
          </div>
          <VsBucketBreakdown buckets={buckets} />
          {isMonotonAccelerating(buckets) && (
            <div className="sinkrate-forensik-trend-note">
              📉 {t("landing.sinkrate_forensik.bucket_trend_drop")}
            </div>
          )}
        </div>
      )}

      {/* Block [4] — Score-Basis */}
      <div className="sinkrate-forensik-section">
        <div className="sinkrate-forensik-section__title">
          ⭐ {t("landing.sinkrate_forensik.score_section_title")}
        </div>
        <div className="sinkrate-forensik-section__subtitle">
          {t("landing.sinkrate_forensik.score_section_subtitle")}
        </div>
        <ScoreBasisTile vs={scoreVs} landingSource={record.landing_source ?? null} />
        {record.peak_vs_pre_flare_fpm != null && (
          <div className="sinkrate-forensik-pre-flare">
            <span className="sinkrate-forensik-pre-flare__label">
              {t("landing.sinkrate_forensik.peak_pre_flare_label")}
            </span>
            <span className="sinkrate-forensik-pre-flare__value">
              {Math.round(record.peak_vs_pre_flare_fpm)} fpm
            </span>
            {flareReduction != null && (
              <span className="sinkrate-forensik-pre-flare__reduction">
                · {flareReduction > 0
                  ? t("landing.sinkrate_forensik.peak_pre_flare_reduction", { reduction: flareReduction })
                  : t("landing.sinkrate_forensik.peak_pre_flare_reduction_none")}
              </span>
            )}
          </div>
        )}
      </div>

      {/* Block [5] — Coaching-Tipp */}
      <div className="sinkrate-forensik-coach">
        💡 {t(`landing.sinkrate_forensik.tip.${tipKey}`)}
      </div>

      {/* Block [6] — Details (collapsible) */}
      {(trace.length >= 3 || record.peak_g_post_500ms != null) && (
        <details className="sinkrate-forensik-details">
          <summary>{t("landing.sinkrate_forensik.details_summary")}</summary>
          {trace.length >= 3 && (
            <PositionTrace samples={trace} />
          )}
          {record.peak_g_post_500ms != null && (
            <ImpactTiles
              g500ms={record.peak_g_post_500ms}
              g1000ms={record.peak_g_post_1000ms ?? null}
            />
          )}
        </details>
      )}
    </section>
  );
}

// ───────────────────────────────────────────────────────────────────────────
// Lokale Sub-Komponenten (Pattern wie StabilityIndicator/CoachTip im
// gleichen LandingPanel-File — NICHT von externer Library)
// ───────────────────────────────────────────────────────────────────────────

function SmoothedVsTile({
  label,
  value,
  volantaStyle,
}: {
  label: string;
  value: number | null;
  volantaStyle?: boolean;
}) {
  const { t } = useTranslation();
  const tone = vsTone(value);
  const valueText = value != null ? `${Math.round(value)}` : "—";
  const naTooltip = value == null ? t("landing.sinkrate_forensik.tile_na_tooltip") : undefined;
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
            {" · "}{t("landing.sinkrate_forensik.tile_volanta_hint")}
          </span>
        )}
      </div>
      <div className="sinkrate-tile__value">
        {valueText}
        {value != null && <span className="sinkrate-tile__unit"> fpm</span>}
      </div>
    </div>
  );
}

function ScoreBasisTile({ vs, landingSource }: { vs: number; landingSource: string | null }) {
  const { t } = useTranslation();
  const tone = vsTone(vs);
  return (
    <div className={`sinkrate-score-tile ${tone ? `sinkrate-score-tile--${tone}` : ""}`}>
      <div className="sinkrate-score-tile__heading">
        {t("landing.sinkrate_forensik.score_basis_label")}
      </div>
      <div className="sinkrate-score-tile__sublabel">
        {t("landing.sinkrate_forensik.score_basis_sublabel")}
      </div>
      <div className="sinkrate-score-tile__value">
        {Math.round(vs)}
        <span className="sinkrate-score-tile__unit"> fpm</span>
      </div>
      {landingSource && landingSource !== "" && (
        <div className="sinkrate-score-tile__source">
          {t("landing.sinkrate_forensik.score_basis_source_pill")}
          <code className="sinkrate-score-tile__source-code">{landingSource}</code>
        </div>
      )}
      <div className="sinkrate-score-tile__hint">
        {t("landing.sinkrate_forensik.score_basis_hint")}
      </div>
    </div>
  );
}

function VsBucketBreakdown({ buckets }: { buckets: Bucket[] }) {
  const maxAbs = Math.max(...buckets.map((b) => Math.abs(b.vs)), 1);
  return (
    <div className="sinkrate-buckets">
      {buckets.map((b, i) => {
        const pct = (Math.abs(b.vs) / maxAbs) * 100;
        const tone = vsTone(b.vs);
        return (
          <div className="sinkrate-bucket-row" key={i}>
            <div className="sinkrate-bucket-row__label">{b.label}</div>
            <div className="sinkrate-bucket-row__bar">
              <div
                className={`sinkrate-bucket-row__fill ${tone ? `sinkrate-bucket-row__fill--${tone}` : ""}`}
                style={{ width: `${pct}%` }}
              />
            </div>
            <div className="sinkrate-bucket-row__value">
              {Math.round(b.vs)} <span className="sinkrate-bucket-row__unit">fpm</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function PositionTrace({ samples }: { samples: TraceSample[] }) {
  const { t } = useTranslation();
  return (
    <div className="sinkrate-trace">
      <div className="sinkrate-trace__title">
        📍 {t("landing.sinkrate_forensik.trace_title")}
      </div>
      <div className="sinkrate-trace__caption">
        {t("landing.sinkrate_forensik.trace_caption")}
      </div>
      <table className="sinkrate-trace__table">
        <tbody>
          {samples.map((s, i) => (
            <tr key={i}>
              <td className="sinkrate-trace__t">
                t = {(s.t_ms / 1000).toFixed(1)} s
              </td>
              <td className="sinkrate-trace__vs">
                vs = {Math.round(s.vs_fpm)} fpm
              </td>
              <td className="sinkrate-trace__agl">
                AGL {Math.round(s.agl_ft)} ft
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ImpactTiles({ g500ms, g1000ms }: { g500ms: number; g1000ms: number | null }) {
  const { t } = useTranslation();
  const gToneFor = (g: number | null): Tone | null => {
    if (g == null) return null;
    if (g >= 1.7) return "err";
    if (g >= 1.4) return "warn";
    return "good";
  };
  return (
    <div className="sinkrate-impact">
      <div className="sinkrate-impact__title">
        💥 {t("landing.sinkrate_forensik.impact_title")}
      </div>
      <div className="sinkrate-impact__tiles">
        <div className={`sinkrate-impact__tile sinkrate-impact__tile--${gToneFor(g500ms) ?? "na"}`}>
          <div className="sinkrate-impact__label">{t("landing.sinkrate_forensik.impact_g_500ms")}</div>
          <div className="sinkrate-impact__value">{g500ms.toFixed(2)} <span>G</span></div>
        </div>
        {g1000ms != null && (
          <div className={`sinkrate-impact__tile sinkrate-impact__tile--${gToneFor(g1000ms) ?? "na"}`}>
            <div className="sinkrate-impact__label">{t("landing.sinkrate_forensik.impact_g_1000ms")}</div>
            <div className="sinkrate-impact__value">{g1000ms.toFixed(2)} <span>G</span></div>
          </div>
        )}
      </div>
    </div>
  );
}
