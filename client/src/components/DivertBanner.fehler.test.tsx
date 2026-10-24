// Eine abgelehnte Abgabe muss ihren Grund zeigen.
//
// GAF 278 (Adrian, 24.10.2026): „Trotzdem nach LOWW einreichen" aus Zagreb,
// 142 nm vor dem Ziel. Das Backend lehnte mit `flight_validation_failed`
// (`not_at_arrival`) ab; der Banner zeigte nur „[object Object]", und Adrian
// wusste nicht, warum.
import { describe, it, expect, vi } from "vitest";
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

import { DivertBanner } from "./DivertBanner";
import type { ActiveFlightInfo } from "../types";

const FLUG = {
  pirep_id: "lJPyeQ3GJ1WNxPdR",
  airline_icao: "GAF",
  flight_number: "278",
  callsign: "",
  dpt_airport: "OMDB",
  arr_airport: "LOWW",
  phase: "arrived",
  was_just_resumed: false,
  divert_hint: {
    actual_icao: "LDZA",
    planned_arr_icao: "LOWW",
    planned_alt_icao: "LHBP",
    distance_to_planned_nmi: 142,
    kind: "nearest",
  },
} as unknown as ActiveFlightInfo;

describe("Divert-Banner bei abgelehnter Abgabe", () => {
  it("zeigt den Grund statt [object Object]", async () => {
    tauriInvoke.mockImplementation((cmd: string) =>
      cmd === "flight_end"
        ? Promise.reject({
            code: "flight_validation_failed",
            message: "flight does not meet minimum requirements for an ACARS PIREP",
            details: { missing: ["not_at_arrival"] },
          })
        : Promise.resolve(null),
    );
    render(<DivertBanner activeFlight={FLUG} onFiledSuccess={() => {}} />);
    fireEvent.click(screen.getByText(/LOWW einreichen|to LOWW/));
    await waitFor(() =>
      expect(document.body.textContent).toMatch(/nicht am Ankunftsflughafen|not at the arrival/i),
    );
    expect(document.body.textContent).not.toContain("[object Object]");
  });
});
