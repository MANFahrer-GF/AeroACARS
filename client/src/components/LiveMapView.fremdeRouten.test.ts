// Routen der Kollegen auf der Karte — ein Klick zeigt, der nächste nimmt weg.
//
// Thomas, 20.09.2026: „in der Map auf die anderen Flieger klicken können und
// zusätzlich die Route angezeigt bekommen, nach einem erneuten Klick wieder
// aus." Mehrere gleichzeitig sind erlaubt, alle gedämpft und dünner als die
// eigene Route.
//
// Der Test liest den Quelltext, wie die Nachbartests dieser Komponente: Ein
// Verhaltenstest müsste die halbe MapLibre-Welt nachbauen.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const quelle = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), "LiveMapView.tsx"),
  "utf-8",
);

function rumpf(name: string): string {
  const von = quelle.indexOf(`function ${name}(`);
  expect(von, `${name} nicht gefunden`).toBeGreaterThan(-1);
  const bis = quelle.indexOf("\n  }", von);
  return quelle.slice(von, bis > von ? bis : undefined);
}

describe("Karte: Routen der Kollegen", () => {
  it("schaltet beim zweiten Klick wieder aus", () => {
    const r = rumpf("fremdeRouteUmschalten");
    // Erst prüfen, ob sie schon an ist — dann entfernen und NICHT laden.
    const hat = r.indexOf("fremdeRoutenRef.current.has(");
    const weg = r.indexOf("fremdeRoutenRef.current.delete(");
    const laden = r.indexOf('invoke<[number, number][]>("fremde_flugroute"');
    expect(hat).toBeGreaterThan(-1);
    expect(weg).toBeGreaterThan(hat);
    expect(weg, "es wird geladen, bevor ausgeblendet wird").toBeLessThan(laden);
  });

  it("sagt Bescheid, wenn es keine Route gibt", () => {
    const r = rumpf("fremdeRouteUmschalten");
    expect(r).toMatch(/punkte\.length < 2/);
    expect(r).toMatch(/setFremdeRouteHinweis\(/);
    // Und bei einem Fehler ebenfalls — nicht still schlucken.
    expect(r.match(/setFremdeRouteHinweis\(/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
  });

  it("zeichnet mehrere gleichzeitig, gedämpft und dünner als die eigene", () => {
    const zeichnen = rumpf("fremdeRoutenZeichnen");
    // Alle Einträge des Refs werden zu Linien — nicht nur der letzte.
    expect(zeichnen).toMatch(/fremdeRoutenRef\.current\.entries\(\)/);
    expect(zeichnen).toMatch(/LineString/);
    // Und Linien mit weniger als zwei Punkten fallen raus.
    expect(zeichnen).toMatch(/punkte\.length >= 2/);

    const ebene = quelle.slice(quelle.indexOf('id: "fremde-routen-line"'));
    const breite = /"line-width":\s*([\d.]+)/.exec(ebene);
    expect(breite, "keine Linienbreite gesetzt").not.toBeNull();
    expect(Number(breite![1]), "so dick wie die eigene Route").toBeLessThan(2);
  });

  it("stellt eingeblendete Routen nach einem Neuaufbau der Karte wieder her", () => {
    // Nach einem Kartenstil-Wechsel ist die Quelle leer, das Ref nicht.
    const anlegen = quelle.slice(quelle.indexOf('map.addSource("fremde-routen"'));
    expect(anlegen.slice(0, 1400)).toMatch(/fremdeRoutenZeichnen\(map\)/);
  });
});
