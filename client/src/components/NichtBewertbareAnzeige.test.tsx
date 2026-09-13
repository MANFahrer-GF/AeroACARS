// Was der Pilot sieht, wenn die Landung nicht gemessen werden konnte.
//
// Codex-Abnahme 12.09.2026 zum ersten Entwurf: Die damalige Testdatei prüfte
// nur zwei Hilfsfunktionen — "Keine dieser Ausgabestellen wird gerendert oder
// geprüft." Genau daran scheiterte der Entwurf an fünf Stellen: Die Tabelle
// erzeugte über `?? 0` wieder eine angezeigte 0, der Bestleistungsvergleich
// zeigte "0 fpm", Note und Punktzahl standen unverändert da, Teilwerte liefen
// ungeprüft durch, und zwei gesperrte Einträge brachten das Verlaufsdiagramm
// mit `Cannot read properties of undefined` zum Absturz.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { LandingDetail } from "./LandingPanel";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";

/** Ein Datensatz wie CFG 2090: erkannt, aber ohne verwertbare Messung. */
function nichtBewertbar() {
  const basis = MOCK_LANDING_OPTIONS[0]!.build();
  return {
    ...basis,
    score_numeric: null,
    score_label: null,
    grade_letter: null,
    landing_rate_fpm: null,
    landing_peak_vs_fpm: null,
    vs_at_edge_fpm: null,
    landing_peak_g_force: null,
    landung_nicht_bewertbar: { groesste_luecke_ms: 920, proben: 5 },
    sampler_diagnose: { proben_je_sekunde: 2.0, laeufe: 10, proben: 10 },
  };
}

function zeige(records: ReturnType<typeof nichtBewertbar>[]) {
  return render(
    <LandingDetail
      record={records[0] as never}
      allRecords={records as never}
      onBack={() => {}}
    />,
  );
}

describe("Anzeige einer nicht bewertbaren Landung", () => {
  it("erklärt, warum es keine Note gibt — mit den gemessenen Zahlen", () => {
    const { container } = zeige([nichtBewertbar()]);
    const text = container.textContent ?? "";
    expect(text).toContain("Aufzeichnung unvollständig");
    expect(text, "die gemessene Lücke gehört in die Begründung").toContain("920");
    expect(text, "und die Zahl der Messpunkte").toContain("5");
  });

  it("zeigt keine erfundene Null als Sinkrate", () => {
    const { container } = zeige([nichtBewertbar()]);
    const text = container.textContent ?? "";
    expect(text).not.toMatch(/\b0 fpm\b/);
    expect(text).not.toMatch(/\b0\/100\b/);
  });

  it("stürzt nicht ab, wenn alle Landungen ungemessen sind", () => {
    // Der Absturzfall aus der Abnahme: Das Verlaufsdiagramm prüfte die
    // Anzahl vor dem Filtern.
    const { container } = zeige([nichtBewertbar(), nichtBewertbar()]);
    expect(container.textContent ?? "").toContain("Aufzeichnung unvollständig");
  });

  it("zeigt weder G-Werte noch Hopser aus dem kaputten Fenster", () => {
    // Prüfbefund 13.09.2026: Sinkrate und Note waren gesperrt, die
    // G-Forensik daneben lief weiter — der Client zeigte volle G-Kacheln
    // und ein "Hard G"-Flag für eine Landung, die niemand gemessen hat.
    //
    // Geprüft wird am konkreten Wert des Datensatzes (1,52 G aus der
    // Demo-Sammlung): Er darf nirgends auftauchen.
    const gesperrt = zeige([nichtBewertbar()]).container.textContent ?? "";
    expect(gesperrt, "kein G-Wert aus dem ungemessenen Fenster").not.toContain("1.52");
    expect(gesperrt, "und kein Hopser-Flag").not.toMatch(/Hopser ×/);

    // Gegenprobe: bei gemessener Landung steht der Wert weiterhin da.
    const gut = MOCK_LANDING_OPTIONS[0]!.build();
    const { container } = render(
      <LandingDetail record={gut as never} allRecords={[gut] as never} onBack={() => {}} />,
    );
    expect(container.textContent ?? "").toContain("1.52");
  });

  it("nennt die Landung nicht 'fest', nur weil die Note fehlt", () => {
    // `recordCategory` fiel bei fehlendem Label in den Standardzweig und
    // lieferte "firm" — im Druckbericht stand dann "FEST".
    const { container } = zeige([nichtBewertbar()]);
    expect(container.textContent ?? "").not.toMatch(/\bFEST\b/);
  });

  it("lässt eine gemessene Landung unverändert", () => {
    const gut = MOCK_LANDING_OPTIONS[0]!.build();
    const { container } = render(
      <LandingDetail record={gut as never} allRecords={[gut] as never} onBack={() => {}} />,
    );
    const text = container.textContent ?? "";
    expect(text).not.toContain("Aufzeichnung unvollständig");
    expect(text).toContain("/100");
  });
});
