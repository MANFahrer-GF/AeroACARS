// Sim-Rate-Hinweis: erscheint erst nach 2 Minuten Abweichung von 1×, in
// beiden Richtungen, und verschwindet bei der Rueckkehr auf 1×.
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { SimRateHinweis } from "./SimRateHinweis";
import { simRateWeichtAb, simRateText } from "../hooks/useSimRateHinweis";

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
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const weiter = (ms: number) =>
  act(() => {
    vi.advanceTimersByTime(ms);
  });

describe("SimRateHinweis", () => {
  it("zeigt bei 8× erst nach 2 Minuten etwas", () => {
    render(<SimRateHinweis rate={8} aktiv />);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
    weiter(60_000);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
    weiter(65_000);
    expect(screen.getByTestId("simrate-hinweis").textContent).toContain("8×");
  });

  it("zeigt auch Zeitlupe (0,25×) mit eigenem Text", () => {
    render(<SimRateHinweis rate={0.25} aktiv />);
    weiter(125_000);
    const t = screen.getByTestId("simrate-hinweis").textContent ?? "";
    expect(t).toContain("0,25×");
    expect(t).toContain("langsamer");
  });

  it("bleibt bei 1× und ohne aktiven Flug stumm", () => {
    const a = render(<SimRateHinweis rate={1} aktiv />);
    weiter(300_000);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
    a.unmount();
    render(<SimRateHinweis rate={8} aktiv={false} />);
    weiter(300_000);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
  });

  it("verschwindet bei Rueckkehr auf 1× und die Uhr startet neu", () => {
    const { rerender } = render(<SimRateHinweis rate={4} aktiv />);
    weiter(125_000);
    expect(screen.getByTestId("simrate-hinweis")).toBeTruthy();
    rerender(<SimRateHinweis rate={1} aktiv />);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
    rerender(<SimRateHinweis rate={2} aktiv />);
    weiter(60_000);
    expect(screen.queryByTestId("simrate-hinweis")).toBeNull();
  });
});

describe("Hilfsfunktionen", () => {
  it("Unbekanntes und 0 gelten als 1×", () => {
    for (const r of [null, undefined, 0, NaN, -1, 1, 1.005]) {
      expect(simRateWeichtAb(r as number)).toBe(false);
    }
    expect(simRateWeichtAb(0.5)).toBe(true);
    expect(simRateWeichtAb(16)).toBe(true);
  });
  it("formatiert mit dem Dezimalzeichen der Sprache", () => {
    expect(simRateText(0.25)).toBe("0,25");
    expect(simRateText(0.25, "de")).toBe("0,25");
    expect(simRateText(0.25, "it")).toBe("0,25");
    expect(simRateText(0.25, "en")).toBe("0.25");
    expect(simRateText(8, "en")).toBe("8");
    expect(simRateText(8)).toBe("8");
  });
});
