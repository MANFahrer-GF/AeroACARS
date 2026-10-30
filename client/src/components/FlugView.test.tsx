// Reiter „Flug“ (3.0): Briefing und Cockpit in einem Reiter.
//
// Gepinnt wird der Zustandswechsel, den Thomas zugesagt bekam:
//   vor dem Flug Briefing → ab Flugstart Cockpit (Briefing zugeklappt
//   darunter) → nach PIREP wieder Briefing, MIT Abschlussmeldung.
// Die schweren Kinder (Flugliste, Flugtafel) sind Attrappen — es geht um
// die Weiche, nicht um deren Inhalt.

import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { ActiveFlightInfo, FlightEndOutcome, LoginResult } from "../types";

vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  invoke: vi.fn(() => Promise.resolve(null)),
  listen: vi.fn(() => Promise.resolve(() => {})),
  openExternal: vi.fn(() => Promise.resolve()),
}));
vi.mock("./BidsList", () => ({
  BidsList: (p: { hasActiveFlight: boolean }) => (
    <div data-testid="bids">{p.hasActiveFlight ? "liste-im-flug" : "liste-vor-flug"}</div>
  ),
}));
vi.mock("./PilotHeader", () => ({ PilotHeader: () => <div data-testid="pilot" /> }));
vi.mock("./ActiveFlightPanel", () => ({
  ActiveFlightPanel: () => <div data-testid="cockpit-tafel" />,
}));
vi.mock("./VdgsBand", () => ({ VdgsPlatte: () => null, useVdgsStand: () => [null] }));
vi.mock("./DivertBanner", () => ({ DivertBanner: () => null }));
vi.mock("./SimRateHinweis", () => ({ SimRateHinweis: () => null }));
vi.mock("./kabine/KabinenHinweis", () => ({ KabinenHinweis: () => null }));
vi.mock("./bordbuch/BordbuchHinweis", () => ({ BordbuchHinweis: () => null }));
vi.mock("./StableApproachBanner", () => ({ StableApproachBanner: () => null }));

import { FlugView } from "./FlugView";

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
  localStorage.clear();
});

const session = { base_url: "https://x", profile: {} } as unknown as LoginResult;
const flug = {
  pirep_id: "P1",
  bid_id: 7,
  airline_icao: "GSG",
  flight_number: "421",
  callsign: "GSG421",
  phase: "cruise",
  was_just_resumed: false,
  takeoff_at: "2026-10-30T10:00:00Z",
} as unknown as ActiveFlightInfo;

function ansicht(activeFlight: ActiveFlightInfo | null, abschluss: FlightEndOutcome | null = null) {
  return (
    <FlugView
      session={session}
      activeFlight={activeFlight}
      setActiveFlight={vi.fn()}
      onLogout={vi.fn()}
      simState="connected"
      simSnapshot={null}
      approachAdvisoriesEnabled
      abschlussVonAussen={abschluss}
      onAbschlussGezeigt={vi.fn()}
    />
  );
}

describe("FlugView — ein Reiter folgt dem Flug", () => {
  it("Briefing → Cockpit → Briefing", () => {
    const r = render(ansicht(null));
    // Vor dem Flug: Briefing als ganze Seite, kein Cockpit, kein Aufklapper.
    expect(screen.getByTestId("bids").textContent).toBe("liste-vor-flug");
    expect(screen.queryByTestId("cockpit-tafel")).toBeNull();
    expect(screen.queryByTestId("flug-briefing")).toBeNull();

    // Flugstart: Cockpit, Briefing nur noch zugeklappt (nicht eingehängt).
    r.rerender(ansicht(flug));
    expect(screen.getByTestId("cockpit-tafel")).toBeTruthy();
    const aufklapper = screen.getByTestId("flug-briefing") as HTMLDetailsElement;
    expect(aufklapper.open).toBe(false);
    expect(screen.queryByTestId("bids")).toBeNull();
    expect(aufklapper.textContent).toContain(deCommon.briefing.aufklapper_titel);

    // Flugende: zurück zum Briefing.
    r.rerender(ansicht(null));
    expect(screen.queryByTestId("cockpit-tafel")).toBeNull();
    expect(screen.queryByTestId("flug-briefing")).toBeNull();
    expect(screen.getByTestId("bids").textContent).toBe("liste-vor-flug");
  });

  it("Aufklappen zeigt das Briefing des laufenden Flugs, ohne Verweis auf einen Cockpit-Reiter", () => {
    render(ansicht(flug));
    const aufklapper = screen.getByTestId("flug-briefing") as HTMLDetailsElement;
    act(() => {
      aufklapper.open = true;
      fireEvent(aufklapper, new Event("toggle"));
    });
    expect(screen.getByTestId("bids").textContent).toBe("liste-im-flug");
    expect(aufklapper.textContent).toContain("GSG421 läuft gerade");
    expect(aufklapper.textContent).not.toMatch(/Cockpit-Tab/);
    // Gemerkt für den nächsten Flug.
    expect(localStorage.getItem("aeroacars.flug.briefingOffen")).toBe("1");
  });

  it("auch während der Wiederaufnahme bleibt das Briefing erreichbar", () => {
    render(ansicht({ ...flug, was_just_resumed: true } as ActiveFlightInfo));
    expect(screen.queryByTestId("cockpit-tafel")).toBeNull();
    expect(screen.getByTestId("flug-briefing")).toBeTruthy();
  });

  it("nach dem PIREP steht die Abschlussmeldung über dem Briefing", () => {
    render(
      ansicht(null, { kind: "filed", callsign: "GSG421", dpt: "EDDF", arr: "EDDM" } as FlightEndOutcome),
    );
    expect(document.querySelector(".cockpit-pirep-success")).not.toBeNull();
    expect(screen.getByText(deCommon.cockpit.pirep_filed_title)).toBeTruthy();
    expect(screen.getByTestId("bids").textContent).toBe("liste-vor-flug");
  });

  it("der alte Leerzustand „Kein aktiver Flug“ ist weg", () => {
    render(ansicht(null));
    expect(document.querySelector(".cockpit-empty")).toBeNull();
  });
});
