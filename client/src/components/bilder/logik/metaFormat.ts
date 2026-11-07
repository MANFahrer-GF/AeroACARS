// Anzeige der Metadaten nach der Liste aus `tabellen/bilder.json` (Reihenfolge und Art
// der Zeilen stehen im Paket, nicht im Code).
import type { MetaZeile } from "./grenzen";
import { bytesText } from "./gruppen";
import type { Eintrag } from "./typen";

export type WertTeil =
  | { art: "text"; text: string; zusatz?: "msl" | "ueber_grund" }
  | { art: "flug"; text: string }
  | { art: "quelle"; wert: string }
  | { art: "leer" };

const gradText = (v: number, pos: string, neg: string) => `${Math.abs(v).toFixed(4)} ${v >= 0 ? pos : neg}`;

export function zeitText(iso: string, sprache: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const tag = d.toLocaleDateString(sprache, { timeZone: "UTC", day: "2-digit", month: "2-digit", year: "numeric" });
  const uhr = d.toLocaleTimeString(sprache, { timeZone: "UTC", hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  return `${tag} ${uhr} UTC`;
}

/** „13:11Z“ fuer die Kachel. */
export function kurzZeit(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : `${d.toISOString().slice(11, 16)}Z`;
}

/** „14:32:08Z“ fuer die Detailspalte. */
export function langZeit(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : `${d.toISOString().slice(11, 19)}Z`;
}

export function simulatorName(roh: string): string {
  if (/x-?plane/i.test(roh)) return /12/.test(roh) ? "X-Plane 12" : /11/.test(roh) ? "X-Plane 11" : "X-Plane";
  if (/msfs.*2024|2024/i.test(roh)) return "MSFS 2024";
  if (/msfs.*2020|2020/i.test(roh)) return "MSFS 2020";
  if (/msfs/i.test(roh)) return "MSFS";
  return roh;
}

export const dateiName = (e: Eintrag): string => (e.pfad ?? "").split("/").pop() ?? "";

export function streckeText(m: { abflug: string | null; ziel: string | null }): string {
  return [m.abflug, m.ziel].filter(Boolean).join("–");
}

export function wertFormatieren(zeile: MetaZeile, e: Eintrag, sprache: string): WertTeil {
  const m = e.sidecar?.meta;
  if (!m) return { art: "leer" };
  const roh = (k: string): unknown => (m as unknown as Record<string, unknown>)[k];
  const zahl = (k: string): number | null => (typeof roh(k) === "number" ? (roh(k) as number) : null);
  switch (zeile.format) {
    case "position": {
      const b = zahl("breite"), l = zahl("laenge");
      return b !== null && l !== null ? { art: "text", text: `${gradText(b, "N", "S")} · ${gradText(l, "E", "W")}` } : { art: "leer" };
    }
    case "hoehe": { const v = zahl(zeile.feld); return v === null ? { art: "leer" } : { art: "text", text: `${Math.round(v).toLocaleString(sprache)} ft`, zusatz: "msl" }; }
    case "gs": { const v = zahl(zeile.feld); return v === null ? { art: "leer" } : { art: "text", text: `${Math.round(v)} kt`, zusatz: "ueber_grund" }; }
    case "kurs": { const v = zahl(zeile.feld); return v === null ? { art: "leer" } : { art: "text", text: `${String(Math.round(v) % 360).padStart(3, "0")}°` }; }
    case "flugzeug": {
      const t = m.flugzeug_titel, i = m.flugzeug_icao;
      return t && i ? { art: "text", text: `${t} (${i})` } : t || i ? { art: "text", text: (t ?? i)! } : { art: "leer" };
    }
    case "flug": {
      const t = [m.rufzeichen, streckeText(m)].filter(Boolean).join(" ");
      return t ? { art: "flug", text: t } : { art: "leer" };
    }
    case "simulator": return m.simulator ? { art: "text", text: simulatorName(m.simulator) } : { art: "leer" };
    case "quelle": return { art: "quelle", wert: m.quelle };
    case "datei": return e.pfad ? { art: "text", text: `${dateiName(e)} · ${bytesText(e.groesse, sprache)}` } : { art: "leer" };
    default: {
      const v = roh(zeile.feld);
      return typeof v === "string" && v !== "" ? { art: "text", text: v } : { art: "leer" };
    }
  }
}
