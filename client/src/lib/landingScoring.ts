// Touchdown-Sub-Score-Berechnung — gemeinsame Quelle der Wahrheit für
// Pilot-App (Tauri-Client) UND Live-Monitor-Webapp.
//
// **Wichtig:** Diese Datei ist 1:1 identisch mit dem File
// `webapp/src/components/landingScoring.ts` im aeroacars-live-Repo.
// Wenn du hier was änderst → SOFORT im Webapp-Modul mitziehen.
// Adrian-Feedback v0.5.47: Pilot soll im Web und im Client für
// denselben Flug exakt dieselben Sub-Scores, Labels, Bands, Schwellen
// und Coach-Tipps sehen. Zwei parallele Score-Tabellen sind ein
// Gift-Apfel — Pilot zweifelt sonst an einer der beiden Plattformen.
//
// V/S- und G-Schwellwerte gespiegelt aus `client/src-tauri/src/lib.rs`
// (TOUCHDOWN_VS_*, TOUCHDOWN_G_*) damit auch der Rust-seitige Master-
// Score auf derselben Skala läuft. Stability/Rollout/Fuel sind
// eigenständige UI-Server-Logik die hier definiert wird.

export type Band = "good" | "ok" | "bad";

/** Wire format for sub-scores. */
export interface SubScore {
  key: "landing_rate" | "g_force" | "bounces" | "stability" | "rollout" | "fuel";
  points: number;
  value: string;
  band: Band;
  rationale: string;
}

// ─── Lokalisierungs-Tabellen (deutsch, Pilot-facing) ────────────────
//
// Die i18n-Schlüssel im Client (locales/de/common.json unter
// `landing.rat.*` und `landing.tip.*`) MÜSSEN inhaltlich mit diesen
// Strings hier übereinstimmen — das Webapp rendert ohne i18next direkt
// aus dieser Tabelle, der Client geht über react-i18next. Beide enden
// beim gleichen deutschen Text.

export const RATIONALE_LABELS: Record<string, string> = {
  // v0.20.x QS fix: possible_float/firm_positive_touchdown were added to
  // the real scoring ladder (sub_landing_rate's target-corridor rework)
  // but never added here — this file's own header comment demands they
  // stay 1:1 with the aeroacars-live webapp mirror, which renders
  // straight from this table with no i18next fallback. Text mirrors
  // locales/de/common.json's landing.rat.*/landing.tip.* keys verbatim.
  possible_float: "Sehr weich aufgesetzt — mögliches langes Abfangen",
  firm_positive_touchdown: "Fest, positiv aufgesetzt",
  smooth_touchdown: "Butterweich aufgesetzt",
  firm_but_clean: "Feste, aber saubere Landung",
  above_target: "Etwas härter als ideal",
  hard_landing: "Harte Landung — Inspektion ggf. ratsam",
  very_hard: "Sehr hart — Inspektion erforderlich",
  severe_inspection: "Schwere Landung — strukturelle Prüfung",
  smooth_g: "Sehr ruhiger G-Aufbau",
  comfortable_g: "Komfortabel",
  noticeable_g: "Deutlich spürbar",
  firm_g: "Fest",
  hard_g: "Hart",
  severe_g: "Strukturell kritisch",
  clean_set: "Sauber gesetzt",
  one_bounce: "Einmal kurz aufgesetzt",
  two_bounces: "Mehrfach aufgesetzt",
  many_bounces: "Mehrere Bounces",
  very_stable: "Sehr stabiler Anflug",
  stable: "Stabiler Anflug",
  average_stability: "Durchschnittlich stabil",
  unstable_approach: "Unruhiger Anflug",
  very_unstable: "Sehr unruhig",
  excellent_stop: "Hervorragender Bremsweg",
  good_stop: "Solider Bremsweg",
  long_rollout: "Langes Ausrollen",
  very_long_rollout: "Sehr lang ausgerollt",
  marginal_runway: "Knappe Bahnreserve",
  on_plan: "Auf Plan",
  near_plan: "Nahe am Plan",
  off_plan: "Vom Plan abgewichen",
  very_off_plan: "Stark vom Plan abgewichen",
  way_off_plan: "Weit vom Plan entfernt",
};

export const TIP_LABELS: Record<string, string> = {
  possible_float:
    "Sehr sanft — das deutet auf ein langes Abfangen hin, was Bodenkontakt, Spoiler/Autobrake und damit auch den Bremsweg verzögert. Etwas entschlossener aufsetzen (Ziel 90–250 fpm).",
  firm_positive_touchdown:
    "Genau richtig — fest und positiv aufgesetzt, ohne zu hart. So sollen Airliner-Landungen aussehen.",
  smooth_touchdown: "Genau so — leichter Flare zur richtigen Zeit, sanfte Reduzierung der Sinkrate kurz vor TD.",
  firm_but_clean: "Solide. Falls ruhiger gewünscht: 1-2 Sekunden früher mit dem Flare beginnen.",
  above_target: "Mehr Flare einleiten und Schubreduktion etwas später timen — nicht aufdrücken.",
  hard_landing: "Anflug-Sinkrate kontrollieren (Ziel −700 fpm im Final), dann sauberen Flare bei ~30 ft.",
  very_hard: "Klassische Hard-Landing-Symptomatik. Stabilizer Approach prüfen, Flare-Timing & Pitch beobachten.",
  severe_inspection: "Strukturkritisch — Maintenance-Inspektion in der Realität fällig. Anflug-Stabilität priorisieren.",
  smooth_g: "Sehr saubere Touchdown-Mechanik. Beibehalten.",
  comfortable_g: "Gut gemacht. Pitch-Übergang in den Flare war sauber.",
  noticeable_g: "Etwas mehr Flare und sanfter setzen — Pitch im letzten Moment leicht erhöhen.",
  firm_g: "G-Spike zu hoch — Pitch-Aufbau im Flare verzögern und sanfter setzen.",
  hard_g: "Hart aufgesetzt. Schub deutlich vor TD reduzieren, sanfter Flare.",
  severe_g: "G-Kritisch — Anflug-Profil komplett überprüfen.",
  clean_set: "Perfekt — einmal sauber gesetzt und auf der Bahn geblieben.",
  one_bounce: "Bei TD nicht aufdrücken — Pitch halten oder leicht erhöhen, dann sanft setzen.",
  two_bounces: "Bouncing entsteht durch zu viel Pitch beim Flare oder zu hohe Sinkrate. Stabileren Anflug fliegen.",
  many_bounces: "Mehrfach-Bounces deuten auf instabiles TD-Profil. Go-Around in Erwägung ziehen wenn unsicher.",
  very_stable: "Beispielhaft stabiler Anflug — V/S und Bank kaum Schwankungen.",
  stable: "Stabiler Anflug. Auf konstantem Glideslope bleiben.",
  average_stability: "OK, aber Pitch- und V/S-Schwankungen reduzieren — kleine Korrekturen statt großer.",
  unstable_approach: "Anflug war unruhig. Stabilized-Approach-Kriterien prüfen: Konfig, Speed, V/S, Track ab 1000 ft AAL.",
  very_unstable: "Sehr unruhiger Anflug — in der Realität Go-Around-Kandidat. Approach-Setup früher fertig haben.",
  excellent_stop: "Hervorragend — kurzer Bremsweg, gute Kontrolle. Kein Reverser-Overuse.",
  good_stop: "Solider Bremsweg. Leichte Reserve — passt.",
  long_rollout: "Etwas lang ausgerollt. Schon im Flare an Auto-Brake oder etwas Reverser denken.",
  very_long_rollout: "Sehr langer Bremsweg. Auto-Brake-Setting prüfen oder früher in den Reverser.",
  marginal_runway: "Bahnreserve war knapp — bei nasser Bahn kritisch. Höheres Auto-Brake-Setting wählen.",
  on_plan: "Sprit exakt auf Plan — saubere Performance-Kalkulation.",
  near_plan: "Sehr nah am Plan, alles im grünen Bereich.",
  off_plan: "Abweichung vom OFP. Cruise-Speed-Mach und Höhe gegen Plan prüfen.",
  very_off_plan: "Deutliche Plan-Abweichung. Cost-Index, Cruise-Höhe oder Wind-Einfluss prüfen.",
  way_off_plan: "Stark vom Plan ab — eventuell falsches OFP oder andere Strecke geflogen.",
  fallback: "Solide Landung. Im Score-Breakdown siehst du wo es noch Luft nach oben gibt.",
};

export const SUB_LABELS: Record<SubScore["key"], string> = {
  landing_rate: "Sinkrate",
  g_force: "G-Kraft",
  bounces: "Bounce-Qualität",
  stability: "Anflug-Stabilität",
  rollout: "Bahn-Auslastung",
  fuel: "Spritverbrauch",
};

export function rationaleLabel(rationale: string): string {
  return RATIONALE_LABELS[rationale] ?? rationale;
}

export function coachTip(subs: SubScore[]): { sub: SubScore; tip: string } | null {
  if (subs.length === 0) return null;
  const sorted = [...subs].sort((a, b) => a.points - b.points);
  const worst = sorted[0]!;
  const tip = TIP_LABELS[worst.rationale] ?? TIP_LABELS.fallback ?? "";
  return { sub: worst, tip };
}

// ─── Sub-Score-Berechnung ─────────────────────────────────────────

// Exported single source of truth for the V/S- and G-tone bands. The
// forensik components (SinkrateForensik / GForceForensik) import these instead
// of shadowing the literals (Q4, Audit 2026-06-27).
export const T_VS_SMOOTH_FPM = 200;
export const T_VS_FIRM_FPM = 400;
export const T_VS_HARD_FPM = 600;
export const T_VS_SEVERE_FPM = 1000;

export const T_G_SMOOTH = 1.2;
export const T_G_FIRM = 1.4;
export const T_G_HARD = 1.7;
export const T_G_SEVERE = 2.1;

export function band(points: number): Band {
  if (points >= 75) return "good";
  if (points >= 45) return "ok";
  return "bad";
}

// v0.20.x (score_algorithm_version 4→5): replaced the old monotonic
// "softer is always better" curve with a TARGET CORRIDOR — kept in
// lockstep with Rust's sub_landing_rate.rs, see that file's doc comment
// for the full rationale. T_VS_SMOOTH_FPM/etc. above are DELIBERATELY
// left unchanged (they still drive classifyByVS's category ladder and
// the forensics-chart tone functions) — only this function's own point
// bands use the new corridor constants below.
const VS_FLOAT_RISK_BELOW_FPM = 90;
const VS_TARGET_CORRIDOR_TOP_FPM = 250;

function subLandingRate(peakVsFpm: number): SubScore {
  const vs = Math.abs(peakVsFpm);
  const signed = Math.round(peakVsFpm);
  const display = `${signed === 0 ? 0 : signed} fpm`;
  if (vs < VS_FLOAT_RISK_BELOW_FPM)
    return { key: "landing_rate", points: 85, value: display, band: "good", rationale: "possible_float" };
  if (vs < VS_TARGET_CORRIDOR_TOP_FPM)
    return { key: "landing_rate", points: 100, value: display, band: "good", rationale: "firm_positive_touchdown" };
  if (vs < T_VS_FIRM_FPM)
    return { key: "landing_rate", points: 80, value: display, band: "good", rationale: "above_target" };
  if (vs < T_VS_HARD_FPM)
    return { key: "landing_rate", points: 45, value: display, band: "ok", rationale: "hard_landing" };
  if (vs < T_VS_SEVERE_FPM)
    return { key: "landing_rate", points: 20, value: display, band: "bad", rationale: "very_hard" };
  return { key: "landing_rate", points: 0, value: display, band: "bad", rationale: "severe_inspection" };
}

function subGForce(peakG: number): SubScore {
  const v = `${peakG.toFixed(2)} G`;
  if (peakG < T_G_SMOOTH) return { key: "g_force", points: 100, value: v, band: "good", rationale: "smooth_g" };
  if (peakG < T_G_FIRM)   return { key: "g_force", points: 85,  value: v, band: "good", rationale: "comfortable_g" };
  if (peakG < T_G_HARD)   return { key: "g_force", points: 60,  value: v, band: "ok",   rationale: "noticeable_g" };
  if (peakG < T_G_SEVERE) return { key: "g_force", points: 30,  value: v, band: "bad",  rationale: "firm_g" };
  return                       { key: "g_force", points: 0,   value: v, band: "bad",  rationale: "severe_g" };
}

function subBounces(bounces: number): SubScore {
  if (bounces === 0) return { key: "bounces", points: 100, value: "0", band: "good", rationale: "clean_set" };
  if (bounces === 1) return { key: "bounces", points: 70,  value: "1", band: "ok",   rationale: "one_bounce" };
  if (bounces === 2) return { key: "bounces", points: 40,  value: "2", band: "bad",  rationale: "two_bounces" };
  return                  { key: "bounces", points: 15,  value: `${bounces}`, band: "bad", rationale: "many_bounces" };
}

function subStability(
  sigmaVsFpm: number | null | undefined,
  sigmaBankDeg: number | null | undefined,
): SubScore | null {
  if (sigmaVsFpm == null && sigmaBankDeg == null) return null;
  const vs = sigmaVsFpm ?? 0;
  const bk = sigmaBankDeg ?? 0;
  const vsBand = vs < 100 ? 100 : vs < 200 ? 80 : vs < 400 ? 50 : vs < 700 ? 25 : 0;
  const bkBand = bk < 2   ? 100 : bk < 5   ? 80 : bk < 10  ? 50 : bk < 15  ? 25 : 0;
  const points = Math.min(vsBand, bkBand);
  const rationale =
    points >= 90 ? "very_stable" :
    points >= 70 ? "stable" :
    points >= 40 ? "average_stability" :
    points >= 20 ? "unstable_approach" : "very_unstable";
  const value = `σ ${Math.round(vs)} fpm / ${bk.toFixed(1)}°`;
  return { key: "stability", points, value, band: band(points), rationale };
}

// FROZEN legacy fallback (Q2, Audit 2026-06-27): the live source of truth is
// the Rust crate's sub_rollout (LDA-/weight-class-based since v0.10.0). These
// absolute-metre thresholds exist ONLY to re-score pre-v0.7.1 PIREPs as they
// were originally scored — do NOT "align" them with the current Rust algorithm
// (that would re-grade old landings). Pinned by landingScoring.golden.test.ts.
function subRollout(rolloutM: number | null | undefined): SubScore | null {
  if (rolloutM == null) return null;
  const m = rolloutM;
  const v = `${Math.round(m)} m`;
  if (m < 800)  return { key: "rollout", points: 100, value: v, band: "good", rationale: "excellent_stop" };
  if (m < 1200) return { key: "rollout", points: 80,  value: v, band: "good", rationale: "good_stop" };
  if (m < 1800) return { key: "rollout", points: 55,  value: v, band: "ok",   rationale: "long_rollout" };
  if (m < 2500) return { key: "rollout", points: 25,  value: v, band: "bad",  rationale: "very_long_rollout" };
  return              { key: "rollout", points: 5,   value: v, band: "bad",  rationale: "marginal_runway" };
}

function subFuel(efficiencyPct: number | null | undefined): SubScore | null {
  if (efficiencyPct == null) return null;
  const dev = Math.abs(efficiencyPct);
  const v = `${efficiencyPct > 0 ? "+" : ""}${efficiencyPct.toFixed(1)}%`;
  if (dev < 2)  return { key: "fuel", points: 100, value: v, band: "good", rationale: "on_plan" };
  if (dev < 5)  return { key: "fuel", points: 80,  value: v, band: "good", rationale: "near_plan" };
  if (dev < 10) return { key: "fuel", points: 55,  value: v, band: "ok",   rationale: "off_plan" };
  if (dev < 20) return { key: "fuel", points: 25,  value: v, band: "bad",  rationale: "very_off_plan" };
  return              { key: "fuel", points: 5,   value: v, band: "bad",  rationale: "way_off_plan" };
}

export function computeSubScores(p: {
  vs_fpm?: number | null;
  peak_g_load?: number | null;
  /** v0.12.3 (LE8): EMA-smoothed scored G. When set, subGForce scores
   *  this value; else it falls back to the raw peak_g_load. */
  scored_g_load?: number | null;
  bounce_count?: number | null;
  approach_vs_stddev_fpm?: number | null;
  approach_bank_stddev_deg?: number | null;
  rollout_distance_m?: number | null;
  fuel_efficiency_pct?: number | null;
}): SubScore[] {
  const out: SubScore[] = [];
  if (p.vs_fpm != null) out.push(subLandingRate(p.vs_fpm));
  // v0.12.3 (LE8): score the EMA-smoothed scored_g_load when present.
  const gForScore = p.scored_g_load ?? p.peak_g_load;
  if (gForScore != null) out.push(subGForce(gForScore));
  out.push(subBounces(p.bounce_count ?? 0));
  const stab = subStability(p.approach_vs_stddev_fpm, p.approach_bank_stddev_deg);
  if (stab) out.push(stab);
  const ro = subRollout(p.rollout_distance_m);
  if (ro) out.push(ro);
  const fu = subFuel(p.fuel_efficiency_pct);
  if (fu) out.push(fu);
  return out;
}

// ─── Master-Score (Mirror von LandingScore::classify in lib.rs) ─────

export type LandingCategory = "smooth" | "acceptable" | "firm" | "hard" | "severe";

const CATEGORY_ORDER: LandingCategory[] = ["smooth", "acceptable", "firm", "hard", "severe"];
const CATEGORY_NUMERIC: Record<LandingCategory, number> = {
  smooth: 100, acceptable: 80, firm: 60, hard: 30, severe: 0,
};

function classifyByVS(peakVsFpm: number): LandingCategory {
  const vs = Math.abs(peakVsFpm);
  if (vs >= T_VS_SEVERE_FPM) return "severe";
  if (vs >= T_VS_HARD_FPM) return "hard";
  if (vs >= T_VS_FIRM_FPM) return "firm";
  if (vs >= T_VS_SMOOTH_FPM) return "acceptable";
  return "smooth";
}

function classifyByG(peakG: number): LandingCategory {
  if (peakG >= T_G_SEVERE) return "severe";
  if (peakG >= T_G_HARD) return "hard";
  if (peakG >= T_G_FIRM) return "firm";
  if (peakG >= T_G_SMOOTH) return "acceptable";
  return "smooth";
}

function worseOf(a: LandingCategory, b: LandingCategory): LandingCategory {
  return CATEGORY_ORDER.indexOf(a) >= CATEGORY_ORDER.indexOf(b) ? a : b;
}

function bumpUp(c: LandingCategory): LandingCategory {
  switch (c) {
    case "smooth": return "acceptable";
    case "acceptable": return "firm";
    case "firm": return "hard";
    case "hard":
    case "severe": return "severe";
  }
}

export function classifyLanding(
  peakVsFpm: number,
  peakG: number | null | undefined,
  bounces: number,
): { category: LandingCategory; numeric: number } {
  const byVs = classifyByVS(peakVsFpm);
  const byG = peakG != null ? classifyByG(peakG) : "smooth";
  let cat = worseOf(byVs, byG);
  if (bounces > 0 && cat !== "severe") cat = bumpUp(cat);
  return { category: cat, numeric: CATEGORY_NUMERIC[cat] };
}

export function aggregateSubScores(subs: SubScore[]): number | null {
  if (subs.length === 0) return null;
  const weights: Record<SubScore["key"], number> = {
    landing_rate: 3,
    g_force: 3,
    bounces: 2,
    stability: 2,
    rollout: 1,
    fuel: 1,
  };
  let sum = 0, wsum = 0;
  for (const s of subs) {
    const w = weights[s.key] ?? 1;
    sum += s.points * w;
    wsum += w;
  }
  return wsum > 0 ? Math.round(sum / wsum) : null;
}
