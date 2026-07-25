// v1.2.3 (#Hoppie-PDC-CPDLC) — the indicator must not overstate what it knows.
//
// "Couldn't reach the network" and "no controller is online" look the
// same in the data (both `online: false`) but mean very different things
// to a pilot deciding whether to send. They must never be shown alike.

import { describe, it, expect, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { StationBadge } from "./StationBadge";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

describe("StationBadge", () => {
  it("shows nothing before the first check", () => {
    const { container } = render(<StationBadge status={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("reports an online station", () => {
    render(<StationBadge status={{ station: "EDDF", online: true, reason: null }} />);
    expect(screen.getByText(/EDDF ist erreichbar/)).toBeInTheDocument();
  });

  // The wording must not overstate: a controller's datalink callsign is
  // free-text on their side and advertised in the ATIS, so "nothing under
  // EDDF" is not the same as "nobody is working EDDF".
  it("says nothing is registered, not that nobody is there", () => {
    render(<StationBadge status={{ station: "EDDF", online: false, reason: null }} />);
    expect(screen.getByText(/Unter EDDF ist nichts registriert/)).toBeInTheDocument();
    expect(screen.getByText(/ATIS/)).toBeInTheDocument();
  });

  it("says 'could not check' instead of claiming offline when the check failed", () => {
    render(
      <StationBadge status={{ station: "EDDF", online: false, reason: "network error" }} />,
    );
    expect(screen.getByText(/konnte nicht geprüft werden/)).toBeInTheDocument();
    expect(
      screen.queryByText(/ist nichts registriert/),
      "a failed check must not be reported as an empty position",
    ).not.toBeInTheDocument();
  });
});
