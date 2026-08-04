// v0.19.x FIX: LogbookView.tsx and FlightProfile.tsx bypassed i18n
// entirely — every label, table header, error message, status badge and
// even the month abbreviations in date formatting were hardcoded German
// string literals, shown verbatim to English/Italian pilots regardless
// of their chosen locale. This pins that switching the active language
// actually changes the visible text (the bug: it never did, for either
// component).

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import { FlightProfile } from "./FlightProfile";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
vi.mock("maplibre-gl", () => ({
  default: { Map: class {}, Marker: class {}, LngLatBounds: class {}, NavigationControl: class {} },
}));
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));

import { LogbookView } from "./LogbookView";

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

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("FlightProfile — band titles follow the active locale", () => {
  it("shows German titles under de, different English titles under en", async () => {
    const route = [
      { t: 0, alt_ft: 1000 },
      { t: 1000, alt_ft: 2000 },
    ];

    await i18next.changeLanguage("de");
    const de = render(<FlightProfile route={route} />);
    expect(de.getByText("Höhe über Meer")).toBeInTheDocument();
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<FlightProfile route={route} />);
    expect(en.getByText("Altitude MSL")).toBeInTheDocument();
    expect(en.queryByText("Höhe über Meer")).not.toBeInTheDocument();
  });
});

describe("LogbookView — list chrome follows the active locale", () => {
  it("shows German labels under de, different English labels under en", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "logbook_stats") return Promise.resolve({ total_flights: 3 });
      if (cmd === "logbook_pireps") return Promise.resolve({ total: 0, items: [] });
      return Promise.resolve(undefined);
    });

    await i18next.changeLanguage("de");
    const de = render(<LogbookView />);
    await flush();
    expect(de.getByText("Logbuch")).toBeInTheDocument();
    expect(de.getByText("Stunden")).toBeInTheDocument();
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<LogbookView />);
    await flush();
    expect(en.getByText("Logbook")).toBeInTheDocument();
    expect(en.getByText("Hours")).toBeInTheDocument();
    expect(en.queryByText("Logbuch")).not.toBeInTheDocument();
    expect(en.queryByText("Stunden")).not.toBeInTheDocument();
  });

  it("localizes the status badge text, not just the CSS class", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "logbook_stats") return Promise.resolve({});
      if (cmd === "logbook_pireps") {
        return Promise.resolve({
          total: 1,
          items: [{ id: "X1", status: "accepted", callsign: "GSG1" }],
        });
      }
      return Promise.resolve(undefined);
    });

    await i18next.changeLanguage("en");
    render(<LogbookView />);
    await flush();

    // The old code rendered the raw English slug verbatim ("accepted")
    // regardless of locale — under EN that's indistinguishable from a
    // real fix, so this also checks under DE that it does NOT read the
    // raw slug but the localized word.
    await i18next.changeLanguage("de");
    cleanup();
    render(<LogbookView />);
    await flush();
    expect(screen.getByText("akzeptiert")).toBeInTheDocument();
    expect(screen.queryByText("accepted")).not.toBeInTheDocument();
  });
});
