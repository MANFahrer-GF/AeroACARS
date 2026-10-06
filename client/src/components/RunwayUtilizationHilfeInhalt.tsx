// Inhalt der Hilfe zur Bahn-Auslastung (ältere Landungen, Achse vor v1.7.0):
// Formel, Bänder, Begriffe, Gründe fürs Auslassen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Die
// Dialog-Hülle (RunwayUtilizationHelpModal.tsx) bleibt je App eigen.

import type { ReactNode } from "react";
import { AltbestandHinweisAbsatz } from "./InfoBadge";
import { useTranslation } from "react-i18next";
import "./approachStabilityHilfe.css";

const BAND_KEYS = [
  "excellent",
  "good",
  "ok",
  "long",
  "marginal",
  "overrun",
] as const;

type BandKey = (typeof BAND_KEYS)[number];

// Farb-Tokens für die Punktezahl pro Band. Zeigen auf die sechsstufige
// --scale-* Skala in App.css (Rückfallwerte für die Webapp, die die Skala
// nicht kennt), die für Hell UND Dunkel eigens auf
// WCAG-4.5:1 gegen die Kartenfläche abgestimmt ist — ein einzelner
// Hex-Wert schafft das nie in beiden Themes gleichzeitig (Weiss vs.
// fast-Schwarz brauchen entgegengesetzt helle/dunkle Töne).
const BAND_COLORS: Record<BandKey, string> = {
  excellent: "var(--scale-excellent, #22c55e)",
  good: "var(--scale-good, #84cc16)",
  ok: "var(--scale-fair, #eab308)",
  long: "var(--scale-poor, #f97316)",
  marginal: "var(--scale-bad, #ef4444)",
  overrun: "var(--scale-critical, #f87171)",
};

const TERM_KEYS = ["td_distance", "rollout", "lda"] as const;
const SKIP_KEYS = [
  "missing_td",
  "missing_rollout",
  "missing_length",
  "untrusted_geometry",
  "off_airport",
  "invalid_lda",
  // v1.6.7: NaN/Unendlich in der Bahn-Geometrie — lieber „nicht
  // bewertet" als eine erfundene Zahl.
  "invalid_geometry",
] as const;

export function RunwayUtilizationHilfeInhalt() {
  const { t } = useTranslation();
  // Der Dialog erscheint nur an der alten Bahn-Achse (Altbestand) — der
  // Versionshinweis wie an jedem Erklärfenster (QS Runde 11).
  return (
    <div className="helpmodal__body">
      <AltbestandHinweisAbsatz />
      <p style={{ margin: 0, fontSize: "0.92rem", lineHeight: 1.5 }}>
        {t("landing.runway_utilization_help.intro")}
      </p>

      <Section heading={t("landing.runway_utilization_help.formula_heading")}>
        <div className="helpmodal__formula">
          {t("landing.runway_utilization_help.formula")}
        </div>
      </Section>

      {/* v0.12.0 (#runway-utilization-refinement, LE6): Float-Toleranz —
          die ersten 20 % der LDA an Float kosten keine Punkte
          (v0.20.x: von 15 % angehoben). */}
      <Section
        heading={t(
          "landing.runway_utilization_help.float_tolerance_heading",
        )}
      >
        <p className="helpmodal__p">
          {t("landing.runway_utilization_help.float_tolerance_body")}
        </p>
      </Section>

      <Section heading={t("landing.runway_utilization_help.terms_heading")}>
        <ul
          style={{
            margin: 0,
            paddingLeft: 18,
            fontSize: "0.88rem",
            lineHeight: 1.55,
            opacity: 0.92,
          }}
        >
          {TERM_KEYS.map((key) => (
            <li key={key} style={{ marginBottom: 4 }}>
              {t(`landing.runway_utilization_help.terms.${key}`)}
            </li>
          ))}
        </ul>
      </Section>

      <Section heading={t("landing.runway_utilization_help.example_heading")}>
        <div className="helpmodal__panel" style={{ fontSize: "0.88rem", lineHeight: 1.5, whiteSpace: "pre-line" }}>
          {t("landing.runway_utilization_help.example")}
        </div>
      </Section>

      <Section heading={t("landing.runway_utilization_help.bands_heading")}>
        <p
          style={{
            margin: "0 0 8px 0",
            fontSize: "0.85rem",
            opacity: 0.78,
          }}
        >
          {t("landing.runway_utilization_help.bands_intro")}
        </p>
        <div className="helpmodal__table-wrap">
          <table className="helpmodal__table">
            <thead>
              <tr>
                <th className="helpmodal__th">
                  {t("landing.runway_utilization_help.bands_header.pct")}
                </th>
                <th className="helpmodal__th" style={{ textAlign: "right", width: 80 }}>
                  {t("landing.runway_utilization_help.bands_header.pts")}
                </th>
                <th className="helpmodal__th">
                  {t("landing.runway_utilization_help.bands_header.label")}
                </th>
              </tr>
            </thead>
            <tbody>
              {BAND_KEYS.map((key) => (
                <tr
                  key={key}
                  className="helpmodal__tr"
                >
                  <td className="helpmodal__td">
                    {t(
                      `landing.runway_utilization_help.bands.${key}.pct`,
                    )}
                  </td>
                  <td
                    style={{
                      textAlign: "right",
                      fontWeight: 700,
                      color: BAND_COLORS[key],
                      fontVariantNumeric: "tabular-nums",
                    }}
                  >
                    {t(
                      `landing.runway_utilization_help.bands.${key}.pts`,
                    )}
                  </td>
                  <td className="helpmodal__td">
                    {t(
                      `landing.runway_utilization_help.bands.${key}.label`,
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Section>

      <Section heading={t("landing.runway_utilization_help.heavy_heading")}>
        <p className="helpmodal__p">
          {t("landing.runway_utilization_help.heavy_body")}
        </p>
      </Section>

      <Section
        heading={t(
          "landing.runway_utilization_help.pre_displaced_heading",
        )}
      >
        <p className="helpmodal__p">
          {t("landing.runway_utilization_help.pre_displaced_body")}
        </p>
      </Section>

      {/* v0.12.0 (#runway-utilization-refinement, LE6): long_float —
          das Gegenstück zum Pre-Displaced-Cap. „Bremsweg top, nur
          zu spät aufgesetzt." */}
      <Section
        heading={t("landing.runway_utilization_help.long_float_heading")}
      >
        <p className="helpmodal__p">
          {t("landing.runway_utilization_help.long_float_body")}
        </p>
      </Section>

      <Section heading={t("landing.runway_utilization_help.skip_heading")}>
        <p
          style={{
            margin: "0 0 8px 0",
            fontSize: "0.85rem",
            opacity: 0.78,
          }}
        >
          {t("landing.runway_utilization_help.skip_intro")}
        </p>
        <ul
          style={{
            margin: 0,
            paddingLeft: 18,
            fontSize: "0.86rem",
            lineHeight: 1.55,
            opacity: 0.92,
          }}
        >
          {SKIP_KEYS.map((key) => (
            <li key={key} style={{ marginBottom: 4 }}>
              {t(`landing.runway_utilization_help.skip_items.${key}`)}
            </li>
          ))}
        </ul>
      </Section>

      <Section heading={t("landing.runway_utilization_help.card_heading")}>
        <p className="helpmodal__p">
          {t("landing.runway_utilization_help.card_body")}
        </p>
      </Section>
    </div>
  );
}

function Section({
  heading,
  children,
}: {
  heading: string;
  children: ReactNode;
}) {
  return (
    <section>
      <h4 className="helpmodal__heading">{heading}</h4>
      {children}
    </section>
  );
}



