// Die Grundkarte der Live-Karte: dunkel, hell oder Satellit in EINEM Kartenstil.
//
// Vorher tauschte jeder Wechsel den ganzen Stil (`setStyle`). Das warf alle Quellen und Ebenen
// weg, setzte die Projektion zurueck und musste mit einer Nachlege-Schleife abgefangen werden.
// Jetzt liegen alle drei Grundkarten von Anfang an in der Karte, unsichtbar bis auf die gewaehlte;
// der Wechsel schaltet nur Sichtbarkeiten, alles darueber bleibt unberuehrt.
//
// Zwei vollstaendige Ebenengruppen (`d:`/`h:`), weil sich Dunkel und Hell bei CARTO nicht nur in
// Farben unterscheiden: 51 von 95 Ebenen haben andere Filter oder Layouts, die Reihenfolge weicht
// ab, die Sprites sind verschieden (gemessen 09.11.2026). Versteckte Ebenen kosten kaum etwas:
// Der Worker ueberspringt sie beim Zerlegen der Kacheln, unsichtbare Quellen laden gar nicht.
import type maplibregl from "maplibre-gl";
import type { LayerSpecification, SourceSpecification, StyleSpecification } from "maplibre-gl";

export type Grundkarte = "dunkel" | "hell" | "satellit";
type Gruppe = "d" | "h";

/** Alles, was zur Grundkarte gehoert, liegt unter dieser Ebene; alles andere darueber. */
export const EBENE_ANKER = "aa-grund-ende";
const EBENE_FLAECHE = "aa-grund-flaeche";
const SAT_EBENEN = ["esri-imagery", "esri-reference"];
const WIEDERHOLEN_MS = 15_000;
/** Haengt die Verbindung, kaeme sonst nie ein Fehler und damit nie ein neuer Versuch. */
const STIL_FRIST_MS = 20_000;

/** Solange die Vektorstile noch fehlen (oder ohne Netz), steht diese Flaeche da. */
const FLAECHE: Record<Grundkarte, string> = { dunkel: "#0b121b", hell: "#eef1f4", satellit: "#05070a" };

/** Der Stil, mit dem die Karte entsteht. Er braucht kein Netz, darum ist sie sofort bedienbar. */
export function startStil(glyphen: string, flaeche: "dunkel" | "hell"): StyleSpecification {
  const aus = { visibility: "none" } as const;
  return {
    version: 8,
    // Auch die Schriften kommen von CARTO; ohne sie fehlten auf Satellit alle Beschriftungen.
    glyphs: glyphen,
    sources: {
      "esri-imagery": {
        type: "raster",
        tiles: ["https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}"],
        tileSize: 256,
        maxzoom: 19,
        attribution: "Imagery © Esri, Maxar, Earthstar Geographics, USDA, USGS, AeroGRID, IGN, GIS User Community",
      },
      "esri-reference": {
        type: "raster",
        tiles: ["https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}"],
        tileSize: 256,
        maxzoom: 19,
      },
    },
    layers: [
      { id: EBENE_FLAECHE, type: "background", paint: { "background-color": FLAECHE[flaeche] } },
      { id: "esri-imagery", type: "raster", source: "esri-imagery", layout: aus },
      { id: "esri-reference", type: "raster", source: "esri-reference", layout: aus },
      { id: EBENE_ANKER, type: "background", layout: aus },
    ],
  };
}

export interface Grundkartensteuerung {
  /** Die Vektorstile einsetzen; ein zweiter Aufruf ersetzt die Gruppen des ersten. */
  einsetzen(stile: { dunkel: StyleSpecification; hell: StyleSpecification }): void;
  zeigen(wahl: Grundkarte): void;
}

export function grundkarteBauen(map: maplibregl.Map): Grundkartensteuerung {
  let wahl: Grundkarte = "dunkel";
  let ebenen: { id: string; gruppe: Gruppe; ursprung: boolean; sichtbar: boolean }[] = [];
  let quellen: string[] = [];
  let sprites: string[] = [];

  const sichtbarkeit = (id: string, an: boolean) => map.setLayoutProperty(id, "visibility", an ? "visible" : "none");

  const anwenden = () => {
    const gruppe: Gruppe | null = wahl === "dunkel" ? "d" : wahl === "hell" ? "h" : null;
    for (const e of ebenen) {
      // Nur echte Aenderungen: jede setzt beim Stil einen Neuaufbau der Quelle in Gang.
      const an = e.gruppe === gruppe && e.ursprung;
      if (an !== e.sichtbar) {
        sichtbarkeit(e.id, an);
        e.sichtbar = an;
      }
    }
    for (const id of SAT_EBENEN) sichtbarkeit(id, wahl === "satellit");
    map.setPaintProperty(EBENE_FLAECHE, "background-color", FLAECHE[wahl]);
  };

  const gruppeEintragen = (
    gruppe: Gruppe,
    stil: StyleSpecification,
    eingetragen: Map<string, string>,
  ) => {
    const namen = new Map<string, string>();
    for (const [name, quelle] of Object.entries(stil.sources)) {
      // Dieselbe Quelle nur einmal, sonst laedt der Hell/Dunkel-Wechsel alle Kacheln neu.
      const inhalt = JSON.stringify(quelle);
      const gemeinsam = `grund-${name}`;
      const ziel = !eingetragen.has(gemeinsam) || eingetragen.get(gemeinsam) === inhalt ? gemeinsam : `grund-${gruppe}-${name}`;
      if (!eingetragen.has(ziel)) {
        map.addSource(ziel, quelle as SourceSpecification);
        eingetragen.set(ziel, inhalt);
        quellen.push(ziel);
      }
      namen.set(name, ziel);
    }
    // In den CARTO-Stilen ist `icon-image` immer ein schlichter Name; darum genuegt das Voranstellen.
    if (typeof stil.sprite === "string") {
      map.addSprite(gruppe, stil.sprite);
      sprites.push(gruppe);
    }
    for (const ebene of stil.layers) {
      const roh = ebene as LayerSpecification & { source?: string; layout?: Record<string, unknown> };
      const bild = roh.layout?.["icon-image"];
      const spec = {
        ...roh,
        id: `${gruppe}:${roh.id}`,
        layout: {
          ...roh.layout,
          visibility: "none",
          ...(typeof bild === "string" && bild !== "" ? { "icon-image": `${gruppe}:${bild}` } : {}),
        },
        ...(roh.source ? { source: namen.get(roh.source) ?? roh.source } : {}),
      } as LayerSpecification;
      try {
        map.addLayer(spec, EBENE_ANKER);
        ebenen.push({ id: spec.id, gruppe, ursprung: roh.layout?.visibility !== "none", sichtbar: false });
      } catch (e) {
        // Eine kaputte Ebene darf fehlen, sie darf nicht die Karte leeren.
        console.warn(`[karte] Grundkarten-Ebene "${spec.id}" nicht angelegt:`, e);
      }
    }
  };

  return {
    einsetzen({ dunkel, hell }) {
      for (const e of ebenen) map.removeLayer(e.id);
      for (const q of quellen) map.removeSource(q);
      for (const s of sprites) map.removeSprite(s);
      ebenen = [];
      quellen = [];
      sprites = [];
      const glyphen = dunkel.glyphs ?? hell.glyphs;
      if (glyphen && glyphen !== map.getGlyphs()) map.setGlyphs(glyphen);
      const eingetragen = new Map<string, string>();
      gruppeEintragen("d", dunkel, eingetragen);
      gruppeEintragen("h", hell, eingetragen);
      anwenden();
    },
    zeigen(neu) {
      wahl = neu;
      anwenden();
    },
  };
}

/** Holt die beiden Stile und setzt sie ein; scheitert es, wird es wiederholt. Rueckgabe: abbrechen. */
export function grundkarteHolen(
  steuerung: Grundkartensteuerung,
  adressen: { dunkel: string; hell: string },
  anfrage: (url: string) => { url: string } | undefined,
): () => void {
  let abbruch = new AbortController();
  let beendet = false;
  let uhr = 0;
  // Selbst holen heisst: Der Umschreiber aus `transformRequest` greift hier nicht von allein.
  const stil = async (url: string, signal: AbortSignal): Promise<StyleSpecification> => {
    const antwort = await fetch(anfrage(url)?.url ?? url, { signal });
    if (!antwort.ok) throw new Error(`Kartenstil: HTTP ${antwort.status}`);
    return (await antwort.json()) as StyleSpecification;
  };
  const holen = async () => {
    abbruch = new AbortController();
    const { signal } = abbruch;
    const frist = window.setTimeout(() => abbruch.abort(), STIL_FRIST_MS);
    try {
      const [dunkel, hell] = await Promise.all([stil(adressen.dunkel, signal), stil(adressen.hell, signal)]);
      if (!beendet) steuerung.einsetzen({ dunkel, hell });
    } catch (e) {
      if (beendet) return;
      console.warn("[karte] Kartenstil nicht geladen, neuer Versuch folgt:", e);
      uhr = window.setTimeout(() => void holen(), WIEDERHOLEN_MS);
    } finally {
      window.clearTimeout(frist);
    }
  };
  void holen();
  return () => {
    beendet = true;
    abbruch.abort();
    window.clearTimeout(uhr);
  };
}
