// X-Plane-Band-Schalter (ADR-0005): lädt den gespeicherten Zustand (Standard
// an), schreibt die Änderung und zeigt bei Fehlschlag die echte Meldung,
// ohne einen Zustand zu behaupten, der nicht gespeichert wurde.

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

import { XplaneBandPanel } from "./XplaneBandPanel";

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

describe("XplaneBandPanel", () => {
  it("lädt den Zustand und schreibt die Änderung", async () => {
    invokeMock.mockImplementation(async (cmd: unknown, args?: unknown) => {
      if (cmd === "xplane_band_get_enabled") return true;
      if (cmd === "xplane_band_set_enabled") return (args as { enabled: boolean }).enabled;
      throw new Error(`unerwarteter Befehl: ${String(cmd)}`);
    });
    render(<XplaneBandPanel />);
    await flush();
    const box = screen.getByRole("checkbox") as HTMLInputElement;
    expect(box.checked).toBe(true);
    fireEvent.click(box);
    await flush();
    expect(box.checked).toBe(false);
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === "xplane_band_set_enabled" && (c[1] as { enabled: boolean }).enabled === false,
      ),
    ).toBe(true);
  });

  it("zeigt den Schreibfehler und lässt den Haken stehen", async () => {
    invokeMock.mockImplementation(async (cmd: unknown) => {
      if (cmd === "xplane_band_get_enabled") return true;
      throw { code: "xplane_band_config", message: "kein Konfigurationsverzeichnis" };
    });
    render(<XplaneBandPanel />);
    await flush();
    const box = screen.getByRole("checkbox") as HTMLInputElement;
    fireEvent.click(box);
    await flush();
    expect(box.checked).toBe(true);
    expect(screen.getByText(/kein Konfigurationsverzeichnis/)).toBeTruthy();
  });
});
