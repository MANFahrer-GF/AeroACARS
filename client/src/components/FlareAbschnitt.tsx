// Abfangbogen (Flare) — zur Einordnung, KEINE Note (seit Score-Version 19
// ohne eigene Zahl 0–100).
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 hieß der Abschnitt im Client „Flare-Qualität", in der Webapp
// „Abfangbogen (keine Note)" mit anderen Beschriftungen und einer selbst
// gerechneten Reduktion. Sichtbar, sobald einer der Messwerte da ist.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { fmtNumber, fmtSigned } from "../lib/landungsFormat";
import "./flareAbschnitt.css";

export function FlareAbschnitt({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  if (
    record.peak_vs_pre_flare_fpm == null &&
    record.vs_at_flare_end_fpm == null &&
    record.flare_reduction_fpm == null &&
    record.flare_dvs_dt_fpm_per_sec == null
  ) {
    return null;
  }
  return (
    <section className="landing-section landing-section--flare">
      <h3>
        {t("landing.flare_section")}
        {record.flare_detected === true && (
          <span className="landing-flare__chip landing-flare__chip--ok">
            ✈ {t("landing.flare_detected")}
          </span>
        )}
        {record.flare_detected === false && (
          <span className="landing-flare__chip landing-flare__chip--warn">
            {t("landing.flare_not_detected")}
          </span>
        )}
      </h3>
      <div className="landing-flare">
        {/* Score-Version 19: kein Teil der Note — die frühere
            Flare-Zahl 0–100 ist entfallen (sah aus wie eine Note,
            die Webapp zeigt sie nicht). Nur die Messwerte. */}
        <dl className="landing-keyvals landing-flare__metrics">
          {record.peak_vs_pre_flare_fpm != null && (
            <div title={t("landing.flare_pre_vs_hint") ?? undefined}>
              <dt>{t("landing.flare_pre_vs")}</dt>
              <dd>{fmtNumber(record.peak_vs_pre_flare_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.vs_at_flare_end_fpm != null && (
            <div title={t("landing.flare_end_vs_hint") ?? undefined}>
              <dt>{t("landing.flare_end_vs")}</dt>
              <dd>{fmtNumber(record.vs_at_flare_end_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.flare_reduction_fpm != null && (
            <div title={t("landing.flare_reduction_hint") ?? undefined}>
              <dt>{t("landing.flare_reduction")}</dt>
              <dd>{fmtSigned(record.flare_reduction_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.flare_dvs_dt_fpm_per_sec != null && (
            <div title={t("landing.flare_dvs_dt_hint") ?? undefined}>
              <dt>{t("landing.flare_dvs_dt")}</dt>
              <dd>{fmtSigned(record.flare_dvs_dt_fpm_per_sec, 0, "fpm/s")}</dd>
            </div>
          )}
        </dl>
      </div>
    </section>
  );
}
