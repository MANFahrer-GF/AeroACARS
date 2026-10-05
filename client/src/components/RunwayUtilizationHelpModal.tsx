// Pilot-Hilfe-Modal für den "Bahn-Auslastung"-Sub-Score (LDA-basiert,
// v0.12.0 mit Float-Toleranz, v0.20.x auf 20 % angehoben).
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
import { RunwayUtilizationHilfeInhalt } from "./RunwayUtilizationHilfeInhalt";

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
      <RunwayUtilizationHilfeInhalt />
    </Modal>
  );
}
