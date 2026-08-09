// v1.5.0 (#msfs-hud, QS 09.08.2026): Der Panel-Server-Schalter ist ein
// Diagnose-Werkzeug für die ungeklärten Beta-Abstürze. Zwei Dinge dürfen
// nie kaputtgehen: (1) der gespeicherte Zustand wird beim Öffnen geladen
// und der Neustart-Hinweis erscheint ERST nach einer Änderung — sonst
// glaubt der Pilot bei jedem Öffnen, ein Neustart stünde aus; (2) ein
// Fehlschlag beim Schreiben zeigt die echte Fehlermeldung und lässt den
// Haken auf dem alten Stand, statt einen Zustand zu behaupten, der nicht
// gespeichert wurde.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: true,
  formatIpcError: (e: unknown) =>
    e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string"
      ? (e as { message: string }).message
      : String(e),
}));

import { MsfsHudPanel } from "./MsfsHudPanel";

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
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("MsfsHudPanel — Panel-Server-Schalter", () => {
  it("lädt den gespeicherten Zustand und zeigt den Neustart-Hinweis erst nach einer Änderung", async () => {
    invokeMock.mockImplementation(async (cmd: unknown, args?: unknown) => {
      if (cmd === "panel_server_get_enabled") return false;
      if (cmd === "panel_server_set_enabled")
        return (args as { enabled: boolean }).enabled;
      throw new Error(`unerwarteter Befehl: ${String(cmd)}`);
    });

    render(<MsfsHudPanel />);
    await flush();

    const box = screen.getByRole("checkbox") as HTMLInputElement;
    expect(box.checked).toBe(false);
    // Beim bloßen Öffnen: KEIN Neustart-Hinweis.
    expect(screen.queryByText(/nächsten Start/i)).toBeNull();

    fireEvent.click(box);
    await flush();

    expect(box.checked).toBe(true);
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === "panel_server_set_enabled" && (c[1] as { enabled: boolean }).enabled === true,
      ),
    ).toBe(true);
    expect(screen.getByText(/nächsten Start/i)).toBeTruthy();
  });

  it("zeigt beim Schreib-Fehlschlag die echte Meldung und behauptet keinen gespeicherten Zustand", async () => {
    invokeMock.mockImplementation(async (cmd: unknown) => {
      if (cmd === "panel_server_get_enabled") return true;
      if (cmd === "panel_server_set_enabled")
        throw { code: "panel_server_config", message: "kein Konfigurationsverzeichnis" };
      throw new Error(`unerwarteter Befehl: ${String(cmd)}`);
    });

    render(<MsfsHudPanel />);
    await flush();

    const box = screen.getByRole("checkbox") as HTMLInputElement;
    expect(box.checked).toBe(true);

    fireEvent.click(box);
    await flush();

    // Der Haken bleibt auf dem GESPEICHERTEN Stand …
    expect(box.checked).toBe(true);
    // … die echte Fehlermeldung ist sichtbar, kein "[object Object]" …
    expect(screen.getByText("kein Konfigurationsverzeichnis")).toBeTruthy();
    // … und kein irreführender Neustart-Hinweis.
    expect(screen.queryByText(/nächsten Start/i)).toBeNull();
  });
});
