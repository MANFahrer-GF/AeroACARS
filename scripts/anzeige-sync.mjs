#!/usr/bin/env node
// Die Landebahn-Anzeige an EINER Stelle pflegen.
//
// # Warum es dieses Skript gibt
//
// Dieselbe Grafik lief zweimal: einmal im Pilot-Client, einmal in der
// Webapp auf dem Server. Zwei Kopien, zwei Repos, keine Verbindung. Am
// 23.08.2026 gemessen unterschieden sie sich in **1066 von 1743 Zeilen** —
// die Webapp-Fassung kannte die halbe v1.7.0-Anzeige nicht, ohne dass
// irgendwo ein Fehler auftauchte. Der Pilot sah im Client eine Queransicht
// mit Spurband und auf der Webseite dieselbe Landung ohne.
//
// Das ist die Fehlerklasse aus `[[aeroacars-landebewertung-zweitimplemen-
// tierungen]]`: Zwei Stellen, die dasselbe zeigen sollen, driften
// auseinander, sobald sie nicht dieselbe Quelle haben. Die Antwort darauf
// ist nicht Sorgfalt, sondern eine Quelle.
//
// # Was kanonisch ist
//
// `client/src` im Repo `aeroacars-src`. Dort wird entwickelt, dort laufen
// die Prüfungen (`RunwayQS.test.tsx`, `RunwayLesbarkeit.test.tsx`), dort
// liegt die Demo mit allen Varianten. Die Webapp bekommt eine Kopie.
//
// # Was NICHT synchronisiert wird
//
// Die **Mapper**. `runwayDiagramV2Mapper.ts` gibt es beidseitig, aber sie
// lesen verschiedene Quellen: der Client einen `LandingRecord`, die Webapp
// ein `TouchdownDto.payload` von der Leitung. Sie sind absichtlich
// verschieden und müssen es bleiben — was sie erzeugen, ist identisch.
//
// Das **Glossar-Modal**. Es zeigt beidseitig dieselben Texte, sitzt aber
// im jeweils eigenen Dialog-Baustein (`./ui`), und die beiden haben
// verschiedene Schnittstellen — die Client-Fassung übersetzt sich nicht.
// Es ist Rahmen, nicht Grafik: Wer eine Erklärung ändert, ändert den
// i18n-Schlüssel, und der liegt ohnehin in beiden Sprachdateien.
//
// # Aufruf
//
//   node scripts/anzeige-sync.mjs            # prüfen (Rückgabewert 1 bei Drift)
//   node scripts/anzeige-sync.mjs --schreiben # kopieren
//
// Der Prüfmodus läuft in `client/src/components/AnzeigeSync.test.tsx` mit.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HIER = dirname(fileURLToPath(import.meta.url));
const CLIENT = resolve(HIER, "..", "client", "src");
// Die Webapp liegt in einem anderen Repo. Auf dem Mac nebenan, in einer
// CI ohne dieses Repo gar nicht — dann meldet das Skript das und endet
// ohne Fehler, statt einen Abgleich vorzutäuschen, den es nicht geführt hat.
const WEBAPP = resolve(HIER, "..", "..", "aeroacars-live", "webapp", "src");

/** Die Dateien der Anzeige. Der Abhängigkeitsbaum ist geschlossen. */
export const DATEIEN = [
  "components/RunwayDiagramV2.tsx",
  "components/RunwayDisciplinePanel.tsx",
  "components/RunwayCrossSection.tsx",
  "components/SkinContext.tsx",
  // Die Farbtabelle. Sie stand nicht in der ersten Fassung dieser Liste,
  // und der Baum-Test hat sie gefunden: Die Webapp-Fassung schleppte noch
  // das `labels`-Feld mit, das der Client in v0.19.x als toten Vertrag
  // entfernt hat (definiert, befüllt, gemerged — und von keiner
  // Komponente gelesen, weil beide längst i18next benutzen). Ohne diese
  // Zeile wären zwei Anzeigen mit denselben Bausteinen und verschiedenen
  // Farben möglich gewesen.
  "components/runwayV2Skin.ts",
  "lib/runwayProjection.ts",
  "lib/useBahnZoom.ts",
];

/**
 * Was zur Grafik gehört, aber bewusst repo-eigen bleibt.
 *
 * Jeder Eintrag braucht einen Grund — sonst ist diese Liste nur ein Weg,
 * den Baum-Test ruhigzustellen.
 */
export const AUSNAHMEN = {
  "./RunwayGlossaryModal":
    "sitzt im repo-eigenen Dialog-Baustein (./ui) mit anderer " +
    "Schnittstelle; zeigt nur i18n-Texte, die ohnehin in beiden " +
    "Sprachdateien liegen",
};

const summe = (t) => createHash("sha256").update(t).digest("hex").slice(0, 16);

export function vergleiche() {
  if (!existsSync(WEBAPP)) return { erreichbar: false, drift: [] };
  const drift = [];
  for (const rel of DATEIEN) {
    const a = resolve(CLIENT, rel);
    const b = resolve(WEBAPP, rel);
    const links = existsSync(a) ? readFileSync(a, "utf-8") : null;
    const rechts = existsSync(b) ? readFileSync(b, "utf-8") : null;
    if (links == null) {
      drift.push({ rel, grund: "fehlt im Client — die kanonische Seite" });
    } else if (rechts == null) {
      drift.push({ rel, grund: "fehlt in der Webapp" });
    } else if (links !== rechts) {
      drift.push({
        rel,
        grund: `Inhalt weicht ab (${summe(links)} gegen ${summe(rechts)})`,
      });
    }
  }
  return { erreichbar: true, drift };
}

function schreibe() {
  let n = 0;
  for (const rel of DATEIEN) {
    const a = resolve(CLIENT, rel);
    if (!existsSync(a)) throw new Error(`fehlt im Client: ${rel}`);
    const b = resolve(WEBAPP, rel);
    const alt = existsSync(b) ? readFileSync(b, "utf-8") : null;
    const neu = readFileSync(a, "utf-8");
    if (alt !== neu) {
      writeFileSync(b, neu, "utf-8");
      console.log(`  kopiert  ${rel}`);
      n++;
    }
  }
  console.log(n === 0 ? "Nichts zu tun — die Anzeige ist gleich." : `${n} Datei(en) übernommen.`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!existsSync(WEBAPP)) {
    console.log(`Webapp nicht gefunden (${WEBAPP}) — nichts abgeglichen.`);
    process.exit(0);
  }
  if (process.argv.includes("--schreiben")) {
    schreibe();
  } else {
    const { drift } = vergleiche();
    if (drift.length === 0) {
      console.log("Die Anzeige ist auf beiden Seiten gleich.");
    } else {
      console.error("Die Anzeige ist auseinandergelaufen:\n");
      for (const d of drift) console.error(`  ${d.rel}\n    ${d.grund}`);
      console.error("\n  node scripts/anzeige-sync.mjs --schreiben");
      process.exit(1);
    }
  }
}
