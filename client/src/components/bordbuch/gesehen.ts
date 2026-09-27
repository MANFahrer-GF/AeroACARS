// Was AeroACARS bei einem „nicht gesehen"-Punkt stattdessen gemessen hat
// (27.09.2026, Thomas: „diesmal ohne — damit kann der Pilot nix anfangen").
// Eine Zeile unter dem Punkt, damit der Pilot sieht, ob er den Schalter
// vergessen hat oder die Messung danebenlag.

import type { Punkt } from "../../lib/bordbuch";

type T = (k: string, o?: Record<string, unknown>) => string;

const ROLLEN = new Set(["rolltempo_abflug", "rolltempo_ankunft"]);

/** Gefahrene Spitze beim Rollen (aus dem Beleg), sonst null. */
export function rollenMax(p: Punkt): number | null {
  if (!ROLLEN.has(p.regel)) return null;
  const kt = p.beleg?.max_kt;
  return typeof kt === "number" && Number.isFinite(kt) ? kt : null;
}

/** „schnellstes Rollen 22 kt · Grenze 30 kt" — Spitze und Grenze
 *  nebeneinander, damit die Zahl nicht wie das erlaubte Tempo aussieht. */
export function rollenText(p: Punkt, grenze: number | undefined, t: T): string | null {
  const kt = rollenMax(p);
  if (kt === null || grenze === undefined) return null;
  const s = p.beleg?.laengste_ueber_grenze_s;
  if ((p.status === "diesmal_ohne" || p.status === "nach_atc") && typeof s === "number" && s > 0) {
    return t("bordbuch.rollen_ueber", { kt: Math.round(kt), s: Math.round(s), grenze: Math.round(grenze) });
  }
  return t("bordbuch.rollen_gefahren", { kt: Math.round(kt), grenze: Math.round(grenze) });
}

/** Die Zeile unter einem „nicht gesehen"-Punkt. */
export function gesehenText(p: Punkt, grenze: number | undefined, t: T): string | null {
  if (p.status !== "diesmal_ohne") return null;
  if (ROLLEN.has(p.regel)) return rollenText(p, grenze, t);
  const stellung = p.stellung?.trim();
  if (stellung) {
    const mit = t(`bordbuch.gesehen_mit.${p.regel}`, { stellung, defaultValue: "" });
    if (mit) return mit;
  }
  const ohne = t(`bordbuch.gesehen.${p.regel}`, { defaultValue: "" });
  return ohne || null;
}
