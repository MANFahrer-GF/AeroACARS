// Plugin 1.1.0 kann das Band, ist aber älter als das Plugin dieser App
// (1.1.1: Band auf zweitem Monitor, Feldbefund 05.10.2026). Ohne Hinweis und
// Installationsfeld erfährt ein Pilot mit 1.1.0 nie vom Fix.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: true,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { XPlanePremiumPanel } from "./XPlanePremiumPanel";

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

function mitPlugin(version: string) {
  invokeMock.mockImplementation(async (cmd: unknown) => {
    if (cmd === "xplane_premium_status")
      return {
        active: true,
        ever_seen: true,
        packet_count: 10,
        last_error: null,
        protokoll: 2,
        plugin_version: version,
        veraltet: false,
      };
    if (cmd === "xplane_detect_install_path") return "/Spiele/X-Plane 12";
    throw new Error(`unerwarteter Befehl: ${String(cmd)}`);
  });
}

describe("Plugin-Update-Hinweis", () => {
  it("bietet Plugin 1.1.0 das Update samt Installationsfeld an", async () => {
    mitPlugin("1.1.0");
    render(<XPlanePremiumPanel simState="connected" />);
    await flush();
    expect(screen.getByText(/Plugin-Update verfügbar \(1\.1\.0 → 1\.1\.1\)/)).toBeTruthy();
    expect(screen.getByText(/zweiten Monitor/)).toBeTruthy();
    expect(screen.getByLabelText("X-Plane-Hauptordner")).toBeTruthy();
    // Kein doppelter Hinweis: das Band selbst kann 1.1.0.
    expect(screen.queryByText(/Plugin ohne HUD-Band/)).toBeNull();
  });

  it("schweigt beim aktuellen Plugin und zeigt kein Installationsfeld", async () => {
    mitPlugin("1.1.1");
    render(<XPlanePremiumPanel simState="connected" />);
    await flush();
    expect(screen.queryByText(/Plugin-Update verfügbar/)).toBeNull();
    expect(screen.queryByLabelText("X-Plane-Hauptordner")).toBeNull();
  });
});
