// Beispiel-Bordbuch für Vorschau und PDF-Bericht (nicht ausgeliefert).
// Werte angelehnt an DLH 255 EPGD–EDDG (26.09.2026), Fenix A320.
import type { Eintrag, Punkt, Regel, Schalter } from "../lib/bordbuch";

const T0 = Date.parse("2026-09-26T18:20:00Z");
const um = (min: number) => new Date(T0 + min * 60_000).toISOString();
const pk = (
  regel: Regel, schalter: Schalter, abschnitt: Punkt["abschnitt"], status: Punkt["status"],
  min: number, hoehe: number, extra: Partial<Punkt> = {},
): Punkt => ({
  regel, schalter, abschnitt, art: "pflicht", status, auto_status: status, zeit: um(min),
  hoehe_ft: hoehe, stellung: null, grund: null, markiert_at: null, ...extra,
});

const profil: Array<[number, number]> = [];
for (let m = 0; m <= 90; m += 2) {
  const h = m < 14 ? 16 : m < 32 ? 16 + (m - 14) * 1550 : m < 62 ? 28000 : m < 84 ? 28000 - (m - 62) * 1270 : 30;
  profil.push([Math.round((T0 + m * 60_000) / 1000), Math.round(h)]);
}

export const BEISPIEL_BORDBUCH: Eintrag = {
  schema: 1, pirep_id: "BEISPIEL", erstellt_at: um(0), updated_at: um(95), client_version: "1.9.2",
  flug: { callsign: "DLH 255", dep: "EPGD", arr: "EDDG", muster: "A320", titel: "FenixA320 CFM SL", profil: "FenixA320", sim: "Msfs2024" },
  klasse: "airliner", klasse_quelle: "profil", regelwerk: "ifr", nacht_start: true, nacht_landung: true, zeitquelle: "sim",
  eingeschaltet: ["beacon", "strobes", "nav", "transponder", "rolltempo", "apu"], aus_grund: null,
  punkte: [
    pk("beacon_anlassen", "beacon", "vor_dem_rollen", "erledigt", 9, 16),
    pk("nav_lichter", "nav", "vor_dem_rollen", "erledigt", 11, 16),
    pk("parkbremse_geloest", "parkbremse", "vor_dem_rollen", "erledigt", 11, 16),
    pk("rolltempo_abflug", "rolltempo", "rollen", "diesmal_ohne", 14, 16, { stellung: "max 33 kt", beleg: { max_kt: 33.4, laengste_ueber_grenze_s: 12.1 } }),
    pk("strobes_start", "strobes", "start", "erledigt", 14, 16),
    pk("landelicht_start", "landelicht", "start", "erledigt", 14, 16),
    pk("transponder_start", "transponder", "start", "erledigt", 14, 16, { stellung: "TA-RA" }),
    pk("tcas_start", "transponder", "start", "erledigt", 14, 16, { stellung: "TA-RA" }),
    pk("klappen_start", "klappen", "start", "erledigt", 14, 16, { stellung: "2" }),
    pk("anschnall_start", "anschnallzeichen", "start", "erledigt", 14, 16, { stellung: "ON" }),
    pk("apu_reiseflug", "apu", "reiseflug", "erledigt", 34, 28000, { art: "bestaetigung" }),
    pk("landelicht_anflug", "landelicht", "anflug", "diesmal_ohne", 76, 9800),
    pk("autobrake_landung", "autobrake", "anflug", "erledigt", 84, 900, { art: "bestaetigung", stellung: "MED" }),
    pk("spoiler_landung", "spoiler", "anflug", "erledigt", 84, 900, { stellung: "ARMED" }),
    pk("anschnall_landung", "anschnallzeichen", "anflug", "erledigt", 84, 900, { stellung: "ON" }),
    pk("rolltempo_ankunft", "rolltempo", "nach_der_landung", "erledigt", 92, 16, { stellung: "max 27 kt", beleg: { max_kt: 26.8, laengste_ueber_grenze_s: 0 } }),
  ],
  rollen_max_abflug_kt: 33, rollen_max_ankunft_kt: 27, rolltempo_grenze_kt: 30, profil,
};
