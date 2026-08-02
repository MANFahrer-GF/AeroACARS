// Pilot-Hilfe-Modal für den "Bahn-Auslastung"-Sub-Score (LDA-basiert,
// v0.12.0 mit 15 %-Float-Toleranz).
//
// Wird über einen "🛬 Wie wird das berechnet?"-Button am Boden der
// rollout-Card im LandingPanel geöffnet. Inhalt erklärt Formel, die
// Float-Toleranz, die fünf Punkte-Bänder, Heavy-Bonus, Pre-Displaced-Cap,
// den long_float-Fall und Skip-Reasons — in einfacher Pilot-Sprache, mit
// derselben Modal-Hülle wie GlossaryModal.
//
// Spec-Quelle für den Inhalt: docs/spec/v0.12.0-runway-utilization-
// refinement.md (Float-Toleranz-Refinement; baut auf v0.10.0-runway-
// utilization-score.md auf). Algorithmus in
// client/src-tauri/crates/landing-scoring/src/sub_rollout.rs.
//
// Accessible: ESC schließt, Focus-Trap auf Modal, role="dialog". DE/EN/IT
// via `landing.runway_utilization_help.*`.

import { useTranslation } from "react-i18next";
import { Button, Modal } from "./ui";

const BAND_KEYS = [
  "excellent",
  "good",
  "ok",
  "long",
  "marginal",
  "overrun",
] as const;

type BandKey = (typeof BAND_KEYS)[number];

// Farb-Tokens für die Punktezahl pro Band — abgeleitet von den
// existierenden Sub-Score-Bands im LandingPanel (good/ok/bad).
const BAND_COLORS: Record<BandKey, string> = {
  excellent: "#22c55e", // green-500
  good: "#84cc16", // lime-500
  ok: "#eab308", // amber-500
  long: "#f97316", // orange-500
  marginal: "#ef4444", // red-500
  overrun: "#dc2626", // red-600
};

const TERM_KEYS = ["td_distance", "rollout", "lda"] as const;
const SKIP_KEYS = [
  "missing_td",
  "missing_rollout",
  "missing_length",
  "untrusted_geometry",
  "off_airport",
  "invalid_lda",
] as const;

interface Props {
  onClose: () => void;
}

export function RunwayUtilizationHelpModal({ onClose }: Props) {
  const { t } = useTranslation();
  return (
    <Modal
      open
      onClose={onClose}
      size="lg"
      title={t("landing.runway_utilization_help.title")}
      closeLabel={t("landing.runway_utilization_help.close_aria") ?? "Close"}
      footer={
        <Button
          onClick={onClose}
          aria-label={t("landing.runway_utilization_help.close_aria") ?? "Close"}
        >
          {t("landing.runway_utilization_help.close_label")}
        </Button>
      }
    >

        <div className="helpmodal__body">
          <p style={{ margin: 0, fontSize: "0.92rem", lineHeight: 1.5 }}>
            {t("landing.runway_utilization_help.intro")}
          </p>

          <Section heading={t("landing.runway_utilization_help.formula_heading")}>
            <div
              style={{
                background: "rgba(34,197,94,0.10)",
                border: "1px solid rgba(34,197,94,0.35)",
                borderRadius: 6,
                padding: "10px 14px",
                fontFamily:
                  "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
                fontSize: "0.92rem",
                color: "#bbf7d0",
                whiteSpace: "pre-line",
              }}
            >
              {t("landing.runway_utilization_help.formula")}
            </div>
          </Section>

          {/* v0.12.0 (#runway-utilization-refinement, LE6): Float-Toleranz —
              die ersten 15 % der LDA an Float kosten keine Punkte. */}
          <Section
            heading={t(
              "landing.runway_utilization_help.float_tolerance_heading",
            )}
          >
            <p style={paragraphStyle}>
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
            <div
              style={{
                background: "rgba(255,255,255,0.04)",
                border: "1px solid rgba(255,255,255,0.08)",
                borderRadius: 6,
                padding: "10px 14px",
                fontSize: "0.88rem",
                lineHeight: 1.5,
                whiteSpace: "pre-line",
              }}
            >
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
            <div
              style={{
                border: "1px solid rgba(255,255,255,0.10)",
                borderRadius: 6,
                overflow: "hidden",
              }}
            >
              <table
                style={{
                  width: "100%",
                  borderCollapse: "collapse",
                  fontSize: "0.86rem",
                }}
              >
                <thead>
                  <tr style={{ background: "rgba(255,255,255,0.06)" }}>
                    <th style={thStyle}>
                      {t("landing.runway_utilization_help.bands_header.pct")}
                    </th>
                    <th style={{ ...thStyle, textAlign: "right", width: 80 }}>
                      {t("landing.runway_utilization_help.bands_header.pts")}
                    </th>
                    <th style={thStyle}>
                      {t("landing.runway_utilization_help.bands_header.label")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {BAND_KEYS.map((key) => (
                    <tr
                      key={key}
                      style={{
                        borderTop: "1px solid rgba(255,255,255,0.06)",
                      }}
                    >
                      <td style={tdStyle}>
                        {t(
                          `landing.runway_utilization_help.bands.${key}.pct`,
                        )}
                      </td>
                      <td
                        style={{
                          ...tdStyle,
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
                      <td style={tdStyle}>
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
            <p style={paragraphStyle}>
              {t("landing.runway_utilization_help.heavy_body")}
            </p>
          </Section>

          <Section
            heading={t(
              "landing.runway_utilization_help.pre_displaced_heading",
            )}
          >
            <p style={paragraphStyle}>
              {t("landing.runway_utilization_help.pre_displaced_body")}
            </p>
          </Section>

          {/* v0.12.0 (#runway-utilization-refinement, LE6): long_float —
              das Gegenstück zum Pre-Displaced-Cap. „Bremsweg top, nur
              zu spät aufgesetzt." */}
          <Section
            heading={t("landing.runway_utilization_help.long_float_heading")}
          >
            <p style={paragraphStyle}>
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
            <p style={paragraphStyle}>
              {t("landing.runway_utilization_help.card_body")}
            </p>
          </Section>
        </div>
    </Modal>
  );
}

function Section({
  heading,
  children,
}: {
  heading: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h4
        style={{
          margin: "0 0 8px 0",
          fontSize: "0.96rem",
          fontWeight: 600,
          color: "rgba(255,255,255,0.92)",
        }}
      >
        {heading}
      </h4>
      {children}
    </section>
  );
}

const thStyle: React.CSSProperties = {
  textAlign: "left",
  padding: "8px 12px",
  fontWeight: 600,
  fontSize: "0.82rem",
  opacity: 0.85,
};

const tdStyle: React.CSSProperties = {
  padding: "8px 12px",
  verticalAlign: "top",
};

const paragraphStyle: React.CSSProperties = {
  margin: 0,
  fontSize: "0.88rem",
  lineHeight: 1.55,
  opacity: 0.92,
};
