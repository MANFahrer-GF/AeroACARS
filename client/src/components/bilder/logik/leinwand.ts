// Canvas-Teil der Bildverarbeitung (Webview): Verkleinern auf die Grenze aus dem
// Paket, JPEG-Neukodierung (entfernt dabei jede Metadatei) und Vorschaubild.
// Immer nur EIN Bild zur Zeit (Speicher: 2560x1440 sind dekodiert ~14 MiB, P-11.33).
import { GRENZEN } from "./grenzen";
import { qualitaetWaehlen, zielMasse } from "./bildverarbeitung";

let kette: Promise<unknown> = Promise.resolve();

/** Reiht Arbeit hinter alle vorherigen: nie zwei Bilder gleichzeitig im Speicher. */
export function nacheinander<T>(arbeit: () => Promise<T>): Promise<T> {
  const lauf = kette.then(arbeit, arbeit);
  kette = lauf.catch(() => undefined);
  return lauf;
}

export interface Kodiert {
  jpeg: Uint8Array;
  vorschau: Uint8Array;
  breite: number;
  hoehe: number;
  quelleBreite: number;
  quelleHoehe: number;
}

function leinwandAnlegen(b: number, h: number): HTMLCanvasElement | OffscreenCanvas {
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(b, h);
  const c = document.createElement("canvas");
  c.width = b;
  c.height = h;
  return c;
}

async function alsJpeg(c: HTMLCanvasElement | OffscreenCanvas, q: number): Promise<Uint8Array> {
  const blob: Blob | null =
    "convertToBlob" in c
      ? await c.convertToBlob({ type: "image/jpeg", quality: q })
      : await new Promise<Blob | null>((ok) => (c as HTMLCanvasElement).toBlob(ok, "image/jpeg", q));
  if (!blob) throw new Error("JPEG-Kodierung fehlgeschlagen");
  return new Uint8Array(await blob.arrayBuffer());
}

function zeichnen(bmp: ImageBitmap, kante: number): HTMLCanvasElement | OffscreenCanvas {
  const z = zielMasse(bmp.width, bmp.height, kante);
  const c = leinwandAnlegen(z.breite, z.hoehe);
  const ctx = c.getContext("2d") as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
  if (!ctx) throw new Error("kein 2D-Kontext");
  ctx.imageSmoothingQuality = "high";
  // JPEG kennt keine Transparenz: PNG mit Alpha auf Schwarz legen, nicht auf Transparent-Schwarz mit Saum.
  ctx.fillStyle = "#000";
  ctx.fillRect(0, 0, z.breite, z.hoehe);
  ctx.drawImage(bmp, 0, 0, z.breite, z.hoehe);
  return c;
}

/** Bytes (JPEG oder PNG) -> verkleinertes JPEG und Vorschau. Reiht sich hinter laufende Arbeit. */
export function kodieren(bytes: Uint8Array, mime: string): Promise<Kodiert> {
  return nacheinander(async () => {
    const kopie = new Uint8Array(bytes.byteLength);
    kopie.set(bytes);
    const bmp = await createImageBitmap(new Blob([kopie.buffer], { type: mime })).catch(() => {
      throw Object.assign(new Error("bild_unlesbar"), { code: "bild_unlesbar" });
    });
    try {
      const gross = zeichnen(bmp, GRENZEN.max_kante_px);
      const jpeg = await qualitaetWaehlen((q) => alsJpeg(gross, q));
      const klein = zeichnen(bmp, GRENZEN.vorschau_kante_px);
      const vorschau = await alsJpeg(klein, GRENZEN.vorschau_qualitaet);
      return { jpeg, vorschau, breite: gross.width, hoehe: gross.height, quelleBreite: bmp.width, quelleHoehe: bmp.height };
    } finally {
      bmp.close();
    }
  });
}
