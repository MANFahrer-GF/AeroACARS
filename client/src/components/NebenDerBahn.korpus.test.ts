// Die 11 Landungen mit Abzug „neben der Bahn“ seit 06.10.2026 (live,
// sqlite -readonly, 22.10.2026; `dev/bahnansicht/fixtures/neben_der_bahn_11.json`).
//
// Thomas (Entscheidung 3): Die Marke sagt Phase und Ort — aus den
// gespeicherten Teilnoten, für den Altbestand genauso. Die BEWERTUNG bleibt
// unverändert: Die Anzeige rechnet keine Note, und die geänderte Hopser-
// Tabelle gibt ohne gezählte leichte Hopser (Altbestand: Feld fehlt) exakt
// die alten Punkte. Beides wird hier an genau diesen 11 Landungen belegt.
//
// Auf dem alten Stand rot: Die Marke hiess bei allen 11 nur „NEBEN DER BAHN“.

import { describe, expect, it } from "vitest";
import i18n from "../i18n";
import daten from "../dev/bahnansicht/fixtures/neben_der_bahn_11.json";
import { nebenDerBahnPhase, nebenDerBahnText, landungsMarkenV19 } from "../lib/landungsUrteil";
import { nebenDerBahnOrt } from "./LandungsBewertung";
import { computeSubScores } from "../lib/landingScoring";
import type { LandingRecord } from "../lib/landungsDatensatz";

type Fall = (typeof daten.faelle)[number];
const FAELLE = daten.faelle as Fall[];
const t = i18n.getFixedT("de");

// Erwartung je Fall, aus den gespeicherten Teilnoten abgelesen (Checkliste
// Punkt 7: fünf nur beim Ausrollen, sechs beim Aufsetzen — davon drei auch
// beim Ausrollen).
const PHASE: Record<number, "aufsetzen" | "ausrollen" | "beides"> = {
  1914: "ausrollen",
  1896: "ausrollen",
  1863: "ausrollen",
  1844: "ausrollen",
  1779: "ausrollen",
  1886: "beides",
  1853: "beides",
  1842: "beides",
  1741: "aufsetzen",
  1738: "aufsetzen",
  1662: "aufsetzen",
};

describe("„neben der Bahn“ — die 11 Landungen seit 06.10.", () => {
  it("sind genau die 11 mit dem Abzug", () => {
    expect(FAELLE.map((f) => f.id).sort()).toEqual(Object.keys(PHASE).map(Number).sort());
    for (const f of FAELLE) expect(f.landing_score_deckel).toMatch(/neben_der_bahn/);
  });

  for (const f of FAELLE) {
    it(`#${f.id} ${f.airport} ${f.runway}: Phase ${PHASE[f.id]} und Ort`, () => {
      expect(nebenDerBahnPhase(f.sub_scores as never)).toBe(PHASE[f.id]);
      const ort = nebenDerBahnOrt(f as unknown as LandingRecord);
      const text = nebenDerBahnText(f.sub_scores as never, ort, t);
      const erwartetWort = { aufsetzen: "beim Aufsetzen", ausrollen: "beim Ausrollen", beides: "beim Aufsetzen und Ausrollen" }[PHASE[f.id]!];
      expect(text).toContain(`NEBEN DER BAHN ${erwartetWort}`);
      // Ort: Aufsetzen = Aufsetzpunkt ab der Schwelle; Ausrollen = die Stelle des gespeicherten Randabstands.
      if (PHASE[f.id] !== "ausrollen" && f.td_distance_from_threshold_m != null)
        expect(text).toContain(new Intl.NumberFormat("de-DE").format(Math.round(f.td_distance_from_threshold_m)));
      // Dieselbe Marke steht in der Markenliste (Kopf von Client und Webapp).
      const marken = landungsMarkenV19(
        { subs: f.sub_scores as never, deckel: f.landing_score_deckel, urteil: null, bounceCount: f.scored_bounce_count ?? 0, nebenDerBahnOrt: ort },
        t,
      );
      expect(marken.map((m) => m.label)).toContain(text);
    });
  }

  it("#1914: „beim Ausrollen (bei 1.534 m)“ — Thomas’ Beispiel", () => {
    const f = FAELLE.find((x) => x.id === 1914)!;
    const text = nebenDerBahnText(f.sub_scores as never, nebenDerBahnOrt(f as unknown as LandingRecord), t);
    expect(text).toMatch(/^NEBEN DER BAHN beim Ausrollen \(bei 1\.5[23]\d m\)$/);
  });

  it("Bewertung unverändert: Hopser-Teilnote des Altbestands = gespeicherte Punkte (TS-Zwilling)", () => {
    for (const f of FAELLE) {
      const gespeichert = f.sub_scores.find((s) => s.key === "bounces")!;
      // Altbestand: kein `light_bounce_count` → alte Tabelle.
      const neu = computeSubScores({ bounce_count: f.scored_bounce_count ?? 0 }).find((s) => s.key === "bounces")!;
      expect(neu.points, `#${f.id}`).toBe(gespeichert.points);
    }
  });

  it("Bewertung unverändert: Gesamt- und Teilnoten werden nur gelesen, nie gerechnet", () => {
    // Die Anzeige zeigt `landing_score`/`sub_scores` aus dem Datensatz. Ein
    // Fall, an dem das brechen würde, wäre eine neue Rechnung in der Anzeige —
    // die Marke darf die Punkte nicht anfassen.
    for (const f of FAELLE) {
      const vorher = JSON.stringify(f.sub_scores);
      nebenDerBahnText(f.sub_scores as never, nebenDerBahnOrt(f as unknown as LandingRecord), t);
      expect(JSON.stringify(f.sub_scores)).toBe(vorher);
    }
  });
});
