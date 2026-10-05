// Gleichheitstest Client ↔ Webapp (06.10.2026): der sichtbare Text einer
// gerenderten Seite, Zeile für Zeile.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Beide Seiten
// zerlegen ihre Landungsanzeige mit DIESER Funktion und vergleichen mit
// derselben Erwartung (lib/landungsFaelle.thy39.txt) — der ganze Text bis
// auf die App-Bedienung, die jede Seite vorher selbst herausnimmt.

const BLOCK = new Set([
  "DIV", "P", "LI", "H1", "H2", "H3", "H4", "DT", "DD", "TR", "TD", "TH",
  "SECTION", "HEADER", "SUMMARY", "PRE", "CODE", "SPAN", "BUTTON", "LABEL",
  "FIGCAPTION", "text", "tspan",
]);

/**
 * Text unter `wurzel` als Zeilen: jedes Block-Element (und jedes SVG-`text`)
 * beginnt eine neue Zeile, Leerraum wird zusammengezogen, leere Zeilen
 * fallen weg. Elemente unter `ohne` (Selektoren) zählen nicht mit.
 */
export function seitenText(wurzel: Element, ohne: string[] = []): string[] {
  const raus = new Set(ohne.flatMap((s) => [...wurzel.querySelectorAll(s)]));
  const teile: string[] = [];
  const gehe = (n: Node) => {
    if (n.nodeType === 3) {
      teile.push(n.textContent ?? "");
      return;
    }
    if (n.nodeType !== 1) return;
    const e = n as Element;
    if (raus.has(e) || e.tagName === "SCRIPT" || e.tagName === "STYLE") return;
    const block = BLOCK.has(e.tagName);
    if (block) teile.push("\n");
    e.childNodes.forEach(gehe);
    if (block) teile.push("\n");
  };
  gehe(wurzel);
  return teile
    .join("")
    .split("\n")
    .map((z) => z.replace(/\s+/g, " ").trim())
    .filter(Boolean);
}
