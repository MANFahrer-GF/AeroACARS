// Vor dem Installieren wird noch einmal nachgesehen.
//
// Thomas, 20.09.2026: „39 / 40 / 41 — es wird nicht gleich 41 gezogen."
// Der Client prüft beim Start, alle vier Stunden und beim Fenster-Fokus.
// Läuft er seit gestern Abend, trägt er die Version von gestern im Knopf
// und installierte sie auch — der Pilot durfte danach ein zweites Mal
// updaten. Die Kette lag NICHT am Server: Der Updater-Endpunkt liefert
// immer nur die neueste Fassung.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const quelle = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), "useUpdateChecker.ts"),
  "utf-8",
);

/** Der Rumpf von `installAndRelaunch` — nur dort zählt die Reihenfolge. */
function installRumpf(): string {
  const von = quelle.indexOf("const installAndRelaunch");
  expect(von, "installAndRelaunch nicht gefunden").toBeGreaterThan(-1);
  const bis = quelle.indexOf("const ", quelle.indexOf("await tauriRelaunch()", von));
  return quelle.slice(von, bis > von ? bis : undefined);
}

describe("Update: frische Prüfung vor dem Installieren", () => {
  it("sieht vor dem Herunterladen noch einmal nach", () => {
    const rumpf = installRumpf();
    const pruefung = rumpf.indexOf("tauriCheckForUpdate()");
    const laden = rumpf.indexOf("downloadAndInstall(");
    expect(pruefung, "keine frische Prüfung vor dem Installieren").toBeGreaterThan(-1);
    expect(pruefung, "die Prüfung liegt NACH dem Herunterladen").toBeLessThan(laden);
  });

  it("installiert die frisch gefundene Fassung, nicht die gemerkte", () => {
    const rumpf = installRumpf();
    // Heruntergeladen wird die Variable, die die frische Prüfung setzt —
    // nicht das seit Stunden gemerkte `update`.
    expect(rumpf).toMatch(/zuInstallieren\.downloadAndInstall\(/);
    expect(rumpf).not.toMatch(/\bupdate\.downloadAndInstall\(/);
  });

  it("bleibt ohne Verbindung beim bekannten Stand", () => {
    const rumpf = installRumpf();
    // Die Prüfung steckt in einem eigenen try/catch: Ein Netzfehler darf
    // das Update nicht verhindern.
    const pruefung = rumpf.indexOf("tauriCheckForUpdate()");
    const fang = rumpf.indexOf("catch", pruefung);
    const laden = rumpf.indexOf("downloadAndInstall(");
    expect(fang).toBeGreaterThan(pruefung);
    expect(fang).toBeLessThan(laden);
  });
});
