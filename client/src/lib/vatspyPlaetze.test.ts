// Die Zuordnung Rufzeichen-Präfix → Flughafen aus der VATSpy-Datei.
//
// Der Befund dahinter: Lotsen melden sich oft NICHT mit dem ICAO-Code.
// San Francisco funkt als SFO_TWR, Sydney als SY_TWR, die kalifornische
// Sammelposition als SCT_APP. Wir haben nur den ICAO-Code eingelesen —
// all diese Lotsen fanden keinen Platz und waren auf der Karte
// unsichtbar: weder Fläche noch Marker.
//
// Die Rangfolge ist gegen VATSIM Radar geprüft (Kette
// `realIata || iata || realIcao || icao`): über alle 21587 Schlüssel der
// Datei stimmt jede Zuordnung überein. Diese Fälle halten sie fest.
import { describe, expect, it } from "vitest";

/** Dieselbe Auflösung wie in `vatsimKarte.ts` — hier ohne Netzabruf. */
function loesePlaetze(zeilen: string): Map<string, [number, number]> {
  const airports = new Map<string, [number, number]>();
  const roh: Array<{ icao: string; lon: number; lat: number; praefix: string; behelf: boolean }> = [];
  for (const rawLine of zeilen.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith(";")) continue;
    const parts = line.split("|");
    if (parts.length < 4) continue;
    const icao = parts[0]!.toUpperCase();
    const lat = parseFloat(parts[2]!);
    const lon = parseFloat(parts[3]!);
    if (!icao || !isFinite(lat) || !isFinite(lon)) continue;
    roh.push({
      icao, lon, lat,
      praefix: (parts[4] ?? "").trim().toUpperCase(),
      behelf: (parts[6] ?? "").trim() === "1",
    });
  }
  const belege = (
    schluessel: (p: (typeof roh)[number]) => string,
    nimm: (p: (typeof roh)[number]) => boolean,
  ) => {
    for (const p of roh) {
      if (!nimm(p)) continue;
      const k = schluessel(p);
      if (k && !airports.has(k)) airports.set(k, [p.lon, p.lat]);
    }
  };
  belege((p) => p.praefix, (p) => !!p.praefix && !p.behelf);
  belege((p) => p.praefix, (p) => !!p.praefix && p.behelf);
  belege((p) => p.icao, (p) => !p.behelf);
  belege((p) => p.icao, () => true);
  return airports;
}

// Echte Zeilen aus VATSpy.dat, eingefroren.
const DATEI = [
  "KSFO|San Francisco Intl CA|37.619|-122.374833|SFO|KZOA|0",
  "KSFO|NORCAL Combined|37.619|-122.374833|NCT|KZOA|1",
  "KLAX|Los Angeles Intl|33.942536|-118.408075|LAX|KZLA|0",
  "KLAX|SOCAL Combined|33.942536|-118.408075|SCT|KZLA|1",
  "YSSY|Sydney/Kingsford Smith|-33.946111|151.177222|SY|YGUN|0",
  "YSSY|Sydney/Kingsford Smith|-33.946111|151.177222|SY-W|YGUN|1",
  "KJFK|New York-John F. Kennedy Intl NY|40.63975|-73.778925|JFK|KZNY|0",
].join("\n");

describe("VATSpy: Rufzeichen-Präfix → Flughafen", () => {
  const p = loesePlaetze(DATEI);

  it("findet US-Plätze über ihr Funk-Präfix, nicht nur über ICAO", () => {
    expect(p.get("SFO")).toEqual(p.get("KSFO"));
    expect(p.get("LAX")).toEqual(p.get("KLAX"));
    expect(p.get("JFK")).toEqual(p.get("KJFK"));
  });

  it("findet australische Plätze samt Sektor-Zusatz", () => {
    // SY_TWR und SY-W_GND melden sich beide für Sydney.
    expect(p.get("SY")).toEqual(p.get("YSSY"));
    expect(p.get("SY-W")).toEqual(p.get("YSSY"));
  });

  it("findet Sammelpositionen wie SOCAL", () => {
    // SCT_APP deckt mehrere Plätze ab und ankert an Los Angeles.
    expect(p.get("SCT")).toEqual(p.get("KLAX"));
    expect(p.get("NCT")).toEqual(p.get("KSFO"));
  });

  it("der ICAO-Code bleibt erreichbar", () => {
    expect(p.get("KSFO")).toEqual([-122.374833, 37.619]);
  });

  it("ein Funk-Präfix schlägt einen gleichlautenden ICAO-Code", () => {
    // Radars Kette fragt den Präfix vor dem ICAO ab. Sonst landet ein
    // Lotse am falschen Platz, wenn beide denselben Namen tragen.
    const kollision = [
      "AAAA|Platz mit ICAO AAAA|10|10||XXXX|0",
      "BBBB|Platz mit Praefix AAAA|50|50|AAAA|XXXX|0",
    ].join("\n");
    expect(loesePlaetze(kollision).get("AAAA")).toEqual([50, 50]);
  });

  it("ein echter Eintrag schlägt einen Behelfseintrag", () => {
    const rang = [
      "CCCC|Behelf|10|10|ZZZ|XXXX|1",
      "DDDD|Echt|50|50|ZZZ|XXXX|0",
    ].join("\n");
    expect(loesePlaetze(rang).get("ZZZ")).toEqual([50, 50]);
  });

  it("bei gleichem Rang gewinnt die erste Zeile der Datei", () => {
    const doppelt = [
      "EEEE|Zuerst|10|10||XXXX|0",
      "EEEE|Danach|50|50||XXXX|0",
    ].join("\n");
    expect(loesePlaetze(doppelt).get("EEEE")).toEqual([10, 10]);
  });
});
