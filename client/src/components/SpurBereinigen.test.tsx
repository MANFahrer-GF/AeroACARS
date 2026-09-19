// Altbestand mit fehlerhafter Spur — durch die echte Anzeige.
//
// Zwei Fehlerbilder aus dem Client (behoben in fix/durchstart-spur) stecken
// in gespeicherten Landungen und lassen sich dort nicht nachbessern. Die
// Anzeige muss sie selbst erkennen. Die Punktfolgen unten sind echten
// Landungen vom Live-Recorder entnommen (gekürzt), nicht erfunden.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { mapLandingRecordToV2Props } from "../dev/runwayDiagramV2Mapper";
import { RunwayDiagramV2 } from "./RunwayDiagramV2";
import { spurBereinigen } from "./RunwayDisciplinePanel";

const p = (laengs_m: number, quer_m: number) => ({ laengs_m, quer_m });

/** EWG9503 (#1431, EDDL 23L): Startlauf nach dem Durchstarten vorn. */
const DURCHSTART = [
  p(2232.1, 9.2), p(2246.5, 9.5), p(2260.7, 9.8), p(2279.9, 10.2), p(2293.9, 10.5), p(2313.1, 11),
  p(595, 1.2), p(642, 1.7), p(698.6, 1.8), p(754, 2.1), p(809.2, 2.6), p(856.5, 2.8),
  p(899.1, 2.3), p(951.2, 1.6), p(1010.4, 0.9), p(1061.4, 0.4), p(1118.1, 0.2), p(1165, 0.3),
  p(1222.6, 0.5), p(1279, 0.8), p(1336.7, 1.2), p(1397.5, 1.7), p(1454.6, 2.2), p(1510.5, 2.5),
];

/** LPPT 02 (#1403): die letzte Probe kehrt zwischen den Live-Punkten wieder. */
const SAEGEZAHN = [
  p(903.2, 1.9), p(913.9, 0.9), p(923.6, 0), p(933.6, -0.9), p(947.6, -2.3), p(959.2, -3.4),
  p(973.9, -4.8), p(1117, -15.1), p(976.4, -5), p(1130.4, -15.7), p(976.4, -5), p(1141.8, -16.2),
  p(976.4, -5), p(1154.5, -16.8),
];

describe("spurBereinigen", () => {
  it("wirft den Startlauf vor dem Aufsetzen weg", () => {
    const s = spurBereinigen(DURCHSTART);
    expect(s[0]).toEqual(p(595, 1.2));
    expect(s).toHaveLength(DURCHSTART.length - 6);
  });

  it("nimmt die wiederkehrende Probe heraus, nicht die Live-Punkte", () => {
    const s = spurBereinigen(SAEGEZAHN);
    expect(s.map((x) => x.laengs_m)).toEqual([
      903.2, 913.9, 923.6, 933.6, 947.6, 959.2, 973.9, 1117, 1130.4, 1141.8, 1154.5,
    ]);
  });

  it("lässt eine Ausfahrt stehen, die schräg nach hinten abgeht", () => {
    // Gegenprobe zur Schwelle: echte Rückwärtsbewegung kommt in Schritten
    // des Mindestabstands. Eine Regel „nur vorwärts" schnitte sie ab.
    const ausfahrt = [
      p(600, 0.5), p(1100, 0.2), p(1800, 0), p(1812, -3), p(1820, -12), p(1814, -22), p(1805, -31),
      p(1796, -40),
    ];
    expect(spurBereinigen(ausfahrt)).toEqual(ausfahrt);
  });

  it("lässt eine gesunde Spur unverändert", () => {
    const gesund = DURCHSTART.slice(6);
    expect(spurBereinigen(gesund)).toEqual(gesund);
  });
});

describe("Altbestand durch die echte Anzeige", () => {
  // Eine Variante mit Spur, Breite und Spurweite — dort wird die Queransicht
  // gezeichnet. Nur Spur und Grösstwert werden durch den Feldfall ersetzt.
  const basis = MOCK_LANDING_OPTIONS.map((o) => mapLandingRecordToV2Props(o.build())!).find(
    (v) =>
      (v.lateral_samples?.length ?? 0) > 5 &&
      v.runway_width_m != null &&
      v.track_width_m != null &&
      !v.lateral_skip_reason,
  )!;

  const zeichne = (samples: Array<{ laengs_m: number; quer_m: number }>, max: number) => {
    const markup = renderToStaticMarkup(
      <RunwayDiagramV2
        {...basis}
        lateral_samples={samples}
        max_lateral_offset_m={max}
        td_distance_from_threshold_m={595}
        // Mit Räumpunkt: sonst trägt der Endpunkt die freie Nummer 2.
        clearance_point_m={1600}
        clearance_side="left"
        scoring_cutoff_m={null}
        mess_ende_laengs_m={null}
        overrun_m={null}
      />,
    );
    const quer = (markup.match(/<svg[\s\S]*?<\/svg>/g) ?? []).find((s) =>
      s.includes("Queransicht"),
    );
    return { markup, quer: quer ?? "" };
  };

  it("zeichnet nur die bereinigten Messpunkte", () => {
    const { quer } = zeichne(DURCHSTART, 2.8);
    expect(quer).not.toBe("");
    const punkte = quer.match(/<circle[^>]*r="1.8"/g) ?? [];
    expect(punkte).toHaveLength(DURCHSTART.length - 6);
  });

  it("setzt die Marke ② an den echten Grösstversatz und nennt den bewerteten Wert", () => {
    // Gespeichert und bewertet: 11,0 m — aus den Fremdpunkten. Die echte
    // Spur kommt nicht über 2,8 m (bei 856,5 m). Thomas: „Punkt 2 nicht
    // vergessen, den bewerten wir doch." Also: Marke ja, aber dort, wo die
    // Spur ihren Grösstwert hat — und der bewertete Wert steht dabei.
    const falsch = zeichne(DURCHSTART, 11.0);
    const ziffern = [...falsch.quer.matchAll(/<text[^>]*fill="#0B0F17"[^>]*>(\d)<\/text>/g)].map(
      (m) => m[1],
    );
    expect(ziffern).toContain("2");
    expect(falsch.markup).toMatch(/bei 857 m/);
    expect(falsch.markup).toMatch(/bewertet wurden 11\.0 m/);

    // Gegenprobe: Trägt die Spur den Wert, gibt es keinen Hinweis.
    const richtig = zeichne(DURCHSTART, 2.8);
    expect(richtig.markup).not.toMatch(/bewertet wurden/);
    expect(richtig.markup).toMatch(/bei 857 m/);
  });

  it("zeichnet den ersten Durchgang getrennt und nennt ihn in der Liste", () => {
    // EWG9503: erstes Aufsetzen bei 831 m, durchgestartet (#1428).
    const erster = [
      p(831.5, 0.3), p(855, 0.5), p(902, 0.9), p(959, 1.6), p(1004, 2), p(1059, 1.9),
      p(1117, 1.6), p(1175, 0.9), p(1232, 0.3), p(1291, -0.4), p(1350, -0.9), p(1421, -0.9),
    ];
    const markup = renderToStaticMarkup(
      <RunwayDiagramV2
        {...basis}
        lateral_samples={DURCHSTART.slice(6)}
        max_lateral_offset_m={2.8}
        vorherige_durchgaenge={[{ lateral_samples: erster }]}
      />,
    );
    const quer = (markup.match(/<svg[\s\S]*?<\/svg>/g) ?? []).find((x) => x.includes("Queransicht"))!;
    expect(quer).toMatch(/data-durchgang="1"/);
    expect(markup).toMatch(/Früherer Durchgang · aufgesetzt/);
    // Die Ziffern gehören der gewerteten Landung — der Durchgang bekommt keine.
    const ziffern = [...quer.matchAll(/<text[^>]*fill="#0B0F17"[^>]*>(\d)<\/text>/g)].map((m) => m[1]);
    expect(new Set(ziffern).size).toBe(ziffern.length);

    // Gegenprobe: ohne Durchgang nichts davon.
    const ohne = renderToStaticMarkup(
      <RunwayDiagramV2 {...basis} lateral_samples={DURCHSTART.slice(6)} max_lateral_offset_m={2.8} />,
    );
    expect(ohne).not.toMatch(/data-durchgang/);
    expect(ohne).not.toMatch(/Früherer Durchgang/);
  });
});
