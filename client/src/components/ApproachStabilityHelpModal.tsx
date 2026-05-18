// Pilot-Hilfe-Modal für die 7-Kacheln-Auswertung der Approach-Stability-Card.
//
// Erklärt das Stable-Approach-Gate-Konzept (FAA AC 120-71B), die STABLE-
// GATE-Pill und alle sieben Einzel-Kennzahlen inkl. Schwellwert-Bändern.
// Strings unter `landing.approach_stability_help.*` in DE/EN/IT.
//
// Accessible: ESC schließt, Focus-Trap, role="dialog". Modal-Hülle 1:1
// wie GlossaryModal/RunwayUtilizationHelpModal.

import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

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
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeBtnRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const root = dialogRef.current;
      if (!root) return;
      const focusables = root.querySelectorAll<HTMLElement>(
        'button, [href], input, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0]!;
      const last = focusables[focusables.length - 1]!;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      onClick={onClose}
      role="presentation"
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.65)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 10000,
        padding: 16,
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="appr-stab-help-title"
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#111827",
          border: "1px solid rgba(255,255,255,0.18)",
          borderRadius: 10,
          maxWidth: 820,
          width: "100%",
          maxHeight: "85vh",
          display: "flex",
          flexDirection: "column",
          boxShadow: "0 20px 60px rgba(0,0,0,0.6)",
        }}
      >
        <header
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "14px 18px",
            borderBottom: "1px solid rgba(255,255,255,0.10)",
          }}
        >
          <h3
            id="appr-stab-help-title"
            style={{ margin: 0, fontSize: "1.1rem" }}
          >
            {t("landing.approach_stability_help.title")}
          </h3>
          <button
            ref={closeBtnRef}
            type="button"
            onClick={onClose}
            aria-label={
              t("landing.approach_stability_help.close_aria") ?? "Close"
            }
            style={{
              padding: "4px 12px",
              background: "rgba(255,255,255,0.08)",
              border: "1px solid rgba(255,255,255,0.18)",
              borderRadius: 6,
              color: "inherit",
              cursor: "pointer",
            }}
          >
            {t("landing.approach_stability_help.close_label")}
          </button>
        </header>

        <div
          style={{
            padding: "16px 20px 20px 20px",
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            gap: 18,
          }}
        >
          <p style={{ margin: 0, fontSize: "0.92rem", lineHeight: 1.5 }}>
            {t("landing.approach_stability_help.intro")}
          </p>

          <Section heading={t("landing.approach_stability_help.gate.heading")}>
            <p style={paragraphStyle}>
              {t("landing.approach_stability_help.gate.body")}
            </p>
          </Section>

          <Section heading={t("landing.approach_stability_help.pill.heading")}>
            <p style={paragraphStyle}>
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
      </div>
    </div>
  );
}

function TileExplain({ tileKey }: { tileKey: string }) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.08)",
        borderRadius: 6,
        padding: "10px 12px",
      }}
    >
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
      <div
        style={{
          fontSize: "0.78rem",
          color: "#bbf7d0",
          fontFamily:
            "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
          padding: "4px 8px",
          background: "rgba(34,197,94,0.08)",
          border: "1px solid rgba(34,197,94,0.25)",
          borderRadius: 4,
          display: "inline-block",
        }}
      >
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

const paragraphStyle: React.CSSProperties = {
  margin: 0,
  fontSize: "0.88rem",
  lineHeight: 1.55,
  opacity: 0.92,
};
