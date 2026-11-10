// Reiter Protokoll: PIREP-Felder nach Themen ordnen und die Buchungen eines
// Flugs zusammenrechnen. Eingeordnet wird über den Feld-`slug`, weil die Namen
// je nach Quelle (Client, Server, Module) wechseln können; was hier nicht steht,
// landet in „Weitere“ und wird nicht verschluckt.
import type { PirepBuchung, PirepFeld } from "./logbuchForm";

export type Gruppe = "zeiten" | "route" | "gewichte" | "sprit" | "landung" | "wetter" | "technik" | "weitere";

const ORDNUNG: Array<[Gruppe, string[]]> = [
  ["zeiten", ["blocks-off-time", "takeoff-time", "landing-time", "blocks-on-time", "total-block-time", "total-flight-time", "taxi-out-time", "taxi-in-time"]],
  ["route", ["skyadventures", "network-online", "network-callsign-used", "network-presence-check", "departure-gate", "arrival-gate", "cruise-level", "flown-distance", "plan-alternate", "diversion-airport", "approach-runway-atc", "plan-route"]],
  ["gewichte", ["aircraft-type", "aircraft-reg", "plan-zfw", "plan-tow", "takeoff-weight", "plan-ldw", "landing-weight"]],
  ["sprit", ["block-fuel", "plan-block-fuel", "fuel-used", "plan-trip-burn", "fuel-efficiency", "landing-fuel", "plan-reserve-fuel", "sprit-rollen-vor-start", "sprit-bis-sinkflug", "sprit-anflug", "sprit-rollen-nach-landung", "contingency", "extra-fuel", "final-reserve", "zeit-unter-schwelle"]],
  ["landung", ["landing-score", "landing-rate", "landing-g-force", "landing-speed", "landing-pitch", "landing-roll", "landing-heading", "touchdown-runway", "touchdown-past-threshold", "rollout-distance", "centerline-offset", "touchdown-headwind", "touchdown-tailwind", "touchdown-crosswind", "touchdown-sideslip", "touchdowns", "bounces", "light-bounces", "hardest-touchdown", "go-arounds", "approach-vs-stddev", "approach-bank-stddev", "anflugstrecke", "takeoff-pitch", "takeoff-roll"]],
  ["wetter", ["departure-metar", "arrival-metar"]],
  ["technik", ["source", "aeroacars-paket", "pmdg-aircraft", "pmdg-fmc-flight", "pmdg-autobrake-landing", "pmdg-ecl-complete", "pmdg-plan-ldg-flaps", "pmdg-plan-to-flaps", "pmdg-to-config-warning", "pmdg-v-speeds-takeoff", "pmdg-vref-landing", "accident", "accident-kind", "accident-confidence", "accident-detected-at", "accident-reasons", "accident-classifier-version"]],
];

const PLATZ = new Map(ORDNUNG.flatMap(([g, slugs]) => slugs.map((s, i) => [s, { g, i }] as const)));
const REIHE: Gruppe[] = [...ORDNUNG.map(([g]) => g), "weitere"];

export interface FeldGruppe { gruppe: Gruppe; zeilen: PirepFeld[] }

/** Leere Felder sind Platzhalter von phpVMS und zählen nicht. */
export function gruppieren(felder: PirepFeld[] | undefined): FeldGruppe[] {
  const belegt = (felder ?? []).filter((f) => f.value.trim() !== "");
  return REIHE.flatMap((gruppe) => {
    const zeilen = belegt
      .filter((f) => (PLATZ.get(f.slug)?.g ?? "weitere") === gruppe)
      .map((f) => ({ f, i: PLATZ.get(f.slug)?.i ?? 0 }))
      .sort((a, b) => a.i - b.i)
      .map(({ f }) => f);
    return zeilen.length ? [{ gruppe, zeilen }] : [];
  });
}

export interface Finanzen {
  einnahmen: PirepBuchung[];
  kosten: PirepBuchung[];
  /** Beträge in Cent, wie sie phpVMS speichert. */
  summeEin: number;
  summeAus: number;
  ergebnis: number;
}

/** Eine Buchung kann Ertrag und Kosten zugleich tragen (Tarife); sie steht dann
 *  bei den Einnahmen, und beide Beträge zählen wie auf der PIREP-Seite. */
export function finanzen(buchungen: PirepBuchung[] | undefined): Finanzen {
  const zeilen = (buchungen ?? []).filter((b) => b.credit_c !== 0 || b.debit_c !== 0);
  const einnahmen = zeilen.filter((b) => b.credit_c !== 0).sort((a, b) => b.credit_c - a.credit_c);
  const kosten = zeilen.filter((b) => b.credit_c === 0).sort((a, b) => b.debit_c - a.debit_c);
  const summeEin = zeilen.reduce((s, b) => s + b.credit_c, 0);
  const summeAus = zeilen.reduce((s, b) => s + b.debit_c, 0);
  return { einnahmen, kosten, summeEin, summeAus, ergebnis: summeEin - summeAus };
}
