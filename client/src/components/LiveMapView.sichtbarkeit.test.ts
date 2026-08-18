import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";

/**
 * Wächter über die Sichtbarkeits-Sperren der Live-Karte (QS 18.08.2026).
 *
 * Seit v1.6.11 wird die Karte beim Reiterwechsel nicht mehr ausgebaut, sondern
 * nur versteckt — sonst kostet jeder Wechsel Kartenstil, Schriften und Kacheln
 * neu. Der Preis dafür: alles, was in dieser Komponente tickt, läuft ohne
 * ausdrückliche Sperre **während des ganzen Fluges** weiter, obwohl niemand
 * hinsieht. Genau das ist in der QS aufgefallen — der Umbau gegen Trägheit
 * hätte sonst selbst dauerhafte Grundlast erzeugt.
 *
 * Der Test liest den Quelltext, weil sich das strukturell prüfen lässt und ein
 * Verhaltenstest die halbe MapLibre-Welt nachbauen müsste. Er ist bewusst
 * stumpf: JEDER `setInterval` muss in einem Effekt stehen, der `sichtbar`
 * prüft. Wer einen neuen Takt hinzufügt, wird hier abgeholt.
 */

// Pfad relativ zum Projektstamm — genauso wie in `sektorenAbgleich.test.ts`.
// `import.meta.url` scheidet aus: unter jsdom ist es kein file:-URL.
const roh = readFileSync("src/components/LiveMapView.tsx", "utf8");

/**
 * Kommentare raus, Zeilenzahl erhalten.
 *
 * ⚠️ Ohne das war dieser Wächter GRÜN, egal was der Code tut: die Kommentare,
 * die die Sperren erklären, enthalten selbst das Wort „sichtbar" — die Suche
 * fand also immer einen Treffer. Aufgefallen erst durch die Gegenprobe (alten
 * Zustand herstellen, Test MUSS rot werden). Ein Wächter ohne Gegenprobe ist
 * eine Behauptung, kein Nachweis.
 */
const quelle = roh
  .replace(/\{\/\*[\s\S]*?\*\/\}/g, (m) => m.replace(/[^\n]/g, " "))
  .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
  .replace(/\/\/[^\n]*/g, (m) => " ".repeat(m.length));

/** Beginn des `useEffect`, in dem `pos` liegt — rückwärts gesucht. */
function effektAnfang(pos: number): number {
  const bis = quelle.lastIndexOf("useEffect(() => {", pos);
  return bis === -1 ? 0 : bis;
}

function zeileVon(pos: number): number {
  return quelle.slice(0, pos).split("\n").length;
}

describe("LiveMapView — Sichtbarkeits-Sperren", () => {
  it("hat die Stütze überhaupt", () => {
    expect(quelle).toContain("sichtbar?: boolean;");
    expect(quelle).toContain("sichtbar = true");
  });

  it("sperrt JEDEN Takt auf `sichtbar`", () => {
    const takte: number[] = [];
    for (let i = quelle.indexOf("setInterval("); i !== -1; i = quelle.indexOf("setInterval(", i + 1)) {
      takte.push(i);
    }
    expect(takte.length).toBeGreaterThanOrEqual(6);

    const ungesperrt = takte
      .filter((pos) => !quelle.slice(effektAnfang(pos), pos).includes("!sichtbar"))
      .map(zeileVon);

    expect(
      ungesperrt,
      `Diese setInterval laufen ohne Sichtbarkeits-Sperre (Zeilen): ${ungesperrt.join(", ")}. ` +
        "Mit der dauerhaft eingehängten Karte ticken sie den ganzen Flug lang im Verborgenen.",
    ).toEqual([]);
  });

  it("sperrt den Marker-/Kamera-Effekt, der an der Telemetrie hängt", () => {
    // Dieser Effekt hat keinen Takt — er feuert bei JEDEM Telemetriepaket und
    // fährt `easeTo`/`jumpTo`. Ohne Sperre die teuerste Stelle von allen.
    const pos = quelle.indexOf("pushSources(map, {");
    expect(pos).toBeGreaterThan(0);
    expect(quelle.slice(effektAnfang(pos), pos)).toContain("!sichtbar");
  });

  it("sperrt auch die Takte, die in einem Hook stecken", () => {
    // QS-Runde 4: `useMapEvents` bringt eigene Takte mit (Aktivitaetsprotokoll
    // alle 2 s, Hoppie-Faden ueber das Netz). Sie stehen in einer ANDEREN Datei
    // und entgehen der Suche nach `setInterval` oben — geprueft wird deshalb
    // die Uebergabe an der Aufrufstelle.
    const aufruf = quelle.match(/useMapEvents\(([^)]*)\)/);
    expect(aufruf, "useMapEvents nicht mehr aufgerufen?").not.toBeNull();
    expect(aufruf![1]).toContain("sichtbar");
  });

  it("haengt taktende Kindkomponenten verdeckt gar nicht erst ein", () => {
    // QS-Runde 5: Kindkomponenten bringen eigene Takte mit und bleiben mit der
    // Karte eingehaengt. Sperren im Inneren waeren aufwendig — verdeckt gar
    // nicht rendern ist billiger und wirkt sofort. Wer hier eine weitere
    // taktende Komponente einbaut, muss sie genauso behandeln.
    const taktendeKinder = ["LiveRecordingIndicator"];
    for (const kind of taktendeKinder) {
      const stelle = quelle.indexOf(`<${kind}`);
      expect(stelle, `${kind} wird nicht mehr gerendert?`).toBeGreaterThan(0);
      // Die umgebende Bedingung steht unmittelbar davor.
      const davor = quelle.slice(Math.max(0, stelle - 400), stelle);
      expect(davor, `${kind} rendert auch verdeckt`).toContain("sichtbar");
    }
  });

  it("misst die Leinwand neu, wenn der Reiter wieder vorkommt", () => {
    // Verdeckt steht der Behälter auf `display: none` und damit auf 0x0. Ohne
    // `resize` bliebe die Karte beim Zurückkommen leer oder verzerrt.
    expect(quelle).toContain("mapRef.current?.resize()");
  });

  it("lässt das Merken des Nordpfeil-Modus auch verdeckt zu", () => {
    // Gegenprobe zur Regel oben: die Sperre darf NICHT so weit vorne stehen,
    // dass sie das Wegschreiben der Einstellung mitverhindert.
    const pos = quelle.indexOf('localStorage.setItem("aaLivemapTrackUp"');
    expect(pos).toBeGreaterThan(0);
    expect(quelle.slice(effektAnfang(pos), pos)).not.toContain("!sichtbar");
  });
});
