// ZIP-Export (P-11.33): ohne Bibliothek, Methode „gespeichert“ (JPEG ist schon komprimiert).
// Das Paket baut die ZIP-Bytes; `bild_export` speichert sie an einem vom Piloten gewaehlten Ort.
const TABELLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

export function crc32(b: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < b.length; i++) c = TABELLE[(c ^ b[i]) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

export interface ZipDatei {
  name: string;
  daten: Uint8Array;
  /** Aenderungszeit (ms); Vorgabe: 1980-01-01. */
  zeit_ms?: number;
}

/** Dateiname im ZIP: kein Pfad-Ausbruch, nur sichere Zeichen. */
export function zipName(s: string): string {
  const n = s.replace(/[\\/]+/g, "_").replace(/[^\p{L}\p{N} ._()+-]/gu, "_").replace(/^\.+/, "_").trim();
  return n === "" ? "datei" : n.slice(0, 120);
}

function dosZeit(ms: number | undefined): { zeit: number; datum: number } {
  const d = new Date(ms ?? Date.UTC(1980, 0, 1));
  const j = Math.max(1980, d.getUTCFullYear());
  return {
    zeit: (d.getUTCHours() << 11) | (d.getUTCMinutes() << 5) | (d.getUTCSeconds() >> 1),
    datum: ((j - 1980) << 9) | ((d.getUTCMonth() + 1) << 5) | d.getUTCDate(),
  };
}

export function zipBauen(dateien: ZipDatei[]): Uint8Array {
  const kodiert = new TextEncoder();
  const teile: Uint8Array[] = [];
  const zentral: Uint8Array[] = [];
  let offset = 0;
  const namen = new Set<string>();
  for (const d of dateien) {
    let name = zipName(d.name);
    for (let i = 2; namen.has(name); i++) name = name.replace(/(\.[^.]*)?$/, `-${i}$1`);
    namen.add(name);
    const nb = kodiert.encode(name);
    const crc = crc32(d.daten);
    const { zeit, datum } = dosZeit(d.zeit_ms);
    const lokal = new DataView(new ArrayBuffer(30));
    lokal.setUint32(0, 0x04034b50, true);
    lokal.setUint16(4, 20, true);
    lokal.setUint16(6, 0x0800, true); // Namen als UTF-8
    lokal.setUint16(8, 0, true); // gespeichert
    lokal.setUint16(10, zeit, true);
    lokal.setUint16(12, datum, true);
    lokal.setUint32(14, crc, true);
    lokal.setUint32(18, d.daten.length, true);
    lokal.setUint32(22, d.daten.length, true);
    lokal.setUint16(26, nb.length, true);
    lokal.setUint16(28, 0, true);
    teile.push(new Uint8Array(lokal.buffer), nb, d.daten);
    const z = new DataView(new ArrayBuffer(46));
    z.setUint32(0, 0x02014b50, true);
    z.setUint16(4, 20, true);
    z.setUint16(6, 20, true);
    z.setUint16(8, 0x0800, true);
    z.setUint16(10, 0, true);
    z.setUint16(12, zeit, true);
    z.setUint16(14, datum, true);
    z.setUint32(16, crc, true);
    z.setUint32(20, d.daten.length, true);
    z.setUint32(24, d.daten.length, true);
    z.setUint16(28, nb.length, true);
    z.setUint32(42, offset, true);
    zentral.push(new Uint8Array(z.buffer), nb);
    offset += 30 + nb.length + d.daten.length;
  }
  const zentralLaenge = zentral.reduce((s, t) => s + t.length, 0);
  const ende = new DataView(new ArrayBuffer(22));
  ende.setUint32(0, 0x06054b50, true);
  ende.setUint16(8, namen.size, true);
  ende.setUint16(10, namen.size, true);
  ende.setUint32(12, zentralLaenge, true);
  ende.setUint32(16, offset, true);
  const alle = [...teile, ...zentral, new Uint8Array(ende.buffer)];
  const aus = new Uint8Array(alle.reduce((s, t) => s + t.length, 0));
  let o = 0;
  for (const t of alle) { aus.set(t, o); o += t.length; }
  return aus;
}
