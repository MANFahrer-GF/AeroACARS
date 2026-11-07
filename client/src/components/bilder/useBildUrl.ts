// Bilder und Vorschauen aus dem verwalteten Ordner als Blob-Adresse (nur lokal, kein Host).
import { useEffect, useState } from "react";
import { bildAktionen } from "./logik/api";

const MAX = 300;
const adressen = new Map<string, string>();
const laufend = new Map<string, Promise<string>>();

async function holen(pfad: string): Promise<string> {
  const da = adressen.get(pfad);
  if (da) return da;
  let p = laufend.get(pfad);
  if (!p) {
    p = bildAktionen.lesen(pfad).then((bytes) => {
      const kopie = new Uint8Array(bytes.byteLength);
      kopie.set(bytes);
      const url = URL.createObjectURL(new Blob([kopie.buffer], { type: "image/jpeg" }));
      adressen.set(pfad, url);
      if (adressen.size > MAX) {
        const [aeltester] = adressen.keys();
        URL.revokeObjectURL(adressen.get(aeltester)!);
        adressen.delete(aeltester);
      }
      return url;
    }).finally(() => laufend.delete(pfad));
    laufend.set(pfad, p);
  }
  return p;
}

/** Adresse eines Bildes; null, solange es laedt oder fehlt. */
export function useBildUrl(pfad: string | null): string | null {
  const [url, setUrl] = useState<string | null>(pfad ? adressen.get(pfad) ?? null : null);
  useEffect(() => {
    let weg = false;
    setUrl(pfad ? adressen.get(pfad) ?? null : null);
    if (pfad) void holen(pfad).then((u) => { if (!weg) setUrl(u); }).catch(() => { if (!weg) setUrl(null); });
    return () => { weg = true; };
  }, [pfad]);
  return url;
}

/** Geloeschte Bilder aus dem Zwischenspeicher nehmen. */
export function bildUrlVergessen(pfade: Array<string | null>): void {
  for (const p of pfade) {
    const u = p ? adressen.get(p) : undefined;
    if (p && u) { URL.revokeObjectURL(u); adressen.delete(p); }
  }
}
