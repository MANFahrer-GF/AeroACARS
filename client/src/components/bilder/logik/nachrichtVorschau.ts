// „So erscheint die Nachricht“: dieselbe Rechnung wie der Server
// (`recorder/src/bilder.ts`: kopfZeile, geoZeile, baueNachricht). Die Vorschau
// zeigt den Aufbau, Discord zeigt ihn am Ende selbst; die Teile sind getrennt,
// damit eine verdeckte Zeile als grauer Balken dargestellt werden kann.
import type { BildMeta, GeoStufe } from "./typen";

/** Feste Felder duerfen keine Discord-Formatierung tragen (wie der Server). */
export function sauber(s: string | null | undefined, max: number): string {
  if (!s) return "";
  return s.normalize("NFKC").replace(/[^\p{L}\p{N} ./+-]/gu, " ").replace(/\s+/g, " ").trim().slice(0, max);
}

export function kopfZeile(pilotName: string | null | undefined, m: Pick<BildMeta, "abflug" | "ziel" | "flugzeug_titel" | "flugzeug_icao" | "rufzeichen">): string {
  const name = sauber(pilotName, 80) || "Pilot";
  const strecke = [sauber(m.abflug, 8), sauber(m.ziel, 8)].filter(Boolean).join(" - ");
  const titel = sauber(m.flugzeug_titel, 80);
  const icao = sauber(m.flugzeug_icao, 8);
  const flugzeug = titel && icao ? `${titel} (${icao})` : titel || icao;
  return [name, sauber(m.rufzeichen, 12), strecke, flugzeug].filter(Boolean).join(" · ");
}

export interface GeoTeil {
  text: string;
  /** Wird in Discord als grauer Balken gezeigt (Spoiler). */
  verdeckt: boolean;
}

export function geoZeile(m: BildMeta, stufe: GeoStufe): GeoTeil | null {
  if (stufe === "aus" || m.breite === null || m.laenge === null) return null;
  const teile: string[] = [];
  if (m.zeit_utc) teile.push(`${new Date(m.zeit_utc).toISOString().slice(11, 16)} UTC`);
  teile.push(
    `${Math.abs(m.breite).toFixed(3)}° ${m.breite >= 0 ? "N" : "S"} ${Math.abs(m.laenge).toFixed(3)}° ${m.laenge >= 0 ? "E" : "W"}`,
  );
  if (m.hoehe_ft !== null) teile.push(`${Math.round(m.hoehe_ft)} ft`);
  if (m.kurs !== null) teile.push(`${String(Math.round(m.kurs) % 360).padStart(3, "0")}°`);
  const platz = sauber(m.naechster_flugplatz, 8);
  if (platz) teile.push(platz);
  return { text: teile.join(" · "), verdeckt: stufe === "verdeckt" };
}

export interface Vorschau {
  kopf: string;
  geo: Array<GeoTeil & { nummer: number | null }>;
  text: string;
  /** Der Text wurde fuer Discord gekuerzt. */
  gekuerzt: boolean;
}

/** Wie `baueNachricht`: Kopf, Ortszeilen, Pilotentext zuletzt; gekuerzt auf die Discord-Grenze. */
export function vorschauBauen(pilotName: string | null, metas: BildMeta[], stufe: GeoStufe, text: string, maxZeichen: number): Vorschau {
  const kopf = kopfZeile(pilotName, metas[0] ?? { abflug: null, ziel: null, flugzeug_titel: null, flugzeug_icao: null, rufzeichen: null });
  const zeilen = metas.map((m) => geoZeile(m, stufe));
  const geo = zeilen
    .map((z, i) => (z ? { ...z, nummer: zeilen.length > 1 ? i + 1 : null } : null))
    .filter((z): z is GeoTeil & { nummer: number | null } => z !== null);
  const roh = [kopf, ...geo.map((z) => (z.nummer ? `${z.nummer}: ` : "") + (z.verdeckt ? `||${z.text}||` : z.text)), text.trim()].filter(Boolean).join("\n");
  return { kopf, geo, text: text.trim(), gekuerzt: roh.length > maxZeichen };
}
