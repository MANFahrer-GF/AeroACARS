// Post-Flight Approach-Stability-Card für das LandingPanel.
//
// Spec-Ursprung: webapp/aeroacars-live zeigt eine 7-Kacheln-Auswertung
// (V/S-Jerk, Bank σ, IAS σ, Sink Rate, Landing-Config, V/S vs. 3°-ILS,
// Max V/S-Dev < 500 ft) plus eine STABLE-GATE-Pill und einen Coaching-
// Text — diese Card portiert dieselbe Logik in den Pilot-Client. Quellen
// für die Werte stehen alle bereits im LandingRecord (Backend rechnet sie
// in `compute_approach_stability_v2`, lib.rs:3832); v0.11.0-dev hat 3
// fehlende Felder ins Storage-Schema nachgezogen.
//
// Banding-Schwellen sind 1:1 die Toleranzen aus der Spec / dem
// Help-Modal (siehe `landing.approach_stability_help.tiles.*.thresholds`):
//
//   V/S-Jerk           < 100 fpm/tick · 100–200 · > 200
//   Bank σ (filtered)  < 3° · 3–6° · > 6°
//   IAS σ              < 5 kt · 5–8 kt · > 8 kt
//   Sink Rate          bool (excessive_sink)
//   Landing-Config     bool (stable_config)
//   V/S vs. 3°-ILS     < 100 fpm · 100–200 · > 200
//   Max V/S-Dev <500ft < 200 fpm · 200–400 · > 400
//
// STABLE-GATE-Pill: 0 bad-Werte → STABLE; 1–2 → PARTIAL; ≥3 → UNSTABLE.
// "bad" für Bools = excessive_sink=true / stable_config=false.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ApproachStabilityHelpModal } from "./ApproachStabilityHelpModal";

type Band = "good" | "ok" | "bad" | "missing";

interface Props {
  vsJerkFpm?: number | null;
  bankStddevDeg?: number | null;
  iasStddevKt?: number | null;
  excessiveSink?: boolean | null;
  stableConfig?: boolean | null;
  vsDeviationFpm?: number | null;
  maxVsDeviationBelow500Fpm?: number | null;
  /** True = Gate auf Height-Above-Touchdown gefiltert.
   *  False = AGL-Fallback (Airport-Elevation unbekannt).
   *  null/undefined = unbekannt (pre-v0.11.0 PIREP). */
  usedHat?: boolean | null;
  /** Anzahl Samples im Gate. Bevorzugt aus gate_window.sample_count,
   *  Fallback auf approach_samples.length. */
  sampleCount: number | null;
  /** "msfs" | "xplane" | null — wird als „MSFS\" / „X-Plane\" gerendert. */
  simKind?: string | null;
}

function bandForRange(
  v: number | null | undefined,
  goodMax: number,
  okMax: number,
): Band {
  if (v == null || !Number.isFinite(v)) return "missing";
  if (v < goodMax) return "good";
  if (v < okMax) return "ok";
  return "bad";
}

function bandForBool(v: boolean | null | undefined, goodValue: boolean): Band {
  if (v == null) return "missing";
  return v === goodValue ? "good" : "bad";
}

const BAND_COLORS: Record<Band, string> = {
  good: "#22c55e",
  ok: "#eab308",
  bad: "#ef4444",
  missing: "rgba(255,255,255,0.35)",
};

function formatSimKind(simKind: string | null | undefined): string | null {
  if (!simKind) return null;
  const k = simKind.toLowerCase();
  if (k.includes("msfs")) return "MSFS";
  if (k.includes("xplane") || k.includes("x-plane")) return "X-Plane";
  return simKind;
}

export function ApproachStabilityCard(props: Props) {
  const { t } = useTranslation();
  const [helpOpen, setHelpOpen] = useState(false);

  const bands: Band[] = [
    bandForRange(props.vsJerkFpm, 100, 200),
    bandForRange(props.bankStddevDeg, 3, 6),
    bandForRange(props.iasStddevKt, 5, 8),
    bandForBool(props.excessiveSink, false),
    bandForBool(props.stableConfig, true),
    bandForRange(props.vsDeviationFpm, 100, 200),
    bandForRange(props.maxVsDeviationBelow500Fpm, 200, 400),
  ];

  const badCount = bands.filter((b) => b === "bad").length;
  const okCount = bands.filter((b) => b === "ok").length;
  const missingCount = bands.filter((b) => b === "missing").length;
  // Wenn ALLE 7 Werte fehlen (= Legacy-PIREP), zeigen wir keinen Pill,
  // sondern nur den Legacy-Hinweis.
  const isLegacy = missingCount === bands.length;
  // STABLE = alle Werte im grünen Band.
  // UNSTABLE = ab 2 harten Verletzungen ODER insgesamt ≥3 Werte außerhalb.
  // Sonst PARTIAL (= 1 bad oder 1–2 ok).
  let pillKey: "stable" | "partial" | "unstable";
  if (badCount === 0 && okCount === 0) pillKey = "stable";
  else if (badCount >= 2 || badCount + okCount >= 3) pillKey = "unstable";
  else pillKey = "partial";

  const pillColor =
    pillKey === "stable" ? "#22c55e" :
    pillKey === "partial" ? "#eab308" : "#ef4444";
  const pillBg =
    pillKey === "stable" ? "rgba(34,197,94,0.12)" :
    pillKey === "partial" ? "rgba(234,179,8,0.12)" :
    "rgba(239,68,68,0.12)";

  const coachKey = pillKey;
  const coachBg = pillBg;
  const coachBorder = pillColor;

  const sim = formatSimKind(props.simKind);
  const sublineKey =
    props.sampleCount == null || props.sampleCount === 0
      ? "subline_no_samples"
      : props.usedHat === false
        ? "subline_agl"
        : "subline_hat";

  return (
    <section className="landing-section landing-approach-stability">
      {helpOpen && (
        <ApproachStabilityHelpModal onClose={() => setHelpOpen(false)} />
      )}

      {/* Header: Titel + STABLE-GATE-Pill */}
      <header
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          marginBottom: 6,
          flexWrap: "wrap",
        }}
      >
        <h3 style={{ margin: 0, fontSize: "1rem" }}>
          {t("landing.approach_stability_card.title")}
        </h3>
        {!isLegacy && (
          <span
            style={{
              padding: "3px 10px",
              borderRadius: 4,
              fontSize: "0.74rem",
              fontWeight: 700,
              color: pillColor,
              background: pillBg,
              border: `1px solid ${pillColor}55`,
              letterSpacing: 0.4,
            }}
          >
            {t(`landing.approach_stability_card.pill_${pillKey}`)}
          </span>
        )}
      </header>

      {/* Sub-Line: Gate-Bereich + Sample-Count */}
      <div
        style={{
          fontSize: "0.78rem",
          opacity: 0.78,
          marginBottom: 4,
        }}
      >
        {t(`landing.approach_stability_card.${sublineKey}`, {
          count: props.sampleCount ?? 0,
        })}
      </div>

      {/* Datenquelle */}
      <div
        style={{
          fontSize: "0.74rem",
          opacity: 0.6,
          marginBottom: 12,
        }}
      >
        {sim
          ? t("landing.approach_stability_card.data_source", { sim })
          : t("landing.approach_stability_card.data_source_unknown")}
      </div>

      {/* Legacy-Hinweis, wenn das PIREP keine Stability-Felder hat */}
      {isLegacy && (
        <div
          style={{
            padding: "10px 12px",
            border: "1px solid rgba(255,255,255,0.12)",
            borderRadius: 6,
            background: "rgba(255,255,255,0.04)",
            fontSize: "0.84rem",
            opacity: 0.78,
          }}
        >
          {t("landing.approach_stability_card.legacy_notice")}
        </div>
      )}

      {/* 7-Kacheln-Grid */}
      {!isLegacy && (
        <>
          <div
            style={{
              display: "grid",
              gridTemplateColumns:
                "repeat(auto-fit, minmax(130px, 1fr))",
              gap: 8,
              marginBottom: 12,
            }}
          >
            <Tile
              label={t("landing.approach_stability_card.tiles.vs_jerk.label")}
              value={fmtNumOrDash(props.vsJerkFpm, 0)}
              unit={t("landing.approach_stability_card.tiles.vs_jerk.unit")}
              band={bands[0]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.bank_sigma.label",
              )}
              value={fmtNumOrDash(props.bankStddevDeg, 1)}
              unit={t(
                "landing.approach_stability_card.tiles.bank_sigma.unit",
              )}
              band={bands[1]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.ias_sigma.label",
              )}
              value={fmtNumOrDash(props.iasStddevKt, 1)}
              unit={t(
                "landing.approach_stability_card.tiles.ias_sigma.unit",
              )}
              band={bands[2]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.sink_rate.label",
              )}
              value={
                props.excessiveSink == null
                  ? t("landing.approach_stability_card.missing_value")
                  : props.excessiveSink
                    ? t(
                        "landing.approach_stability_card.tiles.sink_rate.value_excessive",
                      )
                    : t(
                        "landing.approach_stability_card.tiles.sink_rate.value_ok",
                      )
              }
              band={bands[3]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.landing_config.label",
              )}
              value={
                props.stableConfig == null
                  ? t(
                      "landing.approach_stability_card.tiles.landing_config.value_unknown",
                    )
                  : props.stableConfig
                    ? t(
                        "landing.approach_stability_card.tiles.landing_config.value_ok",
                      )
                    : t(
                        "landing.approach_stability_card.tiles.landing_config.value_partial",
                      )
              }
              band={bands[4]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.vs_vs_ils.label",
              )}
              value={fmtNumOrDash(props.vsDeviationFpm, 0)}
              unit={t(
                "landing.approach_stability_card.tiles.vs_vs_ils.unit",
              )}
              band={bands[5]}
            />
            <Tile
              label={t(
                "landing.approach_stability_card.tiles.max_vs_dev.label",
              )}
              value={fmtNumOrDash(props.maxVsDeviationBelow500Fpm, 0)}
              unit={t(
                "landing.approach_stability_card.tiles.max_vs_dev.unit",
              )}
              band={bands[6]}
            />
          </div>

          {/* Coaching */}
          <div
            style={{
              padding: "10px 12px",
              borderRadius: 6,
              background: coachBg,
              border: `1px solid ${coachBorder}55`,
              fontSize: "0.86rem",
              lineHeight: 1.45,
              color: coachBorder,
            }}
          >
            {t(`landing.approach_stability_card.coach.${coachKey}`)}
          </div>
        </>
      )}

      {/* Help-Button — immer sichtbar, auch bei Legacy */}
      <button
        type="button"
        onClick={() => setHelpOpen(true)}
        style={{
          marginTop: 10,
          padding: "4px 10px",
          background: "rgba(255,255,255,0.06)",
          border: "1px solid rgba(255,255,255,0.18)",
          borderRadius: 4,
          color: "rgba(255,255,255,0.85)",
          fontSize: "0.74rem",
          cursor: "pointer",
          alignSelf: "flex-start",
        }}
      >
        {t("landing.approach_stability_card.help_button")}
      </button>
    </section>
  );
}

function Tile({
  label,
  value,
  unit,
  band,
}: {
  label: string;
  value: string;
  unit?: string;
  band: Band;
}) {
  const color = BAND_COLORS[band];
  return (
    <div
      style={{
        background: "rgba(255,255,255,0.04)",
        border: `1px solid ${color}40`,
        borderRadius: 6,
        padding: "8px 10px",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        minHeight: 64,
      }}
    >
      <div
        style={{
          fontSize: "0.68rem",
          letterSpacing: 0.4,
          opacity: 0.75,
          textTransform: "uppercase",
          fontWeight: 600,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: "1.05rem",
          fontWeight: 700,
          color,
          fontVariantNumeric: "tabular-nums",
          lineHeight: 1.2,
        }}
      >
        {value}
        {unit && value !== "—" && (
          <span
            style={{
              fontSize: "0.66rem",
              fontWeight: 500,
              opacity: 0.75,
              marginLeft: 4,
            }}
          >
            {unit}
          </span>
        )}
      </div>
    </div>
  );
}

function fmtNumOrDash(
  v: number | null | undefined,
  digits: number,
): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return v.toFixed(digits);
}
