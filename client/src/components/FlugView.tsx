/**
 * Reiter „Flug“ — Briefing und Cockpit in einem (AeroACARS 3.0).
 *
 * Thomas: „Der Briefing-Tab ist nur fürs Briefing da und danach total
 * wertlos.“ Deshalb gibt es nur noch einen Reiter, der dem Flug folgt:
 *
 *   - ohne Flug: das Briefing (gebuchte Flüge, SimBrief-OFP, Start-Knöpfe)
 *   - mit Flug (auch pausiert, wiederaufgenommen): das Cockpit; das Briefing
 *     bleibt darunter als zugeklappter Bereich erreichbar — nichts fällt weg
 *   - nach PIREP/Abbruch: von selbst zurück zum Briefing, mit der
 *     Abschlussmeldung des Cockpits darüber
 *
 * Gesteuert allein über `activeFlight` (App.tsx pollt `flight_status`).
 * Kein eigener Zustand für „welche Ansicht“ — der könnte vom Flug abweichen.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ActiveFlightInfo,
  FlightEndOutcome,
  LoginResult,
  Profile,
  SimConnectionState,
  SimSnapshot,
} from "../types";
import { BriefingView } from "./BriefingView";
import { CockpitView } from "./CockpitView";
import "./flugView.css";

const BRIEFING_OFFEN_KEY = "aeroacars.flug.briefingOffen";

function briefingOffenLesen(): boolean {
  try {
    return localStorage.getItem(BRIEFING_OFFEN_KEY) === "1";
  } catch {
    return false;
  }
}

interface Props {
  session: LoginResult;
  activeFlight: ActiveFlightInfo | null;
  setActiveFlight: (info: ActiveFlightInfo | null) => void;
  onLogout: () => void;
  simState: SimConnectionState;
  simSnapshot: SimSnapshot | null;
  approachAdvisoriesEnabled: boolean;
  abschlussVonAussen?: FlightEndOutcome | null;
  onAbschlussGezeigt?: () => void;
  onProfileRefreshed?: (profile: Profile) => void;
  onActiveFlightUpdated?: () => void;
}

export function FlugView({
  session,
  activeFlight,
  setActiveFlight,
  onLogout,
  simState,
  simSnapshot,
  approachAdvisoriesEnabled,
  abschlussVonAussen,
  onAbschlussGezeigt,
  onProfileRefreshed,
  onActiveFlightUpdated,
}: Props) {
  const { t } = useTranslation();
  const [briefingOffen, setBriefingOffen] = useState(briefingOffenLesen);

  const briefing = (imFlug: boolean) => (
    <BriefingView
      session={session}
      activeFlight={activeFlight}
      setActiveFlight={setActiveFlight}
      onLogout={onLogout}
      simState={simState}
      simSnapshot={simSnapshot}
      onProfileRefreshed={onProfileRefreshed}
      onActiveFlightUpdated={onActiveFlightUpdated}
      laufendenFlugZeigen={imFlug}
    />
  );

  return (
    <CockpitView
      session={session}
      activeFlight={activeFlight}
      setActiveFlight={setActiveFlight}
      simSnapshot={simSnapshot}
      approachAdvisoriesEnabled={approachAdvisoriesEnabled}
      abschlussVonAussen={abschlussVonAussen}
      onAbschlussGezeigt={onAbschlussGezeigt}
      vorFlug={briefing(false)}
      briefingImFlug={
        <details
          className="flug-briefing"
          data-testid="flug-briefing"
          open={briefingOffen}
          onToggle={(e) => {
            const offen = (e.currentTarget as HTMLDetailsElement).open;
            setBriefingOffen(offen);
            try {
              localStorage.setItem(BRIEFING_OFFEN_KEY, offen ? "1" : "0");
            } catch {
              /* Private-Mode o.ä. — Aufklappen geht, wird nur nicht gemerkt. */
            }
          }}
        >
          <summary className="flug-briefing__kopf">
            <span className="flug-briefing__titel">{t("briefing.aufklapper_titel")}</span>
            <span className="flug-briefing__inhalt">{t("briefing.aufklapper_inhalt")}</span>
          </summary>
          {/* Erst beim Aufklappen einhängen: die Liste holt beim Einhängen
              Flüge und OFP-Stand — zugeklappt kostet sie im Flug nichts. */}
          {briefingOffen && <div className="flug-briefing__koerper">{briefing(true)}</div>}
        </details>
      }
    />
  );
}
