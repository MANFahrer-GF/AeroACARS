// Gleichheitstest Client ↔ Webapp, Client-Seite (QS 05.10.2026).
//
// Rendert die Landungsseite des Clients mit den Fällen aus
// lib/landungsFaelle.ts und prüft gegen deren Erwartung. Die Webapp prüft
// ihre Landungsanalyse gegen DIESELBE Datei (gespiegelt, byte-gleich):
// webapp/src/__tests__/landungsFaelle.test.tsx. Weichen die Seiten ab, wird
// eine der beiden rot.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { LandingDetail, type LandingRecord } from "../components/LandingPanel";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { LANDUNGS_FAELLE, type LandungsFall } from "./landungsFaelle";

function datensatz(f: LandungsFall): LandingRecord {
  const basis = MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord;
  return {
    ...basis,
    score_numeric: f.landing_score,
    score_label: f.landing_score_label,
    grade_letter: f.landing_score_grade,
    score_deckel: f.landing_score_deckel,
    score_algorithm_version: f.score_algorithm_version,
    ux_version: 1,
    bounce_count: f.bounce_count,
    forensic_bounce_count: 0,
    sub_scores: f.sub_scores,
  } as LandingRecord;
}

describe.each(LANDUNGS_FAELLE.map((f) => [f.name, f] as const))(
  "Gleiche Anzeige wie die Webapp — %s",
  (_name, fall) => {
    const zeige = () => {
      const r = datensatz(fall);
      return render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
    };

    // 05.10.2026 (Thomas: jeder Wert erklärt, eine Variante — der (i)-Knopf):
    // Jede Beschriftung eines Werts trägt einen (i)-Knopf, keine Erklärung
    // steckt mehr nur im Hover-Titel.
    it("jeder Wert hat einen (i)-Knopf", () => {
      // Mit echtem METAR (LTFM, THY39), damit auch dessen Einträge geprüft werden.
      const r = { ...datensatz(fall), arr_metar: "LTFM 052020Z 06009KT 9999 SCT026 17/12 Q1022 NOSIG" };
      const { container } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
      const ohne = (sel: string) =>
        [...container.querySelectorAll(sel)]
          .filter((e) => !e.querySelector(".info-badge"))
          .map((e) => `${sel}: ${e.textContent}`);
      const beschriftungen = [
        ".landing-keyvals dt",
        ".metar-eintrag__k",
        ".landing-section--quality .sinkrate-tile__label",
        '[data-testid="gate-kacheln"] > div > div:first-child',
      ];
      for (const sel of beschriftungen) {
        expect(container.querySelectorAll(sel).length, `${sel} kommt vor`).toBeGreaterThan(0);
      }
      expect(beschriftungen.flatMap(ohne)).toEqual([]);
      // Forensik-Abschnitte (je nach Datenlage mit oder ohne 50-Hz-Werte):
      // die Überschrift erklärt, welcher Wert bewertet wird.
      const forensik = [...container.querySelectorAll("section h3")].filter((h) =>
        /forensik/i.test(h.textContent ?? ""),
      );
      expect(forensik.length).toBeGreaterThan(0);
      expect(forensik.filter((h) => !h.querySelector(".info-badge")).map((h) => h.textContent)).toEqual([]);
      expect(container.querySelector(".landing-headline .info-badge"), "Note im Kopf").not.toBeNull();
      // Keine Erklärung mehr nur als Hover-Titel auf einer Wertkachel.
      expect(container.querySelectorAll(".landing-keyvals > div[title], .sinkrate-tile[title]").length).toBe(0);
    });

    it("Note, Wort und Deckel-Satz im Kopf", () => {
      const { container, queryByTestId } = zeige();
      const kopf = container.querySelector(".landing-headline")!.textContent ?? "";
      expect(kopf).toContain(`${fall.landing_score}/100`);
      expect(kopf.toLowerCase()).toContain(fall.erwartet.wort);
      expect(queryByTestId("kopf-deckel")?.textContent ?? null).toBe(fall.erwartet.deckel);
    });

    it("dieselben Hinweis-Marken in derselben Reihenfolge", () => {
      const { container } = zeige();
      const marken = [...container.querySelectorAll(".landing-flags .landing-flag")].map(
        (e) => e.textContent,
      );
      expect(marken).toEqual(fall.erwartet.marken);
    });

    it("Anflug-Karte: Urteil und Gründe", () => {
      const { container, queryByTestId } = zeige();
      expect(container.textContent).toContain(fall.erwartet.pille);
      const gruende = [...(queryByTestId("gate-gruende")?.querySelectorAll("li") ?? [])].map(
        (e) => e.textContent,
      );
      expect(gruende).toEqual(fall.erwartet.gruende);
    });
  },
);
