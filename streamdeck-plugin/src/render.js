// Rastert Szenen im Plugin selbst (resvg + mitgelieferte Schrift B612), damit
// Windows und Mac exakt dasselbe Bild zeigen — die StreamDeck-Software kennt
// unsere Schrift nicht und fiel früher still auf Arial zurück.
//
// Eine Szene wird EINMAL gerastert und dann in Kacheln geschnitten; jede
// Kachel wird als eigenes PNG kodiert (zlib aus Node, kein Zusatzpaket).

import fs from "node:fs";
import zlib from "node:zlib";
import { initWasm, Resvg } from "@resvg/resvg-wasm";

let bereit = null;
let schriften = [];

/** Lädt Rasterer und Schriften; `ordner` = Wurzel des .sdPlugin. */
export function vorbereiten(ordner) {
  bereit ??= (async () => {
    await initWasm(fs.readFileSync(`${ordner}/wasm/resvg.wasm`));
    schriften = ["B612-Regular", "B612-Bold"].map((f) => fs.readFileSync(`${ordner}/fonts/${f}.ttf`));
  })();
  return bereit;
}

/** SVG → RGBA-Pixel. */
export function rastern(svg) {
  const bild = new Resvg(svg, { font: { fontBuffers: schriften, defaultFontFamily: "B612", loadSystemFonts: false } }).render();
  return { breite: bild.width, hoehe: bild.height, pixel: bild.pixels };
}

// ---- PNG ------------------------------------------------------------------

const CRC = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function block(typ, daten) {
  const laenge = Buffer.alloc(4);
  laenge.writeUInt32BE(daten.length);
  const kopf = Buffer.concat([Buffer.from(typ, "ascii"), daten]);
  const pruef = Buffer.alloc(4);
  pruef.writeUInt32BE(crc32(kopf));
  return Buffer.concat([laenge, kopf, pruef]);
}

/** Ausschnitt (x, y, b, h) aus RGBA-Pixeln als PNG. */
export function pngAusschnitt(bild, x, y, b, h) {
  const zeilen = Buffer.alloc(h * (b * 4 + 1));
  for (let r = 0; r < h; r++) {
    const ziel = r * (b * 4 + 1);
    zeilen[ziel] = 0; // Filter „none“
    const quelle = ((y + r) * bild.breite + x) * 4;
    zeilen.set(bild.pixel.subarray(quelle, quelle + b * 4), ziel + 1);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(b, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // Bittiefe
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    block("IHDR", ihdr),
    block("IDAT", zlib.deflateSync(zeilen, { level: 6 })),
    block("IEND", Buffer.alloc(0)),
  ]);
}

export const alsPngUrl = (png) => `data:image/png;base64,${png.toString("base64")}`;

/** Eine ganze Taste (SVG 144 × 144) als PNG-Daten-URL. */
export async function tasteAlsUrl(svg, ordner) {
  await vorbereiten(ordner);
  const bild = rastern(svg);
  return alsPngUrl(pngAusschnitt(bild, 0, 0, bild.breite, bild.hoehe));
}
