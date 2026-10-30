// Drift-Wächter: Jeder Befehl der Stufe „system“ (befehle.json, dieselbe
// Quelle wie Kern und `SYSTEMBEFEHLE`) hat im Hülle-Dialog einen Satz in DE,
// EN und IT — und das Textmodul kennt keinen Befehl, der nicht „system“ ist.
import { describe, it, expect } from "vitest";
import befehlsliste from "../../../src-tauri/befehle.json";
import { BEFEHLSTEXTE, befehlstext, werteAnzeige } from "./befehlstexte";

const SYSTEM = Object.entries(befehlsliste.befehle as Record<string, string>)
  .filter(([, stufe]) => stufe === "system")
  .map(([b]) => b);

function fehlende(texte: Record<string, Record<string, string>>): string[] {
  return SYSTEM.flatMap((b) =>
    (["de", "en", "it"] as const).filter((s) => !texte[b]?.[s]?.trim()).map((s) => `${b}/${s}`),
  );
}

describe("Klartext der Systembefehle", () => {
  it("jeder system-Befehl hat einen Satz in allen drei Sprachen", () => {
    expect(SYSTEM.length).toBeGreaterThan(30);
    expect(fehlende(BEFEHLSTEXTE)).toEqual([]);
  });

  it("Gegenprobe: ein fehlender Befehl oder eine leere Sprache fällt auf", () => {
    const ohne: Record<string, Record<string, string>> = { ...BEFEHLSTEXTE };
    delete ohne.flight_logs_delete_all;
    ohne.phpvms_logout = { ...BEFEHLSTEXTE.phpvms_logout, it: " " };
    expect(fehlende(ohne)).toEqual(["flight_logs_delete_all/de", "flight_logs_delete_all/en", "flight_logs_delete_all/it", "phpvms_logout/it"]);
  });

  it("keine Texte für Befehle, die nicht system sind", () => {
    expect(Object.keys(BEFEHLSTEXTE).filter((b) => !SYSTEM.includes(b))).toEqual([]);
  });

  it("Sprache mit Region, Rückfall auf Englisch, unbekannter Befehl ohne Satz", () => {
    expect(befehlstext("flight_logs_delete_all", "de-DE")).toBe("Alle Flugprotokolle auf diesem Rechner löschen");
    expect(befehlstext("flight_logs_delete_all", "fr")).toBe(BEFEHLSTEXTE.flight_logs_delete_all.en);
    expect(befehlstext("gibt_es_nicht", "de")).toBeNull();
    expect(befehlstext("toString", "de")).toBeNull();
  });

  it("Geheimnisse maskiert, übrige Werte lesbar, lange Texte gekürzt", () => {
    const w = Object.fromEntries(
      werteAnzeige({ url: "https://german-sky-group.eu", apiKey: "geheim123", code: "ABCD", olderThanDays: 30, inhalt: "x".repeat(500) }),
    );
    expect(w.url).toBe("https://german-sky-group.eu");
    expect(w.apiKey).not.toContain("geheim123");
    expect(w.code).not.toContain("ABCD");
    expect(w.olderThanDays).toBe("30");
    expect(w.inhalt.length).toBeLessThan(220);
    expect(w.inhalt).toContain("(500)");
  });

  it("maskiert auch verschachtelte Geheimnisse; Gegenprobe: übrige Felder bleiben", () => {
    const w = Object.fromEntries(werteAnzeige({ settings: { url: "u1", apiKey: "geheim9", liste: [{ token: "t7" }] } }));
    expect(w.settings).not.toContain("geheim9");
    expect(w.settings).not.toContain("t7");
    expect(w.settings).toContain('"url":"u1"');
  });
});
