// Bilder und Vorschauen aus dem verwalteten Ordner als Blob-Adresse (nur lokal, kein Host).
import { useEffect, useState } from "react";
import { bildAktionen } from "./logik/api";
import type { Eintrag } from "./logik/typen";

const MAX = 300;
/** Vollbilder aus der Grossansicht sind gross: am Tablet zaehlt der Speicher, nicht die Anzahl. */
const MAX_BYTES = 200 * 1024 * 1024;
const adressen = new Map<string, { url: string; bytes: number }>();
let belegt = 0;
const laufend = new Map<string, Promise<string>>();

function vergessen(pfad: string): void {
  const e = adressen.get(pfad);
  if (!e) return;
  URL.revokeObjectURL(e.url);
  belegt -= e.bytes;
  adressen.delete(pfad);
}

async function holen(pfad: string): Promise<string> {
  const da = adressen.get(pfad);
  if (da) return da.url;
  let p = laufend.get(pfad);
  if (!p) {
    p = bildAktionen.lesen(pfad).then((bytes) => {
      const kopie = new Uint8Array(bytes.byteLength);
      kopie.set(bytes);
      const url = URL.createObjectURL(new Blob([kopie.buffer], { type: "image/jpeg" }));
      adressen.set(pfad, { url, bytes: bytes.byteLength });
      belegt += bytes.byteLength;
      // Das eben geladene Bild bleibt, auch wenn es allein ueber der Grenze liegt.
      while (adressen.size > 1 && (adressen.size > MAX || belegt > MAX_BYTES)) {
        const [aeltester] = adressen.keys();
        vergessen(aeltester);
      }
      return url;
    }).finally(() => laufend.delete(pfad));
    laufend.set(pfad, p);
  }
  return p;
}

/** Adresse eines Bildes; null, solange es laedt oder fehlt. */
export function useBildUrl(pfad: string | null): string | null {
  const [stand, setStand] = useState<{ pfad: string | null; url: string | null }>({ pfad: null, url: null });
  useEffect(() => {
    let weg = false;
    if (pfad) void holen(pfad).then((u) => { if (!weg) setStand({ pfad, url: u }); }).catch(() => { if (!weg) setStand({ pfad, url: null }); });
    return () => { weg = true; };
  }, [pfad]);
  // Nie die Adresse des vorigen Pfads zeigen: beim Blaettern stuende sonst kurz das alte Bild unter dem neuen Kopf.
  if (!pfad) return null;
  return stand.pfad === pfad ? stand.url : adressen.get(pfad)?.url ?? null;
}

/** Kleines Bild fuer die Kartenpunkte; null, wenn es keins gibt. Nie das Vollbild: Ueberfahren vieler Punkte verdraengte sonst alles andere aus dem Zwischenspeicher. */
export function vorschauAdresse(e: Eintrag | undefined): Promise<string | null> {
  return e?.vorschau_pfad ? holen(e.vorschau_pfad).catch(() => null) : Promise.resolve(null);
}

/** Geloeschte Bilder aus dem Zwischenspeicher nehmen. */
export function bildUrlVergessen(pfade: Array<string | null>): void {
  for (const p of pfade) if (p) vergessen(p);
}
