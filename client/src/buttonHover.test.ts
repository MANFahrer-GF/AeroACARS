// Farbige Knoepfe muessen beim Drueberfahren farbig bleiben.
//
// App.css hat eine globale Regel `button:hover:not(:disabled)` (Karten-
// farbe, Spezifitaet 0,2,1). Eine eigene Regel `.x:hover` (0,2,0) verliert
// dagegen: Der Update-Knopf wurde beim Drueberfahren dunkel mit dunkler
// Schrift, im hellen Theme weiss auf fast weiss (Thomas, 19.09.2026).
// Jeder Knopf mit Akzent-/Signal-Flaeche braucht deshalb eine Hover-Regel
// mit `:not(:disabled)`, die den Hintergrund selbst setzt.
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const css = readFileSync(resolve(__dirname, "App.css"), "utf-8").replace(/\/\*[\s\S]*?\*\//g, "");
const regeln = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  selektoren: m[1].split(",").map((s) => s.trim()),
  inhalt: m[2],
}));

/** Klassen, die im Code an einem `<button>` stehen (Badges u.a. sind keine Knoepfe). */
function knopfKlassen(): Set<string> {
  const out = new Set<string>();
  const lauf = (dir: string) => {
    for (const n of readdirSync(dir)) {
      const p = join(dir, n);
      if (statSync(p).isDirectory()) lauf(p);
      else if (p.endsWith(".tsx") && !p.includes(".test.")) {
        const src = readFileSync(p, "utf-8");
        for (const m of src.matchAll(/<button\b[^>]*?className=\{?[`"']([^`"'}]+)/gs)) {
          for (const k of m[1].match(/[\w-]+/g) ?? []) out.add(k);
        }
      }
    }
  };
  lauf(__dirname);
  return out;
}

/** Klassen, deren Grundregel eine Akzent- oder Signalflaeche mit eigener Schriftfarbe setzt. */
function farbigeKnoepfe(): string[] {
  const knoepfe = knopfKlassen();
  const out: string[] = [];
  for (const r of regeln) {
    const bg = /(?:^|;)\s*background(?:-color)?:\s*var\(--(acc|accent|sem-[a-z]+)\b/.exec(r.inhalt);
    const schrift = /(?:^|;)\s*color:\s*(var\(--on-acc\)|#fff\b|#ffffff\b|white\b|var\(--bg\))/.exec(r.inhalt);
    if (!bg || !schrift) continue;
    for (const s of r.selektoren) {
      const m = /^\.([\w-]+)$/.exec(s);
      // update-button setzt seinen className aus einem Array zusammen — das findet die Suche oben nicht.
      if (m && (knoepfe.has(m[1]) || m[1] === "update-button")) out.push(m[1]);
    }
  }
  return out;
}

describe("Hover farbiger Knoepfe", () => {
  it("findet die bekannten Knoepfe (sonst prueft der Test nichts)", () => {
    const k = farbigeKnoepfe();
    for (const erwartet of ["update-button", "update-gate__install", "button--primary"]) {
      expect(k).toContain(erwartet);
    }
  });

  it.each(farbigeKnoepfe())(".%s setzt beim Hover selbst einen Hintergrund, der die globale Regel schlaegt", (klasse) => {
    const hover = regeln.some(
      (r) =>
        /(?:^|;)\s*background(?:-color)?:/.test(r.inhalt) &&
        r.selektoren.some((s) => s.includes(`.${klasse}`) && s.includes(":hover") && s.includes(":not(:disabled)")),
    );
    expect(hover, `.${klasse}:hover:not(:disabled) { background: … } fehlt`).toBe(true);
  });
});
