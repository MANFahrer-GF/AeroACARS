// Bilder für „Hilfe anfordern“ (Beta-1-Rückmeldung: „man kann kein Bild
// hochladen“). Die Bilder gehen als Anhang ans HelpDesk-Ticket, nicht in die
// Diagnose auf live.
//
// Grenzen und warum:
// - BILD_MAX_ANZAHL 5: genug für einen Ablauf (vorher/nachher, zwei Fenster),
//   gleiche Grenze im HelpDesk-Controller und in Rust (`hilfe.rs`).
// - BILD_MAX_BYTES 5 MB je Bild NACH dem Verkleinern: Grenze des Controllers
//   (`IMAGE_MAX_KB` 5120) und von Rust; 5 × 5 MB = 25 MB = Modulgrenze.
// - ROH_MAX_BYTES 25 MB vor dem Verkleinern: ein 4K-PNG-Screenshot hat
//   8–15 MB, er soll angenommen und dann verkleinert werden.
// - Nur PNG, JPEG, WebP: Screenshots; SVG/GIF/HEIC nicht (SVG kann Skript
//   tragen, HEIC kann die Oberfläche nicht überall lesen).
// - Jedes Bild wird neu gezeichnet (Canvas) und als JPEG gespeichert, längste
//   Kante höchstens MAX_KANTE: Text bleibt lesbar, ein Screenshot wird
//   typisch einige hundert KB statt mehrerer MB — so landen keine großen
//   Dateien in phpVMS (P-9.22). Neu zeichnen entfernt außerdem EXIF-Daten
//   (Kamera, GPS-Ort, Aufnahmezeit) aus Handyfotos.

export const BILD_MAX_ANZAHL = 5;
export const BILD_MAX_BYTES = 5 * 1024 * 1024;
export const ROH_MAX_BYTES = 25 * 1024 * 1024;
export const MAX_KANTE = 1920;
export const JPEG_QUALITAET = 0.88;
export const BILD_TYPEN = ["image/png", "image/jpeg", "image/webp"] as const;

export type BildFehler = "anzahl" | "typ" | "gross" | "lesen";

export interface HilfeBildVorbereitet {
  /** Base64 ohne `data:`-Vorspann — so geht es an `hilfe_anfordern`. */
  daten: string;
  /** data:-URL für die Vorschau (CSP img-src erlaubt data:). */
  vorschau: string;
  bytes: number;
  name: string;
}

/**
 * Welche der neuen Dateien angenommen werden (Reihenfolge bleibt), und welche
 * Fehler dabei auftraten. Reine Logik, damit sie ohne Canvas testbar ist.
 */
export function auswahlPruefen(
  vorhanden: number,
  dateien: readonly { type: string; size: number }[],
): { angenommen: number[]; fehler: BildFehler[] } {
  const angenommen: number[] = [];
  const fehler = new Set<BildFehler>();
  dateien.forEach((d, i) => {
    if (!(BILD_TYPEN as readonly string[]).includes(d.type)) {
      fehler.add("typ");
    } else if (d.size > ROH_MAX_BYTES) {
      fehler.add("gross");
    } else if (vorhanden + angenommen.length >= BILD_MAX_ANZAHL) {
      fehler.add("anzahl");
    } else {
      angenommen.push(i);
    }
  });
  return { angenommen, fehler: [...fehler] };
}

/** Zielgröße: längste Kante höchstens `max`, nie vergrößern. */
export function zielMasse(breite: number, hoehe: number, max = MAX_KANTE): { breite: number; hoehe: number } {
  const f = Math.min(1, max / Math.max(breite, hoehe));
  return { breite: Math.max(1, Math.round(breite * f)), hoehe: Math.max(1, Math.round(hoehe * f)) };
}

function alsDataUrl(blob: Blob): Promise<string> {
  return new Promise((fertig, fehler) => {
    const r = new FileReader();
    r.onload = () => fertig(String(r.result));
    r.onerror = () => fehler(r.error);
    r.readAsDataURL(blob);
  });
}

/**
 * Zeichnet das Bild neu (verkleinert, ohne Metadaten) und liefert es als JPEG.
 * Wirft `"lesen"`, wenn der Browser es nicht dekodieren kann, `"gross"`, wenn
 * es auch verkleinert über BILD_MAX_BYTES liegt.
 */
export async function bildVorbereiten(datei: Blob, name: string): Promise<HilfeBildVorbereitet> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(datei);
  } catch {
    throw "lesen" satisfies BildFehler;
  }
  const { breite, hoehe } = zielMasse(bitmap.width, bitmap.height);
  const canvas = document.createElement("canvas");
  canvas.width = breite;
  canvas.height = hoehe;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw "lesen" satisfies BildFehler;
  // JPEG kennt keine Transparenz: weiß unterlegen statt schwarz.
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, breite, hoehe);
  ctx.drawImage(bitmap, 0, 0, breite, hoehe);
  bitmap.close?.();
  const blob = await new Promise<Blob | null>((fertig) => canvas.toBlob(fertig, "image/jpeg", JPEG_QUALITAET));
  if (!blob) throw "lesen" satisfies BildFehler;
  if (blob.size > BILD_MAX_BYTES) throw "gross" satisfies BildFehler;
  const vorschau = await alsDataUrl(blob);
  return { daten: vorschau.slice(vorschau.indexOf(",") + 1), vorschau, bytes: blob.size, name };
}

/** Bilddateien aus Zwischenablage oder Ablage (andere Einträge fallen weg). */
export function dateienAus(liste: DataTransfer | null | undefined): File[] {
  if (!liste) return [];
  const aus: File[] = [];
  if (liste.files && liste.files.length > 0) {
    for (let i = 0; i < liste.files.length; i++) aus.push(liste.files[i]);
    return aus;
  }
  for (let i = 0; i < (liste.items?.length ?? 0); i++) {
    const it = liste.items[i];
    if (it.kind === "file") {
      const f = it.getAsFile();
      if (f) aus.push(f);
    }
  }
  return aus;
}
