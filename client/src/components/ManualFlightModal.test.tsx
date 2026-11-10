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
import deCommon from "../../../bundle/texte/de.json";
import enCommon from "../../../bundle/texte/en.json";
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

describe("ManualFlightModal — Beladungsklassen beim VFR-Start", () => {
  const KLASSEN = [
    { id: 6, code: "Y", name: "Economy", typ: 0, kapazitaet: 4 },
    { id: 7, code: "CGO", name: "Cargo", typ: 1, kapazitaet: 150 },
  ];
  let klassenAntwort: () => Promise<unknown>;

  beforeEach(() => {
    localStorage.clear();
    invokeMock.mockReset();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "fleet_list_at_airport") return Promise.resolve(FLEET);
      if (cmd === "vfr_beladungsklassen") return klassenAntwort();
      return Promise.resolve({});
    });
  });

  async function bisZumPlan() {
    const user = userEvent.setup();
    render(<ManualFlightModal bid={makeBid()} simHint={null} onClose={() => {}} onFlightStarted={() => {}} />);
    await waitFor(() => expect(screen.getByText("D-AAAA")).toBeInTheDocument());
    await user.click(screen.getByText("D-AAAA"));
    await user.click(screen.getByRole("button", { name: i18next.t("manual_flight.next") }));
    await user.type(screen.getByPlaceholderText(i18next.t("manual_flight.form.block_fuel_placeholder")), "250");
    await user.type(screen.getByPlaceholderText(i18next.t("manual_flight.form.flight_time_placeholder")), "45");
    return user;
  }
  const startKnopf = () => screen.getByRole("button", { name: i18next.t("manual_flight.submit") });

  it("wartet mit dem Start, bis die Klassen da sind, und schickt dann Tarifzeilen", async () => {
    let liefern: (k: unknown) => void = () => {};
    klassenAntwort = () => new Promise((r) => (liefern = r));
    const user = await bisZumPlan();

    expect(startKnopf()).toBeDisabled();
    expect(screen.getByText(i18next.t("manual_flight.form.beladung_laedt"))).toBeInTheDocument();

    liefern(KLASSEN);
    await waitFor(() => expect(startKnopf()).toBeEnabled());
    await user.click(startKnopf());

    await waitFor(() => expect(invokeMock.mock.calls.some((c) => c[0] === "flight_start_manual")).toBe(true));
    const aufruf = invokeMock.mock.calls.find((c) => c[0] === "flight_start_manual");
    expect(aufruf?.[1].plan.beladung).toEqual([
      { fare_id: 6, anzahl: 0 },
      { fare_id: 7, anzahl: 0 },
    ]);
  });

  it("bleibt unter einem alten Client ohne den Befehl still", async () => {
    klassenAntwort = () => Promise.reject("Command vfr_beladungsklassen not found");
    await bisZumPlan();

    await waitFor(() => expect(startKnopf()).toBeEnabled());
    expect(screen.queryByText(i18next.t("manual_flight.form.beladung_fehler"))).not.toBeInTheDocument();
  });

  it("sperrt den Flug nicht, wenn die Klassen nicht laden, und kann es erneut versuchen", async () => {
    klassenAntwort = () => Promise.reject(new Error("offline"));
    const user = await bisZumPlan();

    await waitFor(() => expect(screen.getByText(i18next.t("manual_flight.form.beladung_fehler"))).toBeInTheDocument());
    expect(startKnopf()).toBeEnabled();

    klassenAntwort = () => Promise.resolve(KLASSEN);
    await user.click(screen.getByRole("button", { name: i18next.t("manual_flight.form.beladung_erneut") }));

    await waitFor(() => expect(screen.getByText("Economy", { exact: false })).toBeInTheDocument());
    expect(screen.queryByText(i18next.t("manual_flight.form.beladung_fehler"))).not.toBeInTheDocument();
  });
});
