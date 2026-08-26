// Woher die Karten ihre Grundlage bekommen — Live-Karte und Logbuch.
//
// # Warum das nicht mehr im Programm steht
//
// CARTO verlangt seit dem 26.08.2026 einen Schlüssel für seine
// Basiskarten (frei bis fünf Millionen Kacheln im Monat; Raster zuerst,
// Vektor später). Bis dahin standen die Stil-Adressen als Konstanten in
// `LiveMapView.tsx` und `LogbookView.tsx`.
//
// Ein Schlüssel im Programm hiesse: Bei jeder Änderung — neuer
// Schlüssel, gesperrter Schlüssel, anderer Anbieter — braucht es ein
// Release, und jeder Pilot muss aktualisieren. Deshalb liegt er auf dem
// Server und wird hier geholt. Danach lässt er sich austauschen, ohne
// dass irgendjemand etwas installiert.
//
// # Ladeweg — derselbe wie bei der Farbtabelle
//
//   1. Sofort die eingebauten Adressen verwenden. Sie sind genau das,
//      was heute läuft, damit beim ersten Bild nichts fehlt.
//   2. Im Hintergrund `/api/basemap` holen.
//   3. Gelingt es: lokal ablegen und übernehmen.
//   4. Gelingt es nicht (offline): den lokalen Stand nehmen, sonst
//      bleibt es bei den eingebauten Adressen.
//
// Ohne hinterlegten Schlüssel liefert der Server dieselben Adressen ohne
// `?key=` zurück. Der Client läuft also auch dann, wenn nie jemand etwas
// einträgt — und läuft weiter, wenn der Server nicht erreichbar ist.

import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

/** Was die Karten brauchen. */
export interface Kartengrundlage {
  /** Stil für die dunkle Darstellung. */
  dunkel: string;
  /** Stil für die helle Darstellung. */
  hell: string;
  /** Schriftschnitte für die Beschriftung der Vektorkarte. */
  glyphen: string;
  /**
   * Die Nennung, die auf der Karte stehen muss.
   *
   * Bedingung des freien Schlüssels, nicht Höflichkeit: „CARTO and
   * OpenStreetMap attribution must stay on your maps."
   */
  nennung: string;
}

/**
 * Was gilt, solange der Server nichts gesagt hat.
 *
 * Genau die Adressen, die vor dem 26.08.2026 fest im Programm standen —
 * ohne Schlüssel. Damit sieht der erste Bildaufbau aus wie immer, und
 * ein Client ohne Netz bleibt benutzbar.
 */
export const EINGEBAUTE_GRUNDLAGE: Kartengrundlage = {
  dunkel: "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json",
  hell: "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json",
  glyphen: "https://tiles.basemaps.cartocdn.com/fonts/{fontstack}/{range}.pbf",
  nennung: "© CARTO · © OpenStreetMap-Mitwirkende",
};

const SPEICHER = "aeroacars.basemap.cache.v1";

/**
 * Aus einer Serverantwort eine vollständige Grundlage machen.
 *
 * Jedes Feld einzeln geprüft: Eine halb gefüllte Antwort darf nicht dazu
 * führen, dass die Karte gar keinen Stil mehr hat. Fehlt eines, gilt für
 * dieses eine der eingebaute Wert — nicht für alle.
 */
export function ausAntwort(roh: unknown): Kartengrundlage {
  const o = (roh ?? {}) as Record<string, unknown>;
  const text = (k: string, ersatz: string) =>
    typeof o[k] === "string" && (o[k] as string).trim().length > 0
      ? (o[k] as string)
      : ersatz;
  return {
    dunkel: text("dunkel", EINGEBAUTE_GRUNDLAGE.dunkel),
    hell: text("hell", EINGEBAUTE_GRUNDLAGE.hell),
    glyphen: text("glyphen", EINGEBAUTE_GRUNDLAGE.glyphen),
    nennung: text("nennung", EINGEBAUTE_GRUNDLAGE.nennung),
  };
}

const BasemapContext = createContext<Kartengrundlage>(EINGEBAUTE_GRUNDLAGE);

export function useKartengrundlage(): Kartengrundlage {
  return useContext(BasemapContext);
}

interface Props {
  children: ReactNode;
  /** Abweichender Endpunkt — für Tests und die Entwicklung. */
  endpunkt?: string;
}

export function BasemapProvider({
  children,
  endpunkt = "https://live.kant.ovh/api/basemap",
}: Props) {
  const [grundlage, setGrundlage] = useState<Kartengrundlage>(() => {
    try {
      const abgelegt = localStorage.getItem(SPEICHER);
      if (abgelegt) return ausAntwort(JSON.parse(abgelegt));
    } catch {
      // Ein unlesbarer Zwischenspeicher darf den Start nicht kosten.
    }
    return EINGEBAUTE_GRUNDLAGE;
  });

  useEffect(() => {
    const ac = new AbortController();
    void (async () => {
      try {
        const r = await fetch(endpunkt, {
          signal: ac.signal,
          headers: { Accept: "application/json" },
        });
        if (!r.ok) return;
        const roh = (await r.json()) as unknown;
        const frisch = ausAntwort(roh);
        setGrundlage(frisch);
        try {
          localStorage.setItem(SPEICHER, JSON.stringify(frisch));
        } catch {
          // Voller Speicher ist kein Grund, die Karte nicht zu zeigen.
        }
      } catch {
        // Offline oder Server weg: Es bleibt beim abgelegten oder
        // eingebauten Stand. Kein Fehler für den Piloten — die Karte
        // sieht dann aus wie vorher.
      }
    })();
    return () => ac.abort();
  }, [endpunkt]);

  return (
    <BasemapContext.Provider value={grundlage}>
      {children}
    </BasemapContext.Provider>
  );
}
