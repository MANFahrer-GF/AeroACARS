// v0.9.0 (#GlitchTip) — First-Run-Banner fuer die anonyme Fehler-Telemetrie.
//
// Spec: docs/spec/v0.9.0-glitchtip-self-hosted.md (LE4 + LE5)
//   + docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (DSGVO Art. 6 (1) a)
//
// Wann sichtbar?
//   - localStorage-Schluessel `aeroacars.errorReporting.enabled` ist `null`
//     (= Pilot hat NIE eine Entscheidung getroffen). Sobald er einmal
//     Ja oder Nein klickt, ist der Banner weg und kommt nicht wieder.
//   - In Settings → Fehler-Telemetrie kann der Pilot die Entscheidung
//     jederzeit aendern.
//
// Banner-Text macht klar:
//   - Was gesendet wird (anonyme Crash/Error-Events, Stack-Traces)
//   - Was NICHT gesendet wird (Position, Route, Login, IP)
//   - Wo es hingeht (self-hosted GlitchTip auf live.kant.ovh, kein 3rd-Party)
//   - Wie der Pilot es spaeter aendert (Settings-Link)

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { consentIsUnset, setConsent } from "../lib/sentry";

interface Props {
  onDecided?: (enabled: boolean) => void;
}

export function ErrorReportingFirstRunBanner({ onDecided }: Props) {
  const { t } = useTranslation();
  const [visible, setVisible] = useState<boolean>(() => consentIsUnset());

  if (!visible) return null;

  const decide = (enabled: boolean) => {
    setConsent(enabled);
    void invoke("error_reporting_set_consent", { enabled }).catch(() => undefined);
    setVisible(false);
    onDecided?.(enabled);
  };

  return (
    <div className="update-banner" role="dialog" aria-live="polite" aria-label={t("error_reporting.banner_title")}>
      <div className="update-banner__icon" aria-hidden="true">
        🛡
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">
          {t("error_reporting.banner_title")}
        </div>
        <div
          className="update-banner__subtitle"
          dangerouslySetInnerHTML={{ __html: t("error_reporting.banner_body") }}
        />
      </div>
      <div className="update-banner__actions">
        <button
          type="button"
          className="button button--primary"
          onClick={() => decide(true)}
        >
          {t("error_reporting.banner_accept")}
        </button>
        <button
          type="button"
          className="button button--ghost"
          onClick={() => decide(false)}
        >
          {t("error_reporting.banner_decline")}
        </button>
      </div>
    </div>
  );
}
