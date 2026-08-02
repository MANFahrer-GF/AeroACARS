// Glossar-Modal für RunwayDiagramV2.
// Spec: docs/spec/runway-diagram-v2.contract.md §Glossar (17 Begriffe).
// Accessible: ESC schließt, Focus-Trap auf Modal, role="dialog".
//
// v0.11.0-dev: alle Strings via i18next (DE/EN/IT). Glossar-Eintrag-Reihenfolge
// bleibt im Code stabil (ENTRY_KEYS) — Übersetzungen liegen unter
// `landing.glossary.entries.<key>.{abbr,full,explanation}`.
//
// Redesign Stufe D: war komplett mit Inline-Styles gebaut und damit an
// Stylesheet, Tokens und Theme vorbei — u.a. hart auf `#111827` und
// `rgba(255,255,255,…)`, also dunkel-only. Fokus-Falle, ESC-Behandlung und
// Schleier lagen als eigene Kopie hier. Beides übernimmt jetzt die
// Modal-Primitive; die Werte kommen aus den Tokens und folgen dem Theme.
// Texte und Eintrags-Reihenfolge unverändert.

import { useTranslation } from "react-i18next";
import { Button, Modal } from "./ui";

const ENTRY_KEYS = [
  "threshold",
  "touchdown",
  "centerline",
  "xtd",
  "tdz",
  "aim",
  "tch",
  "dds",
  "glideslope",
  "brake_point",
  "rollout",
  "runway_util",
  "airac",
  "vps_navdata",
  "ourairports",
  "agl",
  "fpm",
  "kt",
] as const;

export function GlossaryModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();

  return (
    <Modal
      open
      onClose={onClose}
      size="lg"
      title={t("landing.glossary.title")}
      closeLabel={t("landing.glossary.close_aria") ?? "Close"}
      footer={
        <Button onClick={onClose} aria-label={t("landing.glossary.close_aria") ?? "Close"}>
          {t("landing.glossary.close_label")}
        </Button>
      }
    >
      <p className="glossary__intro">{t("landing.glossary.intro")}</p>
      <div className="glossary__list">
        {ENTRY_KEYS.map((key) => {
          const abbr = t(`landing.glossary.entries.${key}.abbr`);
          const full = t(`landing.glossary.entries.${key}.full`);
          const explanation = t(`landing.glossary.entries.${key}.explanation`);
          return (
            <div key={key} className="glossary__entry">
              <div className="glossary__term">
                <strong>{abbr}</strong>
                {full && <span className="glossary__full">— {full}</span>}
              </div>
              <div className="glossary__explanation">{explanation}</div>
            </div>
          );
        })}
      </div>
    </Modal>
  );
}
