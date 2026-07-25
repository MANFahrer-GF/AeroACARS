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
    expect(screen.getByText("erreichbar")).toBeInTheDocument();
    expect(screen.getByTitle(/EDDF ist erreichbar/)).toBeInTheDocument();
  });

  // The wording must not overstate: a controller's datalink callsign is
  // free-text on their side and advertised in the ATIS, so "nothing under
  // EDDF" is not the same as "nobody is working EDDF".
  it("says nothing is registered, not that nobody is there", () => {
    render(<StationBadge status={{ station: "EDDF", online: false, reason: null }} />);
    expect(screen.getByText("nicht registriert")).toBeInTheDocument();
    const full = screen.getByTitle(/Unter EDDF ist nichts registriert/);
    expect(full).toHaveAttribute("title", expect.stringContaining("ATIS"));
  });

  it("says 'could not check' instead of claiming offline when the check failed", () => {
    render(
      <StationBadge status={{ station: "EDDF", online: false, reason: "network error" }} />,
    );
    expect(screen.getByText("nicht prüfbar")).toBeInTheDocument();
    expect(screen.getByTitle(/konnte nicht geprüft werden/)).toBeInTheDocument();
    expect(
      screen.queryByTitle(/ist nichts registriert/),
      "a failed check must not be reported as an empty position",
    ).not.toBeInTheDocument();
  });

  // Layout guard: the badge shares its flex column with the station input,
  // so a long visible label becomes the field's width — that is what pushed
  // the PDC send buttons off the bottom of the panel. The explanation lives
  // in the tooltip; what is PAINTED must stay short.
  it("keeps the visible label short so it can't stretch the input", () => {
    for (const status of [
      { station: "EDDF", online: true, reason: null },
      { station: "EDDF", online: false, reason: null },
      { station: "EDDF", online: false, reason: "network error" },
    ]) {
      const { container, unmount } = render(<StationBadge status={status} />);
      const visible = (container.textContent ?? "").trim();
      expect(
        visible.length,
        `visible badge text "${visible}" is long enough to widen the field`,
      ).toBeLessThanOrEqual(20);
      unmount();
    }
  });
});
