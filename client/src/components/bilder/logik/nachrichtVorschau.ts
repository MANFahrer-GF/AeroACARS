// „So erscheint die Nachricht“: dieselbe Rechnung wie der Server
// (`recorder/src/bilder.ts`: kopfZeile, infoZeile, geoZeile, baueNachricht). Die
// Vorschau zeigt den Aufbau, Discord zeigt ihn am Ende selbst; die Teile sind
// getrennt, damit eine verdeckte Position als grauer Balken dargestellt werden kann.
import type { BildMeta, GeoStufe } from "./typen";

/** Feste Felder duerfen keine Discord-Formatierung tragen (wie der Server). */
export function sauber(s: string | null | undefined, max: number): string {
  if (!s) return "";
  return s.normalize("NFKC").replace(/[^\p{L}\p{N} ./+-]/gu, " ").replace(/\s+/g, " ").trim().slice(0, max);
}

export function kopfZeile(pilotName: string | null | undefined, m: Pick<BildMeta, "abflug" | "ziel" | "flugzeug_titel" | "flugzeug_icao" | "rufzeichen">): string {
  const name = `📷 **${sauber(pilotName, 80) || "Pilot"}**`;
  const strecke = [sauber(m.abflug, 8), sauber(m.ziel, 8)].filter(Boolean).join("\u2013");
  const titel = sauber(m.flugzeug_titel, 80);
  const icao = sauber(m.flugzeug_icao, 8);
  const flugzeug = titel && icao ? `${titel} (${icao})` : titel || icao;
  return [name, sauber(m.rufzeichen, 12), strecke, flugzeug].filter(Boolean).join(" · ");
}

/** Das Feld, das nur der Client kennt: die Phase in der Sprache des Piloten. */
export type SendeMeta = BildMeta & { phase_text?: string | null };

/**
 * Was je Bild zum Server geht. Bei „aus“ bleibt die Position im Programm (so steht es
 * im Dialog), Zeit, Hoehe und Phase gehen mit.
 */
export function sendeMeta(m: BildMeta, stufe: GeoStufe, phaseText: string): SendeMeta {
  const ohneOrt = stufe === "aus" ? { breite: null, laenge: null, kurs: null, naechster_flugplatz: null } : {};
  return { ...m, ...ohneOrt, phase_text: phaseText || null };
}

/** Immer sichtbar, auch bei „verdeckt“ und „aus“: „17:26Z · 92 ft · Boarding“. */
export function infoZeile(m: SendeMeta): string {
  const teile: string[] = [];
  if (m.zeit_utc) teile.push(`${new Date(m.zeit_utc).toISOString().slice(11, 16)}Z`);
  if (m.hoehe_ft !== null) teile.push(`${Math.round(m.hoehe_ft)} ft`);
  const phase = sauber(m.phase_text, 30);
  if (phase) teile.push(phase);
  return teile.join(" · ");
}

export interface GeoTeil {
  /** So steht es in der Nachricht, mit den Links. */
  text: string;
  /** So liest es der Pilot: Discord zeigt nur die Namen der Links. */
  anzeige: string;
  /** Wird in Discord als grauer Balken gezeigt (Spoiler). */
  verdeckt: boolean;
}

export function geoZeile(m: BildMeta, stufe: GeoStufe): GeoTeil | null {
  if (stufe === "aus" || m.breite === null || m.laenge === null) return null;
  const teile: string[] = [];
  const platz = sauber(m.naechster_flugplatz, 8);
  if (platz) teile.push(platz);
  if (m.kurs !== null) teile.push(`${String(Math.round(m.kurs) % 360).padStart(3, "0")}°`);
  const breite = m.breite.toFixed(4);
  const laenge = m.laenge.toFixed(4);
  return {
    text: [...teile, `[Google Maps](<https://www.google.com/maps?q=${breite},${laenge}>)`, `[VATSIM Radar](<https://vatsim-radar.com/?center=${laenge},${breite}&zoom=10>)`].join(" · "),
    anzeige: [...teile, "Google Maps", "VATSIM Radar"].join(" · "),
    verdeckt: stufe === "verdeckt",
  };
}

export interface BildZeile {
  nummer: number | null;
  info: string;
  /** Flugplatz, Kurs, Kartenlinks; `null` bei „aus“ oder ohne Position. */
  ort: string | null;
  /** Dasselbe, wie es die Nachricht traegt. */
  ortRoh: string | null;
  /** Der Ort wird in Discord als grauer Balken gezeigt (Spoiler). */
  verdeckt: boolean;
}

export interface Vorschau {
  kopf: string;
  zeilen: BildZeile[];
  text: string;
  /** Der Text wurde fuer Discord gekuerzt. */
  gekuerzt: boolean;
}

/** Wie `baueNachricht`: Kopf, eine Zeile je Bild, Pilotentext zuletzt; gekuerzt auf die Discord-Grenze. */
export function vorschauBauen(pilotName: string | null, metas: SendeMeta[], stufe: GeoStufe, text: string, maxZeichen: number): Vorschau {
  const kopf = kopfZeile(pilotName, metas[0] ?? { abflug: null, ziel: null, flugzeug_titel: null, flugzeug_icao: null, rufzeichen: null });
  const zeilen = metas
    .map((m, i): BildZeile => {
      const g = geoZeile(m, stufe);
      return { nummer: metas.length > 1 ? i + 1 : null, info: infoZeile(m), ort: g?.anzeige ?? null, ortRoh: g?.text ?? null, verdeckt: g?.verdeckt ?? false };
    })
    .filter((z) => z.info || z.ort);
  const zeileRoh = (z: BildZeile) =>
    (z.nummer ? `${z.nummer}: ` : "") + [z.info, z.ortRoh && (z.verdeckt ? `||${z.ortRoh}||` : z.ortRoh)].filter(Boolean).join(" · ");
  const roh = [kopf, ...zeilen.map(zeileRoh), text.trim()].filter(Boolean).join("\n");
  return { kopf, zeilen, text: text.trim(), gekuerzt: roh.length > maxZeichen };
}
