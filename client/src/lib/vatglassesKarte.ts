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
}

function leer(): SektorenFuerKarte {
  return {
    flaechen: { type: "FeatureCollection", features: [] },
    marken: { type: "FeatureCollection", features: [] },
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
  fl: number,
  signal?: AbortSignal,
  basis: string = STANDARD_BASIS,
): Promise<SektorenFuerKarte> {
  try {
    const res = await fetch(`${basis}/api/vatglasses?fl=${Math.round(fl)}`, { signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const d = (await res.json()) as {
      flaechen?: GeoJSON.FeatureCollection;
      marken?: GeoJSON.FeatureCollection;
      abgedeckt?: string[];
    };
    const l = leer();
    return {
      flaechen: d.flaechen ?? l.flaechen,
      marken: d.marken ?? l.marken,
      abgedeckt: new Set(d.abgedeckt ?? []),
    };
  } catch (e) {
    if ((e as Error)?.name === "AbortError") throw e;
    console.warn("[vatglasses] Sektoren nicht abrufbar:", e);
    return leer();
  }
}
