// Sendezustand je Kachel (P-11.34): sendet, angenommen, gesendet, fehlgeschlagen mit
// Grund. Reine Funktionen, kein React. Fehlgeschlagene bleiben ausgewaehlt; ein
// Wiederholen mit denselben Bildern behaelt die `sendung_id` (der Server sieht
// gleiche Schluessel und gleiche Bytes, also kein `konflikt`).
import { retrySekunden } from "./kontingent";
import type { BildErgebnis, SendeAntwort } from "./typen";
import type { Auswahl } from "./auswahl";

export type Phase = "sendet" | "angenommen" | "gesendet" | "fehlgeschlagen";

export interface KachelStand {
  phase: Phase;
  sendungId: string;
  code?: string;
  retryAfter?: number;
}

export type Sendestand = Readonly<Record<string, KachelStand>>;

/** Codes des Servers (P-7.25) und der Huelle, fuer die der Tab einen Text hat. */
export const FEHLER_CODES = [
  "zu_gross",
  "kontingent",
  "discord_fehler",
  "discord_unklar",
  "ungueltig",
  "konflikt",
  "nicht_eingerichtet",
  "server_nicht_erreichbar",
  "nicht_angemeldet",
  "server_fehler",
  "datei_fehlt",
  "unbekannt",
] as const;
export type FehlerCode = (typeof FEHLER_CODES)[number];

export function fehlerCodeNormal(code: string | undefined): FehlerCode {
  return (FEHLER_CODES as readonly string[]).includes(code ?? "") ? (code as FehlerCode) : "unbekannt";
}

export function sendeStart(stand: Sendestand, ids: string[], sendungId: string): Sendestand {
  const n = { ...stand };
  for (const id of ids) n[id] = { phase: "sendet", sendungId };
  return n;
}

function ausErgebnis(e: BildErgebnis, sendungId: string): KachelStand {
  if (e.status === "gesendet") return { phase: "gesendet", sendungId };
  if (e.status === "angenommen") return { phase: "angenommen", sendungId };
  return { phase: "fehlgeschlagen", sendungId, code: fehlerCodeNormal(e.code), retryAfter: retrySekunden(e.retry_after) };
}

/** Antwort des Servers (Senden oder Status) einarbeiten. Ein Bild ohne Eintrag gilt als unklar. */
export function antwortEinarbeiten(stand: Sendestand, ids: string[], sendungId: string, a: SendeAntwort): Sendestand {
  const n = { ...stand };
  for (const id of ids) {
    const e = a.bilder.find((b) => b.bild_id === id);
    n[id] = e ? ausErgebnis(e, sendungId) : { phase: "fehlgeschlagen", sendungId, code: "discord_unklar" };
  }
  return n;
}

/** Der ganze Aufruf scheiterte (Fehlercode des Programms, `bild_senden`). */
export function aufrufFehler(stand: Sendestand, ids: string[], sendungId: string, code: string, retryAfter?: unknown): Sendestand {
  const abbildung: Record<string, FehlerCode> = {
    live_nicht_erreichbar: "server_nicht_erreichbar",
    live_nicht_angemeldet: "nicht_angemeldet",
    live_antwort_fehlt: "discord_unklar",
    live_kontingent: "kontingent",
    live_fehler: "server_fehler",
    bild_datei_fehlt: "datei_fehlt",
  };
  const c = abbildung[code] ?? "unbekannt";
  const n = { ...stand };
  for (const id of ids) n[id] = { phase: "fehlgeschlagen", sendungId, code: c, retryAfter: c === "kontingent" ? retrySekunden(retryAfter) : undefined };
  return n;
}

/** Auswahl nach dem Senden: Gesendete und Angenommene verlassen sie, Fehlgeschlagene bleiben. */
export function auswahlNachSenden(auswahl: Auswahl, stand: Sendestand): Set<string> {
  return new Set([...auswahl].filter((id) => {
    const p = stand[id]?.phase;
    return p !== "gesendet" && p !== "angenommen";
  }));
}

/** Bilder, deren Zustellung der Server noch nicht bestaetigt hat. */
export function offeneIds(stand: Sendestand): Array<{ id: string; sendungId: string }> {
  return Object.entries(stand)
    .filter(([, s]) => s.phase === "angenommen")
    .map(([id, s]) => ({ id, sendungId: s.sendungId }));
}

/**
 * `sendung_id` fuer einen Sendevorgang: Haben alle gewaehlten Bilder eine gemeinsame
 * fehlgeschlagene Sendung, bleibt sie (Wiederholung, gleiche Bytes); sonst eine neue.
 * Eine bewusste Wiederholung eines gesendeten Bildes ist immer eine neue Nachricht.
 */
export function sendungIdWaehlen(stand: Sendestand, ids: string[], neue: () => string): string {
  const alt = ids.map((id) => stand[id]);
  if (alt.length > 0 && alt.every((s) => s?.phase === "fehlgeschlagen" && s.sendungId === alt[0].sendungId)) return alt[0].sendungId;
  return neue();
}

/** Zusammenfassung fuer die Auswahlleiste. */
export function zusammenfassung(stand: Sendestand, ids: Iterable<string>) {
  let sendet = 0, fehlgeschlagen = 0, gesendet = 0;
  let unklar = false, nichtErreichbar = false;
  for (const id of ids) {
    const s = stand[id];
    if (!s) continue;
    if (s.phase === "sendet" || s.phase === "angenommen") sendet++;
    else if (s.phase === "gesendet") gesendet++;
    else {
      fehlgeschlagen++;
      if (s.code === "discord_unklar") unklar = true;
      if (s.code === "server_nicht_erreichbar") nichtErreichbar = true;
    }
  }
  return { sendet, fehlgeschlagen, gesendet, unklar, nichtErreichbar };
}

/** Stand einer Kachel, wenn es in dieser Sitzung keinen gibt: aus dem letzten Vermerk im Sidecar. */
export function standAusVermerk(v: { sendung_id: string; status: "angenommen" | "gesendet" | "abgelehnt"; code?: string } | undefined): KachelStand | undefined {
  if (!v) return undefined;
  if (v.status === "gesendet") return { phase: "gesendet", sendungId: v.sendung_id };
  if (v.status === "angenommen") return { phase: "angenommen", sendungId: v.sendung_id };
  return { phase: "fehlgeschlagen", sendungId: v.sendung_id, code: fehlerCodeNormal(v.code) };
}

/**
 * Nach dem Laden: Bilder ohne Stand dieser Sitzung, deren letzter Vermerk fehlgeschlagen oder unklar ist,
 * bekommen ihn zurueck. So nutzt „Erneut senden“ nach einem Neustart dieselbe `sendung_id` (keine zweite
 * Nachricht bei unklarer Zustellung). Gesendete und angenommene Vermerke bleiben reine Anzeige.
 */
export function standNachLaden(stand: Sendestand, eintraege: ReadonlyArray<{ id: string; sidecar: { sendungen?: Array<{ sendung_id: string; status: "angenommen" | "gesendet" | "abgelehnt"; code?: string }> } | null }>): Sendestand {
  let n: Record<string, KachelStand> | null = null;
  for (const e of eintraege) {
    if (stand[e.id]) continue;
    const v = [...(e.sidecar?.sendungen ?? [])].reverse()[0];
    if (v?.status !== "abgelehnt") continue;
    n ??= { ...stand };
    n[e.id] = standAusVermerk(v)!;
  }
  return n ?? stand;
}
