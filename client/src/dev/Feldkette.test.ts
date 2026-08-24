// Trägt jedes Bahndisziplin-Feld den ganzen Weg?
//
// # Der Befund, der diese Prüfung erzwungen hat
//
// `runway_exits` stand in beiden Mappern und in der Anzeige. Der Client
// füllte es nie: `ausfahrten::ausfahrten_fuer_bahn` war gebaut, mit sieben
// eigenen Tests abgedeckt — und wurde **nirgends aufgerufen**. Die Grafik
// hatte den Platz für die Ausfahrten, bekam aber nie welche.
//
// Kein Test war rot, kein Bau schlug fehl, kein Typ passte nicht. Ein
// optionales Feld, das niemand füllt, ist in TypeScript und in Rust gleich
// unauffällig — es ist einfach `undefined`, und die Anzeige zeigt brav
// nichts an.
//
// Gefunden wurde es erst, als jemand die ganze Kette nebeneinander legte.
// Genau das tut diese Prüfung, bei jedem Testlauf.
//
// # Was sie NICHT kann
//
// Sie liest Quelltext, nicht Verhalten. Ein Feld, das überall auftaucht
// und trotzdem falsch gefüllt wird, fällt ihr nicht auf — dafür sind die
// Tests daneben da (`RunwayQS.test.tsx`, und in Rust der Verdrahtungstest
// in `bahn_felder`). Sie beantwortet eine engere Frage: Hängt irgendwo
// ein Glied der Kette in der Luft?

import { describe, it, expect } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";

const CLIENT = resolve(__dirname, "..", "..");
const WEBAPP = resolve(CLIENT, "..", "..", "aeroacars-live", "webapp");

/** Die dreizehn Felder aus Spec §8.1 plus die zwei, die dazugekommen sind. */
const FELDER = [
  "clearance_point_m",
  "scoring_cutoff_m",
  "clearance_speed_kt",
  "clearance_side",
  "track_width_m",
  "track_width_source",
  "wingspan_m",
  "runway_width_m",
  "min_edge_clearance_m",
  "max_lateral_offset_m",
  "lateral_samples",
  "surface_paved",
  "overrun_m",
  "runway_exits",
  "lateral_skip_reason",
];

/** Die Glieder der Kette, in der Reihenfolge, in der ein Wert sie durchläuft. */
const GLIEDER: Array<{ name: string; datei: string; ausschnitt?: RegExp }> = [
  {
    name: "bahn_felder (Rechnung)",
    datei: "src-tauri/src/lib.rs",
    // Nur der Rumpf der Funktion — sonst zählt jede zufällige Erwähnung
    // im Rest der 45 000 Zeilen mit.
    ausschnitt: /fn bahn_felder[\s\S]*?\n\}\n/,
  },
  { name: "storage (Platte)", datei: "src-tauri/crates/storage/src/lib.rs" },
  { name: "BahnWire (Leitung)", datei: "src-tauri/crates/aeroacars-mqtt/src/lib.rs" },
  { name: "Client-Mapper", datei: "src/dev/runwayDiagramV2Mapper.ts" },
  { name: "Anzeige", datei: "src/components/RunwayDiagramV2.tsx" },
];

function inhalt(datei: string, ausschnitt?: RegExp): string {
  const pfad = resolve(CLIENT, datei);
  const roh = readFileSync(pfad, "utf-8");
  if (!ausschnitt) return roh;
  const treffer = roh.match(ausschnitt);
  if (!treffer) throw new Error(`Ausschnitt nicht gefunden in ${datei}`);
  return treffer[0];
}

describe("Feldkette der Bahndisziplin", () => {
  it("trägt jedes Feld durch alle Glieder", () => {
    const luecken: string[] = [];
    for (const g of GLIEDER) {
      const text = inhalt(g.datei, g.ausschnitt);
      for (const f of FELDER) {
        if (!text.includes(f)) luecken.push(`${f} fehlt in: ${g.name}`);
      }
    }
    expect(
      luecken,
      "Ein Feld, das ein Glied überspringt, kommt nie an — und nichts meldet es.",
    ).toEqual([]);
  });

  it("trägt jedes Feld auch bis in die Webapp", () => {
    if (!existsSync(WEBAPP)) {
      console.warn(`[Feldkette] Webapp nicht gefunden (${WEBAPP}) — nicht geprüft.`);
      return;
    }
    const text = readFileSync(
      resolve(WEBAPP, "src/components/runwayDiagramV2Mapper.ts"),
      "utf-8",
    );
    const luecken = FELDER.filter((f) => !text.includes(f)).map(
      (f) => `${f} fehlt im Webapp-Mapper`,
    );
    expect(
      luecken,
      "Der Client rechnet es, der Server sieht es nie — die Anzeige sagt dann " +
        "für jede Landung „nicht erfasst“, ohne dass ein Fehler auftaucht.",
    ).toEqual([]);
  });

  it("lässt kein Feld auf der Leitung liegen", () => {
    // Die Umrechnung `BahnFelder::wire()` ist die einzige Übersetzung
    // zwischen Client-Rechnung und Leitung. Fehlt dort ein Feld, ist es
    // berechnet, gespeichert — und wird nicht gesendet.
    const rumpf = inhalt("src-tauri/src/lib.rs", /fn wire\(&self\)[\s\S]*?\n    \}\n/);
    const luecken = FELDER.filter((f) => !rumpf.includes(f)).map(
      (f) => `${f} wird nicht auf die Leitung gelegt`,
    );
    expect(luecken).toEqual([]);
  });
});
