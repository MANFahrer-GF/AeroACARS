// P-14.02 / N-03: Vermerk „bewertet mit 2.x“ im Client an denselben Stellen
// wie in der Webapp (Kopf der Landung, Landungsliste); 3.0-Flüge zeigen nichts.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import enCommon from "../../../bundle/texte/en.json";
import itCommon from "../../../bundle/texte/it.json";
import THY39 from "./landungsFaelle.thy39.json";
import type { LandingRecord } from "./landungsDatensatz";
import { bewertetMit2x, istDreiNull } from "./bewertungsFassung";

const invokeMock = vi.fn();
vi.mock("./ipc", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { LandungsKopf } from "../components/LandungsBewertung";
import { LandingPanel } from "../components/LandingPanel";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon }, en: { common: enCommon }, it: { common: itCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(() => {
  cleanup();
  invokeMock.mockReset();
});

const ZWEI = THY39 as unknown as LandingRecord; // client_version 1.9.24, Fassung 19
const DREI: LandingRecord = { ...ZWEI, pirep_id: "drei", client_version: "3.0.0-beta.1", score_algorithm_version: 27, bewertung_status: "endgueltig" };

describe("bewertetMit2x (Client)", () => {
  it("2.x mit und ohne Fassung ja; 3.0 (Version, Fassung 27, Status) und ohne Note nein", () => {
    expect(bewertetMit2x(ZWEI)).toBe(true);
    expect(bewertetMit2x({ ...ZWEI, client_version: null, score_algorithm_version: null })).toBe(true);
    expect(bewertetMit2x({ ...ZWEI, client_version: "2.1.24", score_algorithm_version: 26 })).toBe(true);
    // Gegenproben
    expect(bewertetMit2x(DREI)).toBe(false);
    expect(bewertetMit2x({ ...ZWEI, client_version: "3.0.0" })).toBe(false);
    expect(bewertetMit2x({ ...ZWEI, score_algorithm_version: 27 })).toBe(false);
    expect(bewertetMit2x({ ...ZWEI, bewertung_status: "modulfehler" })).toBe(false);
    expect(bewertetMit2x({ ...ZWEI, score_numeric: null })).toBe(false);
    expect(istDreiNull({ ...ZWEI, bewertung_status: "" })).toBe(false);
  });

  it("Kopf: Vermerk in de/en/it bei 2.x, nie bei 3.0 — Texte wie die Webapp", async () => {
    for (const [sprache, text] of [["de", "bewertet mit 2.x"], ["en", "rated with 2.x"], ["it", "valutato con 2.x"]] as const) {
      await act(async () => {
        await i18next.changeLanguage(sprache);
      });
      const k = render(<LandungsKopf record={ZWEI} callsign="THY39" />);
      expect(k.getByTestId("kopf-bewertet-mit-2x").textContent).toBe(` · ${text}`);
      k.unmount();
      const k3 = render(<LandungsKopf record={DREI} callsign="THY39" />);
      expect(k3.queryByTestId("kopf-bewertet-mit-2x")).toBeNull();
      k3.unmount();
    }
    await act(async () => {
      await i18next.changeLanguage("de");
    });
  });

  it("Landungsliste: Vermerk neben dem Buchstaben nur beim 2.x-Flug", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "landing_list") return Promise.resolve([ZWEI, DREI]);
      return Promise.resolve(null);
    });
    const { findAllByText, getAllByTestId } = render(<LandingPanel />);
    await findAllByText(/THY39/);
    const vermerke = getAllByTestId("bewertet-mit-2x");
    expect(vermerke).toHaveLength(1);
    expect(vermerke[0].textContent).toBe("bewertet mit 2.x");
    expect(vermerke[0].closest("td")?.className).toContain("landing-ov-grade");
  });
});
