// Verkleinern, JPEG-Neukodierung und Hash (P-11.33). Die Rechnung steht hier ohne
// Canvas (testbar); `leinwand.ts` fuehrt sie in der Webview aus, nacheinander.
import { GRENZEN } from "./grenzen";

/** Zielmasse: laengste Kante hoechstens `maxKante`, nie vergroessern, Seitenverhaeltnis erhalten. */
export function zielMasse(breite: number, hoehe: number, maxKante: number): { breite: number; hoehe: number; verkleinert: boolean } {
  const laengste = Math.max(breite, hoehe);
  if (!(breite > 0) || !(hoehe > 0)) throw new Error("Bildmasse ungueltig");
  if (laengste <= maxKante) return { breite, hoehe, verkleinert: false };
  const f = maxKante / laengste;
  return { breite: Math.max(1, Math.round(breite * f)), hoehe: Math.max(1, Math.round(hoehe * f)), verkleinert: true };
}

/** Erste Qualitaetsstufe, deren Ergebnis unter der Grenze liegt; sonst die kleinste. */
export async function qualitaetWaehlen(
  kodieren: (q: number) => Promise<Uint8Array>,
  qualitaeten: number[] = GRENZEN.jpeg_qualitaeten,
  maxBytes: number = GRENZEN.max_bytes,
): Promise<Uint8Array> {
  let letzte: Uint8Array | null = null;
  for (const q of qualitaeten) {
    letzte = await kodieren(q);
    if (letzte.length <= maxBytes) return letzte;
  }
  if (!letzte) throw new Error("keine Qualitaetsstufe");
  return letzte;
}

export function istJpeg(b: Uint8Array): boolean {
  return b.length >= 4 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff;
}

export function istPng(b: Uint8Array): boolean {
  return b.length >= 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47;
}

/** Pixelmasse aus dem Kopf einer JPEG-Datei (SOF) oder PNG (IHDR); null, wenn nicht lesbar. */
export function pixelMasse(b: Uint8Array): { breite: number; hoehe: number } | null {
  if (istPng(b) && b.length >= 24) {
    const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
    return { breite: v.getUint32(16), hoehe: v.getUint32(20) };
  }
  if (!istJpeg(b)) return null;
  let i = 2;
  while (i + 9 < b.length) {
    if (b[i] !== 0xff) { i++; continue; }
    const m = b[i + 1];
    if (m === 0xd8 || m === 0x01 || (m >= 0xd0 && m <= 0xd7)) { i += 2; continue; }
    const len = (b[i + 2] << 8) | b[i + 3];
    if ((m >= 0xc0 && m <= 0xcf) && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) {
      return { hoehe: (b[i + 5] << 8) | b[i + 6], breite: (b[i + 7] << 8) | b[i + 8] };
    }
    i += 2 + len;
  }
  return null;
}

export async function sha256Hex(b: Uint8Array): Promise<string> {
  const kopie = new Uint8Array(b.byteLength);
  kopie.set(b);
  const h = new Uint8Array(await crypto.subtle.digest("SHA-256", kopie.buffer));
  return Array.from(h, (x) => x.toString(16).padStart(2, "0")).join("");
}

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** Base64 (Standard-Alphabet, mit Auffuellung) ohne Browser-Funktionen: laeuft in Tests und Webview gleich. */
export function alsBase64(b: Uint8Array): string {
  let s = "";
  for (let i = 0; i < b.length; i += 3) {
    const n = (b[i] << 16) | ((b[i + 1] ?? 0) << 8) | (b[i + 2] ?? 0);
    s += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < b.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < b.length ? B64[n & 63] : "=");
  }
  return s;
}
