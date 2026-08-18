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

import { mitZeitgrenze } from "./abbruch";

const STANDARD_BASIS = "https://live.kant.ovh";

/** Welches Netz die Karte zeigt. Nie beide zugleich — zwei Netze
 *  uebereinander ergeben ein Bild, in dem niemand mehr erkennt, wer
 *  welchen Luftraum betreut. */
export type Netz = "aus" | "vatsim" | "ivao";

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
/** Wie lange die Karte auf die Sektoren wartet.
 *
 *  Feldbefund 18.08.2026, Thomas' Frage „brauchen wir den Fix nicht auch im
 *  Client?": auf dem Live-Server hing ein toter Fremddienst OHNE Zeitgrenze im
 *  Anfrageweg und machte aus 1,2 s Kartenabruf 20,8 s. Serverseitig behoben —
 *  aber der Client hatte hier ebenfalls keine Zeitgrenze und haette
 *  unbegrenzt mitgewartet. Die naechste Stoerung irgendwo auf dem Weg waere
 *  derselbe Fehler.
 *
 *  15 s sind grosszuegig: die Antwort ist rund 1,2 MB (etwa 180 KB gepackt),
 *  und der Takt der Karte liegt bei 30 s — es kann also nie mehr als ein
 *  Abruf gleichzeitig unterwegs sein. */
const SEKTOREN_ZEITGRENZE_MS = 15_000;

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
  /** Welches Netz. IVAO laeuft ueber DIESELBE Route und dieselbe
   *  Zuordnung, nur mit anderer Quelle — der Server entscheidet anhand
   *  dieses Werts. Ohne Angabe bleibt es bei VATSIM. */
  netz: Netz = "vatsim",
): Promise<SektorenFuerKarte> {
  try {
    const wert = fl === "alle" ? "alle" : String(Math.round(fl));
    const q = netz === "ivao" ? `&netz=ivao` : "";
    const res = await fetch(`${basis}/api/vatglasses?fl=${wert}${q}`, {
      signal: mitZeitgrenze(signal, SEKTOREN_ZEITGRENZE_MS),
    });
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
