// EXIF-GPS von Hand (ohne Bibliothek): APP1-Segment mit GPS-IFD vor das erste
// Bildsegment eines JPEG setzen, damit die Datei auch ausserhalb des Programms
// Geo-Daten traegt (P-11.32). Fuer den Versand entfernt der Kern alle APP1-Segmente
// selbst; der Hash des Pakets (P-7.25) gilt den Bytes OHNE dieses Segment.

export interface GpsPunkt {
  breite: number;
  laenge: number;
  /** Hoehe ueber dem Meer in Fuss; null = keine Hoehenangabe schreiben. */
  hoehe_ft: number | null;
  /** Kurs in Grad (wahr); null = keine Richtung. */
  kurs: number | null;
  /** Zeit UTC (ISO); null = keine Zeitangabe. */
  zeit_utc: string | null;
}

const FT_IN_M = 0.3048;

/** Grad, Minuten, Sekunden als rationale Werte (Sekunden mit 4 Nachkommastellen). */
export function dms(grad: number): Array<[number, number]> {
  const a = Math.abs(grad);
  let d = Math.floor(a);
  let m = Math.floor((a - d) * 60);
  let s = Math.round(((a - d) * 60 - m) * 60 * 10000);
  if (s >= 60 * 10000) { s -= 60 * 10000; m += 1; }
  if (m >= 60) { m -= 60; d += 1; }
  return [[d, 1], [m, 1], [s, 10000]];
}

interface Eintrag {
  tag: number;
  typ: 1 | 2 | 5; // BYTE, ASCII, RATIONAL
  anzahl: number;
  daten: number[]; // Rohbytes (Big Endian)
}

const u32 = (n: number): number[] => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const u16 = (n: number): number[] => [(n >>> 8) & 255, n & 255];
const rationale = (paare: Array<[number, number]>): number[] => paare.flatMap(([z, n]) => [...u32(z), ...u32(n)]);
const ascii = (s: string): number[] => [...Array.from(s, (c) => c.charCodeAt(0) & 127), 0];

function eintraege(p: GpsPunkt): Eintrag[] {
  const e: Eintrag[] = [
    { tag: 1, typ: 2, anzahl: 2, daten: ascii(p.breite >= 0 ? "N" : "S") },
    { tag: 2, typ: 5, anzahl: 3, daten: rationale(dms(p.breite)) },
    { tag: 3, typ: 2, anzahl: 2, daten: ascii(p.laenge >= 0 ? "E" : "W") },
    { tag: 4, typ: 5, anzahl: 3, daten: rationale(dms(p.laenge)) },
  ];
  if (p.hoehe_ft !== null) {
    const m = p.hoehe_ft * FT_IN_M;
    e.push({ tag: 5, typ: 1, anzahl: 1, daten: [m < 0 ? 1 : 0] });
    e.push({ tag: 6, typ: 5, anzahl: 1, daten: rationale([[Math.round(Math.abs(m) * 100), 100]]) });
  }
  const t = p.zeit_utc ? new Date(p.zeit_utc) : null;
  if (t && !Number.isNaN(t.getTime())) {
    e.push({ tag: 7, typ: 5, anzahl: 3, daten: rationale([[t.getUTCHours(), 1], [t.getUTCMinutes(), 1], [t.getUTCSeconds(), 1]]) });
  }
  if (p.kurs !== null) {
    e.push({ tag: 0x10, typ: 2, anzahl: 2, daten: ascii("T") });
    e.push({ tag: 0x11, typ: 5, anzahl: 1, daten: rationale([[Math.round((((p.kurs % 360) + 360) % 360) * 100), 100]]) });
  }
  if (t && !Number.isNaN(t.getTime())) {
    const d = `${t.getUTCFullYear()}:${String(t.getUTCMonth() + 1).padStart(2, "0")}:${String(t.getUTCDate()).padStart(2, "0")}`;
    e.push({ tag: 0x1d, typ: 2, anzahl: 11, daten: ascii(d) });
  }
  return e;
}

/** Das komplette APP1-Segment (FF E1, Laenge, "Exif\0\0", TIFF mit GPS-IFD). */
export function exifGpsSegment(p: GpsPunkt): Uint8Array {
  const gps = eintraege(p);
  const ifd0Laenge = 2 + 12 + 4;
  const gpsOffset = 8 + ifd0Laenge;
  const gpsTabelle = 2 + gps.length * 12 + 4;
  let daten: number[] = [];
  const zeilen: number[] = [];
  for (const e of gps) {
    const inline = e.daten.length <= 4;
    zeilen.push(...u16(e.tag), ...u16(e.typ), ...u32(e.anzahl));
    if (inline) zeilen.push(...e.daten, ...new Array(4 - e.daten.length).fill(0));
    else {
      zeilen.push(...u32(gpsOffset + gpsTabelle + daten.length));
      daten = daten.concat(e.daten);
      if (daten.length % 2) daten.push(0);
    }
  }
  const tiff = [
    0x4d, 0x4d, 0x00, 0x2a, ...u32(8), // "MM", 42, Offset des ersten IFD
    ...u16(1), ...u16(0x8825), ...u16(4), ...u32(1), ...u32(gpsOffset), ...u32(0), // IFD0: Zeiger auf das GPS-IFD
    ...u16(gps.length), ...zeilen, ...u32(0),
    ...daten,
  ];
  const nutz = [0x45, 0x78, 0x69, 0x66, 0, 0, ...tiff];
  const laenge = nutz.length + 2;
  if (laenge > 0xffff) throw new Error("EXIF-Segment zu gross");
  return Uint8Array.from([0xff, 0xe1, ...u16(laenge), ...nutz]);
}

/** Entfernt alle APP1-Segmente (EXIF, XMP) und setzt `segment` direkt hinter SOI. */
export function exifEinsetzen(jpeg: Uint8Array, segment: Uint8Array): Uint8Array {
  if (!(jpeg.length > 4 && jpeg[0] === 0xff && jpeg[1] === 0xd8)) throw new Error("kein JPEG");
  const teile: Uint8Array[] = [jpeg.subarray(0, 2), segment];
  let i = 2;
  while (i + 4 <= jpeg.length && jpeg[i] === 0xff) {
    const m = jpeg[i + 1];
    if (m === 0xda || m === 0xd9) break; // Bilddaten beginnen
    const len = (jpeg[i + 2] << 8) | jpeg[i + 3];
    if (m !== 0xe1) teile.push(jpeg.subarray(i, i + 2 + len));
    i += 2 + len;
  }
  teile.push(jpeg.subarray(i));
  const aus = new Uint8Array(teile.reduce((s, t) => s + t.length, 0));
  let o = 0;
  for (const t of teile) { aus.set(t, o); o += t.length; }
  return aus;
}

export const app1Anzahl = (jpeg: Uint8Array): number => {
  let n = 0, i = 2;
  while (i + 4 <= jpeg.length && jpeg[i] === 0xff) {
    const m = jpeg[i + 1];
    if (m === 0xda || m === 0xd9) break;
    if (m === 0xe1) n++;
    i += 2 + ((jpeg[i + 2] << 8) | jpeg[i + 3]);
  }
  return n;
};

/** Liest Breite, Laenge und Hoehe aus unserem Segment zurueck (Test, Gegenprobe). */
export function exifGpsLesen(jpeg: Uint8Array): { breite: number; laenge: number; hoehe_m: number | null } | null {
  let i = 2;
  while (i + 4 <= jpeg.length && jpeg[i] === 0xff) {
    const m = jpeg[i + 1];
    if (m === 0xda || m === 0xd9) return null;
    const len = (jpeg[i + 2] << 8) | jpeg[i + 3];
    if (m === 0xe1 && jpeg[i + 4] === 0x45) {
      const t = i + 10; // Beginn des TIFF-Teils
      const v = new DataView(jpeg.buffer, jpeg.byteOffset, jpeg.byteLength);
      const gps = t + v.getUint32(t + 8 + 2 + 8);
      const n = v.getUint16(gps);
      const w: Record<number, { typ: number; stelle: number }> = {};
      for (let k = 0; k < n; k++) {
        const z = gps + 2 + k * 12;
        w[v.getUint16(z)] = { typ: v.getUint16(z + 2), stelle: z + 8 };
      }
      const rat = (tag: number, anz: number) => Array.from({ length: anz }, (_, k) => {
        const p = t + v.getUint32(w[tag].stelle) + k * 8;
        return v.getUint32(p) / v.getUint32(p + 4);
      });
      const ref = (tag: number) => String.fromCharCode(jpeg[w[tag].stelle]);
      const g = (tag: number, ref_: string, neg: string) => {
        const [d, mi, s] = rat(tag, 3);
        const x = d + mi / 60 + s / 3600;
        return ref(ref_ === "N" ? 1 : 3) === neg ? -x : x;
      };
      const breite = g(2, "N", "S");
      const laenge = g(4, "E", "W");
      let hoehe: number | null = null;
      if (w[6]) hoehe = rat(6, 1)[0] * (jpeg[w[5].stelle] === 1 ? -1 : 1);
      return { breite, laenge, hoehe_m: hoehe };
    }
    i += 2 + len;
  }
  return null;
}
