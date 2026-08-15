/** Client und Live-Karte muessen dieselben Sektoren zeigen.
 *
 *  Der Client fragte lange nur eine einzelne Flugflaeche ab, die Live-Karte
 *  dagegen jedes Hoehenband auf einmal. Gemessen am 15.08.2026 waren das
 *  101 gegen 965 Flaechen und 28 gegen 306 Buchungen — die beiden Karten
 *  zeigten sichtbar Verschiedenes, obwohl dieselbe Quelle dahinterstand.
 *  Seitdem holt auch der Client alles und filtert selbst.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { ladeSektoren } from "./vatglassesKarte";
import { readFileSync } from "node:fs";

const leer = { type: "FeatureCollection", features: [] };
const antwort = {
  flaechen: leer, marken: leer, abgedeckt: [], gebucht: leer,
  atc: { plaetze: leer, firFlaechen: leer, firMarken: leer },
  tracon: { flaechen: leer, marken: leer, abgedeckt: [] },
};

afterEach(() => vi.restoreAllMocks());

describe("Sektor-Abruf", () => {
  it("reicht \"alle\" unveraendert an den Server durch", async () => {
    const holen = vi.fn().mockResolvedValue({ ok: true, json: async () => antwort });
    vi.stubGlobal("fetch", holen);
    await ladeSektoren("alle");
    expect(holen.mock.calls[0][0]).toContain("fl=alle");
  });

  it("rundet eine einzelne Flugflaeche weiterhin", async () => {
    const holen = vi.fn().mockResolvedValue({ ok: true, json: async () => antwort });
    vi.stubGlobal("fetch", holen);
    await ladeSektoren(247.6);
    expect(holen.mock.calls[0][0]).toContain("fl=248");
  });
});

describe("Kartenbild", () => {
  // Pfad ab dem Projektordner: `import.meta.url` ist unter dem
  // Test-Werkzeug keine Datei-Adresse.
  const quelle = readFileSync("src/components/LiveMapView.tsx", "utf8");

  it("holt alle Hoehenbaender statt einer Flugflaeche", () => {
    expect(quelle).toContain('ladeSektoren("alle"');
  });

  it("filtert die Hoehe auf der Karte", () => {
    expect(quelle).toContain("function hoehenfilterSetzen");
    // Auf allen drei Sektor-Ebenen, sonst blieben Kanten oder
    // Beschriftungen fremder Baender stehen.
    for (const id of [
      "vatsim-sectors-fill",
      "vatsim-sectors-line",
      "vatsim-sector-labels-symbol",
    ]) {
      expect(quelle).toContain(id);
    }
  });

  it("laedt beim Reglerbewegen nicht neu", () => {
    // Der Abruf-Effekt darf nicht mehr an Regler-Zustaenden haengen.
    expect(quelle).toContain("}, [showVatsim, mapReady]);");
  });
});
