/**
 * Telemetrie-Monitor (v1.8) — Typen, gespiegelt aus
 * `src-tauri/src/telemetrie.rs`.
 */

export type Quelle = "sim" | "addon" | "berechnet" | "zusatz";
export type Art = "zahl" | "schalter" | "text";
export type Gruppe =
  | "flug"
  | "aero"
  | "wind"
  | "triebwerke"
  | "sprit"
  | "fahrwerk"
  | "steuerung"
  | "autopilot"
  | "anflug"
  | "systeme"
  | "licht"
  | "funk"
  | "umgebung"
  | "sim"
  | "flugzeug";

export const GRUPPEN: Gruppe[] = [
  "flug",
  "aero",
  "wind",
  "triebwerke",
  "sprit",
  "fahrwerk",
  "steuerung",
  "autopilot",
  "anflug",
  "systeme",
  "licht",
  "funk",
  "umgebung",
  "sim",
  "flugzeug",
];

export interface Kanal {
  id: string;
  gruppe: Gruppe;
  einheit: string;
  stellen: number;
  quelle: Quelle;
  art: Art;
}

export interface Katalog {
  zahlen: Kanal[];
  texte: Kanal[];
}

/** Ein Messpunkt: `t` in ms seit Epoche, `z`/`s` in Katalog-Reihenfolge. */
export interface Frame {
  t: number;
  z: (number | null)[];
  s: (string | null)[];
}

export interface StartAntwort {
  katalog: Katalog;
  verlauf: Frame[];
}
