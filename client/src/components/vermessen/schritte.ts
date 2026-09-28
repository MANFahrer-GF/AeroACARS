// „Flugzeug vermessen" — die geführten Schritte (28.09.2026).
//
// Schlüssel = `schalter` im Bericht (Server/Admin-Ansicht kennen dieselben).
// Texte stehen in den Locales unter `vermessen.schritt.<schluessel>` und
// `vermessen.stellung.<stellung>`.

export type Schalter =
  | "beacon"
  | "strobe"
  | "nav"
  | "landelicht"
  | "taxilicht"
  | "anschnall"
  | "transponder"
  | "klappen"
  | "spoiler"
  | "autobrake"
  | "apu"
  | "parkbremse";

export interface SchrittDef {
  schalter: Schalter;
  /** Stellungen in dieser Reihenfolge (Schlüssel für `vermessen.stellung.*`). */
  stellungen: string[];
  /** Klappen/Autobrake: die Zahl der Rasten ist je Flugzeug verschieden —
   *  der Pilot kann nach der zweiten Stellung „das war die letzte" wählen. */
  offenesEnde?: boolean;
}

export const SCHRITTE: SchrittDef[] = [
  { schalter: "beacon", stellungen: ["aus", "an"] },
  { schalter: "strobe", stellungen: ["off", "auto", "on"] },
  { schalter: "nav", stellungen: ["aus", "an"] },
  { schalter: "landelicht", stellungen: ["aus", "an"] },
  { schalter: "taxilicht", stellungen: ["aus", "an"] },
  { schalter: "anschnall", stellungen: ["off", "auto", "on"] },
  { schalter: "transponder", stellungen: ["stby", "alt", "auto", "ta", "tara"] },
  {
    schalter: "klappen",
    stellungen: ["klappen_up", "raste1", "raste2", "raste3", "raste4", "raste5", "raste6", "raste7"],
    offenesEnde: true,
  },
  { schalter: "spoiler", stellungen: ["spoiler_ein", "spoiler_armed", "spoiler_ein"] },
  {
    schalter: "autobrake",
    stellungen: ["ab_off", "ab_rto", "ab_1", "ab_2", "ab_3", "ab_max"],
    offenesEnde: true,
  },
  { schalter: "apu", stellungen: ["aus", "an", "aus"] },
  { schalter: "parkbremse", stellungen: ["gesetzt", "geloest", "gesetzt"] },
];
