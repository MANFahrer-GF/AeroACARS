// v0.19.x FIX: two bugs in this Settings → Debug companion panel.
// (1) setError(String(err)) stringified a rejected Tauri command's
// {code, message} object to the useless literal "[object Object]" —
// same class of bug already fixed in DiscordRpcPanel/ReleaseNotesModal.
// (2) the header/status line/table headers were hardcoded English,
// never routed through t(), shown even under DE/IT.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
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

import { XPlaneInspector } from "./XPlaneInspector";

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
  });
}

describe("XPlaneInspector — error display", () => {
  it("shows the actual UiError message, not '[object Object]'", async () => {
    invokeMock.mockRejectedValue({ code: "xplane_udp_error", message: "X-Plane UDP-Feed nicht erreichbar." });
    render(<XPlaneInspector />);
    await flush();

    expect(screen.getByText("X-Plane UDP-Feed nicht erreichbar.")).toBeInTheDocument();
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
  });
});

describe("XPlaneInspector — status line and table headers follow the active locale", () => {
  it("shows the localized status line with live/missing/total counts", async () => {
    invokeMock.mockResolvedValue([
      { index: 0, name: "sim/flightmodel/position/y_agl", value: 1200, has_value: true },
      { index: 1, name: "sim/cockpit2/gauges/actuators/gear", value: 0, has_value: false },
    ]);
    render(<XPlaneInspector />);
    await flush();

    expect(screen.getByText("1 live · 1 fehlend von 2")).toBeInTheDocument();
    expect(screen.getByText("DataRef")).toBeInTheDocument();
    expect(screen.getByText("Wert")).toBeInTheDocument();
  });
});
