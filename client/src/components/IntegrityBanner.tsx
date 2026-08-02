// v0.13.0 Slice 6 — Mid-Session-Integrity-Banner
//
// Zeigt eine Warnung wenn der Recorder ein integrity-flag-Event
// gepublisht hat. Drei Severities:
//   info     — kein Banner (zu noisy für UI)
//   anomaly  — orange info banner, dismissable
//   critical — rotes blockierendes Banner, prominent
//
// Hinweis: Stream C macht den finalen Score-Trust auf PIREP-Submission;
// dieser Banner ist nur die LIVE-Warnung während des Flugs damit der
// Pilot weiß "hier stimmt was nicht, der PIREP wird vermutlich
// untrusted werden".
//
// Redesign Stufe B — BUGFIX: Diese Komponente war vollständig in
// Tailwind-Syntax geschrieben (`fixed top-12`, `bg-red-900/95`,
// `rounded-lg`, `max-w-2xl` …), obwohl das Projekt kein Tailwind hat und
// nie hatte. Keine dieser Klassen existierte in App.css. Ergebnis: wenn
// mitten im Flug ein kritisches Integritätsproblem auftrat, sah der Pilot
// ein ungestyltes <div> im normalen Textfluss — nicht fixiert, nicht rot,
// ohne Rahmen. Jetzt über die Notice-Primitive, die dieselbe Wirkung mit
// echten Tokens erzielt. Texte und Daten unverändert.

import { useTranslation } from "react-i18next";
import { useIntegrityFlags } from "../hooks/useIntegrityFlags";
import { Button, Notice } from "./ui";

export function IntegrityBanner() {
  const { t } = useTranslation();
  const { state, dismiss } = useIntegrityFlags();

  if (state.sessionSeverity === "info" || state.recentFlags.length === 0) return null;
  if (state.dismissed && state.sessionSeverity !== "critical") return null;

  const isCritical = state.sessionSeverity === "critical";

  const latestFlag = state.recentFlags[0];
  const flagType = latestFlag?.flag.type ?? "UNKNOWN";
  const flagPhase = latestFlag?.flag.phase ?? "—";

  const title = isCritical
    ? t("integrity.title_critical", "Data-Integrity-Problem entdeckt")
    : t("integrity.title_anomaly", "Datenanomalie");

  return (
    <Notice
      floating
      role="alert"
      aria-live="assertive"
      tone={isCritical ? "error" : "warn"}
      level={title}
      detail={
        <>
          <span>
            {t("integrity.flag_description", {
              defaultValue: "{{type}} in Phase {{phase}}",
              type: flagType,
              phase: flagPhase,
            })}
          </span>
          {isCritical && (
            <>
              {" · "}
              <span>
                {t(
                  "integrity.critical_warning",
                  "Der PIREP wird wahrscheinlich als 'untrusted' eingestuft und für VA-Admin-Review markiert.",
                )}
              </span>
            </>
          )}
          {state.recentFlags.length > 1 && (
            <>
              {" · "}
              <span>
                {t("integrity.flag_count", {
                  defaultValue: "{{count}} flags in dieser Session",
                  count: state.recentFlags.length,
                })}
              </span>
            </>
          )}
        </>
      }
      actions={
        !isCritical ? (
          <Button
            variant="quiet"
            size="sm"
            onClick={dismiss}
            aria-label={t("integrity.dismiss", "Schließen")}
          >
            ✕
          </Button>
        ) : undefined
      }
    />
  );
}
