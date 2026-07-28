import { useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ActiveFlightInfo,
  Bid,
  LoginResult,
  Profile,
  SimConnectionState,
  SimSnapshot,
} from "../types";
import { BidsList } from "./BidsList";
import { PilotHeader } from "./PilotHeader";
import { resolveFlightIdent } from "../lib/callsign";

interface Props {
  session: LoginResult;
  activeFlight: ActiveFlightInfo | null;
  setActiveFlight: (info: ActiveFlightInfo | null) => void;
  onLogout: () => void;
  simState: SimConnectionState;
  simSnapshot: SimSnapshot | null;
  /** Called when BidsList' refresh handler returns a fresh profile so
   *  App.tsx can update the cached session + the PilotHeader rerenders. */
  onProfileRefreshed?: (profile: Profile) => void;
  /** v0.7.7: Called when Bid-Tab-Refresh successfully refreshed the
   *  active flight's OFP (`changed=true`). Parent triggert dann eine
   *  `flight_status`-Re-Fetch damit Cockpit + Loadsheet sofort den
   *  neuen Plan sehen. Spec docs/spec/ofp-refresh-during-boarding.md §6.5b. */
  onActiveFlightUpdated?: () => void;
}

/**
 * Briefing tab — the pre-flight pilot view. Pilot identity card up
 * top (with the airline logo at a sensible size), then the booked-
 * flights list. When a flight is already active, a short banner
 * reminds the pilot to switch to the cockpit tab so they don't
 * accidentally start a second one.
 */
export function BriefingView({
  session,
  activeFlight,
  setActiveFlight,
  onLogout,
  simState,
  simSnapshot,
  onProfileRefreshed,
  onActiveFlightUpdated,
}: Props) {
  const { t } = useTranslation();
  const [, setSelectedBid] = useState<Bid | null>(null);

  return (
    <>
      <PilotHeader profile={session.profile} onLogout={onLogout} />

      {activeFlight && (
        <div className="briefing-active-hint">
          {t("briefing.active_flight_hint", {
            callsign: activeFlight.airline_icao
              ? `${activeFlight.airline_icao} ${resolveFlightIdent(activeFlight.flight_number, activeFlight.callsign)}`
              : resolveFlightIdent(activeFlight.flight_number, activeFlight.callsign),
          })}
        </div>
      )}

      <BidsList
        baseUrl={session.base_url}
        simState={simState}
        simSnapshot={simSnapshot}
        hasActiveFlight={activeFlight !== null}
        onSelect={setSelectedBid}
        onFlightStarted={setActiveFlight}
        onProfileRefreshed={onProfileRefreshed}
        onActiveFlightUpdated={onActiveFlightUpdated}
      />
    </>
  );
}
