// v0.19.x FIX: RunwayDiagramV2's six SVG <title> hover tooltips (threshold,
// runway end, TDZ, aim point, touchdown point, brake point) were hardcoded
// German prose, never routed through t() — an English/Italian pilot
// hovering the diagram on the post-landing debrief screen saw raw German
// regardless of their chosen locale. Pins that the tooltip text now
// follows the active language.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, cleanup, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import { DEFAULT_SKIN, type V2Skin } from "./runwayV2Skin";

const skinBox = vi.hoisted(() => ({ current: null as V2Skin | null }));
vi.mock("./SkinContext", async () => {
  const actual = await vi.importActual<typeof import("./runwayV2Skin")>("./runwayV2Skin");
  return { useV2Skin: () => skinBox.current ?? actual.DEFAULT_SKIN };
});

import { RunwayDiagramV2, type RunwayDiagramV2Props } from "./RunwayDiagramV2";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon }, en: { common: enCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(async () => {
  cleanup();
  await i18next.changeLanguage("de");
  skinBox.current = null;
});

function props(overrides: Partial<RunwayDiagramV2Props> = {}): RunwayDiagramV2Props {
  return {
    airport_ident: "EDDF",
    runway_ident: "25C",
    length_m: 4000,
    source: "navigraph",
    td_distance_from_threshold_m: 350,
    td_centerline_offset_m: 2,
    td_tdz_length_m: 900,
    aim_point_m: 300,
    ...overrides,
  };
}

function titles(container: HTMLElement): string[] {
  return [...container.querySelectorAll("title")].map((t) => t.textContent ?? "");
}

function polygonPoints(el: Element): number[][] {
  return el
    .getAttribute("points")!
    .trim()
    .split(/\s+/)
    .map((pair) => pair.split(",").map(Number));
}

describe("RunwayDiagramV2 — hover tooltips follow the active locale", () => {
  it("shows German tooltip prose under de, different English prose under en", async () => {
    await i18next.changeLanguage("de");
    const de = render(<RunwayDiagramV2 {...props()} />);
    const deTitles = titles(de.container).join("\n");
    expect(deTitles).toContain("Landeschwelle (Threshold)");
    expect(deTitles).toContain("Bahn-Ende");
    expect(deTitles).toContain("Aufsetzpunkt (Touchdown)");
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<RunwayDiagramV2 {...props()} />);
    const enTitles = titles(en.container).join("\n");
    expect(enTitles).toContain("Landing threshold");
    expect(enTitles).toContain("Runway end");
    expect(enTitles).toContain("Touchdown point");
    expect(enTitles).not.toContain("Landeschwelle");
    expect(enTitles).not.toContain("Bahn-Ende");
  });

  it("localizes the direction words inside the touchdown tooltip, not just the surrounding prose", async () => {
    // Before the threshold, right of the centerline.
    await i18next.changeLanguage("de");
    const de = render(
      <RunwayDiagramV2 {...props({ td_distance_from_threshold_m: -20, td_centerline_offset_m: 3 })} />,
    );
    const deTitle = titles(de.container).find((t) => t.includes("Aufsetzpunkt"))!;
    expect(deTitle).toContain("vor");
    expect(deTitle).toContain("rechts");
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(
      <RunwayDiagramV2 {...props({ td_distance_from_threshold_m: -20, td_centerline_offset_m: 3 })} />,
    );
    const enTitle = titles(en.container).find((t) => t.includes("Touchdown point"))!;
    expect(enTitle).toContain("before");
    expect(enTitle).toContain("right of");
    expect(enTitle).not.toContain("vor ");
    expect(enTitle).not.toContain("rechts");
  });
});

// v0.19.x FIX: the runway-utilization percentage ("Bahn-Auslastung")
// divided by a defensively-floored `Math.max(500, length_m)` instead of
// the real LDA — that floor exists purely to keep the SVG geometry from
// degenerating on missing/corrupt data, but leaked into the SCORE math
// too. For a genuine short strip under 500 m LDA (bush/VFR fields, an
// explicitly supported case), utilization was computed against a
// fictionally inflated runway and read too LOW — a genuinely tight
// landing looked comfortable.
// v0.19.x FIX: the lateral-offset arrowhead's tip/base vertices were
// swapped — a proper arrowhead's single "tip" vertex must be the FURTHEST
// point in the pointing direction, with the flared 2-point base behind it.
// The old code had it backwards: the arrow visually pointed BACK toward
// the touchdown dot instead of away from it in the LEFT/RIGHT direction
// stated by the text label right next to it.

describe("RunwayDiagramV2 — runway-utilization percentage ignores the SVG-geometry floor", () => {
  it("computes utilization against the real (sub-500m) LDA, not the floored value", () => {
    // 300 m LDA, td=100 m past threshold, rollout=150 m -> used = max(100+150, 150) = 250 m.
    // Correct: 250 / 300 = 83%. Buggy (floored to 500): 250 / 500 = 50%.
    const { container } = render(
      <RunwayDiagramV2
        {...props({
          length_m: 300,
          td_distance_from_threshold_m: 100,
          rollout_m: 150,
        })}
      />,
    );
    expect(container.textContent).toContain("83 %");
    expect(container.textContent).not.toContain("50 %");
  });

  it("is unaffected for a normal-length runway (>= 500 m), matching the pre-fix output", () => {
    // 3000 m LDA, td=500, rollout=500 -> used = max(500+500,500) = 1000 -> 1000/3000 = 33%.
    const { container } = render(
      <RunwayDiagramV2
        {...props({
          length_m: 3000,
          td_distance_from_threshold_m: 500,
          rollout_m: 500,
        })}
      />,
    );
    expect(container.textContent).toContain("33 %");
  });
});

// v0.19.x FIX: `V2Skin.thresholds` (peak_g_warn/bad, crosswind_warn/bad,
// bank_warn_above, pitch_bad_below, bahn_auslastung_warn_above,
// centerline_warn_above/bad_above, hinter_schwelle_warn_above) was defined,
// defaulted and merged, but every tone decision in the component used a
// HARDCODED magic number instead of reading it — a VA admin changing the
// deployed VPS skin's thresholds would have had zero effect. These prove
// a non-default skin's thresholds now actually change what the pilot sees.
describe("RunwayDiagramV2 — skin thresholds are actually read, not just hardcoded copies", () => {
  it("faerbt die Auslastung nicht mehr — egal was der Skin sagt", () => {
    // v1.7.0: Die Achse bewertet nicht mehr, wie viel Bahn jemand gebraucht
    // hat. Eine gelbe Pill neben einer Landung mit voller Punktzahl waere
    // ein Widerspruch, den niemand aufloesen kann.
    //
    // Frueher pruefte dieser Test das Gegenteil: dass die Pill die
    // Skin-Schwelle `bahn_auslastung_warn_above` liest. Die Schwelle ist
    // jetzt wirkungslos — und weil Skins vom VPS kommen und dort noch
    // Werte tragen, prueft der Test die schaerfere Aussage: Auch ein Skin,
    // der eine Schwelle setzt, faerbt nichts mehr ein.
    const p = props({ length_m: 1000, td_distance_from_threshold_m: 0, rollout_m: 900, aim_point_m: null });
    const def = render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("90 %")).not.toHaveStyle({ color: "#fbbf24" });
    expect(screen.getByText("90 %")).not.toHaveStyle({ color: "#22c55e" });
    def.unmount();

    skinBox.current = {
      ...DEFAULT_SKIN,
      thresholds: { ...DEFAULT_SKIN.thresholds, bahn_auslastung_warn_above: 95 },
    };
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("90 %")).not.toHaveStyle({ color: "#22c55e" });
  });

  it("faerbt ein Ueberrollen weiterhin rot", () => {
    // Ueber 100 % ist kein Auslastungsgrad mehr, sondern ein Ueberrollen --
    // und das IST ein Kriterium (Spec §5.4). Ohne diesen Test waere die
    // Aenderung oben eine stille Entschaerfung.
    const p = props({ length_m: 1000, td_distance_from_threshold_m: 0, rollout_m: 1200, aim_point_m: null });
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("120 %")).toHaveStyle({ color: "#ef4444" });
  });

  it("re-tones the peak-G readout in the aircraft bar when the skin lowers peak_g_warn", () => {
    const p = props({ aircraft_icao: "A320", landing_peak_g_force: 1.3 });
    const def = render(<RunwayDiagramV2 {...p} />);
    // 1.3 g is below DEFAULT_SKIN's peak_g_warn (1.5) -> green, no warning.
    expect(screen.getByText("1.30 g")).toHaveStyle({ color: "#22c55e" });
    def.unmount();

    // A skin lowering peak_g_warn to 1.0 must flag the SAME 1.3 g as amber.
    skinBox.current = {
      ...DEFAULT_SKIN,
      thresholds: { ...DEFAULT_SKIN.thresholds, peak_g_warn: 1.0 },
    };
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("1.30 g")).toHaveStyle({ color: "#fbbf24" });
  });
});

// v0.19.x FIX: `V2Skin.display` (7 show/hide flags — aim marker, TDZ
// box, brake point, opposite-runway designator, in-diagram runway-
// length label, aircraft bar, L/R offset arrow) was defined, defaulted
// and merged like every other skin section, but the component never
// read it — a VA admin turning an element off via the deployed VPS
// skin saw zero effect. These prove each flag now actually controls
// its element, while a still-true flag leaves the default look intact.
describe("RunwayDiagramV2 — skin display flags actually hide/show elements", () => {
  function withDisplay(overrides: Partial<V2Skin["display"]>) {
    skinBox.current = {
      ...DEFAULT_SKIN,
      display: { ...DEFAULT_SKIN.display, ...overrides },
    };
  }

  it("show_aim_marker toggles the aim-point marker and its legend entry", () => {
    // Case-sensitive regex, NOT exact:false substring matching — the
    // tooltip prose ("Aim-Point — die zwei großen...") also contains
    // the same word in mixed case and exact:false matches case-
    // insensitively, which would find both and throw on multiple hits.
    const p = props({ aim_point_m: 300 });
    const shown = render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText(/AIM-POINT/)).toBeTruthy();
    expect(screen.getByText(deCommon.runway_v2.legend_aim)).toBeTruthy();
    shown.unmount();

    withDisplay({ show_aim_marker: false });
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.queryByText(/AIM-POINT/)).toBeNull();
    expect(screen.queryByText(deCommon.runway_v2.legend_aim)).toBeNull();
  });

  it("show_aufsetzzone_box toggles the TDZ box and its legend entry", () => {
    const p = props({ td_tdz_length_m: 900 });
    const shown = render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText(deCommon.runway_v2.legend_tdz)).toBeTruthy();
    shown.unmount();

    withDisplay({ show_aufsetzzone_box: false });
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.queryByText(deCommon.runway_v2.legend_tdz)).toBeNull();
  });

  it("nennt den Versatz als Wort statt als Pfeil", () => {
    // v1.7.0: Der L/R-Pfeil unter der Bahn ist entfallen. Er lief in die
    // TD-Beschriftung hinein, und bei wenigen Metern Versatz war seine
    // Richtung nicht zu erkennen. Die Aussage steht jetzt in der TD-Zeile.
    //
    // Geprueft wird beides: dass der Pfeil weg ist UND dass die Aussage
    // nicht mit ihm verschwunden ist. Ein Test nur auf „Pfeil weg" waere
    // auch dann gruen, wenn der Versatz gar nicht mehr angezeigt wird.
    const links = render(<RunwayDiagramV2 {...props({ td_centerline_offset_m: -6.6 })} />);
    expect(links.container.textContent).toContain("6.6 m links");
    links.unmount();

    const rechts = render(<RunwayDiagramV2 {...props({ td_centerline_offset_m: 6.6 })} />);
    expect(rechts.container.textContent).toContain("6.6 m rechts");
    rechts.unmount();

    // Auf der Mittellinie steht weder links noch rechts.
    const mitte = render(<RunwayDiagramV2 {...props({ td_centerline_offset_m: 0.1 })} />);
    expect(mitte.container.textContent).not.toContain("m links");
    expect(mitte.container.textContent).not.toContain("m rechts");
  });

  it("zeigt den Bremspunkt nicht mehr — auch nicht auf Wunsch des Skins", () => {
    // v1.7.0: Der Marker „Bremspunkt 40 kt" entfaellt ERSATZLOS
    // (docs/spec/runway-diagram-v2.contract.md, Abschnitt v1.7.0).
    //
    // Der Test prueft die schaerfere Aussage, nicht nur den Normalfall: Der
    // Skin kommt vom VPS, und ein aelterer Skin dort traegt weiterhin
    // `show_brakepoint: true`. Wuerde der Marker daran haengen, waere er bei
    // jedem Piloten mit gecachtem Skin wieder da — und niemand haette den
    // Zusammenhang gesehen. Deshalb: An IST er weg, und auf ausdruecklichen
    // Wunsch bleibt er weg.
    const p = props({ rollout_m: 500 });
    withDisplay({ show_brakepoint: true });
    const an = render(<RunwayDiagramV2 {...p} />);
    expect(screen.queryByText(deCommon.runway_v2.legend_brakepoint)).toBeNull();
    expect(an.container.textContent).not.toContain(
      deCommon.runway_v2.bremspunkt_title,
    );
    an.unmount();

    withDisplay({ show_brakepoint: false });
    const aus = render(<RunwayDiagramV2 {...p} />);
    expect(screen.queryByText(deCommon.runway_v2.legend_brakepoint)).toBeNull();
    expect(aus.container.textContent).not.toContain(
      deCommon.runway_v2.bremspunkt_title,
    );
  });

  it("show_opposite_runway toggles the opposite-runway designator text", () => {
    const p = props();
    const shown = render(<RunwayDiagramV2 {...p} />);
    expect(shown.container.querySelector('text[fill="#94a3b8"]')).not.toBeNull();
    shown.unmount();

    withDisplay({ show_opposite_runway: false });
    const hidden = render(<RunwayDiagramV2 {...p} />);
    expect(hidden.container.querySelector('text[fill="#94a3b8"]')).toBeNull();
  });

  it("show_bahn_length toggles the in-diagram runway-length label", () => {
    const p = props();
    const shown = render(<RunwayDiagramV2 {...p} />);
    expect(shown.container.querySelector('text[fill="#64748b"]')).not.toBeNull();
    shown.unmount();

    withDisplay({ show_bahn_length: false });
    const hidden = render(<RunwayDiagramV2 {...p} />);
    expect(hidden.container.querySelector('text[fill="#64748b"]')).toBeNull();
  });

  it("show_flugzeug_bar toggles the aircraft data bar", () => {
    const p = props({ aircraft_icao: "A320" });
    const shown = render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText(deCommon.runway_v2.flugzeug_label)).toBeTruthy();
    shown.unmount();

    withDisplay({ show_flugzeug_bar: false });
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.queryByText(deCommon.runway_v2.flugzeug_label)).toBeNull();
  });

});

// v1.6.8-QS3: die 500-m-Untergrenze der SVG-Geometrie darf keine echte
// kurze Bahn mehr ueberschreiben. Ausloeser: durch den Abzug der
// versetzten Schwelle rutschen 19 Bahnen unter 500 m NUTZBARE Laenge
// (EDKU 03/21, EDXZ 12/30, LOAD 25 …). Dort zeichnete das Bild eine
// 500-m-Bahn und setzte den Aufsetzpunkt entsprechend falsch.
describe("RunwayDiagramV2 — kurze Bahnen werden gezeichnet wie sie sind", () => {
  it("setzt den Aufsetzpunkt auf einer 400-m-Bahn in die Mitte, nicht ins erste Fuenftel", () => {
    const { container } = render(
      <RunwayDiagramV2
        {...props({
          length_m: 400,
          td_distance_from_threshold_m: 200,
          rollout_m: 100,
        })}
      />,
    );
    // Gemessen am gezeichneten Punkt, nicht am Text: die Prozent-Zeile
    // rechnet seit dem v0.19.x-Fix ohnehin ohne die Untergrenze — der
    // Fehler steckte allein in der SVG-Geometrie.
    //
    // 200 m von 400 m = 50 % der Bahn → der Aufsetzpunkt sitzt in der
    // Mitte des Bahn-Rechtecks. Mit der alten Untergrenze waeren es
    // 200/500 = 40 % gewesen, also gut 100 px zu weit links.
    const dot = container.querySelector('circle[fill="#22d3ee"][r="9"]');
    const tarmac = container.querySelector("rect");
    expect(dot, "Aufsetzpunkt muss gezeichnet sein").not.toBeNull();
    expect(tarmac, "Bahn-Rechteck muss gezeichnet sein").not.toBeNull();
    const x = parseFloat(dot!.getAttribute("cx")!);
    const links = parseFloat(tarmac!.getAttribute("x")!);
    const breite = parseFloat(tarmac!.getAttribute("width")!);
    expect((x - links) / breite).toBeCloseTo(0.5, 2);
  });

  it("faengt unbrauchbare Werte weiterhin ab", () => {
    // 0, negativ, NaN und Bruchteil-Meter: alle vier duerfen die
    // Zeichnung nicht entarten lassen. Der Bruchteil-Fall kam aus dem
    // Review — die erste Fassung des Riegels liess ihn durch.
    for (const kaputt of [0, -50, NaN, 0.5]) {
      const { container } = render(
        <RunwayDiagramV2
          {...props({
            length_m: kaputt,
            td_distance_from_threshold_m: 100,
            rollout_m: 100,
          })}
        />,
      );
      expect(container.querySelector("svg"), `length_m=${kaputt}`).toBeTruthy();
      const dot = container.querySelector('circle[fill="#22d3ee"][r="9"]');
      const tarmac = container.querySelector("rect");
      if (dot && tarmac) {
        const anteil =
          (parseFloat(dot.getAttribute("cx")!) - parseFloat(tarmac.getAttribute("x")!)) /
          parseFloat(tarmac.getAttribute("width")!);
        expect(anteil, `length_m=${kaputt}: Punkt muss im Bild bleiben`).toBeGreaterThanOrEqual(-0.01);
        expect(anteil, `length_m=${kaputt}: Punkt muss im Bild bleiben`).toBeLessThanOrEqual(1.01);
      }
    }
  });
});
