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
import { ApproachStabilityHilfeInhalt } from "./ApproachStabilityHilfeInhalt";

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
      <ApproachStabilityHilfeInhalt />
    </Modal>
  );
}
