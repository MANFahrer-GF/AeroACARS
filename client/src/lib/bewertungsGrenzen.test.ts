// Die Grenzwerte in TS müssen die aus Rust sein — sonst färbt die Anzeige
// nach anderen Grenzen als die Note.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as G from "./bewertungsGrenzen";

const crate = resolve(__dirname, "../../src-tauri/crates/landing-scoring/src");
function rust(datei: string, name: string): number {
  const m = new RegExp(`pub const ${name}: f32 = ([0-9.]+);`).exec(
    readFileSync(resolve(crate, datei), "utf-8"),
  );
  if (!m) throw new Error(`${name} nicht in ${datei}`);
  return Number(m[1]);
}

describe("Grenzwerte = Rust", () => {
  it.each([
    ["sub_landing_rate.rs", "T_VS_SMOOTH_FPM"],
    ["sub_landing_rate.rs", "T_VS_FIRM_FPM"],
    ["sub_landing_rate.rs", "T_VS_HARD_FPM"],
    ["sub_landing_rate.rs", "T_VS_SEVERE_FPM"],
    ["sub_g_force.rs", "T_G_SMOOTH"],
    ["sub_g_force.rs", "T_G_FIRM"],
    ["sub_g_force.rs", "T_G_HARD"],
    ["sub_g_force.rs", "T_G_SEVERE"],
  ])("%s %s", (datei, name) => {
    expect((G as Record<string, number>)[name]).toBe(rust(datei, name));
  });
});
