// v0.19.x FIX: the backend can rotate the LAN-remote pairing PIN on its
// own (remote/auth.rs's bad-guess backstop) with no push event — only
// remote_server_status reflects the new value. The panel used to fetch
// status once on mount and never again, so a rotated PIN stayed on
// screen forever. This pins the poll-while-running fix, and that the
// poll can't clobber an in-progress port edit (the same class of bug
// fixed in ManualFlightModal for aircraft selection).

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { RemoteServerStatus } from "../types";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: true,
}));

import { RemoteServerPanel } from "./RemoteServerPanel";

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
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function status(overrides: Partial<RemoteServerStatus> = {}): RemoteServerStatus {
  return {
    running: true,
    port: 8765,
    urls: ["http://192.168.1.50:8765"],
    pin: "111111",
    qr_svg: "",
    ...overrides,
  };
}

/** Flush the microtask queue enough times for a chained `invoke().then(...)`
 *  (mount effect) to settle, without relying on waitFor's own timer-based
 *  polling — which deadlocks once fake timers are active. */
async function flushMountEffect() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("RemoteServerPanel — PIN stays in sync with backend rotation", () => {
  it("picks up a backend-rotated PIN via polling, without user interaction", async () => {
    // Fake timers must be active BEFORE the running-effect registers its
    // window.setInterval, or that interval binds to the real clock and
    // advanceTimersByTimeAsync below never touches it.
    vi.useFakeTimers();
    invokeMock.mockResolvedValueOnce(status({ pin: "111111" }));
    render(<RemoteServerPanel />);
    await flushMountEffect();
    expect(screen.getByText("111111")).toBeInTheDocument();

    invokeMock.mockResolvedValueOnce(status({ pin: "999999" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(screen.getByText("999999")).toBeInTheDocument();
  });

  it("does not touch the port input while the pilot is mid-edit", async () => {
    vi.useFakeTimers();
    invokeMock.mockResolvedValueOnce(status({ port: 8765 }));
    render(<RemoteServerPanel />);
    await flushMountEffect();
    expect(screen.getByText("111111")).toBeInTheDocument();

    const portField = screen.getByDisplayValue("8765") as HTMLInputElement;
    // Pilot starts typing a new port but hasn't blurred yet.
    await act(async () => {
      fireEvent.change(portField, { target: { value: "999" } });
    });

    invokeMock.mockResolvedValueOnce(status({ port: 8765, pin: "222222" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(screen.getByText("222222")).toBeInTheDocument();
    expect(portField.value).toBe("999");
  });
});

// v0.19.x FIX: a paired tablet's bearer token used to be permanent — a
// lost/stolen tablet, or a device that guessed a leaked PIN before the
// rate-limit backstop caught it, kept working forever with no way to cut
// it off. These pin the new "disconnect all paired devices" action:
// requires confirmation, calls the revoke command, and refreshes status.
describe("RemoteServerPanel — revoke pairing", () => {
  it("asks for confirmation and, once confirmed, calls remote_server_revoke_pairing", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValueOnce(status({ pin: "111111" }));
    render(<RemoteServerPanel />);
    await flushMountEffect();

    await user.click(screen.getByRole("button", { name: "Geräte trennen" }));
    // Confirm dialog is open with the default confirm label.
    const confirmButton = await screen.findByRole("button", { name: "Bestätigen" });

    invokeMock.mockResolvedValueOnce(status({ pin: "111111" }));
    await user.click(confirmButton);

    await flushMountEffect();
    expect(invokeMock.mock.calls.some((c) => c[0] === "remote_server_revoke_pairing")).toBe(true);
  });

  it("does NOT call the revoke command when the pilot cancels", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValueOnce(status({ pin: "111111" }));
    render(<RemoteServerPanel />);
    await flushMountEffect();

    await user.click(screen.getByRole("button", { name: "Geräte trennen" }));
    // Two "Abbrechen" buttons exist (the modal's ✕ close via aria-label,
    // and the footer Cancel button) — the footer one is the last match.
    const cancelButtons = await screen.findAllByRole("button", { name: "Abbrechen" });
    await user.click(cancelButtons[cancelButtons.length - 1]);

    expect(invokeMock.mock.calls.some((c) => c[0] === "remote_server_revoke_pairing")).toBe(false);
  });
});
