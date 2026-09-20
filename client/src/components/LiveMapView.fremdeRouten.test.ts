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

  it("räumt auch auf, wenn der LETZTE Kollege landet", () => {
    // Abnahme 20.09.2026, dritte Runde: Die erste Fassung hängte das
    // Aufräumen an `vaVisible.length > 0`. Landete der letzte Kollege,
    // wurde die Liste leer, das Aufräumen übersprungen — und seine Route
    // blieb als Geisterspur stehen. Ohne Marker liess sie sich nicht mehr
    // abschalten.
    const abschnitt = quelle.slice(quelle.indexOf("fremdeRoutenRef.current.size > 0"));
    const bedingung = abschnitt.slice(0, abschnitt.indexOf(")"));
    expect(bedingung, "hängt wieder an der Länge der Liste").not.toMatch(/vaVisible\.length/);
    expect(bedingung, "prüft nicht, ob die Liste aus einem echten Abruf stammt").toMatch(
      /vaListeAktuellRef\.current/,
    );
  });

  it("unterscheidet einen fehlgeschlagenen Abruf von einer leeren Liste", () => {
    // Ein Netz-Hänger setzt die Liste ebenfalls auf leer — das heisst
    // aber nicht, dass alle gelandet sind.
    const poll = quelle.slice(quelle.indexOf("const poll = async ()"));
    const bis = poll.indexOf("const id = setInterval");
    const rumpf = poll.slice(0, bis > 0 ? bis : 2000);
    expect(rumpf).toMatch(/vaListeAktuellRef\.current = true/);
    expect(rumpf).toMatch(/vaListeAktuellRef\.current = false/);
    // Der Fehlerzweig darf NICHT auf "aktuell" setzen.
    const catchTeil = rumpf.slice(rumpf.indexOf("} catch"));
    expect(catchTeil).toMatch(/vaListeAktuellRef\.current = false/);
    expect(catchTeil).not.toMatch(/vaListeAktuellRef\.current = true/);
  });

  it("vergisst beim Ausschalten, dass die Liste aktuell war", () => {
    // Sonst raeumte das Aufraeumen beim WIEDEREINSCHALTEN alles weg: Die
    // Liste ist dann noch leer (vom Ausschalten), der Merker stuende aber
    // auf „aktuell", waehrend der erste Abruf noch laeuft.
    const aus = quelle.slice(quelle.indexOf("if (!showVa) {"));
    const rumpf = aus.slice(0, aus.indexOf("return;"));
    expect(rumpf).toMatch(/vaListeAktuellRef\.current = false/);
  });

  it("versteckt die Linien, wenn die Kollegen-Anzeige aus ist", () => {
    // Sonst blieben die Routen sichtbar, während die Marker verschwinden.
    // Geprueft wird der WERT, nicht nur, dass irgendwo etwas gesetzt wird:
    // `showVa ? "none" : "visible"` waere sonst genauso gruen.
    const stelle = quelle.slice(quelle.indexOf('if (!map || !mapReady || !map.getLayer("fremde-routen-line")) return;'));
    const effekt = stelle.slice(0, stelle.indexOf("}, ["));
    expect(effekt).toMatch(
      /setLayoutProperty\(\s*"fremde-routen-line",\s*"visibility",\s*showVa \? "visible" : "none",?\s*\)/,
    );
    // Und die Abhaengigkeiten: ohne `showVa` liefe der Effekt beim
    // Umschalten nicht.
    const deps = stelle.slice(stelle.indexOf("}, ["), stelle.indexOf("}, [") + 40);
    expect(deps).toMatch(/\[\s*showVa\s*,/);
  });

  it("zieht die Sichtbarkeit nach einem Kartenstil-Wechsel nach", () => {
    // `addOverlays` legt die Ebene neu an — neue Ebenen sind sichtbar.
    // Ohne das Nachziehen kaemen die Routen zurueck, obwohl die
    // Kollegen-Anzeige aus ist (Abnahme 20.09.2026). Der Effekt oben
    // rettet das NICHT: seine Abhaengigkeiten aendern sich dabei nicht.
    const taxi = quelle.slice(quelle.indexOf("const taxiVis = showTaxiRef.current"));
    const rest = taxi.slice(0, 900);
    expect(rest).toMatch(
      /setLayoutProperty\(\s*"fremde-routen-line",\s*"visibility",\s*showVaRef\.current \? "visible" : "none",?\s*\)/,
    );
    // Ein State statt eines Refs waere hier immer der Stand vom ersten
    // Rendern — `addOverlays` haengt an einem einmaligen Handler.
    expect(quelle).toMatch(/const showVaRef = useRef\(showVa\);\s*\n\s*showVaRef\.current = showVa;/);
  });

  it("stellt eingeblendete Routen nach einem Neuaufbau der Karte wieder her", () => {
    // Nach einem Kartenstil-Wechsel ist die Quelle leer, das Ref nicht.
    const anlegen = quelle.slice(quelle.indexOf('map.addSource("fremde-routen"'));
    expect(anlegen.slice(0, 1400)).toMatch(/fremdeRoutenZeichnen\(map\)/);
  });
});
