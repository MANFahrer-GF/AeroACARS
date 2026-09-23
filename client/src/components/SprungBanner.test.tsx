// Der Banner nach einem unmöglichen Sprung — Anlass MSC1588 (22.09.2026).
//
// Damals gab die App den PIREP nach einem Simulator-Neustart am Zielflughafen
// VON SELBST ab. Seitdem entscheidet der Pilot. Geprüft wird hier an der
// echten Komponente, weil genau die Sichtbarkeit das Problem war: Ein Banner,
// der zur falschen Zeit erscheint (oder gar nicht), ist schlimmer als keiner.
import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, render, screen } from "@testing-library/react";
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
  abgabe_sperre: "sprung",
  was_just_resumed: false,
  divert_hint: null,
} as unknown as ActiveFlightInfo;

const eingereicht = vi.fn();
const verworfen = vi.fn();

function zeige(patch: Partial<ActiveFlightInfo> = {}) {
  eingereicht.mockReset();
  verworfen.mockReset();
  return render(
    <SprungBanner
      activeFlight={{ ...FLUG, ...patch }}
      onFiledSuccess={eingereicht}
      onDiscarded={verworfen}
    />,
  );
}

/** Verwerfen fragt seit 23.09.2026 nach — erst der zweite Klick zaehlt. */
async function verwerfen() {
  await userEvent.click(screen.getByText("Flug verwerfen"));
  await userEvent.click(screen.getByText("Ja, Flug verwerfen"));
}

describe("Banner nach unmöglichem Sprung", () => {
  it("erscheint, wenn der Sprung erkannt wurde und der Flug angekommen ist", () => {
    zeige();
    expect(screen.getByTestId("sprung-banner")).toBeTruthy();
  });

  // Die vier Bedingungen einzeln — jede hat einen eigenen Grund, und eine
  // gemeinsame Prüfung würde verdecken, wenn eine davon wegfällt.
  it("bleibt weg, solange kein Sprung erkannt wurde", () => {
    zeige({ unmoeglicher_sprung: false, abgabe_sperre: null });
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("versteht auch einen aelteren Client ohne den neuen Grund", () => {
    // Die LAN-Bruecke kann eine aeltere Oberflaeche bedienen: Dort kommt
    // nur `unmoeglicher_sprung`, kein `abgabe_sperre`.
    zeige({ abgabe_sperre: undefined });
    expect(screen.getByTestId("sprung-banner")).toBeTruthy();
  });

  it("bleibt weg, solange der Wiederaufnahme-Hinweis noch offen ist", () => {
    zeige({ was_just_resumed: true });
    expect(screen.queryByTestId("sprung-banner")).toBeNull();
  });

  it("bleibt weg, solange der Flug noch laeuft", () => {
    // Echte Phasen aus FlightPhase — ein ausgedachter Wert wie „enroute"
    // haette auch eine umgedrehte Bedingung gruen gelassen
    // (Cloud-QS 23.09.2026).
    for (const phase of ["cruise", "approach", "landing", "taxi_in", "blocks_on"] as const) {
      const { unmount } = zeige({ phase });
      expect(screen.queryByTestId("sprung-banner")).toBeNull();
      unmount();
    }
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
    // Das Elternteil muss erfahren, dass eingereicht wurde — sonst bleibt
    // der Flug auf dem Schirm stehen und das Erfolgsbanner kommt nie.
    expect(eingereicht).toHaveBeenCalledTimes(1);
    expect(eingereicht.mock.calls[0][0]).toMatchObject({ kind: "filed", dpt: "EDDN", arr: "HECA" });
  });

  it("schickt ohne Eingabe null statt eines leeren Textes", async () => {
    zeige();
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(invokeMock).toHaveBeenCalledWith("flight_end", { sprungBegruendung: null });
  });

  it("setzt den Fokus nach der Rückfrage auf den sicheren Weg", async () => {
    // React tauscht die Taste aus; ohne gezielten Fokus landete der
    // Tastaturnutzer am Seitenanfang (Cloud-QS 23.09.2026).
    zeige();
    await userEvent.click(screen.getByText("Flug verwerfen"));
    expect(document.activeElement).toBe(screen.getByText("Doch nicht"));
  });

  it("fragt vor dem Verwerfen nach — ein Fehlklick kostet den ganzen Flug", async () => {
    zeige();
    await userEvent.click(screen.getByText("Flug verwerfen"));
    expect(invokeMock).not.toHaveBeenCalled();
    // Und der Rueckzieher bringt die urspruenglichen Tasten zurueck.
    await userEvent.click(screen.getByText("Doch nicht"));
    expect(screen.getByText("Trotzdem einreichen")).toBeTruthy();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("verwirft den Flug hart, damit kein halber PIREP zurueckbleibt", async () => {
    zeige();
    await verwerfen();
    expect(invokeMock).toHaveBeenCalledWith("flight_cancel", { force: true });
    expect(verworfen).toHaveBeenCalledTimes(1);
  });

  it("behandelt einen schon weggeraeumten Flug als verworfen, nicht als Fehler", async () => {
    invokeMock.mockRejectedValue(new Error("no_active_flight"));
    zeige();
    await verwerfen();
    expect(verworfen).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/no_active_flight/)).toBeNull();
  });

  it("nennt den Grund, aus dem gefragt wird", () => {
    // Zwei Gründe, zwei Texte — sonst stünde bei GAF 9655 (23.09.2026)
    // „der Zustand sprang", obwohl nichts gesprungen ist: Dort fehlte die
    // gemessene Landung nach einer Unterbrechung.
    zeige({ abgabe_sperre: "sprung" });
    expect(screen.getByText("Flug wurde so nicht fortgesetzt")).toBeTruthy();
    expect(screen.queryByText("Landung konnte nicht bewertet werden")).toBeNull();
    cleanup();

    zeige({ abgabe_sperre: "landung_fehlt", unmoeglicher_sprung: false });
    expect(screen.getByText("Landung konnte nicht bewertet werden")).toBeTruthy();
    expect(screen.queryByText("Flug wurde so nicht fortgesetzt")).toBeNull();
  });

  it("nimmt den gemeldeten Grund, auch wenn der alte Schalter noch steht", () => {
    // Sind beide gesetzt, gilt der neue Grund — sonst stünde „der Zustand
    // sprang", wo die fehlende Landung gemeint ist.
    zeige({ abgabe_sperre: "landung_fehlt", unmoeglicher_sprung: true });
    expect(screen.getByText("Landung konnte nicht bewertet werden")).toBeTruthy();
    expect(screen.queryByText("Flug wurde so nicht fortgesetzt")).toBeNull();
  });

  it("behauptet bei einem unbekannten Grund keinen Sprung", () => {
    // Ein neueres Backend oder die LAN-Brücke kann einen Code schicken,
    // den diese Oberfläche nicht kennt. Dann fragt sie trotzdem — aber
    // ohne eine Ursache zu erfinden (Cloud-QS 23.09.2026).
    zeige({ abgabe_sperre: "etwas_neues" } as unknown as Partial<ActiveFlightInfo>);
    expect(screen.getByTestId("sprung-banner")).toBeTruthy();
    expect(screen.queryByText("Landung konnte nicht bewertet werden")).toBeNull();
  });

  it("beschriftet die Taste waehrend der Abgabe um", async () => {
    // Sie blieb frueher stumm — der Pilot klickte nach, weil nichts
    // passierte (Cloud-QS 23.09.2026).
    let loesen: (() => void) | null = null;
    invokeMock.mockImplementation(
      () =>
        new Promise<void>((r) => {
          loesen = r;
        }),
    );
    zeige();
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(screen.queryByText("Trotzdem einreichen")).toBeNull();
    expect(screen.getByText("einen Moment …")).toBeTruthy();
    loesen?.();
  });

  it("zeigt den Fehler an, statt ihn zu verschlucken", async () => {
    invokeMock.mockRejectedValue(new Error("phpVMS returned non-OK"));
    zeige();
    await userEvent.click(screen.getByText("Trotzdem einreichen"));
    expect(screen.getByText(/phpVMS returned non-OK/)).toBeTruthy();
    // Und der Flug darf NICHT als eingereicht gelten.
    expect(eingereicht).not.toHaveBeenCalled();
  });
});

describe("Wo der Banner hängt", () => {
  const ohneLeerraum = (x: string) => x.replace(/\s+/g, "");

  it("hängt im App-Rahmen, nicht im Cockpit-Reiter", () => {
    // Wer nach der Landung ins Logbuch oder in den Chat wechselte, sah
    // die Frage nie — und der Flug blieb offen (Cloud-QS 23.09.2026).
    const app = ohneLeerraum(readFileSync(resolve(__dirname, "../App.tsx"), "utf-8"));
    const cockpit = ohneLeerraum(readFileSync(resolve(__dirname, "CockpitView.tsx"), "utf-8"));
    expect(app.includes("<SprungBanner")).toBe(true);
    expect(cockpit.includes("<SprungBanner")).toBe(false);
  });

  it("ist an keinen Reiter gebunden", () => {
    const app = ohneLeerraum(readFileSync(resolve(__dirname, "../App.tsx"), "utf-8"));
    const stelle = app.indexOf("<SprungBanner");
    // Die Bedingung unmittelbar davor darf keinen `tab===` enthalten.
    const davor = app.slice(Math.max(0, stelle - 120), stelle);
    expect(davor.includes("tab===")).toBe(false);
  });
});
