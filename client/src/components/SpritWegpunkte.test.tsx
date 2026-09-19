// Sprit · Wegpunkt für Wegpunkt — was die Anzeige zusagt.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SpritWegpunkte, AMPEL_FARBE } from "./SpritWegpunkte";
import type { SpritWegpunkt } from "../lib/sprit";

/** Tausenderpunkte kommen je nach ICU als schmales Leerzeichen — gleichsetzen. */
const leer = (h: string) => h.replace(/[\u00a0\u202f]/g, " ");

const Z: SpritWegpunkt[] = [
  { ident: "EDDL", plan_an_bord_kg: 7700, ist_an_bord_kg: 7700, zustand: "gemessen", ampel: "gruen", landung_hochgerechnet_kg: 4520, zeit_ms: Date.UTC(2026, 8, 19, 8, 12) },
  { ident: "KORED", hoehe_ft: 36000, plan_an_bord_kg: 5930, ist_an_bord_kg: 5870, zustand: "gemessen", ampel: "gruen", landung_hochgerechnet_kg: 4450, zeit_ms: Date.UTC(2026, 8, 19, 9, 19) },
  { ident: "ADEKA", plan_an_bord_kg: 5710, ist_an_bord_kg: 5621, zustand: "uebersprungen", ampel: "gruen", landung_hochgerechnet_kg: 4410 },
  { ident: "RESMI", hoehe_ft: 36000, plan_an_bord_kg: 5480, ist_an_bord_kg: 5360, zustand: "gemessen", ampel: "gelb", landung_hochgerechnet_kg: 4348, zeit_ms: Date.UTC(2026, 8, 19, 9, 47) },
  { ident: "LUMAS", plan_an_bord_kg: 5330, zustand: "offen" },
  { ident: "LEPA", plan_an_bord_kg: 4520, min_an_bord_kg: 3790, zustand: "offen" },
];

describe("SpritWegpunkte", () => {
  it("zeigt zugeklappt den letzten überflogenen Wegpunkt in seiner Farbe", () => {
    const html = leer(renderToStaticMarkup(<SpritWegpunkte zeilen={Z} naechster={4} naechsterNm={38} />));
    expect(html).toContain("RESMI");
    expect(html).toContain("120 kg mehr verbraucht");
    expect(html).toContain(AMPEL_FARBE.gelb);
    expect(html).toContain("LUMAS");
    expect(html).not.toContain("<table");
  });

  it("zeigt aufgeklappt jede Zeile, übersprungene gekennzeichnet, Zukunft ohne Ist", () => {
    const html = leer(renderToStaticMarkup(<SpritWegpunkte zeilen={Z} naechster={4} offen />));
    expect(html).toContain("<table");
    expect(html).toMatch(/übersprungen · Direct/);
    // Übersprungen: gerechnet, nicht gemessen — mit ≈.
    expect(html).toMatch(/≈ 5 621/);
    // Weniger verbraucht wird als solches benannt (Thomas: „eher wenn wir mehr verbrauchen").
    expect(html).toContain("60 kg mehr verbraucht");
    expect(html).toContain("Min. laut OFP 3 790");
    expect((html.match(/data-zustand="offen"/g) ?? []).length).toBe(2);
    expect(html).toMatch(/keine Note/);
  });

  it("benennt weniger Verbrauch als weniger, nicht als negativen Mehrverbrauch", () => {
    const html = leer(renderToStaticMarkup(
      <SpritWegpunkte
        zeilen={[
          { ident: "A", plan_an_bord_kg: 5000, ist_an_bord_kg: 5000, zustand: "gemessen", ampel: "gruen" },
          { ident: "B", plan_an_bord_kg: 4000, ist_an_bord_kg: 4080, zustand: "gemessen", ampel: "gruen" },
        ]}
      />,
    ));
    expect(html).toContain("80 kg weniger verbraucht");
    expect(html).not.toContain("−80");
  });

  it("zeigt ohne Zeilen nichts", () => {
    expect(renderToStaticMarkup(<SpritWegpunkte zeilen={[]} />)).toBe("");
    expect(renderToStaticMarkup(<SpritWegpunkte zeilen={undefined} />)).toBe("");
  });
});
