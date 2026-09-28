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
  | "parkbremse"
  // Teil „luft" (28.09.2026): Autopilot rastet am Boden nicht ein.
  | "autopilot"
  | "ap2"
  | "lateral"
  | "vertikal"
  | "anflug"
  | "autoland"
  | "autobrake_luft"
  | "btv";

/** „boden" = am Gate, „luft" = im Flug (Autopilot, Anflug). */
export type Teil = "boden" | "luft";

export interface SchrittDef {
  schalter: Schalter;
  teil: Teil;
  /** Stellungen in dieser Reihenfolge (Schlüssel für `vermessen.stellung.*`). */
  stellungen: string[];
  /** Klappen/Autobrake: die Zahl der Rasten ist je Flugzeug verschieden —
   *  der Pilot kann nach der zweiten Stellung „das war die letzte" wählen. */
  offenesEnde?: boolean;
}

export const SCHRITTE: SchrittDef[] = [
  { schalter: "beacon", teil: "boden", stellungen: ["aus", "an"] },
  { schalter: "strobe", teil: "boden", stellungen: ["off", "auto", "on"] },
  // Airbus: NAV & LOGO in einer Taste mit zwei Stellungen (Fenix 0/1/2) —
  // wer nur an/aus hat, wählt nach AN „letzte Stellung“.
  { schalter: "nav", teil: "boden", stellungen: ["aus", "an", "nav_2"], offenesEnde: true },
  // Airbus: ON / OFF / RETRACT — RETRACT als dritte, optionale Stellung.
  { schalter: "landelicht", teil: "boden", stellungen: ["aus", "an", "lande_retract"], offenesEnde: true },
  // Airbus-Bugscheinwerfer OFF/TAXI/T.O., A220 OFF/NARROW/WIDE — manche
  // haben nur an/aus, dann nach zwei Stellungen „letzte Stellung“.
  { schalter: "taxilicht", teil: "boden", stellungen: ["aus", "taxi_1", "taxi_2"], offenesEnde: true },
  { schalter: "anschnall", teil: "boden", stellungen: ["off", "auto", "on"] },
  { schalter: "transponder", teil: "boden", stellungen: ["stby", "alt", "auto", "ta", "tara"] },
  {
    schalter: "klappen",
    teil: "boden",
    stellungen: ["klappen_up", "raste1", "raste2", "raste3", "raste4", "raste5", "raste6", "raste7"],
    offenesEnde: true,
  },
  { schalter: "spoiler", teil: "boden", stellungen: ["spoiler_ein", "spoiler_armed", "spoiler_ein"] },
  {
    schalter: "autobrake",
    teil: "boden",
    stellungen: ["ab_off", "ab_rto", "ab_1", "ab_2", "ab_3", "ab_max"],
    offenesEnde: true,
  },
  { schalter: "apu", teil: "boden", stellungen: ["aus", "an", "aus"] },
  { schalter: "parkbremse", teil: "boden", stellungen: ["gesetzt", "geloest", "gesetzt"] },
  // In der Luft, im ruhigen Reiseflug. Jeweils zurück in die Ausgangslage —
  // die Auswertung verlangt dann gleiche Werte bei gleicher Stellung.
  { schalter: "autopilot", teil: "luft", stellungen: ["ap_an", "ap_aus", "ap_an"] },
  // Zwei Autopiloten (Airbus AP1/AP2, Boeing CMD A/B): im Reiseflug ist immer
  // nur einer drin — AP2 drücken, AP1 geht aus.
  { schalter: "ap2", teil: "luft", stellungen: ["ap1_aktiv", "ap2_aktiv", "ap1_aktiv"] },
  { schalter: "lateral", teil: "luft", stellungen: ["nav_managed", "hdg_selected", "nav_managed"] },
  { schalter: "vertikal", teil: "luft", stellungen: ["alt_halten", "vs_modus", "alt_halten"] },
  // Nur im Anflug sinnvoll — überspringbar. Auch hier aus/an/aus, damit im
  // Flug driftende Werte (Position, Sprit) herausfallen.
  { schalter: "anflug", teil: "luft", stellungen: ["app_aus", "app_an", "app_aus"] },
  // Autoland / CAT III: im ILS-Anflug mit APPR beide AP. Endet mit beiden an,
  // das Autoland läuft normal weiter.
  { schalter: "autoland", teil: "luft", stellungen: ["ein_ap", "zwei_ap", "ein_ap", "zwei_ap"] },
  // Stufen, die sich erst in der Luft wählen lassen (A220: LO/MED/HI).
  {
    schalter: "autobrake_luft",
    teil: "luft",
    stellungen: ["ab_off", "ab_1", "ab_2", "ab_max"],
    offenesEnde: true,
  },
  { schalter: "btv", teil: "luft", stellungen: ["btv_aus", "btv_an", "btv_aus"] },
];

export const schritteFuer = (teil: Teil) => SCHRITTE.filter((x) => x.teil === teil);
