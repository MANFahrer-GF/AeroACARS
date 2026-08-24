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

/** Die drei Sprachen, die beide Seiten führen. */
export const SPRACHEN = ["de", "en", "it"];

const summe = (t) => createHash("sha256").update(t).digest("hex").slice(0, 16);

/**
 * Die i18n-Schlüssel, die die synchronisierten Bauteile benutzen.
 *
 * Gleiche Datei heisst nicht gleiche Anzeige. Am 24.08.2026 waren die
 * sieben Dateien oben byteweise identisch — und in der Webapp fehlten
 * **46 von 108** Schlüsseln des `runway_v2`-Blocks. `t()` fällt bei einem
 * fehlenden Schlüssel stillschweigend auf seinen `defaultValue` zurück,
 * und der ist im Quelltext deutsch. Ein englischsprachiger Pilot sah in
 * der Landeanalyse die halbe Grafik auf Deutsch: „AUSROLLEN ENDE" neben
 * „ROLLOUT". Kein Fehler, kein roter Test, keine Meldung.
 *
 * Der Bauteil-Abgleich konnte das nicht sehen — er vergleicht Dateien,
 * und die Sprachdateien stehen nicht in seiner Liste. Sie DÜRFEN auch
 * nicht drin stehen: Beide Repos haben eigene Beschriftungen ausserhalb
 * der Grafik. Geprüft wird deshalb die Schnittmenge, die die Grafik
 * wirklich anfasst.
 */
function schluesselAusQuelltext(text) {
  const raus = new Set();
  // t("a.b") und t("a.b", { … }) — beide Anführungsformen.
  for (const m of text.matchAll(/\bt\(\s*["'`]([\w.]+)["'`]/g)) raus.add(m[1]);
  return raus;
}

function sprachdatei(wurzel, sprache) {
  const p = resolve(wurzel, "locales", sprache, "common.json");
  return existsSync(p) ? JSON.parse(readFileSync(p, "utf-8")) : null;
}

function hatSchluessel(baum, punktpfad) {
  let k = baum;
  for (const teil of punktpfad.split(".")) {
    if (k == null || typeof k !== "object" || !(teil in k)) return false;
    k = k[teil];
  }
  return typeof k === "string";
}

/** Welche Schlüssel die Grafik braucht — aus dem kanonischen Quelltext. */
export function benoetigteSchluessel() {
  const alle = new Set();
  for (const rel of DATEIEN) {
    const p = resolve(CLIENT, rel);
    if (!existsSync(p)) continue;
    for (const k of schluesselAusQuelltext(readFileSync(p, "utf-8"))) alle.add(k);
  }
  return [...alle].sort();
}

/**
 * Fehlt eine Beschriftung — auf welcher Seite auch immer?
 *
 * Auch der Client wird geprüft. Ein `defaultValue` im Quelltext ist eine
 * deutsche Notlösung, kein Ersatz für einen Eintrag: Solange er trägt,
 * ist die Zeile in EN und IT still deutsch.
 */
export function fehlendeSchluessel() {
  const noetig = benoetigteSchluessel();
  const luecken = [];
  const seiten = [{ name: "Client", wurzel: CLIENT }];
  if (existsSync(WEBAPP)) seiten.push({ name: "Webapp", wurzel: WEBAPP });
  for (const seite of seiten) {
    for (const sprache of SPRACHEN) {
      const baum = sprachdatei(seite.wurzel, sprache);
      if (baum == null) {
        luecken.push({ seite: seite.name, sprache, schluessel: "(Sprachdatei fehlt)" });
        continue;
      }
      for (const k of noetig) {
        if (!hatSchluessel(baum, k)) {
          luecken.push({ seite: seite.name, sprache, schluessel: k });
        }
      }
    }
  }
  return luecken;
}

/** Die fehlenden Einträge aus dem Client in die Webapp übernehmen. */
function schreibeSchluessel() {
  if (!existsSync(WEBAPP)) return 0;
  const noetig = benoetigteSchluessel();
  let n = 0;
  for (const sprache of SPRACHEN) {
    const quelle = sprachdatei(CLIENT, sprache);
    const ziel = sprachdatei(WEBAPP, sprache);
    if (quelle == null || ziel == null) continue;
    let geaendert = 0;
    for (const k of noetig) {
      if (hatSchluessel(ziel, k) || !hatSchluessel(quelle, k)) continue;
      const teile = k.split(".");
      let q = quelle;
      let z = ziel;
      for (const t of teile.slice(0, -1)) {
        q = q[t];
        if (z[t] == null || typeof z[t] !== "object") z[t] = {};
        z = z[t];
      }
      z[teile[teile.length - 1]] = q[teile[teile.length - 1]];
      geaendert++;
    }
    if (geaendert > 0) {
      writeFileSync(
        resolve(WEBAPP, "locales", sprache, "common.json"),
        JSON.stringify(ziel, null, 2) + "\n",
        "utf-8",
      );
      console.log(`  ergaenzt ${geaendert} Beschriftung(en) in locales/${sprache}`);
      n += geaendert;
    }
  }
  return n;
}

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
  const k = schreibeSchluessel();
  if (n === 0 && k === 0) console.log("Nichts zu tun — die Anzeige ist gleich.");
  else console.log(`${n} Datei(en) übernommen, ${k} Beschriftung(en) ergänzt.`);
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
    const luecken = fehlendeSchluessel();
    if (luecken.length > 0) {
      console.error("Beschriftungen fehlen:\n");
      for (const l of luecken.slice(0, 30)) {
        console.error(`  ${l.seite}/${l.sprache}: ${l.schluessel}`);
      }
      if (luecken.length > 30) console.error(`  … und ${luecken.length - 30} weitere`);
      console.error("\n  node scripts/anzeige-sync.mjs --schreiben");
      process.exit(1);
    }
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
