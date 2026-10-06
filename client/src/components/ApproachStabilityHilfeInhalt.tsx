// Inhalt der Hilfe zur Anflug-Karte (Stable Gate): Konzept, Urteil und die
// sechs Prüfungen. Strings unter `landing.approach_stability_help.*`.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Die
// Dialog-Hülle (ApproachStabilityHelpModal.tsx) bleibt je App eigen — Client
// und Webapp haben verschiedene Dialog-Bausteine —, der Text ist derselbe.

import type { ReactNode } from "react";
import { AltbestandHinweisAbsatz } from "./InfoBadge";
import { useTranslation } from "react-i18next";
import "./approachStabilityHilfe.css";

// Score-Version 19: sechs Prüfungen, Gleitpfad zuerst. Die beiden
// Sinkraten-Abweichungen gegen das 3°-Ideal zählen nicht mehr (GSG1709).
const TILE_KEYS = [
  "gleitpfad",
  "ias_sigma",
  "bank_sigma",
  "vs_jerk",
  "sink_rate",
  "landing_config",
] as const;

export function ApproachStabilityHilfeInhalt() {
  const { t } = useTranslation();
  // Wie jedes Erklärfenster: an einer Landung vor Score-Version 19 der
  // Hinweis, dass die Hilfe die heutigen Regeln beschreibt (QS Runde 10).
  return (
    <div className="helpmodal__body">
      <AltbestandHinweisAbsatz />
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
  children: ReactNode;
}) {
  return (
    <section>
      <h4 className="helpmodal__heading">{heading}</h4>
      {children}
    </section>
  );
}

