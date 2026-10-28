// Format der Flugakte (Server: recorder/src/flugakte.ts) und feste Werte der Flugkarte.
// Gehoert zum Baustein flugkarte.ts und wird mit ihm unveraendert in den Client uebernommen.

export interface Flugakte {
  version: number;
  kopf: { airline_icao: string | null; flight_number: string | null; dpt_airport: string | null; arr_airport: string | null; start: string | null };
  flugzeug: { icao: string | null; titel: string | null; sim: string | null; client: string | null };
  dep_gate: string | null;
  arr_gate: string | null;
  route: string | null;
  /** Klappen aus dem Hebel (Raste) oder ersatzweise aus der Stellung (Prozent); null = gar nicht */
  klappenArt: "hebel" | "stellung" | null;
  /** Speedbrake-Ruhestellung in % (Recorder, seit Version 2) */
  sbRuhe?: number;
  /** je Gruppe: false = in diesem Flugzeug nicht messbar */
  messbar: Record<string, boolean>;
  abdeckung: Record<string, number>;
  land: Record<string, string | number | null> | null;
  filed: Record<string, unknown> | null;
  /** [lat, lon, alt, agl, gs, ias, vs, hdg, pitch, bank, fuel, hh:mm:ss, amBoden, klappen, fahrwerk, spoiler%, ap, athr, landelicht] — fehlende Werte null */
  track: Array<Array<number | string | null>>;
  /** code + werte: Text in der Sprache der Anzeige (flugkarte.ev/slot/phase in common.json); txt: derselbe auf Deutsch (seit Version 3) */
  events: Array<{ t: string; i: number; k: string; code?: string; werte?: Record<string, string | number>; txt: string; [mehr: string]: unknown }>;
}

/** Kuerzel, Name und Erklaerung je Kategorie stehen in common.json unter flugkarte.kat.<k> */
export interface Kategorie {
  col: string;
  /** beim Oeffnen sichtbar („wichtigste“) */
  an: boolean;
}

export const KATEGORIEN: Record<string, Kategorie> = {
  flaps: { col: "#60a5fa", an: true },
  gear: { col: "#fbbf24", an: true },
  spoiler: { col: "#f87171", an: true },
  ap: { col: "#a78bfa", an: true },
  fcu: { col: "#e879f9", an: false },
  alt: { col: "#2dd4bf", an: true },
  light: { col: "#a3e635", an: false },
  cabin: { col: "#f9a8d4", an: false },
  pa: { col: "#fde68a", an: true },
  eng: { col: "#fb923c", an: false },
  brake: { col: "#94a3b8", an: false },
  radio: { col: "#67e8f9", an: false },
  warn: { col: "#ef4444", an: true },
  sim: { col: "#e5e7eb", an: true },
  phase: { col: "#cbd5e1", an: false },
  to: { col: "#ffffff", an: true },
  ga: { col: "#ffffff", an: true },
  td: { col: "#ffffff", an: true },
  exit: { col: "#ffffff", an: true },
  gate: { col: "#ffffff", an: true },
};

/** Etwas mehr Kontrast fuer die dunkle OpenFreeMap-Karte, damit Strassen, Bahnen und Orte unter der Flugspur lesbar bleiben (abgenommen im Entwurf). */
export const FARBEN_DUNKEL: Record<string, Record<string, unknown>> = {
  water: { "fill-color": "#0d2134" }, waterway: { "line-color": "#123049" },
  landuse_residential: { "fill-color": "#1b2532", "fill-opacity": 0.65 },
  landcover_wood: { "fill-color": "#13221b" }, landuse_park: { "fill-color": "#15241c" },
  building: { "fill-color": "#1e2936", "fill-outline-color": "#283547" },
  "aeroway-area": { "fill-color": "#1a2430" }, "aeroway-taxiway": { "line-color": "#3b4858" },
  "aeroway-runway-casing": { "line-color": "rgba(140,160,185,0.9)" }, "aeroway-runway": { "line-color": "#4b5969" },
  highway_path: { "line-color": "#222c38" }, highway_minor: { "line-color": "#27323f" },
  highway_major_casing: { "line-color": "#3c4a5a" }, highway_major_inner: { "line-color": "#2c3845" }, highway_major_subtle: { "line-color": "#2f3b49" },
  highway_motorway_casing: { "line-color": "#4d5c6e" }, highway_motorway_subtle: { "line-color": "#2f3b49" },
  railway: { "line-color": "#2f3a47" }, railway_minor: { "line-color": "#2f3a47" }, railway_transit: { "line-color": "#2f3a47" },
  highway_name_other: { "text-color": "#7f90a3", "text-halo-color": "#0b121b" }, highway_name_motorway: { "text-color": "#8a9bae" },
  water_name: { "text-color": "#6f97be", "text-halo-color": "#0b121b" },
  boundary_state: { "line-color": "#3a4858" }, "boundary_country_z0-4": { "line-color": "#4a5a6c" }, "boundary_country_z5-": { "line-color": "#4a5a6c" },
};
