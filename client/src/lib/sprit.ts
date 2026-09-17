/**
 * v1.7.35 — Sprit-Auswertung ohne Note.
 *
 * Wire-Format von `landing_scoring::sprit::SpritAuswertung` (Rust). Der
 * Client rechnet einmal beim Aufsetzen; hier stehen nur der Typ und
 * Formatierer. **Nichts wird hier nachgerechnet** — fehlt ein Feld, zeigt
 * die Oberfläche „—". Die Webapp trägt denselben Typ in
 * `aeroacars-live/webapp/src/components/sprit.ts`.
 */

export interface SpritPhase {
  ist_kg: number;
  plan_kg: number;
  /** Eine Nachkommastelle, gerundet vom Client. */
  abweichung_pct: number;
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
  landing_fuel_kg: number | null;
  extra_getankt_kg: number | null;
  extra_genutzt_kg: number | null;
  extra_ungenutzt_kg: number | null;
  contingency_verbraucht: boolean | null;
  alternate_und_reserve_intakt: boolean | null;
  leiter: SpritLeiter | null;
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
  const s = v.toFixed(1).replace(".", ",");
  return (v > 0 ? "+" : v < 0 ? "−" : "") + s.replace("-", "") + " %";
}

/** Farbton einer Phase: sparsamer oder nahe Plan = ruhig, deutlich drüber = warm. */
export function phaseTon(p: SpritPhase | null): "ok" | "warn" | "neutral" {
  if (!p) return "neutral";
  if (p.abweichung_pct <= 3) return "ok";
  return "warn";
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
 * von +66 % und Ausreißer bis +352 %. Eine solche Zahl erklärt niemandem
 * etwas, und laut Modul-Kopf gehört diese Phase ohnehin nicht dem Piloten.
 * Deshalb steht dort die Differenz in Kilogramm.
 */
export function hauptzahl(p: SpritPhase | null | undefined, phase: "bis_sinkflug" | "anflug"): string {
  if (!p) return "—";
  if (phase === "anflug" && Math.abs(p.abweichung_pct) > 60) return diffKg(p);
  return pct(p.abweichung_pct);
}
