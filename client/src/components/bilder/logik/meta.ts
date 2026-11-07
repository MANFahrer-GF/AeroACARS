// `bild_meta` Fassung 1 (P-11.32) bauen. Fehlende Werte bleiben null: kein 0 ft,
// kein 0 kt, keine geratene Position (P-11.30). Die Werte selbst kommen aus Rust
// (Fund-Basis zur Entdeckungszeit oder `bild_kontext`), das Paket baut nur das Dokument.
import type { BildMeta, MetaBasis, MetaQuelle } from "./typen";

const im = (v: number | null | undefined, lo: number, hi: number): number | null =>
  typeof v === "number" && Number.isFinite(v) && v >= lo && v <= hi ? v : null;

const nichtLeer = (v: string | null | undefined): string | null => (typeof v === "string" && v.trim() !== "" ? v.trim() : null);

/** Dateizeit (ms) als ISO-UTC; null, wenn es keine gibt. */
export function isoVonMs(ms: number | null | undefined): string | null {
  if (typeof ms !== "number" || !Number.isFinite(ms)) return null;
  const d = new Date(ms);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

export function bildMetaBauen(basis: MetaBasis, quelle: MetaQuelle, dateizeitMs: number | null): BildMeta {
  // Breite und Laenge gehoeren zusammen: eine Haelfte allein ist keine Position.
  const breite = im(basis.breite, -90, 90);
  const laenge = im(basis.laenge, -180, 180);
  const ort = breite !== null && laenge !== null;
  const kurs = im(basis.kurs, 0, 360);
  return {
    fassung: 1,
    // Die Zeit kommt aus dem Fund, sonst aus der Datei (P-11.30: Dateiname gilt nie).
    zeit_utc: nichtLeer(basis.zeit_utc) ?? isoVonMs(dateizeitMs),
    breite: ort ? breite : null,
    laenge: ort ? laenge : null,
    hoehe_ft: ort ? im(basis.hoehe_ft, -2000, 100000) : null,
    gs_kt: ort ? im(basis.gs_kt, 0, 3000) : null,
    kurs: ort ? kurs : null,
    flugzeug_titel: nichtLeer(basis.flugzeug_titel),
    flugzeug_icao: nichtLeer(basis.flugzeug_icao),
    phase: nichtLeer(basis.phase),
    pirep_id: nichtLeer(basis.pirep_id),
    rufzeichen: nichtLeer(basis.rufzeichen),
    abflug: nichtLeer(basis.abflug),
    ziel: nichtLeer(basis.ziel),
    simulator: nichtLeer(basis.simulator),
    naechster_flugplatz: ort ? nichtLeer(basis.naechster_flugplatz) : null,
    quelle,
  };
}

/** Stufe der Daten fuer die Zaehlung des Altbestands (P-11.30). */
export type DatenStufe = "mit_daten" | "nur_flug" | "ohne_daten";

export function datenStufe(m: BildMeta): DatenStufe {
  if (m.breite !== null && m.laenge !== null) return "mit_daten";
  if (m.pirep_id !== null) return "nur_flug";
  return "ohne_daten";
}

/** Hat das Bild eine Position (Karte, EXIF, Geo-Stufen)? */
export const hatPosition = (m: Pick<BildMeta, "breite" | "laenge">): boolean => m.breite !== null && m.laenge !== null;
