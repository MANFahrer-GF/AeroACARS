// Die Landungs-Übersicht mit ungemessenen und teilweise gemessenen Landungen.
//
// Codex, zweite Abnahme 13.09.2026: Die ersten Tests renderten nur die
// Detailansicht — "weder die Übersicht noch die Statistik". Genau dort
// standen die übrigen Fehler: "Ø Sinkrate 0 fpm", "Ø Score 0,0", eine
// ungemessene Landung als weichste und härteste, eine erfundene 0 in der
// Sinkratenspalte, und "Light bounce 20 ft" über dem Ausweichzweig der Hopser.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { LandingPanel, LandingDetail } from "./LandingPanel";

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
  invokeMock.mockReset();
});

/** Erkannt, aber nicht gemessen — wie CFG 2090. */
function ungemessen(id: string) {
  const b = MOCK_LANDING_OPTIONS[0]!.build();
  return {
    ...b,
    pirep_id: id,
    score_numeric: null,
    score_label: null,
    grade_letter: null,
    landing_rate_fpm: null,
    landing_peak_vs_fpm: null,
    vs_at_edge_fpm: null,
    landing_peak_g_force: null,
    landing_g_force: null,
    bounce_count: 1,
    forensic_bounce_count: 2,
    bounce_max_agl_ft: 20,
    landung_nicht_bewertbar: { groesste_luecke_ms: 920, proben: 5 },
    fenster_unzureichend: true,
  };
}

async function uebersicht(records: unknown[]) {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "landing_list") return Promise.resolve(records);
    return Promise.resolve(null);
  });
  const r = render(<LandingPanel />);
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  return r.container.textContent ?? "";
}

describe("Übersicht mit ungemessenen Landungen", () => {
  it("erfindet keine Durchschnitte, wenn keine Landung gemessen wurde", async () => {
    const text = await uebersicht([ungemessen("a"), ungemessen("b")]);
    expect(text, "keine Null-Sinkrate im Kopf").not.toMatch(/\b0\s*fpm/);
    expect(text, "keine Null-Durchschnittsnote").not.toMatch(/\b0,0\b/);
  });

  it("zählt eine ungemessene Landung nicht in den Notenschnitt", async () => {
    // Das Rechenbeispiel aus der Abnahme: ungemessen (vorher 97) und
    // gemessen mit 80 ergaben 88,5 — richtig ist 80.
    const gut = { ...MOCK_LANDING_OPTIONS[0]!.build(), pirep_id: "gut", score_numeric: 80 };
    const text = await uebersicht([ungemessen("a"), gut]);
    expect(text).toContain("80,0");
    expect(text).not.toContain("88,5");
  });

  it("zeigt in der Sinkratenspalte keine erfundene Null", async () => {
    const text = await uebersicht([ungemessen("a")]);
    expect(text).toContain("nicht gemessen");
  });
});

describe("Hopser ohne gemessenes Fenster", () => {
  it("unterdrückt auch den Ausweichzweig 'leichter Hopser'", () => {
    const r = ungemessen("a");
    const { container } = render(
      <LandingDetail record={r as never} allRecords={[r] as never} onBack={() => {}} />,
    );
    const text = container.textContent ?? "";
    expect(text).not.toMatch(/20 ft/);
    expect(text).not.toMatch(/Hopser ×/);
  });
});

describe("Sinkrate vom Simulator, Fenster zu dünn", () => {
  it("behält die Note, blendet aber G-Werte und Hopser aus", () => {
    // Codex, zweite Abnahme, Befund 6: Das Frontend behandelte die
    // Fenster-Markierung als Totalsperre und warf auch gültige MSFS-Landungen
    // aus Verlauf und Teilwerten — während Note und Banner gleichzeitig
    // sichtbar blieben. Richtig: Note ja, Banner nein, Fensterwerte nein.
    const b = MOCK_LANDING_OPTIONS[0]!.build();
    const r = {
      ...b,
      landing_source: "msfs_simvar_latched",
      landung_nicht_bewertbar: null,
      fenster_unzureichend: true,
      bounce_count: 1,
    };
    const { container } = render(
      <LandingDetail record={r as never} allRecords={[r] as never} onBack={() => {}} />,
    );
    const text = container.textContent ?? "";
    expect(text, "die Note bleibt").toContain("/100");
    expect(text, "kein Banner 'keine Bewertung'").not.toContain("Aufzeichnung unvollständig");
    expect(text, "keine Hopser aus dem dünnen Fenster").not.toMatch(/Hopser ×/);
  });
});
