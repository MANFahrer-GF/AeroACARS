// v0.19.x FIX: the backend can rotate the LAN-remote pairing PIN on its
// own (remote/auth.rs's bad-guess backstop) with no push event — only
// remote_server_status reflects the new value. The panel used to fetch
// status once on mount and never again, so a rotated PIN stayed on
// screen forever. This pins the poll-while-running fix, and that the
// poll can't clobber an in-progress port edit (the same class of bug
// fixed in ManualFlightModal for aircraft selection).

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
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
