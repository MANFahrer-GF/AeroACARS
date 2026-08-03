// Wächter gegen einen wiederkehrenden Kontrast-Bug: ein Button-Modifier
// setzt eine eigene "gefüllte" Optik (Akzent-Hintergrund + passende
// Textfarbe), aber die zugehörige Basis-Regel schließt genau diesen
// Modifier per `:not(...)` von ihrer eigenen Hover-Behandlung aus —
// `.foo:not(.foo--active):hover { ... }`. Ohne eine EIGENE Hover-Regel für
// `.foo--active` greift dann die app-weite generische
// `button:hover:not(:disabled) { background: var(--surface-2); }` und
// lässt die für den Akzent-Hintergrund gedachte Textfarbe auf einer
// hellen Fläche stehen — unlesbar, sobald die Maus stehen bleibt.
//
// Feldbefund: erst auf dem PDC/CPDLC-Screen (`.datalink-mode--active`,
// `.cpdlc-chip--active`), dann unabhängig davon noch mal auf der
// Briefing-Seite (`.bid-card__mode-btn--active`, IFR/VFR-Umschalter) —
// derselbe Fehler, zweimal unabhängig eingebaut. Dieser Test sucht das
// STRUKTURELLE Muster (eine `:not(.x--y)`-Ausnahme in einer Hover-Regel)
// und verlangt, dass der ausgeschlossene Modifier eine eigene Hover-Regel
// hat — unabhängig davon, welcher Screen als nächstes einen neuen
// gefüllten Button dieser Art bekommt.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(__dirname, "App.css"), "utf8");

describe("gefüllte Button-Varianten haben eine EIGENE :hover-Regel", () => {
  it("jede `:not(.x--y):hover`-Ausnahme hat eine passende `.x--y:hover`-Regel", () => {
    // Erfasst z.B. `.bid-card__mode-btn:not(.bid-card__mode-btn--active):hover`
    // — BEM-Klassennamen enthalten `_`, das muss im Zeichensatz stehen,
    // sonst matcht das Muster real existierende Klassen nie (stiller
    // Fehlalarm-Ausfall: der Test "besteht", weil er nie feuert).
    const exclusionRe = /\.([a-z0-9_-]+):not\(\.\1(--[a-z0-9_-]+)\)(?::hover)/g;
    const missing: string[] = [];
    let m: RegExpExecArray | null;
    while ((m = exclusionRe.exec(css)) !== null) {
      const excludedClass = `${m[1]}${m[2]}`;
      const hasOwnHover = new RegExp(`\\.${excludedClass.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}:hover`).test(css);
      if (!hasOwnHover) missing.push(excludedClass);
    }
    expect(
      missing,
      "jeder per :not() von der geteilten Hover-Regel ausgeschlossene Modifier braucht eine eigene " +
        ":hover-Regel — sonst greift die generische button:hover-Regel und die für den Füllzustand " +
        "gedachte Textfarbe landet auf dem falschen Hintergrund",
    ).toEqual([]);
  });
});
