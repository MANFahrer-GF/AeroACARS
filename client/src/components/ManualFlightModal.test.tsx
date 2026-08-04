// v0.19.x FIX: the sim-default-aircraft effect used to re-fire and
// unconditionally overwrite the pilot's own aircraft choice whenever
// simHint (live sim telemetry) changed — e.g. the pilot switches livery
// or tail number in the sim after already picking a different aircraft
// in the modal, or even after advancing to the "plan" stage. These pin
// down the pure matcher plus the actual overwrite-race in the component.

import { describe, it, expect, beforeAll, afterEach, vi, beforeEach } from "vitest";
import { render, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import type { Bid } from "../types";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => (e as { message?: string })?.message ?? String(e),
}));

import { ManualFlightModal, matchAircraftFromSimHint } from "./ManualFlightModal";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon }, en: { common: enCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(async () => {
  cleanup();
  await i18next.changeLanguage("de");
});

const FLEET = [
  { id: 1, registration: "D-AAAA", icao: "A320", name: "Airbus A320", airport_id: "EDDF", state: 0, display: "D-AAAA — A320" },
  { id: 2, registration: "D-BBBB", icao: "B738", name: "Boeing 737-800", airport_id: "EDDF", state: 0, display: "D-BBBB — B738" },
];

describe("matchAircraftFromSimHint", () => {
  it("matches by registration, case/trim-insensitive", () => {
    expect(matchAircraftFromSimHint(FLEET, { aircraft_registration: " d-aaaa " })?.id).toBe(1);
  });

  it("falls back to ICAO type when no registration match", () => {
    expect(matchAircraftFromSimHint(FLEET, { aircraft_icao: "b738" })?.id).toBe(2);
  });

  it("returns null when nothing matches", () => {
    expect(matchAircraftFromSimHint(FLEET, { aircraft_registration: "D-ZZZZ" })).toBeNull();
  });

  it("returns null for an empty hint", () => {
    expect(matchAircraftFromSimHint(FLEET, null)).toBeNull();
  });
});

function makeBid(): Bid {
  return {
    id: 1,
    user_id: 1,
    flight_id: "GSG100",
    flight: {
      id: "GSG100",
      flight_number: "GSG100",
      route_code: null,
      route_leg: null,
      callsign: "GSG100",
      dpt_airport_id: "EDDF",
      arr_airport_id: "EDDM",
      alt_airport_id: null,
      flight_time: null,
      dpt_time: null,
      arr_time: null,
      level: null,
      route: null,
      flight_type: null,
      distance: null,
      airline: null,
      dpt_airport: null,
      arr_airport: null,
      simbrief: null,
    },
  };
}

describe("ManualFlightModal — pilot selection survives a later sim-hint change", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fleet_list_at_airport") return Promise.resolve(FLEET);
      return Promise.resolve(undefined);
    });
  });

  it("does not clobber a manual pick when simHint changes afterwards", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <ManualFlightModal
        bid={makeBid()}
        simHint={null}
        onClose={() => {}}
        onFlightStarted={() => {}}
      />,
    );

    // No sim hint yet — pilot picks D-AAAA manually.
    await waitFor(() => expect(screen.getByText("D-AAAA")).toBeInTheDocument());
    await user.click(screen.getByText("D-AAAA"));
    await waitFor(() => expect(screen.getByText("D-AAAA").closest("button")).toHaveClass("selected"));

    // Sim telemetry now reports a (different) aircraft — e.g. the pilot
    // changed livery/tail number in the sim after already picking one
    // here. This changes the effect's dependencies and re-runs it. The
    // old code unconditionally called setSelected(match) and silently
    // reverted the pilot's manual choice.
    rerender(
      <ManualFlightModal
        bid={makeBid()}
        simHint={{ aircraft_registration: "D-BBBB" }}
        onClose={() => {}}
        onFlightStarted={() => {}}
      />,
    );

    await waitFor(() => expect(invokeMock.mock.calls.filter((c) => c[0] === "fleet_list_at_airport").length).toBeGreaterThan(1));
    expect(screen.getByText("D-AAAA").closest("button")).toHaveClass("selected");
    expect(screen.getByText("D-BBBB").closest("button")).not.toHaveClass("selected");
  });
});

// v0.19.x FIX: the "in Use"/"in Flight"/"Maintenance" state badge and the
// hover tooltip (both the "available" and "unavailable" phrasing) were
// hardcoded German string literals, shown verbatim regardless of the
// active locale.
describe("ManualFlightModal — aircraft-state label and tooltip follow the active locale", () => {
  const fleetWithInUse = [
    { id: 1, registration: "D-CCCC", icao: "A320", name: "Airbus A320", airport_id: "EDDF", state: 1, display: "D-CCCC — A320" },
  ];

  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fleet_list_at_airport") return Promise.resolve(fleetWithInUse);
      return Promise.resolve(undefined);
    });
  });

  it("shows the German state badge under de, a different English one under en", async () => {
    await i18next.changeLanguage("de");
    render(<ManualFlightModal bid={makeBid()} simHint={null} onClose={() => {}} onFlightStarted={() => {}} />);
    await waitFor(() => expect(screen.getByText("🔒 belegt")).toBeInTheDocument());
    cleanup();

    await i18next.changeLanguage("en");
    render(<ManualFlightModal bid={makeBid()} simHint={null} onClose={() => {}} onFlightStarted={() => {}} />);
    await waitFor(() => expect(screen.getByText("🔒 in Use")).toBeInTheDocument());
    expect(screen.queryByText("🔒 belegt")).not.toBeInTheDocument();
  });

  it("localizes the unavailable-aircraft tooltip sentence, not just the state word", async () => {
    await i18next.changeLanguage("de");
    const de = render(<ManualFlightModal bid={makeBid()} simHint={null} onClose={() => {}} onFlightStarted={() => {}} />);
    await waitFor(() => expect(screen.getByText("D-CCCC")).toBeInTheDocument());
    expect(screen.getByText("D-CCCC").closest("button")).toHaveAttribute(
      "title",
      "🔒 belegt — phpVMS lehnt ggf. den Prefile ab.",
    );
    de.unmount();

    await i18next.changeLanguage("en");
    render(<ManualFlightModal bid={makeBid()} simHint={null} onClose={() => {}} onFlightStarted={() => {}} />);
    await waitFor(() => expect(screen.getByText("D-CCCC")).toBeInTheDocument());
    expect(screen.getByText("D-CCCC").closest("button")).toHaveAttribute(
      "title",
      "🔒 in Use — phpVMS may reject the prefile.",
    );
  });
});
