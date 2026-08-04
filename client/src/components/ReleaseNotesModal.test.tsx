// v0.19.x FIX: fetch_release_notes rejects with the deserialized Tauri
// UiError object ({code, message}), not a JS Error — String(err) on that
// object stringifies to "[object Object]", which never contains
// "not_found". The old `String(e).includes("not_found")` check was dead:
// EVERY failure, including a genuinely missing release, fell through to
// the generic "offline?" message. These pin the fix: the right message
// shows for each error code.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { ReleaseNotesModal } from "./ReleaseNotesModal";

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

describe("ReleaseNotesModal — error-code handling", () => {
  it("shows the 'no release yet' message for a not_found UiError (the exact bug)", async () => {
    invokeMock.mockRejectedValueOnce({ code: "not_found", message: "no release with tag v9.9.9" });
    render(<ReleaseNotesModal version="9.9.9" onClose={() => {}} />);
    await flush();

    expect(
      screen.getByText("Für diese Version existieren noch keine Release-Notes auf GitHub."),
    ).toBeInTheDocument();
  });

  it("shows the generic offline message for a real network/github error", async () => {
    invokeMock.mockRejectedValueOnce({ code: "network", message: "connection refused" });
    render(<ReleaseNotesModal version="1.2.3" onClose={() => {}} />);
    await flush();

    expect(
      screen.getByText("Konnte die Release-Notes nicht von GitHub laden — bist du offline?"),
    ).toBeInTheDocument();
  });
});
