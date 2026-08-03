// v1.3.5 (#Livemap-4a) — empty-state card for the Live-Karte screen when no
// own flight is active (README §6). Replaces the single centered sentence
// ("Kein aktiver Flug und gerade kein VA-Verkehr.") with a real action card
// naming the next booked flight.
//
// `nextBid` is fetched once in LiveMapView.tsx (not here) — the header's idle
// context line (README §2, "Kein aktiver Flug · nächster {Callsign} ab
// {hh:mm}z") needs the exact same data, so a single fetch is shared rather
// than polling `phpvms_get_bids` twice for the same answer.

import { useTranslation } from "react-i18next";
import type { Bid } from "../types";
import { resolveFlightIdent } from "../lib/callsign";

export interface NextBidInfo {
  ident: string;
  dep: string;
  arr: string;
  std: string;
}

/** Shared derivation used by both the header context line and this card —
 *  BidsList.tsx's own fallback when nothing is pinned as "main" is
 *  `bids.find(b => b.id === mainBidId) ?? bids[0]`; that pin is local,
 *  unpersisted state that doesn't survive leaving Briefing anyway, so
 *  `bids[0]` is the same thing BidsList shows by default.
 *
 *  `std` is resolved by the CALLER, not read off `flight.dpt_time` directly —
 *  a real booking hit this (2026-08-03 field test): phpVMS's own schedule
 *  field was null, and this function used to bail out to "no booking at
 *  all" even though a real one existed. BidsList.tsx has the same fallback
 *  already (`f.dpt_time ?? sched_out_epoch via bid_simbrief_preview`) —
 *  LiveMapView.tsx replicates that same two-step lookup and passes the
 *  resolved result in here. */
export function nextBidInfo(bid: Bid | null, std: string | null): NextBidInfo | null {
  const flight = bid?.flight;
  if (!flight) return null;
  const dep = flight.dpt_airport?.icao ?? flight.dpt_airport_id;
  const arr = flight.arr_airport?.icao ?? flight.arr_airport_id;
  if (!dep || !arr || !std) return null;
  return {
    ident: `${flight.airline?.icao ?? ""}${resolveFlightIdent(flight.flight_number, flight.callsign)}`,
    dep,
    arr,
    std,
  };
}

interface Props {
  /** `undefined` = still loading, `null` = loaded, nothing booked. */
  next: NextBidInfo | null | undefined;
  /** Both actions land on Briefing — starting a flight (OFP/aircraft/prefile)
   *  is Briefing's job, not this card's; "Flug starten" is the more
   *  action-oriented CTA copy, "Zum Briefing" the plain navigation, but
   *  neither can actually prefile a PIREP from here. */
  onGoToBriefing: () => void;
}

// Field feedback (2026-08-03): this card only ever renders when there's
// NEITHER an own flight NOR any visible VA traffic (see the gate at the call
// site in LiveMapView.tsx) — showing it whenever there was no own flight,
// VA-traffic-or-not, visibly blocked the map exactly where colleague
// aircraft render, which the pilot explicitly wants to see. README §6
// point 2's "Kollegen unterwegs" headline variant is therefore unreachable
// and was removed along with its i18n key rather than left as dead code.
export function LiveMapEmptyState({ next, onGoToBriefing }: Props) {
  const { t } = useTranslation();

  return (
    <div className="aa-livemap-idle aa-livemap-overlay">
      <span className="aa-livemap-idle__rubric">{t("livemap.idle_rubric")}</span>
      <p className="aa-livemap-idle__headline">{t("livemap.idle_headline_none")}</p>
      <p className="aa-livemap-idle__detail">
        {next ? t("livemap.idle_detail_next", { ...next, std: `${next.std}z` }) : next === null ? t("livemap.idle_detail_none") : ""}
      </p>
      <div className="aa-livemap-idle__actions">
        <button type="button" className="button button--primary" onClick={onGoToBriefing}>
          {t("livemap.idle_start_flight")}
        </button>
        <button type="button" className="button button--secondary" onClick={onGoToBriefing}>
          {t("livemap.idle_go_briefing")}
        </button>
      </div>
    </div>
  );
}
