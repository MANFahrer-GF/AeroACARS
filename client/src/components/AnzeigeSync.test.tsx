// Die Anzeige darf nicht wieder auseinanderlaufen.
//
// Dieselbe Landebahn-Grafik lief zweimal: einmal im Pilot-Client, einmal
// in der Webapp. Am 23.08.2026 gemessen unterschieden sie sich in **1066
// von 1743 Zeilen**. Die Webapp-Fassung kannte die halbe v1.7.0-Anzeige
// nicht — keine Queransicht, kein Spurband, keine Ereignisliste — und
// nichts hat es gemeldet: Beide Fassungen kompilierten, beide Testläufe
// waren grün, beide Seiten sahen für sich plausibel aus.
//
// Genau das ist das Tückische an dieser Fehlerklasse. Sie fällt nicht auf,
// wenn man eine Seite ansieht, sondern erst, wenn jemand dieselbe Landung
// an beiden Stellen aufruft und sich fragt, welche der beiden Grafiken
// jetzt stimmt.
//
// Der Abgleich läuft über `scripts/anzeige-sync.mjs`. Dieser Test ruft ihn
// bei jedem Lauf auf.

import { describe, it, expect } from "vitest";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-expect-error — reines JS-Werkzeug ohne Typen, bewusst.
import { vergleiche, DATEIEN, AUSNAHMEN } from "../../../scripts/anzeige-sync.mjs";

const WEBAPP = resolve(
  __dirname,
  "..",
  "..",
  "..",
  "..",
  "aeroacars-live",
  "webapp",
  "src",
);

describe("Anzeige Client ↔ Webapp", () => {
  it("zeigt beidseitig dieselbe Grafik", () => {
    if (!existsSync(WEBAPP)) {
      // In einer CI ohne das Webapp-Repo lässt sich nichts vergleichen.
      // Der Test meldet das und geht durch — er täuscht keinen Abgleich
      // vor, den er nicht geführt hat.
      console.warn(
        `[Anzeige-Sync] Webapp nicht gefunden (${WEBAPP}) — nicht verglichen.`,
      );
      return;
    }
    const { drift } = vergleiche() as {
      drift: Array<{ rel: string; grund: string }>;
    };
    expect(
      drift.map((d) => `${d.rel}: ${d.grund}`),
      "Die Anzeige ist auseinandergelaufen. Abgleich:\n" +
        "  node scripts/anzeige-sync.mjs --schreiben",
    ).toEqual([]);
  });

  it("deckt den ganzen Abhängigkeitsbaum der Grafik ab", () => {
    // Eine Datei, die die Grafik braucht, aber nicht in der Liste steht,
    // wird stillschweigend nicht abgeglichen — und das ist derselbe
    // Zustand wie vorher, nur mit einem grünen Test daneben.
    //
    // Deshalb wird der Baum hier nachgerechnet statt geglaubt: Jeder
    // relative Import einer gelisteten Datei muss selbst gelistet sein.
    const liste = DATEIEN as string[];
    const readFileSync = (p: string) =>
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      (require("node:fs") as typeof import("node:fs")).readFileSync(p, "utf-8");

    const fehlend: string[] = [];
    for (const rel of liste) {
      const quelle = readFileSync(resolve(__dirname, "..", rel));
      for (const m of quelle.matchAll(/from\s+"(\.[^"]+)"/g)) {
        const ziel = resolve(resolve(__dirname, "..", rel), "..", m[1]);
        // Auf welchen Listeneintrag zeigt der Import?
        const passt = liste.some((k) =>
          resolve(__dirname, "..", k).replace(/\.tsx?$/, "") ===
          ziel.replace(/\.tsx?$/, ""),
        );
        const erlaubt = Object.keys(AUSNAHMEN as Record<string, string>).includes(
          m[1],
        );
        if (!passt && !erlaubt) fehlend.push(`${rel} → ${m[1]}`);
      }
    }
    expect(
      fehlend,
      "Diese Importe gehören zur Grafik, werden aber nicht abgeglichen.\n" +
        "Entweder in DATEIEN aufnehmen — oder im Kopf von anzeige-sync.mjs\n" +
        "begründen, warum sie repo-eigen bleiben (wie das Glossar-Modal).",
    ).toEqual([]);
  });
});
