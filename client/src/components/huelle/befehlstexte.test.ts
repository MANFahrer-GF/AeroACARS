// Drift-Wächter: Jeder Befehl der Stufe „system“ (befehle.json, dieselbe
// Quelle wie Kern und `SYSTEMBEFEHLE`) hat im Hülle-Dialog einen Satz in DE,
// EN und IT — und das Textmodul kennt keinen Befehl, der nicht „system“ ist.
import { describe, it, expect } from "vitest";
import befehlsliste from "../../../src-tauri/befehle.json";
import { BEFEHLSTEXTE, RAHMEN, dialogText, werteAnzeige, type Eintrag } from "./befehlstexte";

const SYSTEM = Object.entries(befehlsliste.befehle as Record<string, string>)
  .filter(([, stufe]) => stufe === "system")
  .map(([b]) => b);

function fehlende(texte: Record<string, Eintrag>): string[] {
  return SYSTEM.flatMap((b) =>
    (["de", "en", "it"] as const)
      .filter((s) => !texte[b]?.[s]?.frage?.trim() || !texte[b]?.[s]?.knopf?.trim())
      .map((s) => `${b}/${s}`),
  );
}

describe("Klartext der Systembefehle", () => {
  it("jeder system-Befehl hat Frage und Knopf in allen drei Sprachen", () => {
    expect(SYSTEM.length).toBeGreaterThan(15);
    expect(fehlende(BEFEHLSTEXTE)).toEqual([]);
  });

  it("Gegenprobe: ein fehlender Befehl oder eine leere Sprache fällt auf", () => {
    const ohne: Record<string, Eintrag> = { ...BEFEHLSTEXTE };
    delete ohne.flight_logs_delete_all;
    ohne.landing_delete = { ...BEFEHLSTEXTE.landing_delete, it: { frage: "x", knopf: " " } };
    expect(fehlende(ohne)).toEqual(["flight_logs_delete_all/de", "flight_logs_delete_all/en", "flight_logs_delete_all/it", "landing_delete/it"]);
  });

  it("keine Texte für Befehle, die nicht system sind", () => {
    expect(Object.keys(BEFEHLSTEXTE).filter((b) => !SYSTEM.includes(b))).toEqual([]);
  });

  it("Sprache mit Region, Rückfall auf Englisch, unbekannter Befehl ohne Text", () => {
    expect(dialogText("flight_logs_delete_all", {}, "de-DE")).toEqual({
      frage: "Alle Flugprotokolle auf diesem Rechner löschen? Das lässt sich nicht rückgängig machen.",
      knopf: "Alle löschen",
    });
    expect(dialogText("flight_logs_delete_all", {}, "fr")).toEqual(BEFEHLSTEXTE.flight_logs_delete_all.en);
    expect(dialogText("gibt_es_nicht", {}, "de")).toBeNull();
    expect(dialogText("toString", {}, "de")).toBeNull();
  });

  it("Schalter: die Frage nennt die Richtung; Gegenprobe ohne enabled", () => {
    expect(dialogText("flight_logs_auto_purge_set", { enabled: false }, "de")?.knopf).toBe("Ausschalten");
    expect(dialogText("flight_logs_auto_purge_set", { enabled: true }, "de")?.knopf).toBe("Einschalten");
    expect(dialogText("flight_logs_auto_purge_set", {}, "de")?.knopf).toBe("Einschalten");
  });

  it("Werte in der Frage: eingesetzt, einzeilig, gekürzt, Geheimnisse nie", () => {
    expect(dialogText("remote_server_set_port", { port: 8787 }, "de")?.frage).toBe("Die LAN-Fernbedienung auf Port 8787 umstellen?");
    expect(dialogText("landing_delete", {}, "de")?.frage).toContain("(…)");
    const lang = dialogText("kabinen_musik_entfernen", { datei: "a\nb" + "x".repeat(100) }, "de")!.frage;
    expect(lang).not.toContain("\n");
    expect(lang.length).toBeLessThan(140);
    // Richtungs-/Steuerzeichen werden entfernt; Gegenprobe: normaler Text bleibt.
    const bidi = dialogText("kabinen_musik_entfernen", { datei: "a\u202Eb\u200Ec" }, "de")!.frage;
    expect(bidi).not.toMatch(/[\u202E\u200E]/);
    expect(bidi).toContain("a b c");
    expect(dialogText("flight_logs_purge_older_than", { olderThanDays: 0 }, "de")!.frage).toContain("älter als 0 Tage");
    expect(dialogText("update_installieren", { version: "9.9.9" }, "de")!.frage).not.toContain("9.9.9");
  });

  it("Rahmen des Dialogs in allen drei Sprachen", () => {
    for (const s of ["de", "en", "it"] as const) {
      for (const [k, v] of Object.entries(RAHMEN[s])) expect(v.trim(), `${s}/${k}`).not.toBe("");
    }
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
