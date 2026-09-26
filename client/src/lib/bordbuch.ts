// Bordbuch (26.09.2026) — Typen und Anzeige-Regeln. Die Auswertung selbst
// läuft in Rust (`src-tauri/src/bordbuch.rs`); hier steht nur, was die
// Oberfläche daraus macht. Grundsatz: zählt Erledigtes, nie Fehler.

export type Klasse = "airliner" | "business_jet" | "ga";
export type Art = "pflicht" | "bestaetigung";
export type Status =
  | "offen"
  | "erledigt"
  | "diesmal_ohne"
  | "nach_atc"
  | "nicht_messbar"
  | "nicht_anwendbar";
export type Grund = "wert_fehlt" | "kein_tcas_modus";

export type Regel =
  | "beacon_anlassen"
  | "nav_lichter"
  | "parkbremse_geloest"
  | "rolltempo_abflug"
  | "strobes_start"
  | "landelicht_start"
  | "transponder_start"
  | "tcas_start"
  | "klappen_start"
  | "anschnall_start"
  | "apu_reiseflug"
  | "landelicht_anflug"
  | "autobrake_landung"
  | "spoiler_landung"
  | "anschnall_landung"
  | "rolltempo_ankunft";

export type Schalter =
  | "beacon"
  | "strobes"
  | "nav"
  | "transponder"
  | "landelicht"
  | "rolltempo"
  | "parkbremse"
  | "apu"
  | "autobrake"
  | "spoiler"
  | "anschnallzeichen"
  | "klappen";

export type Abschnitt =
  | "vor_dem_rollen"
  | "rollen"
  | "start"
  | "reiseflug"
  | "anflug"
  | "nach_der_landung";

export const ABSCHNITTE: Abschnitt[] = [
  "vor_dem_rollen",
  "rollen",
  "start",
  "reiseflug",
  "anflug",
  "nach_der_landung",
];

/** Reihenfolge der Schalter in Einstellungen und Routine. */
export const SCHALTER: Schalter[] = [
  "beacon",
  "strobes",
  "nav",
  "transponder",
  "landelicht",
  "rolltempo",
  "parkbremse",
  "apu",
  "autobrake",
  "spoiler",
  "anschnallzeichen",
  "klappen",
];

/** Voreinstellung = Umfrage-Favoriten (spiegelt `Schalter::voreinstellung`). */
export const VOREINSTELLUNG: Record<Schalter, boolean> = {
  beacon: true,
  strobes: true,
  nav: true,
  transponder: true,
  landelicht: false,
  rolltempo: true,
  parkbremse: false,
  apu: true,
  autobrake: false,
  spoiler: false,
  anschnallzeichen: false,
  klappen: false,
};

export interface Punkt {
  regel: Regel;
  schalter: Schalter;
  abschnitt: Abschnitt;
  art: Art | null;
  status: Status;
  auto_status: Status;
  zeit: string | null;
  hoehe_ft: number | null;
  stellung: string | null;
  grund: Grund | null;
  beleg?: Record<string, unknown>;
  markiert_at: string | null;
}

export interface FlugInfo {
  callsign?: string | null;
  dep?: string | null;
  arr?: string | null;
  muster?: string | null;
  titel?: string | null;
  profil?: string | null;
  sim?: string | null;
}

export interface Eintrag {
  schema: number;
  pirep_id: string;
  erstellt_at: string;
  updated_at: string;
  client_version: string;
  flug: FlugInfo;
  klasse: Klasse;
  klasse_quelle: string;
  regelwerk: "ifr" | "vfr" | string;
  nacht_start: boolean | null;
  nacht_landung: boolean | null;
  zeitquelle: string | null;
  eingeschaltet: Schalter[];
  aus_grund: "ga" | "vfr" | string | null;
  punkte: Punkt[];
  rollen_max_abflug_kt: number;
  rollen_max_ankunft_kt: number;
  rolltempo_grenze_kt: number;
  profil: Array<[number, number]>;
  synced?: boolean;
}

export interface Hinweis {
  regel: Regel;
  seit: string;
}

export interface LiveAnsicht {
  klasse: Klasse | null;
  nacht_start: boolean | null;
  eingeschaltet: Schalter[];
  aus_grund: string | null;
  punkte: Punkt[];
  hinweis: Hinweis | null;
  profil: Array<[number, number]>;
  rolltempo_grenze_kt: number;
  hinweise_im_flug: boolean;
}

export interface Einstellungen {
  schema: number;
  updated_at: string | null;
  hinweise_im_flug: boolean;
  ga_an: boolean;
  vfr_an: boolean;
  regeln: Partial<Record<Schalter, boolean>>;
  rolltempo_kt: number;
  rolltempo_ga_kt: number;
  klassen_override: Record<string, Klasse>;
}

export function schalterAn(e: Einstellungen, s: Schalter): boolean {
  return e.regeln[s] ?? VOREINSTELLUNG[s];
}

/**
 * Was der Pilot sieht. Pflichtpunkte immer (ausser „kam nicht vor"),
 * Bestätigungen nur, wenn erledigt — eine nicht befolgte Empfehlung
 * taucht gar nicht auf. Ausgeschaltete Regeln und Punkte, die nicht zur
 * Klasse passen, fehlen ebenfalls.
 */
export function sichtbar(p: Punkt, eingeschaltet: Schalter[]): boolean {
  if (!eingeschaltet.includes(p.schalter) || p.art == null) return false;
  if (p.status === "nicht_anwendbar") return false;
  if (p.art === "bestaetigung") {
    return p.status === "erledigt" || p.status === "nach_atc" || p.status === "offen";
  }
  return true;
}

export function erledigt(s: Status): boolean {
  return s === "erledigt" || s === "nach_atc";
}

/** „11 von 13" — dieselbe Zählung wie `Eintrag::bilanz` in Rust. */
export function bilanz(punkte: Punkt[], eingeschaltet: Schalter[]): { ok: number; von: number } {
  const zaehlbar = punkte.filter(
    (p) =>
      p.art === "pflicht" &&
      eingeschaltet.includes(p.schalter) &&
      p.status !== "nicht_messbar" &&
      p.status !== "nicht_anwendbar" &&
      p.status !== "offen",
  );
  return { ok: zaehlbar.filter((p) => erledigt(p.status)).length, von: zaehlbar.length };
}

/** Uhrzeit als „09:41z". */
export function zulu(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}z`;
}

// ---------------------------------------------------------------------------
// Routine (Variante C) — rein lokal, streng privat.
// ---------------------------------------------------------------------------

export type Zelle = "ok" | "atc" | "ohne" | "nm" | "leer";

/** Wie lief ein Schalter in einem Flug? Mehrere Punkte je Schalter (z. B.
 *  Rolltempo ab/an) fassen sich zusammen: ein „diesmal ohne" färbt. */
export function zelle(e: Eintrag, s: Schalter): Zelle {
  const ps = e.punkte.filter(
    (p) => p.schalter === s && p.art === "pflicht" && p.status !== "nicht_anwendbar" && p.status !== "offen",
  );
  if (ps.length === 0) return "leer";
  if (ps.some((p) => p.status === "diesmal_ohne")) return "ohne";
  if (ps.every((p) => p.status === "nicht_messbar")) return "nm";
  if (ps.some((p) => p.status === "nach_atc")) return "atc";
  return "ok";
}

export interface RoutineZeile {
  schalter: Schalter;
  zellen: Zelle[]; // ältester zuerst
  serie: number; // aktuelle Serie (vom neuesten Flug rückwärts)
  laengste: number;
  letzteFuenf: number; // erledigt in den letzten fünf gewerteten Flügen
}

/** Serie: zählt erledigte Flüge rückwärts; „nicht messbar" und „leer"
 *  unterbrechen nicht, nur „diesmal ohne". */
export function routine(eintraege: Eintrag[], eingeschaltet: Schalter[], anzahl = 12): RoutineZeile[] {
  const gewertet = eintraege
    .filter((e) => !e.aus_grund)
    .sort((a, b) => a.erstellt_at.localeCompare(b.erstellt_at));
  const fenster = gewertet.slice(-anzahl);
  return SCHALTER.filter((s) => eingeschaltet.includes(s))
    .map((s) => {
      const alle = gewertet.map((e) => zelle(e, s));
      const zellen = fenster.map((e) => zelle(e, s));
      let serie = 0;
      for (let i = alle.length - 1; i >= 0; i--) {
        const z = alle[i];
        if (z === "ok" || z === "atc") serie++;
        else if (z === "ohne") break;
      }
      let laengste = 0;
      let lauf = 0;
      for (const z of alle) {
        if (z === "ok" || z === "atc") laengste = Math.max(laengste, ++lauf);
        else if (z === "ohne") lauf = 0;
      }
      const gewerteteZellen = alle.filter((z) => z === "ok" || z === "atc" || z === "ohne").slice(-5);
      const letzteFuenf = gewerteteZellen.filter((z) => z !== "ohne").length;
      return { schalter: s, zellen, serie, laengste, letzteFuenf };
    })
    .filter((z) => z.zellen.some((c) => c !== "leer"));
}

export type Meilenstein =
  | { art: "rekord"; schalter: Schalter; serie: number }
  | { art: "zieht_an"; schalter: Schalter; von5: number };

/** Serienlängen, die einen Meilenstein wert sind — sonst käme „Neu: …"
 *  nach jedem sauberen Flug wieder. */
export function istMeilenstein(n: number): boolean {
  return [3, 5, 10, 15, 20, 25].includes(n) || (n > 25 && n % 25 === 0);
}

/** Kleine Meilensteine — Fortschritt, nie Tadel. Rekord nur, wenn die
 *  aktuelle Serie gerade eine runde Marke erreicht UND die längste ist. */
export function meilensteine(zeilen: RoutineZeile[]): Meilenstein[] {
  const m: Meilenstein[] = [];
  for (const z of zeilen) {
    if (istMeilenstein(z.serie) && z.serie === z.laengste) {
      m.push({ art: "rekord", schalter: z.schalter, serie: z.serie });
    } else if (z.letzteFuenf >= 4 && z.serie < 4) {
      m.push({ art: "zieht_an", schalter: z.schalter, von5: z.letzteFuenf });
    }
  }
  return m.slice(0, 3);
}
