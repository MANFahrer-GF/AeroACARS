// Pilot-Hilfe-Modal für die 7-Kacheln-Auswertung der Approach-Stability-Card.
//
// Erklärt das Stable-Approach-Gate-Konzept (FAA AC 120-71B), die STABLE-
// GATE-Pill und alle sieben Einzel-Kennzahlen inkl. Schwellwert-Bändern.
// Strings unter `landing.approach_stability_help.*` in DE/EN/IT.
//
// Accessible: ESC schließt, Focus-Trap, role="dialog". Modal-Hülle 1:1
// wie GlossaryModal/RunwayUtilizationHelpModal.

import { useTranslation } from "react-i18next";
import { Button, Modal } from "./ui";

const TILE_KEYS = [
  "vs_jerk",
  "bank_sigma",
  "ias_sigma",
  "sink_rate",
  "landing_config",
  "vs_vs_ils",
  "max_vs_dev",
] as const;

interface Props {
  onClose: () => void;
}

export function ApproachStabilityHelpModal({ onClose }: Props) {
  const { t } = useTranslation();
  return (
    <Modal
      open
      onClose={onClose}
      size="lg"
      title={t("landing.approach_stability_help.title")}
      closeLabel={t("landing.approach_stability_help.close_aria") ?? "Close"}
      footer={
        <Button
          onClick={onClose}
          aria-label={t("landing.approach_stability_help.close_aria") ?? "Close"}
        >
          {t("landing.approach_stability_help.close_label")}
        </Button>
      }
    >

        <div className="helpmodal__body">
          <p style={{ margin: 0, fontSize: "0.92rem", lineHeight: 1.5 }}>
            {t("landing.approach_stability_help.intro")}
          </p>

          <Section heading={t("landing.approach_stability_help.gate.heading")}>
            <p className="helpmodal__p">
              {t("landing.approach_stability_help.gate.body")}
            </p>
          </Section>

          <Section heading={t("landing.approach_stability_help.pill.heading")}>
            <p className="helpmodal__p">
              {t("landing.approach_stability_help.pill.body")}
            </p>
          </Section>

          {/* Kachel-Erklärungen direkt ohne extra Heading — die Kachel-
              Labels selbst tragen den Titel pro Block. */}
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
            }}
          >
            {TILE_KEYS.map((key) => (
              <TileExplain key={key} tileKey={key} />
            ))}
          </div>
        </div>
    </Modal>
  );
}

function TileExplain({ tileKey }: { tileKey: string }) {
  const { t } = useTranslation();
  return (
    <div className="helpmodal__panel" style={{ padding: "10px 12px" }}>
      <div
        style={{
          fontWeight: 600,
          fontSize: "0.92rem",
          marginBottom: 4,
        }}
      >
        {t(`landing.approach_stability_help.tiles.${tileKey}.label`)}
      </div>
      <div
        style={{
          fontSize: "0.86rem",
          lineHeight: 1.5,
          opacity: 0.92,
          marginBottom: 6,
        }}
      >
        {t(`landing.approach_stability_help.tiles.${tileKey}.body`)}
      </div>
      <div className="helpmodal__threshold">
        {t(`landing.approach_stability_help.tiles.${tileKey}.thresholds`)}
      </div>
    </div>
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

