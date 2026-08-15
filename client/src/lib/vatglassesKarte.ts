// VATGlasses-Sektoren für die Karte im Client — Abruf beim Live-Server.
//
// Gerechnet wird auf live.kant.ovh (`recorder/src/vatglassesKarte.ts`).
// Zwei Gründe:
//   1. Das Datenrepo hat 194 Länderdateien mit rund 28 MB. Die in den
//      Client zu laden ist unzumutbar; die frühere feste Liste von zwölf
//      Ländern hatte dafür Balkan, US-Center und ganz Asien verschluckt.
//   2. Client und Live-Karte hatten je eine eigene Kopie der Zuordnung.
//      Die sind auseinandergelaufen und haben genau deshalb
//      unterschiedliche Sektoren gezeigt. Jetzt gibt es eine Quelle.
//
// Datenquelle: github.com/lennycolton/vatglasses-data, CC BY-NC-SA 4.0.

const STANDARD_BASIS = "https://live.kant.ovh";

export interface SektorenFuerKarte {
  flaechen: GeoJSON.FeatureCollection;
  marken: GeoJSON.FeatureCollection;
  /** Rufzeichen mit echter Höhenfläche. */
  abgedeckt: Set<string>;
  /** Platz-Marker und grobe FIR-Grenzen — seit dem Umbau vom Server,
   *  damit Client und Live-Karte dieselbe Darstellung zeigen. Vorher
   *  rechnete der Client das selbst und lief auseinander: Marker
   *  uebereinander, "LGT" statt Balken, ganze FIRs statt Teilsektoren. */
  plaetze: GeoJSON.FeatureCollection;
  firFlaechen: GeoJSON.FeatureCollection;
  firMarken: GeoJSON.FeatureCollection;
  nahbereich: GeoJSON.FeatureCollection;
  nahbereichMarken: GeoJSON.FeatureCollection;
  gebucht: GeoJSON.FeatureCollection;
}

function leer(): SektorenFuerKarte {
  return {
    flaechen: { type: "FeatureCollection", features: [] },
    marken: { type: "FeatureCollection", features: [] },
    nahbereich: { type: "FeatureCollection", features: [] },
    nahbereichMarken: { type: "FeatureCollection", features: [] },
    plaetze: { type: "FeatureCollection", features: [] },
    firFlaechen: { type: "FeatureCollection", features: [] },
    firMarken: { type: "FeatureCollection", features: [] },
    gebucht: { type: "FeatureCollection", features: [] },
    abgedeckt: new Set(),
  };
}

/** Sektoren für eine Flugfläche holen.
 *
 *  Ist der Server nicht erreichbar, liefert die Funktion eine leere Lage
 *  statt zu werfen: die Karte zeigt dann Flugzeuge und Plätze weiter,
 *  nur ohne Sektorflächen. Ein Abbruch (Kartenwechsel, Neuabfrage) wird
 *  durchgereicht, damit der Aufrufer ihn wie gewohnt verwerfen kann. */
export async function ladeSektoren(
  /** Flugflaeche, oder "alle" fuer jedes Hoehenband auf einmal. Mit "alle"
   *  filtert die Karte selbst und der Hoehenregler wirkt augenblicklich,
   *  statt bei jedem Schritt neu zu laden — genauso haelt es die Live-Karte
   *  auf live.kant.ovh. Eine einzelne Flugflaeche liefert nur ein Zehntel
   *  der Sektoren (gemessen: 103 statt 1014), weshalb die beiden Karten
   *  vorher sichtbar auseinanderliefen. */
  fl: number | "alle",
  signal?: AbortSignal,
  basis: string = STANDARD_BASIS,
): Promise<SektorenFuerKarte> {
  try {
    const wert = fl === "alle" ? "alle" : String(Math.round(fl));
    const res = await fetch(`${basis}/api/vatglasses?fl=${wert}`, { signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const d = (await res.json()) as {
      flaechen?: GeoJSON.FeatureCollection;
      marken?: GeoJSON.FeatureCollection;
      abgedeckt?: string[];
      gebucht?: GeoJSON.FeatureCollection;
      tracon?: {
        flaechen?: GeoJSON.FeatureCollection;
        marken?: GeoJSON.FeatureCollection;
        abgedeckt?: string[];
      };
      atc?: {
        plaetze?: GeoJSON.FeatureCollection;
        firFlaechen?: GeoJSON.FeatureCollection;
        firMarken?: GeoJSON.FeatureCollection;
      };
    };
    const l = leer();
    return {
      flaechen: d.flaechen ?? l.flaechen,
      marken: d.marken ?? l.marken,
      nahbereich: d.tracon?.flaechen ?? l.nahbereich,
      nahbereichMarken: d.tracon?.marken ?? l.nahbereichMarken,
      gebucht: d.gebucht ?? l.gebucht,
      plaetze: d.atc?.plaetze ?? l.plaetze,
      firFlaechen: d.atc?.firFlaechen ?? l.firFlaechen,
      firMarken: d.atc?.firMarken ?? l.firMarken,
      abgedeckt: new Set([...(d.abgedeckt ?? []), ...(d.tracon?.abgedeckt ?? [])]),
    };
  } catch (e) {
    if ((e as Error)?.name === "AbortError") throw e;
    console.warn("[vatglasses] Sektoren nicht abrufbar:", e);
    return leer();
  }
}
