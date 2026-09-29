// Gleitpfad und Anflugruhe im Landungs-Tab — Lernpaket AP4/AP5 (29.09.2026).
//
// Geprüft wird, was der Pilot liest: Werte mit Vorzeichen, die Quelle des
// Bezugs, der Grund, wenn es keine Werte gibt — und dass jede Quelle und
// jeder Grund, den das RUST-Backend erzeugen kann, in allen drei Sprachen
// einen Text hat (sonst stünde der rohe Schlüssel in der Anzeige).
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import fs from "node:fs";
import path from "node:path";
import { AnflugForensikInfo } from "./AnflugForensikInfo";

const WURZEL = path.resolve(__dirname, "../..");

describe("AnflugForensikInfo", () => {
  it("zeigt Gleitpfad mit Vorzeichen, Tore und Quelle", () => {
    render(
      <AnflugForensikInfo
        gleitpfad={{
          quelle: "navigraph_ils",
          winkel_deg: 3,
          tch_ft: 50,
          gesamt: { proben: 62, mittel_abs_dots: 0.42, max_dots: -1.27, max_abw_ft: -41.4 },
          tor_1000_500: { proben: 40, mittel_abs_dots: 0.3, max_dots: 0.61, max_abw_ft: 30 },
          tor_500_200: { proben: 22, mittel_abs_dots: 0.6, max_dots: -1.27, max_abw_ft: -41.4 },
        }}
        ruhe={null}
      />,
    );
    const text = screen.getByTestId("anflug-forensik").textContent ?? "";
    expect(text).toContain("im Mittel 0,4 Dots, größte Abweichung −1,3 Dots (−41 ft) · 62 Messpunkte");
    expect(text).toContain("1000–500 ft: im Mittel 0,3, größte +0,6 Dots");
    expect(text).toContain("500–200 ft: im Mittel 0,6, größte −1,3 Dots");
    expect(text).toContain("Bezug: ILS-Gleitweg (Navigraph) · 3,0° · TCH 50 ft");
    expect(text).toContain("fließt nicht in die Note ein");
    expect(text).not.toContain("landing.anflug_forensik");
  });

  it("nennt den Grund statt Werte und markiert eine angenommene TCH", () => {
    const { rerender } = render(
      <AnflugForensikInfo
        gleitpfad={{
          quelle: "navigraph_bahn",
          winkel_deg: 3,
          tch_ft: 50,
          tch_angenommen: true,
          grund_ohne_werte: "schwellenhoehe_fehlt",
        }}
      />,
    );
    let text = screen.getByTestId("anflug-forensik").textContent ?? "";
    expect(text).toContain("keine Schwellenhöhe");
    expect(text).toContain("TCH 50 ft (angenommen");
    expect(text).not.toContain("Messpunkte");

    rerender(
      <AnflugForensikInfo
        gleitpfad={{
          quelle: "angenommen_3grad",
          winkel_deg: 3,
          tch_ft: 50,
          grund_ohne_werte: "keine_bahn",
        }}
      />,
    );
    text = screen.getByTestId("anflug-forensik").textContent ?? "";
    expect(text).toContain("Ohne Navdaten zur gelandeten Bahn");
    expect(text).toContain("Bezug: keine Navdaten zur gelandeten Bahn");
  });

  it("zeigt die Anflugruhe nur mit gemessenen Teilen und sagt, wenn Schub fehlt", () => {
    render(
      <AnflugForensikInfo
        ruhe={{
          hoehenbezug: "schwelle",
          tor_1000_500: {
            proben: 40,
            dauer_s: 39,
            pfad_vorzeichenwechsel: 2,
            nick_unruhe_deg_s: 0.84,
            roll_unruhe_deg_s: 1.26,
            schub_umkehr_pro_min: null,
          },
          tor_500_200: null,
        }}
      />,
    );
    const text = screen.getByTestId("anflug-forensik").textContent ?? "";
    expect(text).toContain(
      "1000–500 ft: 2× Seitenwechsel am Pfad · Nickrate ±0,8 °/s · Rollrate ±1,3 °/s",
    );
    expect(text).not.toContain("Schubwechsel/min");
    expect(text).toContain("ohne N1-Daten nicht erfasst");
    expect(text).not.toContain("500–200 ft:");
  });

  it("rendert nichts bei alten Landungen ohne die Felder", () => {
    const { container } = render(<AnflugForensikInfo gleitpfad={undefined} ruhe={null} />);
    expect(container.innerHTML).toBe("");
  });

  it("jede Quelle und jeder Grund aus dem Rust-Backend hat in allen Sprachen einen Text", () => {
    const rust = fs.readFileSync(
      path.join(WURZEL, "src-tauri/src/anflug_forensik.rs"),
      "utf-8",
    );
    const werte = (praefix: string) =>
      [...rust.matchAll(new RegExp(`const ${praefix}_[A-Z0-9_]+: &str = "([a-z0-9_]+)"`, "g"))].map(
        (m) => m[1]!,
      );
    const quellen = werte("QUELLE");
    const gruende = werte("GRUND");
    // Riegel: ohne Treffer prüfte der Test nichts.
    expect(quellen).toEqual(["navigraph_ils", "navigraph_bahn", "angenommen_3grad"]);
    expect(gruende).toEqual(["keine_bahn", "schwellenhoehe_fehlt", "keine_proben"]);
    for (const code of ["de", "en", "it"]) {
      const daten = JSON.parse(
        fs.readFileSync(path.join(WURZEL, "src/locales", code, "common.json"), "utf-8"),
      );
      const af = daten.landing.anflug_forensik;
      for (const q of quellen) expect(af.quelle[q], `${code}: quelle.${q}`).toBeTruthy();
      for (const g of gruende) expect(af.grund[g], `${code}: grund.${g}`).toBeTruthy();
    }
  });
});
