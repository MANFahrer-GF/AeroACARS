/** Der lokale Build darf die Signatur der CI nicht aushebeln.
 *
 *  Ein lokaler `tauri build` endete mit Fehlercode, obwohl die App fertig
 *  war: Tauri wollte die Updater-Dateien signieren, und der private
 *  Schluessel liegt bewusst nur in der CI. Die naheliegende "Loesung" waere,
 *  `createUpdaterArtifacts` global abzuschalten — dann baute die CI keine
 *  `.sig` und keine `latest.json` mehr, und das Auto-Update aller Piloten
 *  stuende still, ohne dass es jemand merkt.
 *
 *  Deshalb: Hauptkonfiguration bleibt an, nur die lokale Beikonfiguration
 *  schaltet ab (`npm run build:lokal`).
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

const lies = (p: string) => JSON.parse(readFileSync(p, "utf8"));

describe("Updater-Signatur", () => {
  it("bleibt in der Hauptkonfiguration eingeschaltet", () => {
    const c = lies("src-tauri/tauri.conf.json");
    expect(c.bundle.createUpdaterArtifacts).toBe(true);
  });

  it("ist im Client ein oeffentlicher Schluessel hinterlegt", () => {
    const c = lies("src-tauri/tauri.conf.json");
    expect(c.plugins?.updater?.pubkey?.length ?? 0).toBeGreaterThan(100);
  });

  it("schaltet nur die lokale Beikonfiguration ab", () => {
    const l = lies("src-tauri/tauri.lokal.conf.json");
    expect(l.bundle.createUpdaterArtifacts).toBe(false);
    // Sie darf NICHTS anderes umstellen — sonst weicht der lokale Build
    // still vom Release ab und man testet etwas anderes als man ausliefert.
    expect(Object.keys(l.bundle)).toEqual(["createUpdaterArtifacts"]);
    expect(Object.keys(l).filter((k) => k !== "$schema")).toEqual(["bundle"]);
  });

  it("nutzt die Beikonfiguration nur im lokalen Skript", () => {
    const pkg = lies("package.json");
    expect(pkg.scripts["build:lokal"]).toContain("tauri.lokal.conf.json");
    // Kein anderes Skript darf sie ziehen, damit ein Release-Build sie nie sieht.
    const andere = Object.entries(pkg.scripts as Record<string, string>)
      .filter(([k, v]) => k !== "build:lokal" && v.includes("tauri.lokal"));
    expect(andere).toEqual([]);
  });
});
