// v1.3.0 (#Hoppie-PDC-CPDLC) — connection-flow regression tests.
//
// The bug these pin down: blur-saving the callsign raced the connect
// click. The pilot typed a callsign, hit "start reception", and the
// backend read the *old* (empty) value and refused with "no callsign
// configured" — reported as "ACARS isn't logged on".

import { describe, it, expect, beforeAll, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => (e as { message?: string })?.message ?? String(e),
}));

import { CpdlcPanel } from "./CpdlcPanel";

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

const OFFLINE = {
  connected: false,
  logged_on: false,
  pending_response_count: 0,
  pending_uplink_count: 0,
  last_error: null,
  logon_verified: null,
  station_id: null,
};

/** Mirrors the backend: connect fails unless a callsign was persisted. */
function backend() {
  let stored: string | null = null;
  return (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "hoppie_get_settings":
        return Promise.resolve({
          enabled: true,
          callsign_override: stored,
          notify_sound: false,
        });
      case "hoppie_set_settings":
        stored = (args!.settings as { callsign_override: string | null }).callsign_override;
        return Promise.resolve(args!.settings);
      case "hoppie_get_flight_context":
        return Promise.resolve({ callsign: null, aircraft_type: null, dep_icao: null, dest_icao: null });
      case "hoppie_connect":
        if (!stored) {
          return Promise.reject({ code: "hoppie_no_callsign", message: "Kein Callsign hinterlegt." });
        }
        return Promise.resolve({ ...OFFLINE, connected: true, station_id: "SERVER" });
      case "hoppie_status":
        return Promise.resolve(stored ? OFFLINE : OFFLINE);
      case "hoppie_get_thread":
        return Promise.resolve([]);
      default:
        return Promise.resolve(undefined);
    }
  };
}

beforeEach(() => {
  invokeMock.mockReset();
  const impl = backend();
  invokeMock.mockImplementation((cmd: string, args?: Record<string, unknown>) => impl(cmd, args));
});

const t = (k: string) => i18next.t(k);

describe("CpdlcPanel connection flow", () => {
  it("persists a freshly typed callsign before connecting", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);

    const field = await screen.findByLabelText(t("cpdlc.callsign_label"));
    await userEvent.type(field, "gsg123");

    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.acars_start") }));

    await waitFor(() => {
      expect(
        invokeMock.mock.calls.some((c) => c[0] === "hoppie_connect"),
        "connect must actually be attempted",
      ).toBe(true);
    });

    const order = invokeMock.mock.calls.map((c) => c[0]);
    const save = order.lastIndexOf("hoppie_set_settings");
    const connect = order.indexOf("hoppie_connect");
    expect(save).toBeGreaterThan(-1);
    expect(
      save,
      "the callsign write must complete BEFORE connect, else the backend reads the old value",
    ).toBeLessThan(connect);

    // And it must have gone out normalized, not as typed.
    const saved = invokeMock.mock.calls.find((c) => c[0] === "hoppie_set_settings");
    expect(saved![1]).toMatchObject({ settings: { callsign_override: "GSG123" } });

    expect(await screen.findByText(t("cpdlc.acars_online"))).toBeInTheDocument();
  });

  it("surfaces a connect refusal instead of silently staying offline", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);
    await screen.findByLabelText(t("cpdlc.callsign_label"));

    // No callsign typed at all — the backend refuses.
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.acars_start") }));

    expect(await screen.findByText("Kein Callsign hinterlegt.")).toBeInTheDocument();
    expect(screen.getByText(t("cpdlc.acars_offline"))).toBeInTheDocument();
  });

  it("uppercases the callsign in the field itself, not just visually", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);
    const field = (await screen.findByLabelText(t("cpdlc.callsign_label"))) as HTMLInputElement;
    await userEvent.type(field, "gsg123");
    await userEvent.tab();
    await waitFor(() => expect(field.value).toBe("GSG123"));
  });
});
