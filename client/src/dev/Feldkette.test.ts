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

  /**
   * Und der Name allein genügt nicht: Die QUELLE muss stimmen.
   *
   * Der Pilot-Client publiziert den Touchdown, **bevor** die Bewertung
   * läuft. Alles, was erst danach entsteht, ist im Touchdown-Payload
   * zwangsläufig leer — der Recorder ergänzt später nur `sub_scores`.
   *
   * Genau daran ist Runde 23 gescheitert: `lateral_skip_reason` stand im
   * Webapp-Mapper (der Name war da, diese Prüfung war grün) und wurde aus
   * `pl.lateral_skip_reason` gelesen, also aus dem Feld, das nie ankommt.
   * Die Korrektur wirkte im Pilot-Client und in der Webapp gar nicht.
   *
   * Für diese Felder muss der Mapper eine zweite Quelle haben.
   */
  it("liest Felder, die erst nach dem Publish entstehen, aus der richtigen Quelle", () => {
    if (!existsSync(WEBAPP)) return;
    const mapper = readFileSync(
      resolve(WEBAPP, "src/components/runwayDiagramV2Mapper.ts"),
      "utf-8",
    );

    // Feld → woher es in der Webapp kommen MUSS.
    const ERST_NACH_DEM_PUBLISH: Record<string, string> = {
      lateral_skip_reason: "subScores",
    };

    const falsch: string[] = [];
    for (const [feld, quelle] of Object.entries(ERST_NACH_DEM_PUBLISH)) {
      // Die Zuweisung im Mapper — mitsamt dem, was rechts davon steht.
      const stelle = new RegExp(`${feld}:[\\s\\S]{0,240}?,\\n`).exec(mapper);
      if (!stelle) {
        falsch.push(`${feld}: keine Zuweisung im Webapp-Mapper gefunden`);
        continue;
      }
      if (!stelle[0].includes(quelle)) {
        falsch.push(
          `${feld} wird nicht aus \`${quelle}\` gelesen — der ` +
            `Touchdown-Payload trägt es nie`,
        );
      }
    }
    expect(falsch).toEqual([]);

    // Und im Client muss die Publish-Stelle es ausdrücklich leer lassen —
    // sonst behauptet sie einen Wert, den sie nicht haben kann.
    const rust = readFileSync(resolve(CLIENT, "src-tauri/src/lib.rs"), "utf-8");
    expect(
      // Grosszuegiges Fenster: Zwischen dem Aufruf und dem `None` steht
      // die Begründung, warum dort nichts stehen kann — beim ersten Anlauf
      // waren 400 Zeichen zu wenig, und der Test schlug an, obwohl der
      // Code richtig war. Falscher Alarm ist so schädlich wie keiner.
      /bahn_felder\([\s\S]{0,1200}?None,\s*\)\s*\.wire\(\)/.test(rust),
      "die MQTT-Publish-Stelle gibt einen Skip-Grund mit, obwohl die " +
        "Bewertung dort noch nicht gelaufen ist",
    ).toBe(true);
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
