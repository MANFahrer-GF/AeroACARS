import { describe, it, expect } from "vitest";
import { evaluateApproach, glideslopeScaleFactor } from "./StableApproachBanner";
import type { SimSnapshot } from "../types";

// Minimaler Snapshot — evaluateApproach liest nur diese Felder.
function snap(o: Partial<SimSnapshot>): SimSnapshot {
  return {
    on_ground: false,
    altitude_agl_ft: 800,
    vertical_speed_fpm: -700,
    bank_deg: 0,
    gear_position: 1,
    flaps_position: 1,
    ...o,
  } as unknown as SimSnapshot;
}

const GS_55 = glideslopeScaleFactor(5.5); // ≈ 1.837 (London City)

describe("glideslopeScaleFactor (= Backend gs_factor)", () => {
  it("3° / unbekannt / außerhalb 2–7,5° → 1 (keine Skalierung)", () => {
    expect(glideslopeScaleFactor(3)).toBeCloseTo(1, 6);
    expect(glideslopeScaleFactor(null)).toBe(1);
    expect(glideslopeScaleFactor(undefined)).toBe(1);
    expect(glideslopeScaleFactor(1.5)).toBe(1); // < 2°
    expect(glideslopeScaleFactor(8)).toBe(1); // > 7,5°
  });
  it("steiler Winkel skaliert tan-proportional", () => {
    expect(glideslopeScaleFactor(5.5)).toBeCloseTo(1.837, 2);
    expect(glideslopeScaleFactor(4)).toBeCloseTo(1.334, 2);
  });
});

describe("evaluateApproach — gleitwinkel-aware Sink-Schwellen", () => {
  // DER Fix: steiler, aber profil-konformer Anflug am 1000-ft-Gate.
  const steepAt1000 = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -1500 });

  it("3° (gsFactor=1): −1500 fpm @1000 ft wird als instabil geflaggt", () => {
    expect(evaluateApproach(steepAt1000, "approach", 1)?.key).toBe("gate1000_unstable");
  });

  it("5,5° (skaliert): DASSELBE −1500 fpm wird NICHT geflaggt (auf Profil)", () => {
    expect(evaluateApproach(steepAt1000, "approach", GS_55)).toBeNull();
  });

  it("sub-100 ft: steiler on-profile Sink nicht fälschlich 'pull up' (skaliert)", () => {
    const s = snap({ altitude_agl_ft: 80, vertical_speed_fpm: -1100 });
    expect(evaluateApproach(s, "final", 1)?.key).toBe("sink_rate_pull_up");
    expect(evaluateApproach(s, "final", GS_55)).toBeNull();
  });

  it("echt exzessiver Sink wird auch skaliert noch geflaggt", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -2500 });
    expect(evaluateApproach(s, "approach", GS_55)?.key).toBe("gate1000_unstable");
  });

  it("Bank-Überschreitung triggert unabhängig von der Skalierung", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -700, bank_deg: 8 });
    expect(evaluateApproach(s, "approach", GS_55)?.reason).toContain("Bank");
  });

  it("Config (Gear/Flaps) triggert unabhängig von der Skalierung", () => {
    const s = snap({ altitude_agl_ft: 800, vertical_speed_fpm: -700, flaps_position: 0.5 });
    expect(evaluateApproach(s, "approach", GS_55)?.reason).toContain("Flaps");
  });

  it("nicht in der Approach-Phase → keine Advisory", () => {
    expect(evaluateApproach(snap({ vertical_speed_fpm: -3000 }), "cruise", 1)).toBeNull();
  });
});

describe("evaluateApproach — die Schwellen hängen an der Geschwindigkeit", () => {
  // DLH 373, KJFK→EDDM, 20.09.2026, Session 2242. Die echten Messwerte
  // aus dem Flugprotokoll: konstant −720 fpm bei 139 kt Groundspeed vom
  // Endanflug bis zum Flare. Die eigene Prüfung des Clients bewertete
  // denselben Anflug als stabil (`stable_at_gate=true`, Abweichung
  // 46 fpm) — und das Banner zeigte trotzdem ab 100 ft durchgehend die
  // rote „PULL UP"-Meldung.
  //
  // Erfundene Zahlen wären hier wertlos: Der Fehler ist nur aufgefallen,
  // weil ein echter Anflug ihn ausgelöst hat.
  const dlh373 = (aglFt: number) =>
    snap({
      altitude_agl_ft: aglFt,
      vertical_speed_fpm: -723,
      groundspeed_kt: 139,
    });

  it("DLH 373: kein Alarm bei 93 ft — das ist der 3°-Pfad, nicht zu steil", () => {
    // 139 kt × 5,31 = 738 fpm Soll. −723 ist FLACHER als das Soll.
    expect(evaluateApproach(dlh373(93), "final", 1, 3)).toBeNull();
  });

  it("DLH 373: auch bei 150 und 220 ft ruhig", () => {
    expect(evaluateApproach(dlh373(150), "final", 1, 3)).toBeNull();
    expect(evaluateApproach(dlh373(220), "final", 1, 3)).toBeNull();
  });

  it("aber eine ECHTE Übersinkrate meldet es weiterhin", () => {
    // Doppelte Soll-Sinkrate bei derselben Geschwindigkeit.
    const zuSteil = snap({
      altitude_agl_ft: 93,
      vertical_speed_fpm: -1500,
      groundspeed_kt: 139,
    });
    expect(evaluateApproach(zuSteil, "final", 1, 3)?.key).toBe("sink_rate_pull_up");
  });

  it("langsames Muster: dieselbe Rate ist dort sehr wohl zu steil", () => {
    // PC-12 mit 95 kt: Soll ist 504 fpm. −723 sind dort 43 % zu viel —
    // die alte Festschwelle von 700 fpm hätte geschwiegen.
    const pc12 = snap({
      altitude_agl_ft: 93,
      vertical_speed_fpm: -723,
      groundspeed_kt: 95,
    });
    expect(evaluateApproach(pc12, "final", 1, 3)?.key).toBe("sink_rate_pull_up");
  });

  it("ohne Geschwindigkeit bleibt es bei den alten Festwerten", () => {
    // Ein alter Client liefert `groundspeed_kt` nicht. Dann lieber die
    // bekannte Näherung als gar keine Warnung.
    const ohneGs = snap({ altitude_agl_ft: 93, vertical_speed_fpm: -800 });
    expect(evaluateApproach(ohneGs, "final", 1, 3)?.key).toBe("sink_rate_pull_up");
  });
});

describe("evaluateApproach — die Schwelle wird nie strenger als vorher", () => {
  // Externe Abnahme 21.09.2026: Die geschwindigkeitsabhängige Schwelle
  // kehrte den behobenen Fehler bei langsamen Mustern um. Unter 100 ft
  // lag sie bei 40 kt auf −345 fpm, bei 60 kt auf −478 — ein Hubschrauber
  // oder Buschflieger mit 500 fpm hätte die rote Meldung bekommen, die es
  // nie gab. Deshalb ein Deckel: nie strenger als die alte Festzahl.
  it("Hubschrauber mit 45 kt und 500 fpm bleibt ruhig", () => {
    const heli = snap({
      altitude_agl_ft: 90,
      vertical_speed_fpm: -500,
      groundspeed_kt: 45,
    });
    expect(evaluateApproach(heli, "final", 1, 3)).toBeNull();
  });

  it("… aber bei 900 fpm meldet es auch dort", () => {
    const zuSteil = snap({
      altitude_agl_ft: 90,
      vertical_speed_fpm: -900,
      groundspeed_kt: 45,
    });
    expect(evaluateApproach(zuSteil, "final", 1, 3)?.key).toBe("sink_rate_pull_up");
  });

  it("die PC-12 bleibt trotzdem strenger als ein Jet", () => {
    // 95 kt: Soll −504, Schwelle −710 — unter dem Deckel von −700, also
    // greift die gerechnete. Der Deckel nimmt nur die Überstrenge weg,
    // nicht die Schärfe.
    const pc12 = snap({
      altitude_agl_ft: 93,
      vertical_speed_fpm: -723,
      groundspeed_kt: 95,
    });
    expect(evaluateApproach(pc12, "final", 1, 3)?.key).toBe("sink_rate_pull_up");
  });
});

describe("evaluateApproach — auch die obere Grenze wird nie strenger", () => {
  // Externe Nachprüfung 21.09.2026: `sink1000Hi = soll * 0.4` war NICHT
  // gedeckelt. Oberhalb von 141 kt war die Grenze für zu flaches Sinken
  // strenger als die alte Festzahl (160 kt → −340 statt −300), und die
  // Release-Notes sagten dem Piloten das Gegenteil zu.
  it("160 kt mit −320 fpm meldet nicht — wie vorher", () => {
    const flach = snap({
      altitude_agl_ft: 800,
      vertical_speed_fpm: -320,
      groundspeed_kt: 160,
    });
    expect(evaluateApproach(flach, "approach", 1, 3)).toBeNull();
  });

  it("… und bei −250 fpm meldet es, ebenfalls wie vorher", () => {
    const zuFlach = snap({
      altitude_agl_ft: 800,
      vertical_speed_fpm: -250,
      groundspeed_kt: 160,
    });
    expect(evaluateApproach(zuFlach, "approach", 1, 3)?.key).toBe("gate1000_unstable");
  });
});
