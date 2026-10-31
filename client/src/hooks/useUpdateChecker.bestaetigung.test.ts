// 3.0.0-beta.2.1 (Thomas 31.10.2026): Der Update-Dialog zeigte „Installiere —
// App startet gleich neu…“, während das Bestätigungsfenster der Hülle noch
// offen war; Abbrechen endete als „Fehler“.
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BITTE_BESTAETIGEN, istAbgelehnt } from "./useUpdateChecker";

const quelle = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "useUpdateChecker.ts"), "utf8");
const rumpf = quelle.slice(quelle.indexOf("const installAndRelaunch"));

describe("Update über die Hülle", () => {
  it("wartet zuerst auf die Bestätigung, „Installiere“ erst ab Finished", () => {
    expect(BITTE_BESTAETIGEN).toContain("AeroACARS — Bestätigung");
    const erster = rumpf.indexOf("setProgress(");
    expect(rumpf.slice(erster, erster + 40)).toContain("BITTE_BESTAETIGEN");
    const installiere = rumpf.indexOf(`setProgress("Installiere`);
    expect(installiere).toBeGreaterThan(rumpf.indexOf('case "Finished"'));
    expect(rumpf).not.toContain("Lädt Update herunter");
  });

  it("Abbrechen in der Hülle ist kein Fehler", () => {
    expect(istAbgelehnt({ code: "huelle_abgelehnt", message: "x" })).toBe(true);
    expect(istAbgelehnt({ code: "update_failed", message: "x" })).toBe(false);
    expect(istAbgelehnt("huelle_abgelehnt")).toBe(false);
    expect(istAbgelehnt(null)).toBe(false);
    const fang = rumpf.slice(rumpf.indexOf("} catch (err)"));
    expect(fang.indexOf("istAbgelehnt(err)")).toBeLessThan(fang.indexOf("setInstallationGescheitert(true)"));
  });
});
