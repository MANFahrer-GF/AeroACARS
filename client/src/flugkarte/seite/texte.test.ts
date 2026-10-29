// v3 (Etappe 5, P-8.21): Hilfen der eigenständigen Flugkarten-Seite.
import { describe, expect, it } from "vitest";
import de from "../../../../bundle/texte/de.json";
import en from "../../../../bundle/texte/en.json";
import { akteAusNachricht, seitenSprache, uebersetzer } from "./texte";

const TEXTE = { de: de.flugkarte, en: en.flugkarte } as Record<string, Record<string, unknown>>;

describe("Flugkarten-Seite", () => {
  it("übersetzt Schlüssel wie der Client, mit Werten und null bei Fehlendem", () => {
    const t = uebersetzer(TEXTE, "de");
    expect(t("flugkarte.fenster.titel")).toBe("Flugkarte");
    expect(t("flugkarte.fenster.fehler", { fehler: "x" })).toBe("Die Flugkarte konnte nicht geladen werden (x).");
    expect(t("flugkarte.gibt.es.nicht")).toBeNull();
    expect(t("anderes.fenster.titel")).toBeNull();
    expect(uebersetzer(TEXTE, "fr")("flugkarte.fenster.titel")).toBe(en.flugkarte.fenster.titel);
  });

  it("wählt die Sprache aus der Adresse, sonst dem Browser, sonst Englisch", () => {
    expect(seitenSprache("?sprache=de", "en-US", ["de", "en"])).toBe("de");
    expect(seitenSprache("", "de-DE", ["de", "en"])).toBe("de");
    expect(seitenSprache("?sprache=xx", "fr-FR", ["de", "en"])).toBe("en");
  });

  it("nimmt eine Akte nur vom Elternfenster und nur in der vereinbarten Form", () => {
    const akte = { kopf: {} };
    expect(akteAusNachricht({ typ: "flugkarte-akte", akte, sprache: "it" }, true)).toEqual({ akte, sprache: "it" });
    expect(akteAusNachricht({ typ: "flugkarte-akte", akte }, false)).toBeNull();
    expect(akteAusNachricht({ typ: "anders", akte }, true)).toBeNull();
    expect(akteAusNachricht({ typ: "flugkarte-akte", akte: "text" }, true)).toBeNull();
    expect(akteAusNachricht(null, true)).toBeNull();
  });
});
