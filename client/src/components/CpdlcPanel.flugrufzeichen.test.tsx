// 3.0 — Feldbefund Thomas 31.10.2026: SimBrief plante THY509 mit dem
// „ATC Callsign“ THY4TK, oben im PDC stand aber weiter BTI4TK vom Vorflug.
// Zwei Ursachen, beide hier festgehalten:
//   1. Das Flug-Rufzeichen wurde nur beim Öffnen gelesen — ein Flugstart
//      danach kam nie an, also auch kein Hinweis auf die Abweichung.
//   2. Der eigene Eintrag gilt für alle Flüge; es gab keinen Weg, mit einem
//      Klick das Rufzeichen des Flugs zu übernehmen.
import { describe, it, expect, beforeAll, vi, afterEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

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

afterEach(() => {
  vi.useRealTimers();
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

/** Backend wie bei Thomas: BTI4TK gespeichert, Flug startet erst NACH dem Öffnen. */
function backend(enabled: boolean) {
  const state = { stored: "BTI4TK" as string | null, flight: null as string | null };
  const impl = (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "hoppie_get_settings":
        return Promise.resolve({ enabled, callsign_override: state.stored, notify_sound: false });
      case "hoppie_set_settings":
        state.stored = (args!.settings as { callsign_override: string | null }).callsign_override;
        return Promise.resolve(args!.settings);
      case "hoppie_get_flight_context":
        return Promise.resolve({ callsign: state.flight, aircraft_type: null, dep_icao: null, dest_icao: null });
      case "hoppie_status":
        return Promise.resolve(OFFLINE);
      case "hoppie_get_thread":
      case "hoppie_list_elements":
        return Promise.resolve([]);
      default:
        return Promise.resolve(undefined);
    }
  };
  return { state, impl };
}

const t = (k: string, opts?: Record<string, unknown>) => i18next.t(k, opts);

describe.each([
  ["Hoppie an, nicht verbunden (ACARS AUS)", true],
  // Hoppie ganz aus: VDGS fragt trotzdem unter dem gespeicherten Rufzeichen.
  ["Hoppie ausgeschaltet", false],
])("PDC-Rufzeichen folgt dem Flug — %s", (_name, enabled) => {
  it("bemerkt einen Flugstart nach dem Öffnen und übernimmt THY4TK auf Klick", async () => {
    vi.useFakeTimers();
    const be = backend(enabled);
    invokeMock.mockReset();
    invokeMock.mockImplementation(be.impl);

    render(<CpdlcPanel onOpenSettings={() => {}} mode="pdc" onModeChange={() => {}} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    // Noch kein Flug: nur der gespeicherte Eintrag, kein Knopf.
    expect(screen.queryByRole("button", { name: t("cpdlc.callsign_use_flight", { flight: "THY4TK" }) })).toBeNull();

    // Flug startet mit SimBrief-OFP (ATC Callsign THY4TK).
    be.state.flight = "THY4TK";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(
      screen.getByText(t("cpdlc.callsign_override_active", { entered: "BTI4TK", flight: "THY4TK" }), { exact: false }),
    ).toBeTruthy();
    const knopf = screen.getByRole("button", { name: t("cpdlc.callsign_use_flight", { flight: "THY4TK" }) });

    await act(async () => {
      fireEvent.click(knopf);
      await vi.advanceTimersByTimeAsync(0);
    });

    // Eigener Eintrag gelöscht → ab jetzt gilt das Rufzeichen des Flugs.
    expect(be.state.stored).toBeNull();
    if (enabled) {
      expect(document.querySelector("#datalink-callsign-input")?.textContent).toBe("THY4TK");
    }
    expect(screen.queryByRole("button", { name: t("cpdlc.callsign_use_flight", { flight: "THY4TK" }) })).toBeNull();
  });
});

describe("PDC-Rufzeichen — Kern verwirft den Eintrag vom Vorflug", () => {
  it("zeigt ohne Klick das Rufzeichen des Flugs, sobald der Kern den Eintrag gelöscht hat", async () => {
    vi.useFakeTimers();
    const be = backend(true);
    invokeMock.mockReset();
    invokeMock.mockImplementation(be.impl);

    render(<CpdlcPanel onOpenSettings={() => {}} mode="pdc" onModeChange={() => {}} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(document.querySelector("#datalink-callsign-input")?.textContent).toBe("BTI4TK");

    // Flug startet; der Kern (`hoppie::resolve_callsign`) verwirft BTI4TK.
    be.state.flight = "THY4TK";
    be.state.stored = null;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });

    expect(document.querySelector("#datalink-callsign-input")?.textContent).toBe("THY4TK");
  });
});
