/**
 * v1.7.35 — Sprit-Auswertung ohne Note.
 *
 * Wire-Format von `landing_scoring::sprit::SpritAuswertung` (Rust). Der
 * Client rechnet einmal beim Aufsetzen; hier stehen nur der Typ und
 * Formatierer. **Nichts wird hier nachgerechnet** — fehlt ein Feld, zeigt
 * die Oberfläche „—". Die Webapp trägt denselben Typ in
 * `aeroacars-live/webapp/src/components/sprit.ts`.
 */

import i18n from "i18next";

/**
 * Das Dezimalzeichen der eingestellten Sprache — Komma für Deutsch und
 * Italienisch, Punkt für Englisch.
 *
 * Bis v1.7.36 stand überall ein Komma, auch in der englischen Ansicht
 * („+3,4 %" über „Fuel"). Die Tausendertrennung bleibt in allen Sprachen
 * das schmale Leerzeichen (U+202F): Das ist die international eindeutige
 * Schreibweise, und sie verwechselt niemand mit einem Dezimalzeichen.
 */
export function dezimal(v: number, stellen: number): string {
  const s = Math.abs(v).toFixed(stellen);
  const sprache = (i18n.language || "de").slice(0, 2);
  const zahl = sprache === "en" ? s : s.replace(".", ",");
  // Das Vorzeichen bleibt erhalten — als echtes Minus (U+2212) wie in
  // `pct`. Bis v1.7.36-Entwurf fiel es hier still weg (QS-Vorschlag V-e):
  // harmlos, solange nur `pct` und Minutenwerte fragten, eine Falle für
  // jeden späteren Aufrufer. Eine auf null gerundete Zahl bekommt keins.
  return v < 0 && Number(s) !== 0 ? "−" + zahl : zahl;
}

/**
 * Abstand zur Final Reserve in kg — positiv darüber, negativ darunter.
 *
 * Kommt aus der Rechnung (`reserve_abstand_kg`, `sprit.rs`). Datensätze vor
 * v1.7.37 tragen das Feld nicht; für sie gilt dieselbe Differenz der beiden
 * gespeicherten, gerundeten Werte — genau so rechnet auch `sprit.rs`. Damit
 * zeigt ein alter Flug denselben Abstand wie ein neuer, und Client und
 * Live-Übersicht können nicht auseinanderlaufen (eine Datei).
 */
export function reserveAbstand(s: SpritAuswertung): number | null {
  if (s.reserve.status === "nicht_pruefbar") return null;
  let roh: number;
  if (s.reserve_abstand_kg != null) roh = s.reserve_abstand_kg;
  else if (s.landing_fuel_kg == null || s.reserve_kg == null) return null;
  else roh = s.landing_fuel_kg - s.reserve_kg;
  // Dieselbe Klemme wie `abstand_nach_status` in sprit.rs: unterschritten
  // heißt mindestens −1 kg, intakt nie negativ — nie „− 0 kg".
  return s.reserve.status === "unterschritten" ? Math.min(roh, -1) : Math.max(roh, 0);
}

export interface SpritPhase {
  ist_kg: number;
  plan_kg: number;
  /** Eine Nachkommastelle, gerundet vom Client. */
  abweichung_pct: number;
  /**
   * Gehört die Hauptzahl in Kilogramm statt in Prozent?
   *
   * Die Rechnung entscheidet das (`sprit.rs`), nicht die Anzeige. Bis
   * v1.7.35 stand dieselbe 60-%-Regel an VIER Stellen — Client, Live-
   * Übersicht, Bericht und die PIREP-Felder für die GSG-Webseite. Wer eine
   * ändert, lässt die anderen lautlos zurück, und der Pilot liest im
   * Client „−12 kg" und auf der Webseite „−8,3 %".
   */
  als_kg: boolean;
}

export type SpritReserve =
  | { status: "intakt"; quote_pct: number }
  | { status: "unterschritten"; quote_pct: number }
  | { status: "nicht_pruefbar"; grund: string };

export interface SpritLeiter {
  taxi_kg: number;
  trip_kg: number;
  contingency_kg: number;
  alternate_kg: number;
  reserve_kg: number;
  extra_kg: number;
  block_kg: number;
  /** v1.7.36: Zusatzsprit im Block, der in keinem der sechs Posten steckt (ETOPS, Minimum). */
  sonstiges_kg?: number;
  /** v1.7.36: mehr getankt als geplant — aus der Rechnung, nicht aus der Anzeige. */
  uebertankung_kg?: number;
  /**
   * v1.7.36: Was vom Extra nie an Bord war (Tank beim Anlassen unter dem
   * Block). Die Grafik zeichnet das Extra um genau diesen Betrag kürzer —
   * gerechnet in `sprit.rs`, hier nur gelesen.
   */
  untertankung_kg?: number;
}

export type SpritBadge = "gruen" | "gelb" | "grau";

export interface SpritAuswertung {
  fassung: number;
  bis_sinkflug: SpritPhase | null;
  anflug: SpritPhase | null;
  zeit_unter_schwelle_min: number | null;
  schwelle_ft: number | null;
  strecke_anflug_nm: number | null;
  plan_strecke_anflug_nm: number | null;
  reserve: SpritReserve;
  reserve_kg: number | null;
  /**
   * v1.7.37: Landesprit minus Final Reserve in kg — positiv darüber,
   * negativ darunter. Gerechnet in `sprit.rs`, hier nur gelesen. Fehlt bei
   * älteren Datensätzen; dann gilt dieselbe Differenz der gespeicherten
   * Werte (`reserveAbstand`).
   */
  reserve_abstand_kg?: number | null;
  /** Tankstand beim Abheben — steht im Ergebnis, weil er sich aus den
   *  übrigen Feldern nicht zurückrechnen lässt. */
  takeoff_fuel_kg: number | null;
  landing_fuel_kg: number | null;
  extra_getankt_kg: number | null;
  extra_genutzt_kg: number | null;
  extra_ungenutzt_kg: number | null;
  contingency_verbraucht: boolean | null;
  alternate_und_reserve_intakt: boolean | null;
  leiter: SpritLeiter | null;
  /** v1.7.36: Rollen vor dem Start, gegen den geplanten Taxi-Anteil. */
  rollen_vor_start: SpritPhase | null;
  /**
   * Rollen nach der Landung — nur die Zahl, kein Plan.
   *
   * SimBrief plant einen einzigen Taxi-Block, und der gilt dem Weg zum
   * Start. Für den Weg zurück zum Stand gibt es nichts zu vergleichen.
   */
  rollen_nach_landung_kg: number | null;
  /**
   * v1.7.36: Die Aufzeichnung begann in der Luft — `takeoff_fuel_kg` ist
   * dann der Tankstand beim Einstieg, nicht beim Abheben. Fehlt bei älteren
   * Datensätzen (dann: abgehoben).
   */
  einstieg_in_der_luft?: boolean;
  badge: SpritBadge;
}

/** Tausendertrennung mit schmalem Leerzeichen, ganze kg. */
export function kg(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return Math.round(v).toLocaleString("de-DE").replace(/\./g, " ");
}

/** „+12,3 %" / „−3,4 %" — Vorzeichen immer, eine Nachkommastelle. */
export function pct(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return (v > 0 ? "+" : v < 0 ? "−" : "") + dezimal(Math.abs(v), 1) + " %";
}

/**
 * Farbton einer Phase — **ruhig oder gar nichts, nie eine Warnung**.
 *
 * Die Sprit-Achse ist aus der Note geflogen, weil sie die Flugsicherung
 * benotete statt den Piloten. Eine Warnfarbe wäre derselbe Vorwurf, nur in
 * Rot statt in Punkten — deshalb gibt es sie hier nicht. Wer sparsam fliegt,
 * bekommt dafür weiterhin die ruhige Farbe; wer darüber liegt, bekommt die
 * Zahl und sonst nichts.
 *
 * Am Bestand gemessen (Korpus-Prüfung 18.09.2026, 39 Flüge):
 *
 * - **Anflug**: 82 % lägen über +3 %, und zwischen 0 und +10 % liegt kein
 *   einziger Flug. Die Phase misst überwiegend die Dauer der Radarführung —
 *   ein A359 verbrannte dort 3 113 kg statt geplanter 704, in 36,5 Minuten.
 *   Was fast jeder bekommt, ist keine Auskunft.
 * - **bis Sinkflug**: Hier wäre eine Schwelle vertretbar (Median −8,5 %),
 *   aber sie träfe das Falsche. Seit der Vergleichspunkt auf den echten
 *   Sinkflugbeginn wartet, wandert der Reiseflug eines spät sinkenden Fluges
 *   in diese Phase. Von sechs Flügen über +3 % sind fünf genau solche Fälle —
 *   die Farbe markierte einen verschobenen Phasenschnitt, kein Verhalten.
 *
 * Die Landemarke in der Leiter hält es genauso („Ton nach Reservestand, nie
 * die Fehlerfarbe"). Gewertet wird allein im Badge, und dort nach dem
 * Reservestand — das ist eine Sicherheitsaussage, keine Sparnote.
 */
export function phaseTon(
  p: SpritPhase | null,
  art: "bis_sinkflug" | "anflug",
): "ok" | "neutral" {
  if (!p) return "neutral";
  if (art === "anflug") return "neutral";
  return p.abweichung_pct <= 3 ? "ok" : "neutral";
}

/** Balkenbreite (0–100) für Ist gegen Plan, gedeckelt bei 3×Plan. */
export function balken(p: SpritPhase | null): { plan: number; ist: number } {
  if (!p || p.plan_kg <= 0) return { plan: 0, ist: 0 };
  const max = Math.max(p.plan_kg, p.ist_kg, 1);
  return { plan: (p.plan_kg / max) * 100, ist: (p.ist_kg / max) * 100 };
}

/** Differenz in kg mit Vorzeichen — „+3 596 kg". */
export function diffKg(p: SpritPhase | null | undefined): string {
  if (!p) return "—";
  const d = Math.round(p.ist_kg - p.plan_kg);
  return (d > 0 ? "+" : d < 0 ? "−" : "") + kg(Math.abs(d)) + " kg";
}

/**
 * Die Hauptzahl einer Phase.
 *
 * Bis zum Sinkflug sagt der Prozentwert etwas — dort liegen die Werte in
 * einem lesbaren Band. Im Anflug nicht: die Korpus-Prüfung fand einen Median
 * von +44,9 % und Ausreißer bis +342 %. Eine solche Zahl erklärt niemandem
 * etwas, und laut Modul-Kopf gehört diese Phase ohnehin nicht dem Piloten.
 * Deshalb steht dort die Differenz in Kilogramm.
 */
export function hauptzahl(
  p: SpritPhase | null | undefined,
  _phase?: "bis_sinkflug" | "anflug",
): { wert: string; einheit: string } {
  if (!p) return { wert: "", einheit: "" };
  // v1.7.36: Die Entscheidung trifft die Rechnung (`als_kg`), nicht die
  // Anzeige. Bis dahin stand die 60-%-Regel an vier Stellen; wer eine
  // änderte, ließ die anderen lautlos zurück.
  if (p.als_kg) {
    return { wert: diffKg(p).replace(/\s*kg$/, ""), einheit: "kg" };
  }
  return { wert: pct(p.abweichung_pct).replace(/\s*%$/, ""), einheit: "%" };
}

/**
 * Dieselbe Zahl als ein Stück Text — für Stellen, die keine zwei Elemente
 * setzen können (Fließtext, PDF, PIREP-Feld).
 *
 * Wert und Einheit stehen getrennt in `hauptzahl`, weil die Live-Übersicht
 * die Einheit kleiner setzt. Wer sie zusammen braucht, ruft das hier —
 * statt eine zweite Regel danebenzustellen. Genau daran sind Client und
 * Webapp bis v1.7.35 auseinandergelaufen.
 */
export function hauptzahlText(
  p: SpritPhase | null | undefined,
  phase: "bis_sinkflug" | "anflug",
): string {
  const h = hauptzahl(p, phase);
  if (!h.wert) return "—";
  return `${h.wert} ${h.einheit}`;
}
