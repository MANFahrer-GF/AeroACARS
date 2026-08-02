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

// Farb-Tokens für die Punktezahl pro Band. Zeigen auf die sechsstufige
// --scale-* Skala in App.css, die für Hell UND Dunkel eigens auf
// WCAG-4.5:1 gegen die Kartenfläche abgestimmt ist — ein einzelner
// Hex-Wert schafft das nie in beiden Themes gleichzeitig (Weiss vs.
// fast-Schwarz brauchen entgegengesetzt helle/dunkle Töne).
const BAND_COLORS: Record<BandKey, string> = {
  excellent: "var(--scale-excellent)",
  good: "var(--scale-good)",
  ok: "var(--scale-fair)",
  long: "var(--scale-poor)",
  marginal: "var(--scale-bad)",
  overrun: "var(--scale-critical)",
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
            <div className="helpmodal__formula">
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
      <h4 className="helpmodal__heading">{heading}</h4>
      {children}
    </section>
  );
}



