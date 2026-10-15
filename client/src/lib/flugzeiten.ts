// „Zeiten gegen Flugplan" (13.10.2026): Plan aus dem SimBrief-OFP gegen die
// Ist-Zeiten von AeroACARS. Reine Anzeige — keine Note, kein Abzug
// (Thomas: „kein Punktabzug, einfach nur darstellen").
//
// Farbe JA, Note NEIN (Thomas 15.10.2026: „ein bisschen mit Farben spielen,
// Delay und zu früh darstellen, aber nicht in die Bewertung"): Abweichungen
// bekommen eine Hinweisfarbe nach `abweichungTon` — für Cockpit,
// Landungen-Reiter und Webseite aus dieser einen Regel.
//
// Rust liefert nur die acht Zeitpunkte (landing_scoring::flugzeiten,
// Unix-Sekunden UTC). Alles, was daraus folgt — Abweichungen, Dauern, der
// Kopfsatz, die Schätzung im Cockpit —, steht HIER und nur hier.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN): Client und
// Webapp rechnen mit genau dieser Datei.

/** Plan und Ist als Unix-Sekunden (UTC). Fehlende Werte fehlen. */
export interface FlugZeiten {
  plan_off_block?: number | null;
  plan_abheben?: number | null;
  plan_aufsetzen?: number | null;
  plan_on_block?: number | null;
  off_block?: number | null;
  abheben?: number | null;
  aufsetzen?: number | null;
  on_block?: number | null;
  /** Wann das OFP erstellt wurde (Unix-s) — „Plan aus dem OFP vom …". */
  ofp_erstellt?: number | null;
}

export type ZeitSchritt = "off_block" | "abheben" | "aufsetzen" | "on_block";

export const SCHRITTE: ZeitSchritt[] = ["off_block", "abheben", "aufsetzen", "on_block"];

/** Wie Airlines zählen: bis 15 min nach Plan gilt als pünktlich (A15).
 *  Nur ein Text neben der Zahl, keine Farbe und keine Wertung. */
export const PUENKTLICH_BIS_MIN = 15;

/** Das Airline-Urteil nur, solange die Abweichung nach einem echten
 *  Umlauf aussieht (Thomas 13.10.2026, Variante c): Bei mehr als einer
 *  Stunde lag meist der Plan daneben — die Minuten stehen trotzdem da. */
export const A15_URTEIL_BIS_MIN = 60;

function zahl(v: number | null | undefined): number | null {
  return v != null && Number.isFinite(v) && v > 0 ? v : null;
}

export function plan(z: FlugZeiten, s: ZeitSchritt): number | null {
  return zahl(z[`plan_${s}`]);
}

export function ist(z: FlugZeiten, s: ZeitSchritt): number | null {
  return zahl(z[s]);
}

export function hatPlan(z: FlugZeiten | null | undefined): boolean {
  return !!z && SCHRITTE.some((s) => plan(z, s) != null);
}

/** Liegt der Plan weiter als das neben dem Flug, gehört er nicht zu ihm. */
export const PLAN_PASST_BIS_STUNDEN = 6;

/**
 * Gehört der Plan zu diesem Flug? (14.10.2026)
 *
 * Bei GSG entsteht der SimBrief-Plan beim Erstellen des OFP: `sched_out`
 * liegt fast immer rund 40 min danach. Wer mit einem OFP von gestern
 * fliegt, wäre sonst „1440 min verspätet". Verglichen wird der erste
 * Schritt, der Plan UND Ist hat; vor dem Abrollen der geplante Off-Block
 * mit `jetzt` (nur im Cockpit). Ohne beides gibt es nichts zu prüfen.
 */
export function planPasst(z: FlugZeiten, jetzt?: number | null): boolean {
  const grenze = PLAN_PASST_BIS_STUNDEN * 3600;
  for (const s of SCHRITTE) {
    const p = plan(z, s);
    const i = ist(z, s);
    if (p != null && i != null) return Math.abs(i - p) <= grenze;
  }
  const pOff = plan(z, "off_block");
  const j = zahl(jetzt);
  if (pOff != null && j != null) return Math.abs(j - pOff) <= grenze;
  return true;
}

/** Abweichung in ganzen Minuten; + = später als geplant. */
export function abweichungMin(p: number | null, i: number | null): number | null {
  if (p == null || i == null) return null;
  // Betrag runden, dann Vorzeichen: ±30 s ergeben beide 1 min (Math.round
  // allein rundet −30 s auf −0, +30 s auf +1 — Codex-QS 13.10.2026).
  const d = i - p;
  const m = Math.sign(d) * Math.round(Math.abs(d) / 60);
  return m === 0 ? 0 : m;
}

/** Wie eine Abweichung eingefärbt wird. `null` = ohne Farbe.
 *
 *  - bis ±PUENKTLICH_BIS_MIN: wie Airlines zählen pünktlich → keine Farbe
 *  - später: "spaet" (Hinweisfarbe), früher: "frueh" (Infofarbe — zu früh
 *    heißt auch: das Gate ist vielleicht noch nicht frei)
 *  - über A15_URTEIL_BIS_MIN: meist lag der Plan daneben, nicht der Pilot →
 *    keine Farbe (dieselbe Grenze wie beim Airline-Urteil)
 *
 *  ⚠ Nie Rot: Rot steht für harte Landungen und Warnungen. Eine Uhrzeit darf
 *  nicht nach Gefahr oder Abzug aussehen. */
export type AbweichungTon = "spaet" | "frueh";
export function abweichungTon(min: number | null | undefined): AbweichungTon | null {
  if (min == null || !Number.isFinite(min)) return null;
  const a = Math.abs(min);
  if (a <= PUENKTLICH_BIS_MIN || a > A15_URTEIL_BIS_MIN) return null;
  return min > 0 ? "spaet" : "frueh";
}

export interface ZeitZeile {
  schritt: ZeitSchritt;
  plan: number | null;
  ist: number | null;
  abw_min: number | null;
}

/** Eine Zeile je Schritt, der Plan ODER Ist hat. */
export function zeitZeilen(z: FlugZeiten): ZeitZeile[] {
  return SCHRITTE.map((s) => {
    const p = plan(z, s);
    const i = ist(z, s);
    return { schritt: s, plan: p, ist: i, abw_min: abweichungMin(p, i) };
  }).filter((r) => r.plan != null || r.ist != null);
}

function dauer(a: number | null, b: number | null): number | null {
  if (a == null || b == null || b < a) return null;
  return Math.round((b - a) / 60);
}

export interface Dauern {
  block_plan: number | null;
  block_ist: number | null;
  flug_plan: number | null;
  flug_ist: number | null;
}

/** Block- und Flugzeit in Minuten, Plan und Ist. */
export function dauern(z: FlugZeiten): Dauern {
  return {
    block_plan: dauer(plan(z, "off_block"), plan(z, "on_block")),
    block_ist: dauer(ist(z, "off_block"), ist(z, "on_block")),
    flug_plan: dauer(plan(z, "abheben"), plan(z, "aufsetzen")),
    flug_ist: dauer(ist(z, "abheben"), ist(z, "aufsetzen")),
  };
}

export interface Kopf {
  /** Der späteste Schritt, der Plan UND Ist hat (meist On-Block). */
  schritt: ZeitSchritt;
  abw_min: number;
  /** Blockzeit Ist minus Plan (min), wenn beide bekannt. */
  block_diff_min: number | null;
}

export function kopf(z: FlugZeiten): Kopf | null {
  const zeile = [...zeitZeilen(z)].reverse().find((r) => r.abw_min != null);
  if (!zeile || zeile.abw_min == null) return null;
  const d = dauern(z);
  return {
    schritt: zeile.schritt,
    abw_min: zeile.abw_min,
    block_diff_min:
      d.block_plan != null && d.block_ist != null ? d.block_ist - d.block_plan : null,
  };
}

/** „14:30z" */
export function uhrzeit(sek: number | null | undefined): string {
  if (sek == null || !Number.isFinite(sek)) return "—";
  return `${new Date(sek * 1000).toISOString().slice(11, 16)}z`;
}

/** „13.10. 13:48z" */
export function datumUhrzeit(sek: number | null | undefined): string {
  if (sek == null || !Number.isFinite(sek)) return "—";
  const iso = new Date(sek * 1000).toISOString();
  return `${iso.slice(8, 10)}.${iso.slice(5, 7)}. ${iso.slice(11, 16)}z`;
}

/** „1:10" (Stunden:Minuten) */
export function dauerText(min: number | null | undefined): string {
  if (min == null || !Number.isFinite(min)) return "—";
  const m = Math.abs(Math.round(min));
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}`;
}

/** „+7 min", „−4 min", „±0 min" — mit echtem Minuszeichen. */
export function minutenText(min: number | null | undefined): string {
  if (min == null || !Number.isFinite(min)) return "—";
  if (min === 0) return "±0 min";
  return `${min > 0 ? "+" : "−"}${Math.abs(min)} min`;
}

/** Geplante Rollzeit nach der Landung (s) — für die Schätzung im Cockpit. */
export function rollzeitNachLandungS(z: FlugZeiten): number | null {
  const a = plan(z, "aufsetzen");
  const b = plan(z, "on_block");
  return a != null && b != null && b >= a ? b - a : null;
}

export interface CockpitSchaetzung {
  /** Erwartetes On-Block (Unix-s). */
  erwartet: number;
  /** Erwartet minus Plan (min). */
  gegen_plan_min: number;
  /** Seit dem Abheben gutgemacht (+) bzw. verloren (−), min. */
  aufgeholt_min: number | null;
  /** Woraus die Schätzung stammt — für die Unterzeile. */
  quelle: "on_block" | "aufsetzen" | "route" | "abheben" | "off_block" | "plan";
}

/**
 * Erwartetes On-Block für das Cockpit. Das Neueste, was feststeht, gewinnt:
 * tatsächliches On-Block → Landung + geplante Rollzeit → Planzeit ab dem
 * Ort auf der Route (`landungErwartet`, aus dem Navlog) → Abheben + geplante
 * Flug- und Rollzeit → Off-Block + geplante Blockzeit → Plan.
 */
export function cockpitSchaetzung(
  z: FlugZeiten,
  landungErwartet: number | null | undefined,
): CockpitSchaetzung | null {
  const planOn = plan(z, "on_block");
  if (planOn == null) return null;
  const rollen = rollzeitNachLandungS(z) ?? 0;
  const pAb = plan(z, "abheben");
  const pAuf = plan(z, "aufsetzen");
  const pOff = plan(z, "off_block");
  let erwartet: number;
  let quelle: CockpitSchaetzung["quelle"];
  const on = ist(z, "on_block");
  const auf = ist(z, "aufsetzen");
  const ab = ist(z, "abheben");
  const off = ist(z, "off_block");
  const route = zahl(landungErwartet);
  if (on != null) {
    erwartet = on;
    quelle = "on_block";
  } else if (auf != null) {
    erwartet = auf + rollen;
    quelle = "aufsetzen";
  } else if (ab != null && route != null) {
    erwartet = route + rollen;
    quelle = "route";
  } else if (ab != null && pAb != null && pAuf != null) {
    erwartet = ab + (pAuf - pAb) + rollen;
    quelle = "abheben";
  } else if (off != null && pOff != null) {
    erwartet = off + (planOn - pOff);
    quelle = "off_block";
  } else {
    erwartet = planOn;
    quelle = "plan";
  }
  const gegen = abweichungMin(planOn, erwartet) ?? 0;
  const beimAbheben = abweichungMin(pAb, ab);
  return {
    erwartet,
    gegen_plan_min: gegen,
    aufgeholt_min: beimAbheben != null && quelle !== "abheben" ? beimAbheben - gegen : null,
    quelle,
  };
}

export interface AbflugSchaetzung {
  /** Geplantes Off-Block (Unix-s). */
  plan: number;
  /** Erwartetes Off-Block: der Plan oder — wenn er vorbei ist — jetzt. */
  erwartet: number;
  /** Erwartet minus Plan (min), 0 bis zum geplanten Off-Block. */
  gegen_plan_min: number;
  /** Minuten bis zum geplanten Off-Block, 0 wenn er vorbei ist. */
  noch_min: number;
}

/**
 * Das Cockpit VOR dem Abrollen (14.10.2026, Thomas: „Zeiten sind wichtig
 * für wen, der danach fliegen möchte"): Am Gate zählt der geplante
 * Off-Block. Ist er vorbei, rollt man frühestens jetzt ab — die
 * Verspätung steht dann schon am Gate da, nicht erst nach dem Abrollen.
 * `null` nach dem Abrollen oder ohne geplanten Off-Block.
 */
export function abflugSchaetzung(
  z: FlugZeiten,
  jetzt: number | null | undefined,
): AbflugSchaetzung | null {
  const pOff = plan(z, "off_block");
  const j = zahl(jetzt);
  if (pOff == null || j == null || ist(z, "off_block") != null) return null;
  const erwartet = Math.max(j, pOff);
  return {
    plan: pOff,
    erwartet,
    gegen_plan_min: abweichungMin(pOff, erwartet) ?? 0,
    // Aufrunden: Bis zum geplanten Off-Block bleibt es „noch 1 min", nie
    // vorzeitig „vorbei" (Codex PR #139).
    noch_min: Math.max(0, Math.ceil((pOff - j) / 60)),
  };
}

export interface ZeitachsenBalken {
  /** Anteil 0–1 der Achse */
  von: number;
  bis: number;
  art: "rollen" | "flug";
}

export interface Zeitachse {
  plan: ZeitachsenBalken[];
  ist: ZeitachsenBalken[];
  /** Beschriftete Striche: Anteil der Achse + Uhrzeit (Unix-s). */
  striche: { pos: number; sek: number }[];
}

const STRICH_SCHRITTE_MIN = [15, 30, 60, 120, 180, 360, 720];

function balken(
  a: number | null,
  b: number | null,
  c: number | null,
  d: number | null,
): [number, number, ZeitachsenBalken["art"]][] {
  const raus: [number, number, ZeitachsenBalken["art"]][] = [];
  if (a != null && b != null && b >= a) raus.push([a, b, "rollen"]);
  if (b != null && c != null && c >= b) raus.push([b, c, "flug"]);
  if (c != null && d != null && d >= c) raus.push([c, d, "rollen"]);
  return raus;
}

/** Plan- und Ist-Balken auf einer gemeinsamen Zeitachse. `null`, wenn es
 *  keinen einzigen vollständigen Abschnitt gibt. */
export function zeitachse(z: FlugZeiten): Zeitachse | null {
  const p = balken(plan(z, "off_block"), plan(z, "abheben"), plan(z, "aufsetzen"), plan(z, "on_block"));
  const i = balken(ist(z, "off_block"), ist(z, "abheben"), ist(z, "aufsetzen"), ist(z, "on_block"));
  const alle = [...p, ...i].flatMap(([a, b]) => [a, b]);
  if (alle.length === 0) return null;
  const min = Math.min(...alle);
  const max = Math.max(...alle);
  const spanne = Math.max(max - min, 600);
  const schritt =
    (STRICH_SCHRITTE_MIN.find((m) => spanne / (m * 60) <= 4) ?? 1440) * 60;
  const start = Math.floor(min / schritt) * schritt;
  const ende = Math.max(Math.ceil(max / schritt) * schritt, start + schritt);
  const pos = (t: number) => (t - start) / (ende - start);
  const striche: { pos: number; sek: number }[] = [];
  for (let t = start; t <= ende; t += schritt) striche.push({ pos: pos(t), sek: t });
  const zu = (xs: typeof p) => xs.map(([a, b, art]) => ({ von: pos(a), bis: pos(b), art }));
  return { plan: zu(p), ist: zu(i), striche };
}
