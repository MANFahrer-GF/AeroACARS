/**
 * Anzeige von Telemetriewerten: Stellen, Einheit, Schalter, Aufzaehlungen.
 */

import type { TFunction } from "i18next";
import type { Kanal } from "./typen";

/** Kanaele, deren Zahl ein Aufzaehlungswert ist (Text statt Zahl). */
const AUFZAEHLUNG: Record<string, string> = {
  untergrund: "telemetrie.untergrund.",
  // Wahlschalter 0=OFF 1=AUTO 2=ON — als Schalter las sich AUTO wie ON.
  anschnallzeichen: "telemetrie.anschnallzeichen.",
};

/**
 * Kanaele, die das erkannte Muster nicht verlaesslich liefert: der Kanal
 * `nicht_verlaesslich` traegt ihre IDs, leerzeichengetrennt.
 */
export function nichtVerlaesslich(liste: string | null | undefined): Set<string> {
  return new Set((liste ?? "").split(/\s+/).filter(Boolean));
}

export function zahl(v: number, stellen: number, sprache: string): string {
  return new Intl.NumberFormat(sprache, {
    minimumFractionDigits: stellen,
    maximumFractionDigits: stellen,
  }).format(v);
}

/** Wert ohne Einheit, wie er in Tabellen steht. */
export function wertText(
  k: Kanal,
  v: number | string | null | undefined,
  t: TFunction,
  sprache: string,
): string {
  if (v === null || v === undefined || v === "") return "–";
  if (typeof v === "string") return v;
  if (k.art === "schalter") return v >= 0.5 ? t("telemetrie.an") : t("telemetrie.aus");
  const auf = AUFZAEHLUNG[k.id];
  if (auf) return t(`${auf}${Math.round(v)}`, { defaultValue: String(Math.round(v)) });
  if (k.id === "squawk") return String(Math.round(v)).padStart(4, "0");
  if (k.einheit === "°" && /^(kurs|wind_richtung|soll_kurs)/.test(k.id)) {
    const w = ((Math.round(v) % 360) + 360) % 360;
    return String(w === 0 ? 360 : w).padStart(3, "0");
  }
  return zahl(v, k.stellen, sprache);
}

/** Wert mit Einheit. */
export function wertMitEinheit(
  k: Kanal,
  v: number | string | null | undefined,
  t: TFunction,
  sprache: string,
): string {
  const s = wertText(k, v, t, sprache);
  if (s === "–" || k.art !== "zahl" || !k.einheit || AUFZAEHLUNG[k.id] || k.id === "squawk") return s;
  const eng = k.einheit === "°" || k.einheit === "%" || k.einheit === "×";
  return eng ? `${s}${k.einheit}` : `${s} ${k.einheit}`;
}

/** Vorzeichen fuer Abweichungen (V/S, Ablagen). */
export function mitVorzeichen(v: number, stellen: number, sprache: string): string {
  const s = zahl(Math.abs(v), stellen, sprache);
  if (Math.abs(v) < 0.5 * 10 ** -stellen) return s;
  return (v > 0 ? "+" : "−") + s;
}
