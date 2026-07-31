// v1.3.5 — hard-landing banner trigger regression tests.
//
// Field report 31.07.2026: the banner used to fire straight off the raw
// MSFS touchdown SimVar (-770 fpm for a touchdown the refined analysis
// scored at -455 fpm). These tests pin the fix: the banner must wait for
// `activeFlight.landing_score_finalized` and then show the SAME canonical
// `landing_rate_fpm` the Logbook/PIREP use — never an earlier guess.
//
// Note on that real flight's numbers: -455 fpm does NOT cross this banner's
// -600 fpm FCOM hard-landing threshold at all. So the fix isn't just "show
// the right number" — for that exact flight, the banner should not have
// appeared in the first place. The old raw -770 fpm was a false POSITIVE,
// not just a wrong digit. See `landing_score_matches_the_real_flight...`
// below, which pins that specific case.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { ActiveFlightInfo } from "../types";

import { StableApproachBanner } from "./StableApproachBanner";

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
  cleanup();
  vi.useRealTimers();
});

/** Minimal valid flight — only the fields StableApproachBanner actually
 *  reads are meaningful; the rest are cast through for type convenience. */
function flight(overrides: Partial<ActiveFlightInfo>): ActiveFlightInfo {
  return {
    phase: "taxi_in",
    approach_glideslope_angle: null,
    landing_rate_fpm: null,
    landing_score_finalized: false,
    ...overrides,
  } as ActiveFlightInfo;
}

describe("StableApproachBanner — hard-landing trigger", () => {
  it("does NOT show while the score is not yet finalized, even if the early value already looks hard", () => {
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: -770, landing_score_finalized: false })}
        simSnapshot={null}
        enabled
      />,
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows the canonical value once finalized, not an earlier raw one", async () => {
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: -650, landing_score_finalized: true })}
        simSnapshot={null}
        enabled
      />,
    );
    // The trigger lives in a useEffect (state update after mount), so the
    // alert isn't necessarily in the DOM in the same synchronous pass as
    // render() — findByRole retries until React's effect-driven update lands.
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("-650");
    expect(alert.textContent).not.toContain("-770");
  });

  it("the exact reported flight (-455 fpm final) does not trigger the banner at all", () => {
    // This is the real number from the field report. -455 is a firm/hard
    // landing by the PIREP's OWN scoring bands (grade C), but it does not
    // cross THIS banner's stricter -600 fpm FCOM threshold. Before the fix,
    // the raw SimVar (-770) crossed it — a false alarm on top of being the
    // wrong number.
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: -455, landing_score_finalized: true })}
        simSnapshot={null}
        enabled
      />,
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not show when finalized but the rate is still null (defensive)", () => {
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: null, landing_score_finalized: true })}
        simSnapshot={null}
        enabled
      />,
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("stays hidden entirely while disabled, regardless of finalized state", () => {
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: -900, landing_score_finalized: true })}
        simSnapshot={null}
        enabled={false}
      />,
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("dismisses itself after the display window, not immediately and not forever", async () => {
    vi.useFakeTimers();
    render(
      <StableApproachBanner
        activeFlight={flight({ landing_rate_fpm: -650, landing_score_finalized: true })}
        simSnapshot={null}
        enabled
      />,
    );
    // Flush the mount-time effect (state update) under fake timers.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.queryByRole("alert")).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(19000);
    });
    expect(screen.queryByRole("alert"), "still within the display window").toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(screen.queryByRole("alert"), "window has elapsed").toBeNull();
  });
});
