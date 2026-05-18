// Glossar-Modal für RunwayDiagramV2.
// Spec: docs/spec/runway-diagram-v2.contract.md §Glossar (17 Begriffe).
// Accessible: ESC schließt, Focus-Trap auf Modal, role="dialog".
//
// v0.11.0-dev: alle Strings via i18next (DE/EN/IT). Glossar-Eintrag-Reihenfolge
// bleibt im Code stabil (ENTRY_KEYS) — Übersetzungen liegen unter
// `landing.glossary.entries.<key>.{abbr,full,explanation}`.

import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

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
        aria-labelledby="rwy-glossary-title"
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#111827",
          border: "1px solid rgba(255,255,255,0.18)",
          borderRadius: 10,
          maxWidth: 760,
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
          <h3 id="rwy-glossary-title" style={{ margin: 0, fontSize: "1.1rem" }}>
            {t("landing.glossary.title")}
          </h3>
          <button
            ref={closeBtnRef}
            type="button"
            onClick={onClose}
            aria-label={t("landing.glossary.close_aria") ?? "Close"}
            style={{
              padding: "4px 12px",
              background: "rgba(255,255,255,0.08)",
              border: "1px solid rgba(255,255,255,0.18)",
              borderRadius: 6,
              color: "inherit",
              cursor: "pointer",
            }}
          >
            {t("landing.glossary.close_label")}
          </button>
        </header>
        <div
          style={{
            padding: "12px 18px 18px 18px",
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            gap: 14,
          }}
        >
          <p style={{ margin: 0, opacity: 0.75, fontSize: "0.88rem" }}>
            {t("landing.glossary.intro")}
          </p>
          {ENTRY_KEYS.map((key) => {
            const abbr = t(`landing.glossary.entries.${key}.abbr`);
            const full = t(`landing.glossary.entries.${key}.full`);
            const explanation = t(
              `landing.glossary.entries.${key}.explanation`,
            );
            return (
              <div
                key={key}
                style={{
                  background: "rgba(255,255,255,0.04)",
                  border: "1px solid rgba(255,255,255,0.08)",
                  borderRadius: 6,
                  padding: "10px 12px",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    gap: 8,
                    flexWrap: "wrap",
                    marginBottom: 4,
                  }}
                >
                  <strong style={{ fontSize: "0.98rem" }}>{abbr}</strong>
                  {full && (
                    <span style={{ opacity: 0.6, fontSize: "0.85rem" }}>
                      — {full}
                    </span>
                  )}
                </div>
                <div
                  style={{ fontSize: "0.9rem", lineHeight: 1.5, opacity: 0.92 }}
                >
                  {explanation}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
