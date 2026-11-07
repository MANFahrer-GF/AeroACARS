import { describe, expect, it } from "vitest";
import { TABELLE } from "./grenzen";
import { dateiName, kurzZeit, langZeit, simulatorName, streckeText, wertFormatieren, zeitText } from "./metaFormat";
import { eintrag } from "./testhilfen";

describe("Metadaten-Anzeige", () => {
  const z = (feld: string) => TABELLE.metadaten.find((m) => m.feld === feld)!;
  const w = (feld: string, meta = {}, sprache = "de") => wertFormatieren(z(feld), eintrag({ meta }), sprache);
  it("die Liste kommt aus dem Paket und beginnt mit der Position", () => {
    expect(TABELLE.metadaten[0].feld).toBe("position");
    const erlaubt = new Set(["position", "hoehe", "gs", "kurs", "text", "flugzeug", "flug", "simulator", "quelle", "datei"]);
    expect(TABELLE.metadaten.filter((m) => !erlaubt.has(m.format))).toEqual([]);
  });
  it("formatiert nach Format aus der Tabelle", () => {
    expect(w("position", { breite: -33.9461, laenge: 8.5706 })).toEqual({ art: "text", text: "33.9461 S · 8.5706 E" });
    expect(w("hoehe_ft", { hoehe_ft: 354.4 })).toEqual({ art: "text", text: "354 ft", zusatz: "msl" });
    expect(w("gs_kt", { gs_kt: 148 })).toEqual({ art: "text", text: "148 kt", zusatz: "ueber_grund" });
    expect(w("kurs", { kurs: 5 })).toEqual({ art: "text", text: "005°" });
    expect(w("kurs", { kurs: 360 })).toEqual({ art: "text", text: "000°" });
    expect(w("flugzeug", { flugzeug_titel: "A320-251N", flugzeug_icao: "A20N" })).toEqual({ art: "text", text: "A320-251N (A20N)" });
    expect(w("flug", { rufzeichen: "GSG412", abflug: "EDDF", ziel: "LSZH" })).toEqual({ art: "flug", text: "GSG412 EDDF–LSZH" });
    expect(w("quelle", { quelle: "protokoll" })).toEqual({ art: "quelle", wert: "protokoll" });
  });
  it("fehlende Werte bleiben leer, nie 0", () => {
    expect(w("hoehe_ft", { hoehe_ft: null })).toEqual({ art: "leer" });
    expect(w("gs_kt", { gs_kt: null })).toEqual({ art: "leer" });
    expect(w("position", { breite: null, laenge: null })).toEqual({ art: "leer" });
    expect(w("flug", { rufzeichen: null, abflug: null, ziel: null })).toEqual({ art: "leer" });
    expect(w("naechster_flugplatz", { naechster_flugplatz: null })).toEqual({ art: "leer" });
  });
  it("Datei: Name und Groesse", () => {
    const e = eintrag({ id: "x", bytes: 1800000 });
    expect(dateiName(e)).toBe("x.jpg");
    expect(wertFormatieren(z("datei"), e, "de")).toEqual({ art: "text", text: "x.jpg · 1,7 MB" });
  });
  it("Zeiten in UTC, ungueltig bleibt wie geliefert", () => {
    expect(zeitText("2026-11-07T10:05:09Z", "de")).toBe("07.11.2026 10:05:09 UTC");
    expect(zeitText("kaputt", "de")).toBe("kaputt");
    expect(kurzZeit("2026-11-07T13:11:20Z")).toBe("13:11Z");
    expect(langZeit("2026-11-07T14:32:08Z")).toBe("14:32:08Z");
    expect(kurzZeit(null)).toBe("");
  });
  it("Simulatorname und Strecke", () => {
    expect(simulatorName("xplane12")).toBe("X-Plane 12");
    expect(simulatorName("Msfs2024")).toBe("MSFS 2024");
    expect(simulatorName("Msfs2020")).toBe("MSFS 2020");
    expect(simulatorName("andere")).toBe("andere");
    expect(streckeText({ abflug: "EDDF", ziel: null })).toBe("EDDF");
  });
});
