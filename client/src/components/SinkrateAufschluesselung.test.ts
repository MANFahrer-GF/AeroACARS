// v1.6.9 — wann meldet sich der Bericht zur Sinkrate?
//
// Variante C: nur wenn es etwas zu sagen gibt. Die Schwellen stammen aus
// dem Bestand (828 aufgezeichnete Landungen): Gelände ab 100 fpm trifft
// 12 %, Sim-Abstand ab 200 fpm trifft 8 % der MSFS-Landungen.
import { describe, it, expect } from "vitest";
import {
  sinkrateHinweis,
  GELAENDE_SCHWELLE_FPM,
  SIM_ABSTAND_SCHWELLE_FPM,
} from "./SinkrateForensik";

describe("sinkrateHinweis", () => {
  it("schweigt bei einer unauffälligen Landung", () => {
    // OMDB 30R aus dem Bestand: kein Gelände, Sim-Referenz 1 fpm daneben.
    expect(
      sinkrateHinweis({
        vs_at_edge_fpm: -100,
        vs_gelaende_fpm: 0,
        vs_eigensinken_fpm: -100,
        vs_sim_referenz_fpm: -101,
      }),
    ).toBeNull();
  });

  it("erklärt das Gelände, wenn es die Zahl trägt", () => {
    // LIRN 24: gemessen −553, davon −387 Gelände, eigener Sinkflug −166.
    const h = sinkrateHinweis({
      vs_at_edge_fpm: -553,
      vs_gelaende_fpm: -387,
      vs_eigensinken_fpm: -166,
    })!;
    expect(h.art).toBe("gelaende");
    expect(h.gelaende).toBe(387);
    expect(h.eigensinken).toBe(-166);
  });

  it("nennt den Abstand zur Referenz des Simulators", () => {
    // KEWR 04R: gemessen −602, Simulator −108.
    const h = sinkrateHinweis({
      vs_at_edge_fpm: -602,
      vs_gelaende_fpm: -27,
      vs_eigensinken_fpm: -575,
      vs_sim_referenz_fpm: -108,
    })!;
    expect(h.art).toBe("sim_abstand");
    expect(h.abstand).toBe(494);
    expect(h.simReferenz).toBe(-108);
  });

  it("gibt dem Gelände den Vorrang, wenn beides zutrifft", () => {
    const h = sinkrateHinweis({
      vs_at_edge_fpm: -500,
      vs_gelaende_fpm: -200,
      vs_eigensinken_fpm: -300,
      vs_sim_referenz_fpm: -150,
    })!;
    // Das Gelände erklärt die Zahl selbst; der Abstand sagt nur, dass zwei
    // Messungen auseinanderliegen.
    expect(h.art).toBe("gelaende");
  });

  it("hält die Schwellen ein", () => {
    const knappDarunter = sinkrateHinweis({
      vs_at_edge_fpm: -400,
      vs_gelaende_fpm: -(GELAENDE_SCHWELLE_FPM - 1),
      vs_eigensinken_fpm: -301,
    });
    expect(knappDarunter).toBeNull();
    const genauDrauf = sinkrateHinweis({
      vs_at_edge_fpm: -400,
      vs_gelaende_fpm: -GELAENDE_SCHWELLE_FPM,
      vs_eigensinken_fpm: -300,
    });
    expect(genauDrauf?.art).toBe("gelaende");

    expect(
      sinkrateHinweis({
        vs_at_edge_fpm: -300,
        vs_sim_referenz_fpm: -300 + (SIM_ABSTAND_SCHWELLE_FPM - 1),
      }),
    ).toBeNull();
    expect(
      sinkrateHinweis({
        vs_at_edge_fpm: -300,
        vs_sim_referenz_fpm: -300 + SIM_ABSTAND_SCHWELLE_FPM,
      })?.art,
    ).toBe("sim_abstand");
  });

  it("schweigt, wenn die Daten fehlen oder unbrauchbar sind", () => {
    expect(sinkrateHinweis({})).toBeNull();
    expect(sinkrateHinweis({ vs_at_edge_fpm: null, vs_gelaende_fpm: -300 })).toBeNull();
    expect(sinkrateHinweis({ vs_at_edge_fpm: NaN, vs_gelaende_fpm: -300 })).toBeNull();
    // Gelände ohne Eigensinken ergibt keinen erklärbaren Satz.
    expect(sinkrateHinweis({ vs_at_edge_fpm: -400, vs_gelaende_fpm: -300 })).toBeNull();
    // Altdatensätze vor v1.6.9: beide Felder fehlen, kein Hinweis.
    expect(sinkrateHinweis({ vs_at_edge_fpm: -250 })).toBeNull();
  });
});
