// Grenzen der Hilfe-Bilder (ohne Canvas prüfbar): Anzahl, Typ, Rohgröße,
// Zielmaße und das Auslesen von Zwischenablage/Ablage.
import { describe, it, expect } from "vitest";
import HILFE_RS from "../../src-tauri/src/hilfe.rs?raw";
import { BILD_MAX_ANZAHL, BILD_MAX_BYTES, ROH_MAX_BYTES, auswahlPruefen, dateienAus, zielMasse } from "./hilfeBilder";

const png = (size = 1000) => ({ type: "image/png", size });

describe("auswahlPruefen", () => {
  it("nimmt PNG, JPEG und WebP", () => {
    const r = auswahlPruefen(0, [png(), { type: "image/jpeg", size: 1 }, { type: "image/webp", size: 1 }]);
    expect(r).toEqual({ angenommen: [0, 1, 2], fehler: [] });
  });

  it("lehnt andere Typen ab (SVG, GIF, HEIC, Text)", () => {
    for (const type of ["image/svg+xml", "image/gif", "image/heic", "text/plain", ""]) {
      expect(auswahlPruefen(0, [{ type, size: 1 }])).toEqual({ angenommen: [], fehler: ["typ"] });
    }
  });

  it("höchstens fünf, auch über mehrere Einfügungen", () => {
    expect(BILD_MAX_ANZAHL).toBe(5);
    const r = auswahlPruefen(0, Array.from({ length: 7 }, () => png()));
    expect(r.angenommen).toEqual([0, 1, 2, 3, 4]);
    expect(r.fehler).toEqual(["anzahl"]);
    expect(auswahlPruefen(4, [png(), png()])).toEqual({ angenommen: [0], fehler: ["anzahl"] });
    expect(auswahlPruefen(5, [png()])).toEqual({ angenommen: [], fehler: ["anzahl"] });
  });

  it("Rohgröße bis 25 MB (wird verkleinert), darüber nicht", () => {
    expect(auswahlPruefen(0, [png(ROH_MAX_BYTES)]).angenommen).toEqual([0]);
    expect(auswahlPruefen(0, [png(ROH_MAX_BYTES + 1)])).toEqual({ angenommen: [], fehler: ["gross"] });
  });

  it("ein abgelehntes Bild belegt keinen Platz", () => {
    const r = auswahlPruefen(3, [{ type: "image/gif", size: 1 }, png(), png()]);
    expect(r).toEqual({ angenommen: [1, 2], fehler: ["typ"] });
  });
});

describe("Grenzen stimmen mit Rust überein", () => {
  it("Anzahl und Bytes je Bild wie in hilfe.rs", () => {
    expect(HILFE_RS).toContain(`const BILD_MAX_ANZAHL: usize = ${BILD_MAX_ANZAHL};`);
    expect(BILD_MAX_BYTES).toBe(5 * 1024 * 1024);
    expect(HILFE_RS).toContain("const BILD_MAX_BYTES: usize = 5 * 1024 * 1024;");
  });
});

describe("zielMasse", () => {
  it("verkleinert auf 1920 an der längsten Kante, Seitenverhältnis bleibt", () => {
    expect(zielMasse(3840, 2160)).toEqual({ breite: 1920, hoehe: 1080 });
    expect(zielMasse(1080, 2400)).toEqual({ breite: 864, hoehe: 1920 });
  });
  it("vergrößert nie", () => {
    expect(zielMasse(800, 600)).toEqual({ breite: 800, hoehe: 600 });
    expect(zielMasse(1920, 1)).toEqual({ breite: 1920, hoehe: 1 });
  });
});

describe("dateienAus", () => {
  const datei = new File(["x"], "a.png", { type: "image/png" });
  it("liest files, sonst Datei-Einträge der items", () => {
    expect(dateienAus({ files: [datei] as unknown as FileList, items: [] } as unknown as DataTransfer)).toEqual([datei]);
    const items = [
      { kind: "string", getAsFile: () => null },
      { kind: "file", getAsFile: () => datei },
    ];
    expect(dateienAus({ files: [] as unknown as FileList, items } as unknown as DataTransfer)).toEqual([datei]);
  });
  it("reiner Text: keine Dateien", () => {
    expect(dateienAus({ files: [] as unknown as FileList, items: [{ kind: "string", getAsFile: () => null }] } as unknown as DataTransfer)).toEqual([]);
    expect(dateienAus(null)).toEqual([]);
  });
});
