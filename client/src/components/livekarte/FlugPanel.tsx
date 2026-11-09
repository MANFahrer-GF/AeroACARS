// Das Panel des eigenen Flugs über der Karte: Werte und Ereignisse, einklappbar.
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo } from "../../types";
import { phaseColor } from "../../lib/phaseColors";

const SPEICHER = "aaLivemapPanelZu";

function Wert({ name, wert }: { name: string; wert: string }) {
  return (
    <div>
      <span className="aa-livemap-flightcard__label">{name}</span>
      <span className="aa-livemap-flightcard__value">{wert}</span>
    </div>
  );
}

export function FlugPanel({ flug, phase, werte, ereignisse }: {
  flug: ActiveFlightInfo;
  phase: string;
  werte: { hoehe: string; gs: string; rest: string; eta: string; restNm: number | null | undefined };
  /** Die Ereignisliste; bleibt beim Einklappen weg. */
  ereignisse: ReactNode;
}) {
  const { t } = useTranslation();
  const [zu, setZu] = useState(() => {
    try { return localStorage.getItem(SPEICHER) === "1"; } catch { return false; }
  });
  useEffect(() => {
    try { localStorage.setItem(SPEICHER, zu ? "1" : "0"); } catch { /* best-effort */ }
  }, [zu]);

  // `distance_nm` ist die bereits GEFLOGENE Strecke, `restNm` die noch zu fliegende: Anteil = geflogen / (geflogen + Rest).
  const fortschritt = flug.distance_nm && werte.restNm != null
    ? Math.min(100, Math.max(0, (flug.distance_nm / (flug.distance_nm + Math.max(0, werte.restNm))) * 100))
    : 0;
  // Geplantes Muster aus phpVMS, nicht das gerade im Simulator geladene.
  const muster = [flug.aircraft_name || flug.aircraft_icao, flug.planned_registration].filter(Boolean).join(" · ");
  const name = t(zu ? "livemap.panel_auf" : "livemap.panel_zu");

  return (
    <div className="aa-livemap-panel">
      <div className="aa-livemap-flightcard aa-livemap-overlay">
        <div className="aa-livemap-flightcard__head">
          <span className="aa-livemap-flightcard__rubric">{t("livemap.own_flight")}</span>
          <span className="aa-livemap-flightcard__live">{t("livemap.live")}</span>
          <button
            type="button"
            className="aa-livemap-flightcard__klappe"
            aria-expanded={!zu}
            aria-label={name}
            title={name}
            onClick={() => setZu((v) => !v)}
          >
            <span aria-hidden="true">{zu ? "▾" : "▴"}</span>
          </button>
        </div>
        <div className="aa-livemap-flightcard__route">
          <span className="aa-livemap-flightcard__airport">{flug.dpt_airport}</span>
          <span className="aa-livemap-flightcard__arrow" aria-hidden="true">→</span>
          <span className="aa-livemap-flightcard__airport">{flug.arr_airport}</span>
        </div>
        {!zu && (
          <>
            <div className="aa-livemap-flightcard__bar">
              <div className="aa-livemap-flightcard__bar-fill" style={{ width: `${fortschritt}%` }} />
            </div>
            <div className="aa-livemap-flightcard__grid">
              <div className="aa-livemap-flightcard__phase-row">
                <span className="aa-livemap-flightcard__label">{t("livemap.field_phase")}</span>
                <span className="aa-livemap-flightcard__value" style={{ color: phaseColor(flug.phase) }}>{phase}</span>
              </div>
              <Wert name={t("livemap.field_alt")} wert={werte.hoehe} />
              <Wert name={t("livemap.field_gs")} wert={werte.gs} />
              <Wert name={t("livemap.field_rest")} wert={werte.rest} />
              <Wert name={t("livemap.field_eta")} wert={werte.eta} />
            </div>
            {muster && <div className="aa-livemap-flightcard__foot"><span>{muster}</span></div>}
          </>
        )}
      </div>
      {!zu && ereignisse}
    </div>
  );
}
