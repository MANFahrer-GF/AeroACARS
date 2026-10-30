// Reiter „Flug“ (3.0): im aufgeklappten Briefing während des Flugs steht die
// Buchung des LAUFENDEN Flugs als eigene Karte (OFP-Werte, Links) über der
// Liste — ohne Start-Knöpfe und ohne „anderer Flug aktiv“. „Gebuchte Flüge“
// zählt weiter nur die noch offenen Buchungen (v1.4.7).

import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { Bid } from "../types";

const bid = (id: number, nr: string, dpt: string, arr: string): Bid =>
  ({
    id,
    user_id: 1,
    flight_id: `F${id}`,
    flight: {
      id: `F${id}`, flight_number: nr, route_code: null, route_leg: null, callsign: null,
      dpt_airport_id: dpt, arr_airport_id: arr, alt_airport_id: null, flight_time: 75,
      dpt_time: "10:15", arr_time: "11:30", level: 360, route: "SOBRA Y163 NATOR",
      flight_type: "J", distance: { mi: null, km: null, nmi: 330 },
      airline: { id: 1, icao: "GSG", iata: "GS", name: "German Sky Group", logo: null },
      dpt_airport: null, arr_airport: null,
      simbrief: { id: `sb${id}`, url: null, aircraft_id: 7, subfleet: null },
    },
  }) as unknown as Bid;

let ofpZiel = "LSZH";
const BIDS = [bid(11, "421", "EDDF", "LSZH"), bid(12, "422", "LSZH", "EDDF"), bid(13, "1910", "EDDF", "EGLL")];

vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  invoke: vi.fn((cmd: string, args?: { bidId?: number }) => {
    if (cmd === "phpvms_get_bids") return Promise.resolve(BIDS);
    if (cmd === "bid_simbrief_preview") {
      return Promise.resolve({
        request_id: `r${args?.bidId}`, planned_block_fuel_kg: 5200, planned_burn_kg: 3100,
        planned_reserve_kg: 1450, planned_zfw_kg: 58200, planned_tow_kg: 63200, planned_ldw_kg: 60100,
        alternate: "LSGG", ofp_flight_number: "421", ofp_origin_icao: "EDDF", ofp_destination_icao: ofpZiel,
        ofp_generated_at: new Date().toISOString(), pax_count: 156, cargo_kg: 2100, freight_kg: 600,
        aircraft_icao: "A320", max_passengers: 180,
        // Abflug längst vorbei — im laufenden Flug darf das nicht warnen.
        sched_out_epoch: Math.floor(Date.now() / 1000) - 3 * 3600,
        sched_in_epoch: Math.floor(Date.now() / 1000) - 3600,
        planned_taxi_kg: 200, planned_contingency_kg: 150,
      });
    }
    return Promise.resolve(null);
  }),
  openExternal: vi.fn(() => Promise.resolve()),
}));

import { BidsList } from "./BidsList";

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "de",
    resources: { de: { common: deCommon } },
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
});

function liste(laufendenFlugZeigen: boolean) {
  return render(
    <BidsList
      baseUrl="https://x"
      simState="connected"
      simSnapshot={null}
      hasActiveFlight
      activeBidId={11}
      laufendenFlugZeigen={laufendenFlugZeigen}
    />,
  );
}

describe("BidsList — Buchung des laufenden Flugs im Flug-Briefing", () => {
  it("zeigt sie als eigene Karte, ohne Start und ohne Abflug-Warnung", async () => {
    liste(true);
    const karte = await screen.findByTestId("bids-laufend");
    await waitFor(() => expect(karte.textContent).toContain("5.200"));
    expect(karte.textContent).toContain(deCommon.bids.laufender_flug);
    expect(karte.textContent).toContain("GSG421");
    const k = within(karte);
    expect(k.queryByRole("button", { name: deCommon.bid_card.start_flight })).toBeNull();
    expect(k.queryByRole("group", { name: deCommon.bid_card.mode_toggle_label })).toBeNull();
    expect(karte.textContent).not.toContain(deCommon.bids.start_disabled_active_flight);
    expect(k.queryByTestId("ofp-off-block-vorbei")).toBeNull();
    // Links bleiben.
    expect(k.getByRole("button", { name: deCommon.bids.open_ofp })).toBeTruthy();
    expect(k.getByRole("button", { name: deCommon.bids.open_flight_page })).toBeTruthy();
    // „Gebuchte Flüge“ zählt weiter nur die offenen Buchungen.
    expect(document.querySelector(".bids__count")?.textContent).toBe("2");
  });

  // SimBrief liefert den zuletzt erstellten OFP. Ist das schon der nächste
  // Flug, darf die Karte des laufenden Flugs nicht „neuen OFP erstellen“ sagen.
  it("fordert beim laufenden Flug keinen neuen OFP, wenn SimBrief schon den nächsten hat", async () => {
    ofpZiel = "EGLL";
    try {
      liste(true);
      const karte = await screen.findByTestId("bids-laufend");
      await waitFor(() => expect(document.querySelector(".bids__count")?.textContent).toBe("2"));
      await new Promise((r) => setTimeout(r, 50));
      expect(karte.textContent).not.toContain(deCommon.bids.ofp_mismatch_hint);
      expect(karte.querySelector(".bid-card__ofp-compare")).toBeNull();
    } finally {
      ofpZiel = "LSZH";
    }
  });

  it("ohne den Schalter bleibt alles wie bisher (Briefing vor dem Flug)", async () => {
    liste(false);
    await waitFor(() => expect(document.querySelector(".bids__count")?.textContent).toBe("2"));
    expect(screen.queryByTestId("bids-laufend")).toBeNull();
  });
});
