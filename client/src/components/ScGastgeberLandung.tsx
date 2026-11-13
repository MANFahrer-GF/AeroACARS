// Shared Cockpit (B4): Beim Partner steht hier die Landung des Gastgebers
// statt der eigenen Bewertung. Die eigene Messung bleibt im Flugprotokoll,
// gezeigt wird sie nicht (Konzept 6b: die Landung des Spurträgers wird nie
// als Landung des Partners gezeigt).

import { useTranslation } from "react-i18next";
import type { ScPartnerLandung } from "../lib/scLandung";

export function ScGastgeberLandung({ landung }: { landung: ScPartnerLandung }) {
  const { t } = useTranslation();
  const titel = landung.gastgeber_name
    ? t("landing.sc.titel", { name: landung.gastgeber_name })
    : t("landing.sc.titel_ohne");
  if (landung.status === "ausstehend") {
    return (
      <section className="landing-section" data-testid="sc-landung">
        <h3>{titel}</h3>
        <p>{t("landing.sc.folgt")}</p>
      </section>
    );
  }
  const urteil = landung.score_text
    ? t(`landing.gesamt.${landung.score_text}`, { defaultValue: landung.score_text })
    : null;
  return (
    <section className="landing-section" data-testid="sc-landung">
      <h3>{titel}</h3>
      <dl>
        <dt>{t("landing.sc.sinkrate")}</dt>
        <dd>{landung.landing_rate_fpm != null ? `${Math.round(landung.landing_rate_fpm)} fpm` : "--"}</dd>
        {landung.landing_score != null && (
          <>
            <dt>{t("landing.sc.punkte")}</dt>
            <dd>{Math.round(landung.landing_score)}{urteil ? ` · ${urteil}` : ""}</dd>
          </>
        )}
        {landung.g_force != null && (
          <>
            <dt>{t("landing.sc.g")}</dt>
            <dd>{landung.g_force.toFixed(2)} G</dd>
          </>
        )}
        {landung.bounce_count != null && (
          <>
            <dt>{t("landing.sc.springer")}</dt>
            <dd>{landung.bounce_count}</dd>
          </>
        )}
      </dl>
      <p>{t("landing.sc.hinweis")}</p>
    </section>
  );
}
