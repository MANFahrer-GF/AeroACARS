// Grenzwerte, Ordnername und Metadatenliste des Bilder-Tabs: eine Quelle,
// `bundle/tabellen/bilder.json` (Paketinhalt, P-11.33). Nichts davon steht im Code.
import tabelle from "../../../../../bundle/tabellen/bilder.json";
import type { GeoStufe } from "./typen";

export interface MetaZeile {
  feld: string;
  format: "position" | "hoehe" | "gs" | "kurs" | "text" | "flugzeug" | "flug" | "simulator" | "quelle" | "datei";
}

export interface Grenzen {
  max_je_sendung: number;
  max_kante_px: number;
  vorschau_kante_px: number;
  vorschau_qualitaet: number;
  jpeg_qualitaeten: number[];
  max_bytes: number;
  text_max_zeichen: number;
  nachricht_max_zeichen: number;
  serien_abstand_s: number;
  serien_min_bilder: number;
  seite: number;
  aufraeumen_tage: number;
  zuordnung_toleranz_s: number;
  sende_abfrage_s: number;
  sende_abfrage_max: number;
  tile_breite_px: number;
}

export interface Tabelle {
  grenzen: Grenzen;
  ohneFlugOrdner: string;
  geoStufen: GeoStufe[];
  geoVorgabe: GeoStufe;
  metadaten: MetaZeile[];
}

/** Liest die Tabelle; prueft die Form, damit eine kaputte Paketdatei laut scheitert. */
export function tabelleLesen(roh: unknown): Tabelle {
  const r = roh as {
    grenzen?: Record<string, unknown>;
    ordner?: { ohne_flug?: unknown };
    geo?: { stufen?: unknown; vorgabe?: unknown };
    metadaten?: unknown;
  };
  const g = r.grenzen ?? {};
  const zahl = (k: string): number => {
    const v = g[k];
    if (typeof v !== "number" || !Number.isFinite(v) || v <= 0) throw new Error(`tabellen/bilder.json: grenzen.${k} fehlt oder ist keine positive Zahl`);
    return v;
  };
  const quali = g.jpeg_qualitaeten;
  if (!Array.isArray(quali) || quali.length === 0 || !quali.every((q) => typeof q === "number" && q > 0 && q <= 1)) {
    throw new Error("tabellen/bilder.json: grenzen.jpeg_qualitaeten fehlt");
  }
  const stufen = r.geo?.stufen;
  const vorgabe = r.geo?.vorgabe;
  if (!Array.isArray(stufen) || stufen.length === 0 || typeof vorgabe !== "string" || !stufen.includes(vorgabe)) {
    throw new Error("tabellen/bilder.json: geo.stufen/vorgabe fehlen");
  }
  const ordner = r.ordner?.ohne_flug;
  if (typeof ordner !== "string" || !ordner) throw new Error("tabellen/bilder.json: ordner.ohne_flug fehlt");
  if (!Array.isArray(r.metadaten)) throw new Error("tabellen/bilder.json: metadaten fehlt");
  return {
    grenzen: {
      max_je_sendung: zahl("max_je_sendung"),
      max_kante_px: zahl("max_kante_px"),
      vorschau_kante_px: zahl("vorschau_kante_px"),
      vorschau_qualitaet: zahl("vorschau_qualitaet"),
      jpeg_qualitaeten: quali as number[],
      max_bytes: zahl("max_bytes"),
      text_max_zeichen: zahl("text_max_zeichen"),
      nachricht_max_zeichen: zahl("nachricht_max_zeichen"),
      serien_abstand_s: zahl("serien_abstand_s"),
      serien_min_bilder: zahl("serien_min_bilder"),
      seite: zahl("seite"),
      aufraeumen_tage: zahl("aufraeumen_tage"),
      zuordnung_toleranz_s: zahl("zuordnung_toleranz_s"),
      sende_abfrage_s: zahl("sende_abfrage_s"),
      sende_abfrage_max: zahl("sende_abfrage_max"),
      tile_breite_px: zahl("tile_breite_px"),
    },
    ohneFlugOrdner: ordner,
    geoStufen: stufen as GeoStufe[],
    geoVorgabe: vorgabe as GeoStufe,
    metadaten: r.metadaten as MetaZeile[],
  };
}

export const TABELLE: Tabelle = tabelleLesen(tabelle);
export const GRENZEN: Grenzen = TABELLE.grenzen;
