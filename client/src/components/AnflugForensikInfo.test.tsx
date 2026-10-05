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

function text(): string {
  return screen.getByTestId("anflug-forensik").textContent ?? "";
}

describe("AnflugForensikInfo", () => {
  it("zeigt Gleitpfad mit Vorzeichen, Tore und Quelle", () => {
    render(
      <AnflugForensikInfo
        gleitpfad={{
          quelle: "navigraph_ils",
          winkel_deg: 3,
          tch_ft: 50,
          grad_je_dot: 0.35,
          hoehenbezug: "navigraph",
          gesamt: {
            proben: 62,
            mittel_abs_dots: 0.42,
            max_dots: -1.27,
            max_abw_ft: -41.4,
            oberste_hoehe_ft: 996,
          },
          tor_1000_500: {
            proben: 40,
            mittel_abs_dots: 0.3,
            max_dots: 0.61,
            max_abw_ft: 30,
            oberste_hoehe_ft: 996,
          },
          tor_500_200: {
            proben: 22,
            mittel_abs_dots: 0.6,
            max_dots: -1.27,
            max_abw_ft: -41.4,
            oberste_hoehe_ft: 499,
          },
        }}
        ruhe={null}
      />,
    );
    const t = text();
    expect(t).toContain(
      "im Mittel 0,4 Dots, größte Abweichung −1,3 Dots (−41 ft) · 62 Messpunkte",
    );
    expect(t).toContain("1000–500 ft: im Mittel 0,3, größte +0,6 Dots");
    expect(t).toContain("500–200 ft: im Mittel 0,6, größte −1,3 Dots");
    expect(t).toContain("Bezug: ILS-Gleitweg (Navigraph) · 3,0° · TCH 50 ft");
    expect(t).toContain("1 Dot = 0,35°");
    expect(t).toContain("zählt ab AeroACARS 1.9.26 zum Stable Gate");
    // Vollständig erfasst → kein Hinweis, kein Sim-Boden-Hinweis.
    expect(t).not.toContain("erfasst ab");
    expect(t).not.toContain("Boden des Simulators");
    expect(t).not.toContain("landing.anflug_forensik");
  });

  it("nennt einen Steilanflug-Dot, ein spät erfasstes Tor und den Sim-Boden", () => {
    const tor = {
      proben: 10,
      mittel_abs_dots: 0.2,
      max_dots: 0.5,
      max_abw_ft: 20,
      oberste_hoehe_ft: 720,
    };
    render(
      <AnflugForensikInfo
        gleitpfad={{
          quelle: "navigraph_ils",
          winkel_deg: 5.5,
          tch_ft: 50,
          grad_je_dot: 0.6417,
          hoehenbezug: "sim_boden",
          schwellenhoehe_navigraph_ft: 300,
          sim_boden_ft: 330,
          gesamt: { ...tor, proben: 30 },
          tor_1000_500: tor,
        }}
      />,
    );
    const t = text();
    expect(t).toContain("1 Dot = 0,64°");
    expect(t).toContain("1000–500 ft: im Mittel 0,2, größte +0,5 Dots (erfasst ab 720 ft)");
    expect(t).toContain(
      "Boden des Simulators an der Schwelle (330 ft; Navdaten nennen 300 ft)",
    );
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
    let t = text();
    expect(t).toContain("keine Schwellenhöhe");
    expect(t).toContain("TCH 50 ft (angenommen");
    expect(t).not.toContain("Messpunkte");

    // Ohne Bahn liefert das Backend keinen Winkel und keine TCH — dann
    // steht auch kein „3,0°" da, das eine Rechnung vortäuschen würde.
    rerender(
      <AnflugForensikInfo
        gleitpfad={{ quelle: "angenommen_3grad", grund_ohne_werte: "keine_bahn" }}
      />,
    );
    t = text();
    expect(t).toContain("Ohne Navdaten zur gelandeten Bahn");
    expect(t).toContain("Bezug: keine Navdaten zur gelandeten Bahn");
    expect(t).not.toContain("3,0°");
  });

  it("zeigt die Anflugruhe mit gemessenen Teilen und nennt den Schub-Grund", () => {
    render(
      <AnflugForensikInfo
        ruhe={{
          hoehenbezug: "schwelle",
          tor_1000_500: {
            proben: 40,
            dauer_s: 39,
            oberste_hoehe_ft: 995,
            pfad_vorzeichenwechsel: 2,
            nick_unruhe_deg_s: 0.84,
            roll_unruhe_deg_s: 1.26,
            schub_umkehr_pro_min: null,
            schub_grund: "kein_n1",
          },
          tor_500_200: {
            proben: 6,
            dauer_s: 5,
            oberste_hoehe_ft: 490,
            roll_unruhe_deg_s: 0.5,
            schub_umkehr_pro_min: null,
            schub_grund: "zu_kurz",
          },
        }}
      />,
    );
    const t = text();
    expect(t).toContain(
      "1000–500 ft: 2× Seitenwechsel am Pfad · Nickrate ±0,8 °/s · Rollrate ±1,3 °/s · Schub ohne N1-Daten nicht erfasst",
    );
    // N1 war da, aber zu kurz — das ist etwas anderes als „ohne N1".
    expect(t).toContain("500–200 ft: Rollrate ±0,5 °/s · Schub zu kurz gemessen");
    expect(t).not.toContain("Schubwechsel/min");
  });

  it("rendert nichts bei alten Landungen ohne die Felder", () => {
    const { container } = render(<AnflugForensikInfo gleitpfad={undefined} ruhe={null} />);
    expect(container.innerHTML).toBe("");
  });

  it("jede Quelle und jeder Grund aus dem Rust-Backend hat in allen Sprachen einen Text", () => {
    const rust = fs.readFileSync(path.join(WURZEL, "src-tauri/src/anflug_forensik.rs"), "utf-8");
    const werte = (praefix: string) =>
      [
        ...rust.matchAll(new RegExp(`const ${praefix}_[A-Z0-9_]+: &str = "([a-z0-9_]+)"`, "g")),
      ].map((m) => m[1]!);
    const quellen = werte("QUELLE");
    const gruende = werte("GRUND");
    const schub = werte("SCHUB_GRUND");
    // Riegel: ohne Treffer prüfte der Test nichts.
    expect(quellen).toEqual(["navigraph_ils", "navigraph_bahn", "angenommen_3grad"]);
    expect(gruende).toEqual(["keine_bahn", "schwellenhoehe_fehlt", "keine_proben"]);
    expect(schub).toEqual(["kein_n1", "zu_kurz"]);
    for (const code of ["de", "en", "it"]) {
      const daten = JSON.parse(
        fs.readFileSync(path.join(WURZEL, "src/locales", code, "common.json"), "utf-8"),
      );
      const af = daten.landing.anflug_forensik;
      for (const q of quellen) expect(af.quelle[q], `${code}: quelle.${q}`).toBeTruthy();
      for (const g of gruende) expect(af.grund[g], `${code}: grund.${g}`).toBeTruthy();
      for (const s of schub) expect(af.schub_grund[s], `${code}: schub_grund.${s}`).toBeTruthy();
    }
  });
});
