// Thomas 31.10.2026: Unter Einstellungen › Cockpit soll der hinterlegte
// Hoppie-Logon-Code im Klartext stehen, damit man ihn nachpruefen kann.
// Ein Programm ohne `hoppie_get_logon_code` (aelter, LAN-Fernbedienung)
// lehnt den Aufruf ab — dann bleibt es beim Hinweis „hinterlegt“.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => String(e),
}));

import { HoppieSettingsPanel } from "./HoppieSettingsPanel";

const EIN = { enabled: true, callsign_override: null, station_id: "", notify_os: false, notify_sound: false };

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

afterEach(() => {
  cleanup();
  invokeMock.mockReset();
});

async function flush() {
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve();
  });
}

function backend(antworten: Record<string, () => unknown>) {
  invokeMock.mockImplementation((cmd: string) => {
    const a = antworten[cmd];
    return a ? Promise.resolve().then(a) : Promise.resolve(undefined);
  });
}

describe("HoppieSettingsPanel — Logon-Code im Klartext", () => {
  it("zeigt den hinterlegten Code im Klartext", async () => {
    backend({
      hoppie_get_settings: () => EIN,
      hoppie_has_logon_code: () => true,
      hoppie_get_logon_code: () => "ABC123xyz",
    });
    render(<HoppieSettingsPanel />);
    await flush();

    expect(screen.getByTestId("hoppie-stored-code")).toHaveTextContent("ABC123xyz");
    expect(screen.getByText("Hinterlegter Code:", { exact: false })).toBeInTheDocument();
    expect(screen.queryByText("Ein Logon-Code ist hinterlegt.")).not.toBeInTheDocument();
  });

  it("faellt auf „hinterlegt“ zurueck, wenn das Programm den Befehl nicht kennt", async () => {
    backend({
      hoppie_get_settings: () => EIN,
      hoppie_has_logon_code: () => true,
      hoppie_get_logon_code: () => {
        throw { code: "ipc", message: "Command hoppie_get_logon_code not found" };
      },
    });
    render(<HoppieSettingsPanel />);
    await flush();

    expect(screen.queryByTestId("hoppie-stored-code")).not.toBeInTheDocument();
    expect(screen.getByText("Ein Logon-Code ist hinterlegt.")).toBeInTheDocument();
  });

  it("zeigt den neuen Code nach dem Speichern, Eingabe ist kein Passwortfeld", async () => {
    let gespeichert: string | null = null;
    invokeMock.mockImplementation((cmd: string, args?: { code?: string }) => {
      if (cmd === "hoppie_set_logon_code") {
        gespeichert = args?.code ?? null;
        return Promise.resolve();
      }
      if (cmd === "hoppie_get_settings") return Promise.resolve(EIN);
      if (cmd === "hoppie_has_logon_code") return Promise.resolve(gespeichert !== null);
      if (cmd === "hoppie_get_logon_code") return Promise.resolve(gespeichert);
      return Promise.resolve(undefined);
    });
    render(<HoppieSettingsPanel />);
    await flush();
    expect(screen.queryByTestId("hoppie-stored-code")).not.toBeInTheDocument();

    const feld = screen.getByPlaceholderText("dein hoppie.nl-Logon-Code");
    expect(feld).toHaveAttribute("type", "text");
    fireEvent.change(feld, { target: { value: "NEU999" } });
    fireEvent.click(screen.getByRole("button", { name: deCommon.hoppie_settings.save_button }));
    await flush();

    expect(screen.getByTestId("hoppie-stored-code")).toHaveTextContent("NEU999");
  });
});
