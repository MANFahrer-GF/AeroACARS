// „Ordner wählen …" (X-Plane-Hauptordner per System-Dialog): Wählt der Pilot
// einen Unterordner, landet der geprüfte Hauptordner im Feld; liegt dort kein
// X-Plane, bleibt seine Wahl stehen und ein Hinweis erklärt, was fehlt.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
const openMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openMock(...args),
}));

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
  openMock.mockReset();
});

async function flush() {
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve();
  });
}

function befehle(root: string | null) {
  invokeMock.mockImplementation(async (cmd: unknown, args?: unknown) => {
    if (cmd === "xplane_premium_status")
      return { active: false, ever_seen: false, packet_count: 0, last_error: null };
    if (cmd === "xplane_detect_install_path") return null;
    if (cmd === "xplane_check_install_path") {
      expect((args as { path: string }).path).toBe("/Spiele/X-Plane 12/Resources");
      return root;
    }
    throw new Error(`unerwarteter Befehl: ${String(cmd)}`);
  });
}

describe("X-Plane-Hauptordner per Dialog", () => {
  it("geht vom gewählten Unterordner zum Hauptordner", async () => {
    befehle("/Spiele/X-Plane 12");
    openMock.mockResolvedValue("/Spiele/X-Plane 12/Resources");
    render(<XPlanePremiumPanel simState="disconnected" />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Ordner wählen …" }));
    await flush();
    expect((screen.getByLabelText("X-Plane-Hauptordner") as HTMLInputElement).value).toBe(
      "/Spiele/X-Plane 12",
    );
    expect(openMock).toHaveBeenCalledWith(expect.objectContaining({ directory: true }));
  });

  it("meldet, wenn im gewählten Ordner kein X-Plane liegt", async () => {
    befehle(null);
    openMock.mockResolvedValue("/Spiele/X-Plane 12/Resources");
    render(<XPlanePremiumPanel simState="disconnected" />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Ordner wählen …" }));
    await flush();
    expect(screen.getByText(/In diesem Ordner liegt kein X-Plane/)).toBeTruthy();
  });

  it("Abbrechen im Dialog ändert nichts", async () => {
    befehle("/egal");
    openMock.mockResolvedValue(null);
    render(<XPlanePremiumPanel simState="disconnected" />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Ordner wählen …" }));
    await flush();
    expect(invokeMock).not.toHaveBeenCalledWith("xplane_check_install_path", expect.anything());
  });
});
