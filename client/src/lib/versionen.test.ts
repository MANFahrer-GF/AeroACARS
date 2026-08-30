import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

/**
 * Alle vier Stellen, die eine Version tragen, müssen dieselbe nennen.
 *
 * ⚠ `package-lock.json` stand am 30.08.2026 noch auf 1.6.3, während die
 * anderen drei bei 1.7.12 lagen — neun Fassungen Rückstand. Es fällt
 * nicht auf: Nichts liest die Zahl zur Laufzeit, und `npm ci` beschwert
 * sich nicht. Sichtbar wird es erst, wenn jemand die Herkunft eines
 * Bündels nachvollziehen will.
 */
const WURZEL = path.resolve(__dirname, "../..");

function lies(p: string): string {
  return fs.readFileSync(path.join(WURZEL, p), "utf-8");
}

describe("Versionsangaben", () => {
  it("stimmen in allen vier Dateien überein", () => {
    const paket = JSON.parse(lies("package.json")).version as string;
    expect(paket).toMatch(/^\d+\.\d+\.\d+$/);

    const sperre = JSON.parse(lies("package-lock.json"));
    const tauri = JSON.parse(lies("src-tauri/tauri.conf.json")).version as string;
    // Die ERSTE `version = "…"` im `[workspace.package]`-Block.
    const cargo = /^version = "([^"]+)"$/m.exec(lies("src-tauri/Cargo.toml"))?.[1];

    const gefunden = {
      "package.json": paket,
      "package-lock.json (Wurzel)": sperre.version,
      'package-lock.json (packages[""])': sperre.packages?.[""]?.version,
      "tauri.conf.json": tauri,
      "Cargo.toml": cargo,
    };
    const abweichend = Object.entries(gefunden).filter(([, v]) => v !== paket);
    expect(
      abweichend,
      `diese Stellen nennen nicht ${paket}: ${JSON.stringify(gefunden)}`,
    ).toEqual([]);
  });
});
