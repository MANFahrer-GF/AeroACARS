// „Vom Boden weiterfliegen" (GAF 278, 24.10.2026): MSFS kann viele Flüge nicht
// in der Luft neu laden. Steht der Flieger nach dem Absturz auf einem anderen
// Platz am Boden, bietet der Banner genau einen Knopf zum Weiterfliegen an —
// und setzt nichts von selbst fort.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

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
import type { ActiveFlightInfo } from "../types";

function flug(vomBoden: boolean) {
  return {
    pirep_id: "lJPyeQ3GJ1WNxPdR",
    airline_icao: "GAF",
    flight_number: "278",
    callsign: "",
    dpt_airport: "OMDB",
    arr_airport: "LOWW",
    phase: "descent",
    was_just_resumed: true,
    resume_position_suspect: true,
    resume_vom_boden_moeglich: vomBoden,
  } as unknown as ActiveFlightInfo;
}

function aufrufe(cmd: string) {
  return tauriInvoke.mock.calls.filter((c) => c[0] === cmd);
}

describe("Weiterflug vom Boden nach Sim-Absturz", () => {
  beforeEach(() => {
    tauriInvoke.mockReset();
    tauriInvoke.mockImplementation((cmd: string) =>
      cmd === "sim_status" ? Promise.resolve({ snapshot: { lat: 45.749 } }) : Promise.resolve(null),
    );
  });

  it("bietet den Knopf an und setzt erst nach dem Klick fort", async () => {
    render(<ResumeFlightBanner activeFlight={flug(true)} onAdopted={() => {}} onCancelled={() => {}} />);
    const knopf = await screen.findByText(/Vom Boden weiterfliegen|Continue from the ground/);
    expect(aufrufe("flight_resume_confirm")).toHaveLength(0);
    fireEvent.click(knopf);
    await waitFor(() => expect(aufrufe("flight_resume_confirm")).toHaveLength(1));
    expect(aufrufe("flight_resume_confirm")[0]![1]).toEqual({ force: true, vomBoden: true });
  });

  it("zeigt ihn nicht, wenn der Weiterflug vom Boden nicht passt", async () => {
    render(<ResumeFlightBanner activeFlight={flug(false)} onAdopted={() => {}} onCancelled={() => {}} />);
    // Der Hard-Stop steht (Positionsprüfung da), nur der Weiterflug-Knopf fehlt.
    await screen.findByRole("button", { name: /Position jetzt prüfen|Check position now/ });
    expect(screen.queryByText(/Vom Boden weiterfliegen|Continue from the ground/)).toBeNull();
    expect(aufrufe("flight_resume_confirm")).toHaveLength(0);
  });
});
