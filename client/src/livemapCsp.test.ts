// Regressions-Guard für die Live-Map in der PAKETIERTEN App.
//
// v0.14.0 ist mit schwarzer Karte ausgeliefert worden, weil die Tauri-CSP
// `worker-src ... blob:` nicht erlaubte — MapLibre startet seinen Render-Worker
// aus einer blob:-URL, ohne ihn rendert die Karte NICHTS. Tückisch: Dev-App und
// Browser erzwingen die strenge Produktions-CSP nicht, nur der signierte Build.
// Dieser Test läuft im Release-Gate (vitest) und schlägt fehl, falls die CSP je
// wieder so geändert wird, dass die Karte im Build kaputtgeht.
import { describe, it, expect } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

/** tauri.conf.json finden — robust gegen cwd (client/ oder Repo-Root). */
function findTauriConf(): string {
  const candidates = [
    resolve(process.cwd(), "src-tauri/tauri.conf.json"),
    resolve(process.cwd(), "client/src-tauri/tauri.conf.json"),
  ];
  for (const c of candidates) if (existsSync(c)) return c;
  throw new Error("tauri.conf.json nicht gefunden (cwd=" + process.cwd() + ")");
}

const cfg = JSON.parse(readFileSync(findTauriConf(), "utf8"));
const csp: string = cfg.app?.security?.csp ?? "";

/** Quellen-Liste einer CSP-Direktive (oder []). */
function directive(name: string): string[] {
  const part = csp
    .split(";")
    .map((s) => s.trim())
    .find((s) => s === name || s.startsWith(name + " "));
  return part ? part.slice(name.length).trim().split(/\s+/).filter(Boolean) : [];
}

/** Effektive Quelle für Worker-Erstellung (mit CSP-Fallback worker→child→default). */
function workerSources(): string[] {
  const w = directive("worker-src");
  if (w.length) return w;
  const c = directive("child-src");
  if (c.length) return c;
  return directive("default-src");
}

describe("Live-Map CSP (paketierte App)", () => {
  it("hat überhaupt eine CSP gesetzt", () => {
    expect(csp.length).toBeGreaterThan(0);
  });

  it("erlaubt MapLibres Render-Worker (blob:) — sonst bleibt die Karte schwarz", () => {
    expect(workerSources()).toContain("blob:");
  });

  it("connect-src erlaubt die CARTO-Basemap (https:)", () => {
    expect(directive("connect-src")).toContain("https:");
  });

  it("img-src erlaubt Raster-Kacheln/Sprites (https:)", () => {
    expect(directive("img-src")).toContain("https:");
  });
});
