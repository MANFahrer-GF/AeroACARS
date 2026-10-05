// Abfangbogen (Flare).
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 hieß der Abschnitt im Client „Flare-Qualität", in der Webapp
// „Abfangbogen (keine Note)" mit anderen Beschriftungen und einer selbst
// gerechneten Reduktion.
//
// Seit 05.10.2026 misst der Client das Abfangen über die Höhe (ab dem
// letzten 50-ft-Durchgang) und bewertet die Dauer als Teilnote `abfangen`.
// Flüge mit dieser Messung zeigen sie; ältere zeigen unverändert die alten
// Werte aus den 2 s vor dem Aufsetzen — die waren nie eine Note, und bei
// langem Schweben meldeten sie fälschlich „Kein Flare" (QAF434, THY39).

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { fmtNumber, fmtSigned } from "../lib/landungsFormat";
import "./flareAbschnitt.css";
import { InfoBadge } from "./InfoBadge";

export function FlareAbschnitt({ record }: { record: LandingRecord }) {
  const a = record.abfangen;
  if (a && a.dauer_ab_50ft_s != null) return <AbfangenNeu record={record} />;
  return <AbfangenAlt record={record} />;
}

function AbfangenNeu({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const a = record.abfangen!;
  return (
    <section className="landing-section landing-section--flare">
      <h3>{t("landing.abfangen.titel")}</h3>
      <div className="landing-flare">
        <dl className="landing-keyvals landing-flare__metrics">
          <div>
            <dt>
              {t("landing.abfangen.dauer")}{" "}
              <InfoBadge explanation={t("landing.abfangen.dauer_hint")} />
            </dt>
            <dd>{fmtNumber(a.dauer_ab_50ft_s, 1, "s")}</dd>
          </div>
          {a.vs_50ft_fpm != null && (
            <div>
              <dt>
                {t("landing.abfangen.vs")}{" "}
                <InfoBadge explanation={t("landing.abfangen.vs_hint")} />
              </dt>
              <dd>
                {fmtNumber(a.vs_50ft_fpm, 0, "fpm")}
                {a.vs_aufsetzen_fpm != null && <> → {fmtNumber(a.vs_aufsetzen_fpm, 0, "fpm")}</>}
              </dd>
            </div>
          )}
          {a.reduktion_fpm != null && (
            <div>
              <dt>
                {t("landing.abfangen.reduktion")}{" "}
                <InfoBadge explanation={t("landing.abfangen.reduktion_hint")} />
              </dt>
              <dd>{fmtSigned(a.reduktion_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {a.beginn_hoehe_ft != null && (
            <div>
              <dt>
                {t("landing.abfangen.beginn")}{" "}
                <InfoBadge explanation={t("landing.abfangen.beginn_hint")} />
              </dt>
              <dd>{fmtNumber(a.beginn_hoehe_ft, 0, "ft")}</dd>
            </div>
          )}
          {a.schweben_s != null && (
            <div>
              <dt>
                {t("landing.abfangen.schweben")}{" "}
                <InfoBadge explanation={t("landing.abfangen.schweben_hint")} />
              </dt>
              <dd>
                {fmtNumber(a.schweben_s, 1, "s")}
                {a.schweben_m != null && a.schweben_m > 0 && <> · {fmtNumber(a.schweben_m, 0, "m")}</>}
              </dd>
            </div>
          )}
          {a.max_vs_fpm != null && a.max_vs_fpm > 0 && (
            <div>
              <dt>
                {t("landing.abfangen.steigen")}{" "}
                <InfoBadge explanation={t("landing.abfangen.steigen_hint")} />
              </dt>
              <dd>{fmtSigned(a.max_vs_fpm, 0, "fpm")}</dd>
            </div>
          )}
        </dl>
      </div>
    </section>
  );
}

function AbfangenAlt({ record }: { record: LandingRecord }) {
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
            <div>
              <dt>
                {t("landing.flare_pre_vs")}{" "}
                <InfoBadge explanation={t("landing.flare_pre_vs_hint")} />
              </dt>
              <dd>{fmtNumber(record.peak_vs_pre_flare_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.vs_at_flare_end_fpm != null && (
            <div>
              <dt>
                {t("landing.flare_end_vs")}{" "}
                <InfoBadge explanation={t("landing.flare_end_vs_hint")} />
              </dt>
              <dd>{fmtNumber(record.vs_at_flare_end_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.flare_reduction_fpm != null && (
            <div>
              <dt>
                {t("landing.flare_reduction")}{" "}
                <InfoBadge explanation={t("landing.flare_reduction_hint")} />
              </dt>
              <dd>{fmtSigned(record.flare_reduction_fpm, 0, "fpm")}</dd>
            </div>
          )}
          {record.flare_dvs_dt_fpm_per_sec != null && (
            <div>
              <dt>
                {t("landing.flare_dvs_dt")}{" "}
                <InfoBadge explanation={t("landing.flare_dvs_dt_hint")} />
              </dt>
              <dd>{fmtSigned(record.flare_dvs_dt_fpm_per_sec, 0, "fpm/s")}</dd>
            </div>
          )}
        </dl>
      </div>
    </section>
  );
}
