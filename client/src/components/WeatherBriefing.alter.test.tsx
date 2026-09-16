import { describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

vi.mock("../lib/ipc", () => ({ invoke: vi.fn(() => Promise.resolve(null)), isTauri: false }));

import { WeatherBriefing } from "./WeatherBriefing";
import type { MetarSnapshotDto } from "../types";

function metar(over: Partial<MetarSnapshotDto>): MetarSnapshotDto {
  return {
    icao: "EDDM",
    raw: "EDDM 160850Z 27008KT 9999 FEW040 18/09 Q1017",
    time: new Date(Date.now() - 6 * 60000).toISOString(),
    time_is_estimated: false,
    wind_direction_deg: 270,
    wind_speed_kt: 8,
    gust_kt: null,
    visibility_m: 9999,
    temperature_c: 18,
    dewpoint_c: 9,
    qnh_hpa: 1017,
    ...over,
  } as MetarSnapshotDto;
}

describe("Wetterkarte: Alter des METAR", () => {
  it("zeigt, wie alt das Wetter ist", () => {
    render(
      <WeatherBriefing dptIcao="EDDM" arrIcao={null} prefetchedDpt={metar({})} prefetchedArr={null} />,
    );
    expect(screen.getAllByText(/6 Min\./).length).toBeGreaterThan(0);
  });

  it("kennzeichnet altes Wetter", () => {
    render(
      <WeatherBriefing
        dptIcao="EDDM"
        arrIcao={null}
        prefetchedDpt={metar({ time: new Date(Date.now() - 95 * 60000).toISOString() })}
        prefetchedArr={null}
      />,
    );
    expect(screen.getAllByText(/veraltet/).length).toBeGreaterThan(0);
  });

  it("behauptet nichts, wenn die Zeit nur unser Abrufzeitpunkt ist", () => {
    render(
      <WeatherBriefing
        dptIcao="EDDM"
        arrIcao={null}
        prefetchedDpt={metar({ time_is_estimated: true })}
        prefetchedArr={null}
      />,
    );
    expect(screen.queryByText(/Min\.|gerade eben/)).toBeNull();
  });

  it("rechnet von selbst weiter, auch ohne neues Zeichnen", () => {
    vi.useFakeTimers();
    try {
      render(
        <WeatherBriefing
          dptIcao="EDDM"
          arrIcao={null}
          prefetchedDpt={metar({ time: new Date(Date.now() - 6 * 60000).toISOString() })}
          prefetchedArr={null}
        />,
      );
      expect(screen.getAllByText(/6 Min\./).length).toBeGreaterThan(0);
      act(() => {
        vi.advanceTimersByTime(4 * 60_000);
      });
      expect(screen.getAllByText(/10 Min\./).length).toBeGreaterThan(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
