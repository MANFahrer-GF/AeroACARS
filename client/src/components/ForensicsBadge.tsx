// v0.7.1 Phase 3 F4: Forensik-v2 + Confidence-Badge.
//
// Spec docs/spec/v0.7.1-landing-ux-fairness.md F4 + P1.1-C:
// Bedingung `(ux_version >= 1 && forensics_version >= 2)` damit
// v0.7.0-PIREPs (forensics_version=2 aber kein landing_confidence)
// nicht das Badge bekommen.

import { useTranslation } from "react-i18next";

interface Props {
  forensicsVersion?: number | null;
  uxVersion?: number;
  confidence?: string | null;
  source?: string | null;
}

/// Mappt Confidence-Enum auf Pill-Farbe + i18n-Key.
function pillFor(confidence: string | undefined | null): {
  color: string;
  bg: string;
  textKey: string;
} {
  switch (confidence) {
    case "High":
      return { color: "#1f6e3a", bg: "#d4eddc", textKey: "landing.confidence.high" };
    case "Medium":
      return { color: "#1e4d80", bg: "#d6e4f7", textKey: "landing.confidence.medium" };
    case "Low":
      return { color: "#8a4500", bg: "#fde2c4", textKey: "landing.confidence.low" };
    case "VeryLow":
      return { color: "#7a1d1d", bg: "#f7d4d4", textKey: "landing.confidence.very_low" };
    default:
      return { color: "#555", bg: "#eee", textKey: "landing.confidence.unknown" };
  }
}

export function ForensicsBadge({ forensicsVersion, uxVersion, confidence, source }: Props) {
  const { t } = useTranslation();
  const ux = uxVersion ?? 0;
  const fv = forensicsVersion ?? 0;
  // P1.1-C: nur fuer v0.7.1+ PIREPs anzeigen (sonst fehlt Confidence)
  if (ux < 1 || fv < 2) {
    return null;
  }
  const pill = pillFor(confidence);
  const sourceTooltip = source
    ? t("landing.forensics.source", { source })
    : t("landing.forensics.source_unknown");

  return (
    <div
      // Heller Kasten mit dunkler Schrift — schon papiertauglich, im Druck
      // nicht abdunkeln (App.css `.papierfest`, Abnahme 06.10.2026).
      className="papierfest"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "0.5rem",
        padding: "0.35rem 0.6rem",
        background: "#f7f9fc",
        borderRadius: "0.5rem",
        border: "1px solid #e1e6ec",
        fontSize: "0.85rem",
      }}
      title={sourceTooltip}
    >
      <span style={{ fontWeight: 600, color: "#333" }}>
        {t("landing.forensics.label")}
      </span>
      <span
        className="papierfest"
        style={{
          padding: "0.15rem 0.5rem",
          borderRadius: "1rem",
          background: pill.bg,
          color: pill.color,
          fontWeight: 600,
          fontSize: "0.75rem",
          letterSpacing: "0.02em",
        }}
      >
        {t(pill.textKey)}
      </span>
    </div>
  );
}
