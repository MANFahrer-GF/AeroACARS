// Der Banner nach einem unmöglichen Sprung — Anlass MSC1588 (22.09.2026).
//
// Damals gab die App den PIREP nach einem Simulator-Neustart am Zielflughafen
// VON SELBST ab. Seitdem entscheidet der Pilot. Geprüft wird hier an der
// echten Komponente, weil genau die Sichtbarkeit das Problem war: Ein Banner,
// der zur falschen Zeit erscheint (oder gar nicht), ist schlimmer als keiner.
import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { ActiveFlightInfo } from "../types";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => String(e),
}));

import { SprungBanner } from "./SprungBanner";

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

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

const FLUG = {
  pirep_id: "abc123",
  flight_number: "1588",
  callsign: null,
  airline_icao: "MSC",
  dpt_airport: "EDDN",
  arr_airport: "HECA",
  phase: "arrived",
  unmoeglicher_sprung: true,
  was_just_resumed: false,
  divert_hint: null,
} as unknown as ActiveFlightInfo;

function zeige(patch: Partial<ActiveFlightInfo> = {}) {
  return render(
    <SprungBanner
      activeFlight={{ ...FLUG, ...patch }}
      onFiledSuccess={() => {}}
      onDiscarded={() => {}}
    />,
  );
}

describe("Banner nach unmöglichem Sprung", () => {
  it("erscheint, wenn der Sprung erkannt wurde und der Flug angekommen ist", () => {
    zeige();
    expect(screen.getByTestId("sprung-banner")).toBeTruthy();
  });

  // Die vier Bedingungen einzeln — jede hat einen eigenen Grund, und eine
  // gemeinsame Prüfung würde verdecken, wenn eine davon wegfällt.
  it("bleibt weg, solange kein Sprung erkannt wurde", () => {
    zeige({ unmoeglicher_sprung: false });
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("bleibt weg, solange der Wiederaufnahme-Hinweis noch offen ist", () => {
    zeige({ was_just_resumed: true });
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("bleibt weg, solange der Flug noch laeuft", () => {
    zeige({ phase: "enroute" } as Partial<ActiveFlightInfo>);
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("tritt hinter einen Divert zurueck — zwei Banner widersprechen sich", () => {
    zeige({ divert_hint: { arr_airport: "HEBA" } } as unknown as Partial<ActiveFlightInfo>);
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("reicht die Begruendung des Piloten an flight_end weiter", async () => {
    zeige();
    await userEvent.type(
      screen.getByRole("textbox"),
      "  Sim abgestuerzt, danach neu geladen  ",
    );
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(invokeMock).toHaveBeenCalledWith("flight_end", {
      sprungBegruendung: "Sim abgestuerzt, danach neu geladen",
    });
  });

  it("schickt ohne Eingabe null statt eines leeren Textes", async () => {
    zeige();
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(invokeMock).toHaveBeenCalledWith("flight_end", { sprungBegruendung: null });
  });

  it("verwirft den Flug hart, damit kein halber PIREP zurueckbleibt", async () => {
    zeige();
    await userEvent.click(screen.getByText("Flug verwerfen"));
    expect(invokeMock).toHaveBeenCalledWith("flight_cancel", { force: true });
  });

  it("zeigt den Fehler an, statt ihn zu verschlucken", async () => {
    invokeMock.mockRejectedValue(new Error("phpVMS returned non-OK"));
    zeige();
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(screen.getByText(/phpVMS returned non-OK/)).toBeTruthy();
  });
});
