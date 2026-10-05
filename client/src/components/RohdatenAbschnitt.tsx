// Rohdaten zur Nachprüfung (aufklappbar): Koordinaten des Aufsetzpunkts,
// Zeit, Kurse, Bahnbezug mit Vorzeichen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 nur in der Webapp („Raw Data / Verification"); der Client hatte
// die Koordinaten nicht in seiner Aufzeichnung.

import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import "./rohdatenAbschnitt.css";
import { useDruck } from "../lib/druck";

/** ISO-Zeit in UTC, auf Millisekunden — unabhängig davon, wie die Quelle
 *  sie geschrieben hat (Rust: bis Mikrosekunden, Webapp: Millisekunden). */
function zeitUtc(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toISOString();
}

export function RohdatenAbschnitt({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  const druck = useDruck();
  const lat = record.landing_lat;
  const lon = record.landing_lon;
  const koord = lat != null && lon != null ? `${lat.toFixed(7)}, ${lon.toFixed(7)}` : "—";
  const karte =
    lat != null && lon != null
      ? `https://www.google.com/maps/place/${lat.toFixed(7)},${lon.toFixed(7)}/@${lat.toFixed(7)},${lon.toFixed(7)},19z`
      : null;
  const versatz = record.runway_match?.centerline_distance_m ?? null;
  const schwelle = record.td_distance_from_threshold_m ?? null;
  const zeile = (k: string, v: string, extra?: ReactNode) => (
    <div className="rohdaten__zeile">
      <span className="rohdaten__k">{k}</span>
      <code className="rohdaten__v">{v}</code>
      {extra}
    </div>
  );
  return (
    <details className="landing-section rohdaten" open={druck || undefined}>
      <summary>{t("landing.rohdaten.title")}</summary>
      <div className="rohdaten__liste">
        {zeile(
          t("landing.rohdaten.koordinaten"),
          koord,
          karte && (
            <a className="rohdaten__link" href={karte} target="_blank" rel="noreferrer">
              Google Maps ↗
            </a>
          ),
        )}
        {zeile(t("landing.rohdaten.zeit"), zeitUtc(record.touchdown_at))}
        {zeile(
          t("landing.rohdaten.kurs"),
          `${record.landing_heading_true_deg != null ? `${record.landing_heading_true_deg.toFixed(2)}°T` : "—"} / ${
            record.landing_heading_deg != null ? `${record.landing_heading_deg.toFixed(2)}°M` : "—"
          }`,
        )}
        {zeile(
          t("landing.rohdaten.bahn"),
          `${record.runway_match?.airport_ident ?? "—"} / ${record.runway_match?.runway_ident ?? "—"}`,
        )}
        {zeile(
          t("landing.rohdaten.versatz"),
          versatz != null
            ? `${versatz > 0 ? "+" : ""}${versatz.toFixed(2)} m (${t(
                versatz > 0 ? "landing.rohdaten.rechts" : versatz < 0 ? "landing.rohdaten.links" : "landing.rohdaten.mitte",
              )})`
            : "—",
        )}
        {zeile(
          t("landing.rohdaten.schwelle"),
          schwelle != null ? `${schwelle >= 0 ? "+" : ""}${schwelle.toFixed(2)} m` : "—",
        )}
        {zeile(
          t("landing.rohdaten.ausrollen"),
          record.rollout_distance_m != null ? `${record.rollout_distance_m.toFixed(0)} m` : "—",
        )}
        {zeile(t("landing.rohdaten.version"), record.client_version ?? "—")}
      </div>
      <p className="rohdaten__hinweis">{t("landing.rohdaten.hinweis")}</p>
    </details>
  );
}
