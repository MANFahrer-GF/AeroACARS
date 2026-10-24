// Ein Flug, der nur auf dem Server gefunden wird, darf nie von selbst
// übernommen werden.
//
// 24.10.2026: Thomas flog auf Windows, auf dem Mac wurde AeroACARS geöffnet.
// Der Mac fand den laufenden PIREP, zeigte einen 30-s-Countdown und hätte den
// Flug danach selbst übernommen — zwei Clients auf demselben PIREP.
//
// Geprüft wird an der echten Komponente.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";

const tauriInvoke = vi.hoisted(() => {
  (globalThis as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  if (typeof window !== "undefined") {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  }
  return vi.fn();
});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
}));

import { ResumeFlightBanner } from "./ResumeFlightBanner";

const GEFUNDEN = {
  pirep_id: "DqO0MADWPx7qr01d",
  flight_number: "442",
  dpt_airport: "YBBN",
  arr_airport: "VHHH",
  status: "ENR",
  zuletzt_gemeldet_vor_s: 40,
  laeuft_woanders: true,
};

function aufrufe(cmd: string) {
  return tauriInvoke.mock.calls.filter((c) => c[0] === cmd);
}

/** Der Countdown tickt je Sekunde über ein neues Rendern — darum in Schritten. */
async function eineMinuteVergeht() {
  for (let i = 0; i < 60; i++) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
  }
}

function zeige(flug: typeof GEFUNDEN) {
  tauriInvoke.mockImplementation((cmd: string) =>
    cmd === "flight_discover_resumable" ? Promise.resolve([flug]) : Promise.resolve(null),
  );
  render(<ResumeFlightBanner activeFlight={null} onAdopted={() => {}} onCancelled={() => {}} />);
}

describe("Gefundener Flug von einem anderen Gerät", () => {
  beforeEach(() => {
    tauriInvoke.mockReset();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("übernimmt auch nach einer Minute nicht von selbst", async () => {
    zeige({ ...GEFUNDEN, laeuft_woanders: false, zuletzt_gemeldet_vor_s: 7200 });
    await waitFor(() => expect(screen.getByText(/442/)).toBeTruthy());
    await eineMinuteVergeht();
    expect(aufrufe("flight_adopt")).toHaveLength(0);
  });

  it("zeigt den Hinweis und fragt vor dem Übernehmen nach", async () => {
    zeige(GEFUNDEN);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByRole("alert").textContent).toMatch(/anderen Gerät|another device/);

    const knoepfe = screen.getAllByRole("button");
    fireEvent.click(knoepfe[0]!);
    // Die Rückfrage steht, übernommen ist noch nichts.
    await waitFor(() =>
      expect(document.body.textContent).toMatch(/Wirklich hier übernehmen|Really take it over/),
    );
    expect(aufrufe("flight_adopt")).toHaveLength(0);
  });

  it("„Nicht jetzt“ schließt den Banner, ohne zu übernehmen oder zu verwerfen", async () => {
    zeige(GEFUNDEN);
    await waitFor(() => expect(screen.getByText(/442/)).toBeTruthy());
    fireEvent.click(screen.getByText(/Nicht jetzt|Not now/));
    await waitFor(() => expect(screen.queryByText(/442/)).toBeNull());
    await eineMinuteVergeht();
    expect(aufrufe("flight_adopt")).toHaveLength(0);
    expect(aufrufe("flight_cancel")).toHaveLength(0);
    expect(screen.queryByText(/442/)).toBeNull();
  });
});
