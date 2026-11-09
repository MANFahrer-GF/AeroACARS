// Crew Story an der Buchung: Das Fenster gibt es nur mit `modul-api-1`; ein älteres
// Programm öffnet die OFP-Seite, ohne OFP erscheint der Hinweis auf das neue AeroACARS.
import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { Bid } from "../types";

const bid = (ofp: boolean) =>
  ({
    id: 11, user_id: 1, flight_id: "F11",
    flight: {
      id: "F11", flight_number: "421", route_code: null, route_leg: null, callsign: null, dpt_airport_id: "EDDF", arr_airport_id: "LSZH", alt_airport_id: null,
      flight_time: 75, dpt_time: "10:15", arr_time: "11:30", level: 360, route: null, flight_type: "J", distance: null,
      airline: { id: 1, icao: "GSG", iata: "GS", name: "German Sky Group", logo: null }, dpt_airport: null, arr_airport: null,
      simbrief: ofp ? { id: "sb11", url: null, aircraft_id: 7, subfleet: null } : null,
    },
  }) as unknown as Bid;

const lage = { faehigkeiten: [] as string[], bids: [bid(true)] };
const { invokeMock, openMock } = vi.hoisted(() => ({ invokeMock: vi.fn(), openMock: vi.fn(() => Promise.resolve()) }));

vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  invoke: invokeMock,
  openExternal: openMock,
}));

import { BidsList } from "./BidsList";

const TITEL = deCommon.bids.crew_story.titel;

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "de",
    resources: { de: { common: deCommon } },
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
});

beforeEach(() => {
  openMock.mockClear();
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "phpvms_get_bids") return Promise.resolve(lage.bids);
    if (cmd === "paket_info") return Promise.resolve({ faehigkeiten: lage.faehigkeiten });
    if (cmd === "modul_api") return Promise.resolve({ status: 200, daten: { text: "Story", hash: null, pruefung: null, gesperrt: false, max: 500 } });
    return Promise.resolve(null);
  });
});

function liste(aktiv = false) {
  render(<BidsList baseUrl="https://x" simState="connected" simSnapshot={null} hasActiveFlight={aktiv} activeBidId={aktiv ? 11 : null} laufendenFlugZeigen={aktiv} />);
}

const faehigkeitGeladen = () =>
  waitFor(() => expect(invokeMock.mock.results.some((r, i) => invokeMock.mock.calls[i]?.[0] === "paket_info" && r.type === "return")).toBe(true));

describe("BidsList — Crew Story", () => {
  it("mit modul-api-1 öffnet der Link das Fenster dieser Buchung", async () => {
    lage.faehigkeiten = ["modul-api-1"];
    lage.bids = [bid(true)];
    liste();
    const link = await screen.findByRole("button", { name: TITEL });
    await faehigkeitGeladen();
    await waitFor(() => {
      fireEvent.click(link);
      expect(screen.getByRole("dialog")).toBeTruthy();
    });
    expect(openMock).not.toHaveBeenCalled();
    expect(((await screen.findByRole("textbox")) as HTMLTextAreaElement).value).toBe("Story");
  });

  it("älteres Programm mit OFP öffnet die OFP-Seite, kein Fenster", async () => {
    lage.faehigkeiten = ["bilder-1"];
    lage.bids = [bid(true)];
    liste();
    fireEvent.click(await screen.findByRole("button", { name: TITEL }));
    await waitFor(() => expect(openMock).toHaveBeenCalledWith("https://x/paxstudio/ofp/sb11"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(invokeMock.mock.calls.some((c) => c[0] === "modul_api")).toBe(false);
  });

  it("älteres Programm ohne OFP zeigt den Hinweis", async () => {
    lage.faehigkeiten = ["bilder-1"];
    lage.bids = [bid(false)];
    liste();
    fireEvent.click(await screen.findByRole("button", { name: TITEL }));
    await screen.findByText(deCommon.bids.crew_story.neues_aeroacars);
    expect(openMock).not.toHaveBeenCalled();
  });

  it("laufender Flug: Link in der Ansichts-Karte öffnet nur zum Lesen", async () => {
    lage.faehigkeiten = ["modul-api-1"];
    lage.bids = [bid(true)];
    liste(true);
    const karte = await screen.findByTestId("bids-laufend");
    await faehigkeitGeladen();
    const link = [...karte.querySelectorAll("button")].find((b) => b.textContent === TITEL)!;
    await waitFor(() => {
      fireEvent.click(link);
      expect(screen.getByRole("dialog")).toBeTruthy();
    });
    expect(((await screen.findByRole("textbox")) as HTMLTextAreaElement).readOnly).toBe(true);
  });
});
