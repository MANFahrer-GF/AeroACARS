// Sind die echten Rollspuren aktuell — und in sich stimmig?
//
// # Die zwei Lücken, die das erzwungen haben
//
// **Die Datei war veraltet.** `echteSpuren.json` ist eine committete
// Momentaufnahme aus dem Exporter. Zwei Werkzeugkorrekturen später zeigte
// die Demo weiterhin die alten Werte — die Tests rendern sie erfolgreich,
// weil sie nur prüfen, DASS gerendert wird.
//
// **Und sie stammte aus dem geometrischen Rückfall.** Der Exporter las
// den Landekurs aus `touchdown_detected`; dort gibt es kein
// `heading_true_deg`. Der Kurswechsel-Zweig konnte also nie greifen, und
// alle neun Einträge kamen aus der Ersatzrechnung — ohne `kurs_diff`,
// ohne gemessene Geschwindigkeit. Ein stiller Rückfall, der plausibel
// aussah, weil Zahlen da waren.
//
// # Was diese Prüfung leistet
//
// Sie erkennt beides an den Daten selbst, ohne den Exporter zu kennen:
// Ein Eintrag aus dem Rückfall hat kein `kurs_diff` und kein `kt`. Eine
// Datei, die nach einer Werkzeugänderung nicht neu erzeugt wurde, fällt
// damit auf, sobald der Exporter ein Feld ergänzt.

import { describe, it, expect } from "vitest";
import spuren from "./echteSpuren.json";

interface Raeum {
  m: number;
  kt: number | null;
  kurs_diff?: number | null;
  kante_m?: number | null;
  seite?: string | null;
}
interface Spur {
  pirep: string;
  icao: string;
  rwy: string;
  breite_m: number;
  raeum: Raeum | null;
  punkte: Array<{ laengs_m: number; quer_m: number }>;
}

const alle = spuren as unknown as Spur[];

describe("echte Rollspuren", () => {
  it("stammen aus dem gemessenen Kurswechsel, nicht aus dem Rückfall", () => {
    const rueckfall = alle
      .filter((s) => s.raeum != null)
      .filter((s) => s.raeum!.kurs_diff == null || s.raeum!.kt == null)
      .map((s) => `${s.icao} ${s.rwy} (${s.pirep.slice(0, 8)})`);
    expect(
      rueckfall,
      "Diese Einträge haben keinen gemessenen Kurswechsel — die Datei stammt " +
        "aus dem geometrischen Rückfall und ist vor der Werkzeugkorrektur " +
        "erzeugt worden.\n" +
        "  ssh live 'cat > /tmp/spuren_export.py' < tools/korpus/spuren_export.py\n" +
        "  ssh live 'python3 /tmp/spuren_export.py'\n" +
        "  scp live:/tmp/echte_spuren.json client/src/dev/echteSpuren.json",
    ).toEqual([]);
  });

  it("hält die Reihenfolge Ausschwenken → Kante ein", () => {
    const verdreht = alle
      .filter((s) => s.raeum?.kante_m != null)
      .filter((s) => s.raeum!.kante_m! < s.raeum!.m)
      .map((s) => `${s.icao} ${s.rwy}: Kante ${s.raeum!.kante_m} < Räumpunkt ${s.raeum!.m}`);
    expect(verdreht).toEqual([]);
  });

  it("nennt eine Ausfahrtsseite, wo eine Richtung gemessen wurde", () => {
    // Nicht überall zwingend — die Regel verlangt zwei übereinstimmende
    // Größen. Aber wenn ein Kurswechsel UND eine Querbewegung vorliegen,
    // muss sie da sein.
    const ohne = alle
      .filter((s) => s.raeum?.kurs_diff != null && s.raeum?.seite == null)
      .map((s) => `${s.icao} ${s.rwy}`);
    expect(ohne).toEqual([]);
  });

  it("liefert Spuren, die die Bahn auch verlassen", () => {
    // Eine Spur, die nie über die Kante geht, taugt nicht als Demo für
    // eine Ausfahrt — und war der Zustand, solange sie bei 40 kt abbrach.
    const zuKurz = alle
      .filter((s) => {
        const halbe = s.breite_m / 2;
        return !s.punkte.some((p) => Math.abs(p.quer_m) > halbe);
      })
      .map((s) => `${s.icao} ${s.rwy}`);
    expect(zuKurz).toEqual([]);
  });
});
