// v0.19.x FIX: a rejected Tauri command resolves with the deserialized
// UiError object ({code, message}), not a JS Error. `setError(String(e))`
// stringified that object to the useless literal "[object Object]" —
// the pilot saw that instead of the actual failure reason. Switched to
// the shared formatIpcError() helper (already used correctly elsewhere,
// e.g. HoppieSettingsPanel). This pins that the real message now shows.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) =>
    e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string"
      ? (e as { message: string }).message
      : String(e),
}));

import { DiscordRpcPanel } from "./DiscordRpcPanel";

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

afterEach(() => cleanup());

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("DiscordRpcPanel — error display", () => {
  it("surfaces a failed initial load instead of a permanently stuck loading spinner", async () => {
    // Both bugs fixed together: (1) the early `if (!settings) return` used
    // to ALWAYS render the loading text, so setError() from the initial
    // fetch had nowhere to show up; (2) String(e) on the rejected UiError
    // object rendered the useless literal "[object Object]".
    invokeMock.mockRejectedValueOnce({ code: "discord_ipc_error", message: "Discord ist nicht erreichbar." });
    render(<DiscordRpcPanel />);
    await flush();

    expect(screen.getByText("Discord ist nicht erreichbar.")).toBeInTheDocument();
    expect(screen.queryByText("Lade Discord-RPC-Status …")).not.toBeInTheDocument();
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
  });

  it("shows the actual UiError message for a later action, not '[object Object]'", async () => {
    invokeMock.mockResolvedValueOnce({ enabled: false, anonymize_callsign: false, show_profile_button: false });
    invokeMock.mockResolvedValueOnce({
      status: "disabled",
      last_connect_attempt_at: null,
      last_update_at: null,
      client_id: "",
      error_message: null,
    });
    render(<DiscordRpcPanel />);
    await flush();

    invokeMock.mockRejectedValueOnce({ code: "discord_ipc_error", message: "Discord-Verbindung fehlgeschlagen." });
    await act(async () => {
      fireEvent.click(screen.getAllByRole("checkbox")[0]);
    });
    await flush();

    expect(screen.getByText("Discord-Verbindung fehlgeschlagen.")).toBeInTheDocument();
  });
});
