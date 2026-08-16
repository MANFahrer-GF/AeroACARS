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
    // Auf die Absicht pruefen, nicht auf die Schreibweise: der Aufruf ist
    // mehrzeilig, seit das Netz mitgegeben wird.
    const aufruf = quelle.match(/ladeSektoren\([\s\S]{0,120}?\)/);
    expect(aufruf?.[0]).toContain('"alle"');
  });

  it("gibt das gewaehlte Netz an den Server weiter", () => {
    const aufruf = quelle.match(/ladeSektoren\([\s\S]{0,120}?\)/);
    expect(aufruf?.[0]).toContain("netzRef.current");
  });

  it("bietet den Netz-Umschalter mit genau drei Zustaenden", () => {
    // Ein Umschalter statt zweier Schalter: so KANN nicht beides zugleich
    // an sein. Die Ausschliesslichkeit steckt in der Bauform.
    expect(quelle).toContain('(["aus", "vatsim", "ivao"] as const)');
  });

  it("zeigt die Hoehenwahl nur bei VATSIM", () => {
    // IVAO liefert keine Hoehenbaender — ein Regler ohne Wirkung waere
    // eine Luege.
    expect(quelle).toContain('{netz === "vatsim" && (');
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

describe("Darstellung wie auf der Live-Karte", () => {
  // Die Werte stammen wortgleich aus webapp/src/tabs/LiveMap.tsx auf
  // live.kant.ovh. Client und Live-Karte sind zwei getrennte Fassungen
  // derselben Ansicht und sind genau deshalb auseinandergelaufen: der
  // Client malte jeden Platz als dunklen Punkt mit tuerkisem Rand, die
  // Live-Karte faerbt nach Stationsart. Weicht hier etwas ab, sieht der
  // Pilot etwas anderes als der Beobachter.
  const quelle = readFileSync("src/components/LiveMapView.tsx", "utf8");

  it("faerbt Plaetze nach Stationsart", () => {
    expect(quelle).toContain('"twr", "#38bdf8"');
    expect(quelle).toContain('"gnd", "#818cf8"');
    expect(quelle).toContain('"circle-stroke-color": "#07090e"');
  });

  it("nutzt dieselbe Schrift", () => {
    expect(quelle).not.toContain("Open Sans Regular");
    expect(quelle).toContain('"text-font": ["Noto Sans Regular"]');
  });

  it("zeichnet die Bodendaten ueber den Sektoren", () => {
    // Sonst uebermalen die Nahverkehrsbereiche die Rollwege.
    const sektor = quelle.indexOf('id: "vatsim-sectors-fill"');
    const boden = quelle.indexOf("source: SRC_GROUND");
    expect(sektor).toBeGreaterThan(-1);
    expect(boden).toBeGreaterThan(sektor);
  });
});

