// Bilder-Tab (N-13): Datenformen. Die Antworten des Programms sind bewusst mit
// optionalen Feldern beschrieben: der Rust-Teil darf Felder hinzufuegen (P-2.15),
// das Paket liest nur, was es braucht (`normalisieren.ts` macht daraus feste Formen).

/** Herkunft eines Metadatenwerts (P-11.30, P-11.32). */
export type MetaQuelle = "sim" | "protokoll" | "logbuch" | "keine";

/** `bild_meta` Fassung 1 (P-11.32). Fehlende Werte fehlen (null), nie ein Ersatzwert. */
export interface BildMeta {
  fassung: 1;
  zeit_utc: string | null;
  breite: number | null;
  laenge: number | null;
  hoehe_ft: number | null;
  gs_kt: number | null;
  kurs: number | null;
  flugzeug_titel: string | null;
  flugzeug_icao: string | null;
  phase: string | null;
  pirep_id: string | null;
  rufzeichen: string | null;
  abflug: string | null;
  ziel: string | null;
  simulator: string | null;
  naechster_flugplatz: string | null;
  quelle: MetaQuelle;
}

/** Eine Sendung eines Bildes, im Sidecar festgehalten (Fortsetzen nach Neustart). */
export interface SendeVermerk {
  sendung_id: string;
  zeit_ms: number;
  geo_stufe: GeoStufe;
  status: "angenommen" | "gesendet" | "abgelehnt";
  code?: string;
}

/** Sidecar-Dokument, wie das Paket es ueber `bild_schreiben` ablegt. */
export interface Sidecar {
  fassung: 1;
  bild_id: string;
  text: string;
  favorit: boolean;
  meta: BildMeta;
  /** Pixelmasse des gespeicherten JPEG. */
  breite_px: number | null;
  hoehe_px: number | null;
  sha256: string | null;
  erstellt_ms: number;
  sendungen: SendeVermerk[];
  /** Zuordnung per Hand (Flug nachtraeglich gesetzt). */
  flug_von_hand?: boolean;
}

export type GeoStufe = "sichtbar" | "verdeckt" | "aus";

/** Ein Eintrag aus `bilder_liste` (Kern: `speicher::Eintrag`). */
export interface Eintrag {
  id: string;
  gruppe: string;
  pfad: string | null;
  vorschau_pfad: string | null;
  quelle_pfad: string | null;
  groesse: number;
  vorschau_groesse: number;
  quelle_groesse: number;
  belegt_bytes: number;
  dateizeit_ms: number | null;
  sidecar: Partial<Sidecar> | null;
  sidecar_fehler: string | null;
  vollstaendig: boolean;
}

/** Bildquelle (`art`: standard, aufgespuert, hand). */
export interface Quelle {
  pfad: string;
  aktiv: boolean;
  original_entfernen: boolean;
  art: string | null;
}

export interface QuellenStatus extends Quelle {
  beobachtet: boolean;
  wird_geprueft: boolean;
  fehler: string | null;
  altbestand: number;
}

export interface Fundort {
  pfad: string;
  art: string;
  simulator: string | null;
  existiert: boolean;
  vorschlag_modus: string;
}

export interface AltEintrag {
  pfad: string;
  dateizeit_ms: number | null;
  groesse: number;
  format: string;
  quelle: string;
}

/** Metadaten-Basis eines Funds (Stand zur Entdeckungszeit, aus Rust). */
export type MetaBasis = Partial<Omit<BildMeta, "fassung" | "quelle">>;

/** Ein Fund im Eingang (`<bilder>/.eingang/<fund_id>.json`). */
export interface Fund {
  fund_id: string;
  /** Pfad der Originaldatei oder Eingangskopie (Aufnahme per Taste). */
  pfad: string | null;
  dateizeit_ms: number | null;
  groesse: number;
  format: string | null;
  uebernehmbar: boolean;
  /** Pixelmasse der Datei. */
  px_breite: number | null;
  px_hoehe: number | null;
  /** Ordner der Quelle, in der der Fund liegt. */
  quelle: string | null;
  art: string | null;
  sim_vordergrund: boolean | null;
  /** Simulatorfenster zur Dateizeit, falls das Programm es meldet (sonst keine Massregel). */
  sim_fenster: { breite: number; hoehe: number } | null;
  basis: MetaBasis;
}

export interface Altbestand {
  eintraege: AltEintrag[];
}

export interface BilderListe {
  eintraege: Eintrag[];
  belegt_bytes: number;
  quellen: QuellenStatus[];
  tastenkuerzel: { text: string; verfuegbar: boolean; aktiv: boolean; fehler: string | null } | null;
  aufnahme_verfuegbar: boolean;
  funde: Fund[];
  altbestand: AltEintrag[];
}

/** Stand eines Zeitpunkts aus `bild_kontext`. */
export interface KontextStand {
  zeit: string | null;
  quelle: MetaQuelle;
  basis: MetaBasis;
}

export interface Kontext {
  staende: KontextStand[];
  spur: Array<[number, number]> | null;
}

/** Antwort des Servers je Bild (P-7.25). */
export interface BildErgebnis {
  bild_id: string;
  status: "angenommen" | "gesendet" | "abgelehnt";
  code?: string;
  retry_after?: number;
}

export interface Kontingent {
  rest: number;
  grenze: number;
  retry_after: number;
}

export interface SendeAntwort {
  sendung_id?: string;
  bilder: BildErgebnis[];
  kontingent: Kontingent | null;
}
