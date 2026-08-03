// v1.3.5 (#Datalink-3a) — connection-flow regression tests.
//
// The bug these pin down: blur-saving the callsign raced the connect
// click. The pilot typed a callsign, hit "start reception", and the
// backend read the *old* (empty) value and refused with "no callsign
// configured" — reported as "ACARS isn't logged on". Still applies after
// the rebuild: the callsign is now edit-toggled (README §2b, "Ändern"),
// so these tests open that editor first.

import { describe, it, expect, beforeAll, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
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
  station_id: null,
  logon_pending: false,
  logon_timed_out: false,
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
        return Promise.resolve(OFFLINE);
      case "hoppie_get_thread":
        return Promise.resolve([]);
      case "hoppie_list_elements":
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

const t = (k: string, opts?: Record<string, unknown>) => i18next.t(k, opts);

/** Opens the callsign editor and returns the now-visible input. Scoped to
 *  the status row: the composer's read-only CALLSIGN mirror (README
 *  §3.3's PDC grid) shares the exact same label text. */
async function openCallsignEditor() {
  await userEvent.click(await screen.findByRole("button", { name: t("cpdlc.callsign_change") }));
  const statusRow = document.querySelector(".datalink-status")!;
  return within(statusRow as HTMLElement).findByLabelText(t("cpdlc.callsign_label"));
}

describe("CpdlcPanel connection flow", () => {
  it("persists a freshly typed callsign before connecting", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);

    const field = await openCallsignEditor();
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
    await screen.findByText(t("cpdlc.acars_offline"));

    // No callsign typed at all — the backend refuses.
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.acars_start") }));

    expect(await screen.findByText("Kein Callsign hinterlegt.")).toBeInTheDocument();
    expect(screen.getByText(t("cpdlc.acars_offline"))).toBeInTheDocument();
  });

  it("uppercases the callsign in the field itself, not just visually", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);
    const field = (await openCallsignEditor()) as HTMLInputElement;
    await userEvent.type(field, "gsg123");
    await userEvent.tab();
    const statusRow = document.querySelector(".datalink-status")!;
    await waitFor(() => expect(within(statusRow as HTMLElement).getByText("GSG123")).toBeInTheDocument());
  });

  it("renders the PDC/CPDLC mode toggle as tabs in the composer", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);
    await screen.findByText(t("cpdlc.acars_offline"));
    expect(screen.getByRole("tab", { name: t("cpdlc.mode_pdc") })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: t("cpdlc.mode_cpdlc") })).toBeInTheDocument();
  });

  // Field bug (2026-08-03): the CPDLC-logon block used to sit dimmed-but-
  // visible in the shared status row for both modes, cramming CPDLC-only
  // content into a row that also had to carry always-relevant things
  // (connection state, callsign) — the direct cause of three separate
  // layout bugs in one afternoon (a CSS specificity collision on the
  // input's width, the poll-age readout pushed off-canvas, a hyphen in
  // "CPDLC-LOGON" wrapping the label). It now lives in the composer,
  // rendered only in CPDLC mode — simply absent in PDC mode, not dimmed.
  it("shows CPDLC-logon only in CPDLC mode, not dimmed in PDC mode", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);
    await screen.findByText(t("cpdlc.acars_offline"));
    expect(screen.queryByText(t("cpdlc.logon_label"))).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: t("cpdlc.mode_cpdlc") }));
    const label = await screen.findByText(t("cpdlc.logon_label"));
    expect(label.closest(".datalink-logon")).not.toHaveClass("datalink-block--dim");
    expect(label.closest(".datalink-logon")).not.toHaveAttribute("title");
  });

  // Field bug (found in live testing, 2026-08-03): the station field was
  // gated behind "Wechseln", which itself only appears once a logon
  // already exists or was attempted — a pilot who had never logged on to
  // anything saw a bare, un-editable "—" and no way to type a centre at
  // all. The field must be directly reachable with nothing sent yet.
  it("lets a pilot type a centre and log on with no prior session — no Wechseln detour", async () => {
    render(<CpdlcPanel onOpenSettings={() => {}} />);

    // Connect first — logging on to a centre needs ACARS up regardless
    // of the field-reachability bug this test is really about. The logon
    // UI itself now only renders in CPDLC mode (it moved out of the
    // shared status row into the composer — see DatalinkComposer.tsx).
    const field = await openCallsignEditor();
    await userEvent.type(field, "gsg123");
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.acars_start") }));
    await screen.findByText(t("cpdlc.acars_online"));
    await userEvent.click(screen.getByRole("tab", { name: t("cpdlc.mode_cpdlc") }));
    await screen.findByText(t("cpdlc.logon_none"));

    expect(screen.queryByRole("button", { name: t("cpdlc.logon_switch") })).not.toBeInTheDocument();
    const stationInput = screen.getByPlaceholderText(t("cpdlc.center_placeholder"));
    expect(stationInput).toBeInTheDocument();

    await userEvent.type(stationInput, "eddf_gnd");
    const logonButton = screen.getByRole("button", { name: t("cpdlc.logon_send") });
    expect(logonButton).toBeEnabled();

    await userEvent.click(logonButton);
    await waitFor(() =>
      expect(invokeMock.mock.calls.some((c) => c[0] === "hoppie_send_logon_request")).toBe(true),
    );
    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_logon_request");
    expect(call![1]).toMatchObject({ station: "EDDF_GND" });
  });

  // Field bug (2026-08-03): the LOGON field's typed-but-unsent draft lived in
  // `localStorage` with no expiry and no clear-on-send — a station typed once
  // (e.g. during testing) kept resurfacing forever, forcing the pilot to
  // delete it every session. Fixed to `sessionStorage` (still survives a tab
  // switch/remount within one running app instance — the original point of
  // the cache — but not an app restart) plus an explicit clear once a logon
  // request actually goes out.
  it("ignores a stale localStorage draft and clears the sessionStorage draft once logon is sent", async () => {
    const STATION_DRAFT_KEY = "aeroacars.cpdlc.station_draft";
    sessionStorage.removeItem(STATION_DRAFT_KEY);
    localStorage.setItem(STATION_DRAFT_KEY, "EDDF"); // the old bug's kind of leftover

    try {
      render(<CpdlcPanel onOpenSettings={() => {}} />);
      const field = await openCallsignEditor();
      await userEvent.type(field, "gsg123");
      await userEvent.click(screen.getByRole("button", { name: t("cpdlc.acars_start") }));
      await screen.findByText(t("cpdlc.acars_online"));
      await userEvent.click(screen.getByRole("tab", { name: t("cpdlc.mode_cpdlc") }));
      await screen.findByText(t("cpdlc.logon_none"));

      const stationInput = screen.getByPlaceholderText(t("cpdlc.center_placeholder")) as HTMLInputElement;
      expect(stationInput.value, "a leftover localStorage draft must not resurface").toBe("");

      await userEvent.type(stationInput, "eddf_gnd");
      expect(sessionStorage.getItem(STATION_DRAFT_KEY)).toBe("EDDF_GND");

      await userEvent.click(screen.getByRole("button", { name: t("cpdlc.logon_send") }));
      await waitFor(() =>
        expect(invokeMock.mock.calls.some((c) => c[0] === "hoppie_send_logon_request")).toBe(true),
      );

      expect(
        sessionStorage.getItem(STATION_DRAFT_KEY),
        "the draft must be cleared once a logon request actually goes out",
      ).toBeNull();
    } finally {
      localStorage.removeItem(STATION_DRAFT_KEY);
      sessionStorage.removeItem(STATION_DRAFT_KEY);
    }
  });
});
