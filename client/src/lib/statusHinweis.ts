// v3 (P-9.12): Wartung und Störungen aus der Statusseite von live.
//
// Der Client liest `/status.json` beim Start und alle 10 Minuten. Die Datei
// schreibt der Status-Dienst auf live (aeroacars-live: recorder/src/
// statusseite.ts `baueStatusJson`), Caddy liefert sie statisch mit
// `Access-Control-Allow-Origin: *` aus — auch wenn der Recorder steht.
//
// Bewusst ein Abruf aus der Oberfläche und kein neuer Tauri-Befehl: Ein neuer
// Befehl ändert den Vertrag Kern ↔ Paket (`ui-befehle-2`, P-2.15). Der Host
// steht fest in der CSP (`connect-src https://live.kant.ovh`); die Adresse ist
// darum hier fest, so wie dort.

export const STATUS_URL = "https://live.kant.ovh/status.json";
/** Abruf beim Start und danach in diesem Takt (P-9.12). */
export const STATUS_TAKT_MS = 10 * 60_000;
/** Ein hängender Abruf zählt nach dieser Zeit als „kein Netz". */
export const STATUS_ZEITGRENZE_MS = 15_000;

export type Text3 = { de?: string; en?: string; it?: string };

export interface Wartung {
  von: string;
  bis: string;
  text: Text3;
  notfall: boolean;
  aktiv: boolean;
}

export interface StatusHinweis {
  /** Laufende Wartung zuerst, sonst die nächste angekündigte. */
  wartung: Wartung | null;
  /** Namen der gestörten Dienste (je Sprache). */
  stoerungen: Text3[];
}

/** Nur Zeichenketten je Sprache — ein Objekt an dieser Stelle braeche die
 *  Darstellung (Codex 30.10.). `null`, wenn keine Sprache Text hat. */
function text3Aus(v: unknown): Text3 | null {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  const t: Text3 = {};
  for (const k of ["de", "en", "it"] as const) if (typeof o[k] === "string") t[k] = o[k] as string;
  return Object.keys(t).length > 0 ? t : null;
}

/** Liest `status.json` (Format `version: 1`). Alles Unbekannte oder Kaputte
 *  ergibt `null` — dann zeigt der Client nichts. */
export function statusLesen(roh: unknown, jetzt: number): StatusHinweis | null {
  if (typeof roh !== "object" || roh === null) return null;
  const s = roh as Record<string, unknown>;
  if (s.version !== 1) return null;
  const wartungen: Wartung[] = (Array.isArray(s.wartung) ? s.wartung : [])
    .filter((w): w is Record<string, unknown> => typeof w === "object" && w !== null)
    .map((w) => ({
      von: String(w.von ?? ""),
      bis: String(w.bis ?? ""),
      text: text3Aus(w.text) ?? {},
      notfall: w.notfall === true,
      aktiv: false,
    }))
    // Laufend oder angekündigt entscheidet die eigene Uhr: status.json ist
    // bis zu einer Minute alt, der Client fragt nur alle 10 Minuten.
    .filter((w) => Number.isFinite(Date.parse(w.von)) && Date.parse(w.bis) > jetzt)
    .map((w) => ({ ...w, aktiv: Date.parse(w.von) <= jetzt }))
    .sort((a, b) => Number(b.aktiv) - Number(a.aktiv) || Date.parse(a.von) - Date.parse(b.von));
  const stoerungen = (Array.isArray(s.dienste) ? s.dienste : [])
    .filter((d): d is Record<string, unknown> => typeof d === "object" && d !== null)
    .filter((d) => d.status === "stoerung")
    .map((d) => text3Aus(d.name))
    .filter((n): n is Text3 => n !== null);
  return { wartung: wartungen[0] ?? null, stoerungen };
}

/** Text in der Sprache der Oberfläche, sonst Englisch, sonst Deutsch. */
export function text3(t: Text3, sprache: string): string {
  const k = sprache.slice(0, 2) as keyof Text3;
  return t[k] || t.en || t.de || "";
}

/** Holt und liest `status.json`; ohne Netz, bei Fehler oder Zeitüberschreitung `null`. */
export async function statusHolen(jetzt: () => number = Date.now): Promise<StatusHinweis | null> {
  const abbruch = new AbortController();
  const uhr = setTimeout(() => abbruch.abort(), STATUS_ZEITGRENZE_MS);
  try {
    const r = await fetch(STATUS_URL, { cache: "no-store", signal: abbruch.signal });
    if (!r.ok) return null;
    return statusLesen(await r.json(), jetzt());
  } catch {
    return null;
  } finally {
    clearTimeout(uhr);
  }
}
