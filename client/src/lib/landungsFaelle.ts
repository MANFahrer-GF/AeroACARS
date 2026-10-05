// Score-Version 19 (QS 05.10.2026): Gleichheitstest Client ↔ Webapp.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Zwei echte
// Landungen mit dem, was die Bewertung einfriert, und der ERWARTETEN
// Anzeige (Deutsch). Beide Seiten rendern ihre Oberfläche mit diesen Daten
// und prüfen gegen dieselbe Erwartung — weil die Datei byte-gleich ist,
// können die Erwartungen nicht auseinanderlaufen:
//   client/src/lib/landungsFaelle.test.tsx       (LandingDetail)
//   webapp/src/__tests__/landungsFaelle.test.tsx (LandingAnalysis)
//
// QAF434: Teilnoten und Deckel aus der v19-Neubewertung des Flugs.
// GSG1709: Gate-Messwerte und übrige Teilnoten aus dem Flug; Stabilität mit
// dem v19-Gate neu (σ 84 fpm / 0,9° → 100 Punkte, kein Deckel), Gesamtnote
// 99 — festgehalten in landing-scoring (`gsg1709_v19_gesamtnote`).

export interface LandungsFall {
  name: string;
  landing_score: number;
  landing_score_label: string;
  landing_score_grade: string;
  landing_score_deckel: string | null;
  score_algorithm_version: number;
  bounce_count: number;
  sub_scores: Array<{
    key: string;
    label_key: string;
    points: number;
    score: number;
    band: "good" | "ok" | "bad";
    value?: string;
    rationale_key?: string;
    tip_key?: string;
    skipped: boolean;
    warning?: string;
    gate?: Array<{
      key: string;
      stufe: "gut" | "mittel" | "schlecht";
      wert?: number;
      gut_unter?: number;
    }>;
  }>;
  /** Was beide Oberflächen zeigen müssen (de). */
  erwartet: {
    /** Wort zur Gesamtnote (Groß-/Kleinschreibung je Seite verschieden). */
    wort: string;
    /** Deckel-Satz unter der Note, `null` = keiner. */
    deckel: string | null;
    /** Hinweis-Marken in dieser Reihenfolge. */
    marken: string[];
    /** Kopf der Anflug-Karte. */
    pille: string;
    /** „Warum nicht stabil"-Liste der Anflug-Karte. */
    gruende: string[];
  };
}

export const LANDUNGS_FAELLE: readonly LandungsFall[] = [
  {
    name: "QAF434",
    landing_score: 80,
    landing_score_label: "acceptable",
    landing_score_grade: "B",
    landing_score_deckel: "anflug_partial_gesamt",
    score_algorithm_version: 19,
    bounce_count: 0,
    sub_scores: [
      {
        key: "landing_rate",
        label_key: "landing.sub.landing_rate",
        points: 100,
        score: 100,
        band: "good",
        value: "-178 fpm",
        rationale_key: "landing.rat.firm_positive_touchdown",
        tip_key: "landing.tip.firm_positive_touchdown",
        skipped: false,
      },
      {
        key: "g_force",
        label_key: "landing.sub.g_force",
        points: 100,
        score: 100,
        band: "good",
        value: "1.12 G",
        rationale_key: "landing.rat.smooth_g",
        tip_key: "landing.tip.smooth_g",
        skipped: false,
      },
      {
        key: "bounces",
        label_key: "landing.sub.bounces",
        points: 100,
        score: 100,
        band: "good",
        value: "0",
        rationale_key: "landing.rat.clean_set",
        tip_key: "landing.tip.clean_set",
        skipped: false,
      },
      {
        key: "stability",
        label_key: "landing.sub.stability",
        points: 80,
        score: 80,
        band: "good",
        value: "σ 183 fpm / 1.5°",
        rationale_key: "landing.rat.partly_stable",
        tip_key: "landing.tip.partly_stable",
        skipped: false,
        warning: "anflug_partial",
        gate: [
          {
            key: "gleitpfad",
            stufe: "mittel",
            wert: 1.69,
            gut_unter: 1.0,
          },
          {
            key: "fahrt",
            stufe: "gut",
            wert: 0.97,
            gut_unter: 5.0,
          },
          {
            key: "querneigung",
            stufe: "gut",
            wert: 1.51,
            gut_unter: 3.0,
          },
          {
            key: "ruck",
            stufe: "gut",
            wert: 21.42,
            gut_unter: 100.0,
          },
          {
            key: "sinken",
            stufe: "gut",
          },
          {
            key: "konfiguration",
            stufe: "gut",
          },
        ],
      },
      {
        key: "rollout",
        label_key: "landing.sub.runway_discipline",
        points: 100,
        score: 100,
        band: "good",
        value: "3.2 m Versatz · äußeres Rad 9.3 m von der Mitte · Rand +13.1 m",
        rationale_key: "landing.rat.centered",
        tip_key: "landing.tip.centered",
        skipped: false,
      },
      {
        key: "alignment",
        label_key: "landing.sub.alignment",
        points: 100,
        score: 100,
        band: "good",
        value: "4 m · 1.8° (−2.1° XW)",
        rationale_key: "landing.rat.aligned_on_centerline",
        tip_key: "landing.tip.aligned_on_centerline",
        skipped: false,
      },
      {
        key: "touchdown_point",
        label_key: "landing.sub.touchdown_point",
        points: 85,
        score: 85,
        band: "good",
        value: "638 m · Ziel 400 m · Δ +238 m",
        rationale_key: "landing.rat.in_tdz",
        tip_key: "landing.tip.in_tdz",
        skipped: false,
      },
    ],
    erwartet: {
      wort: "gut",
      deckel:
        "Gedeckelt auf höchstens 80 Punkte: Anflug nur teilweise stabil (Stable Gate nicht erreicht). Ein guter Touchdown gleicht das nicht aus.",
      marken: ["ANFLUG TEILWEISE STABIL"],
      pille: "⚠ PARTIAL",
      gruende: ["Gleitpfad: im Schnitt 1,69 Dots daneben (gut unter 1)"],
    },
  },
  {
    name: "GSG1709",
    landing_score: 99,
    landing_score_label: "smooth",
    landing_score_grade: "A+",
    landing_score_deckel: null,
    score_algorithm_version: 19,
    bounce_count: 0,
    sub_scores: [
      {
        key: "landing_rate",
        label_key: "landing.sub.landing_rate",
        points: 100,
        score: 100,
        band: "good",
        value: "-127 fpm",
        rationale_key: "landing.rat.firm_positive_touchdown",
        tip_key: "landing.tip.firm_positive_touchdown",
        skipped: false,
      },
      {
        key: "g_force",
        label_key: "landing.sub.g_force",
        points: 100,
        score: 100,
        band: "good",
        value: "1.11 G",
        rationale_key: "landing.rat.smooth_g",
        tip_key: "landing.tip.smooth_g",
        skipped: false,
      },
      {
        key: "bounces",
        label_key: "landing.sub.bounces",
        points: 100,
        score: 100,
        band: "good",
        value: "0",
        rationale_key: "landing.rat.clean_set",
        tip_key: "landing.tip.clean_set",
        skipped: false,
      },
      {
        key: "stability",
        label_key: "landing.sub.stability",
        points: 100,
        score: 100,
        band: "good",
        value: "σ 84 fpm / 0.9°",
        rationale_key: "landing.rat.very_stable",
        tip_key: "landing.tip.very_stable",
        skipped: false,
        gate: [
          {
            key: "gleitpfad",
            stufe: "gut",
            wert: 0.24,
            gut_unter: 1,
          },
          {
            key: "fahrt",
            stufe: "gut",
            wert: 3.39,
            gut_unter: 5,
          },
          {
            key: "querneigung",
            stufe: "gut",
            wert: 0.87,
            gut_unter: 3,
          },
          {
            key: "ruck",
            stufe: "gut",
            wert: 23.62,
            gut_unter: 100,
          },
          {
            key: "sinken",
            stufe: "gut",
          },
          {
            key: "konfiguration",
            stufe: "gut",
          },
        ],
      },
      {
        key: "rollout",
        label_key: "landing.sub.runway_discipline",
        points: 100,
        score: 100,
        band: "good",
        value: "6.9 m Versatz · äußeres Rad 8.5 m von der Mitte · Rand +21.6 m",
        rationale_key: "landing.rat.centered",
        tip_key: "landing.tip.centered",
        skipped: false,
      },
      {
        key: "alignment",
        label_key: "landing.sub.alignment",
        points: 100,
        score: 100,
        band: "good",
        value: "1 m · 4.5° (−3.6° XW)",
        rationale_key: "landing.rat.aligned_on_centerline",
        tip_key: "landing.tip.aligned_on_centerline",
        skipped: false,
      },
      {
        key: "touchdown_point",
        label_key: "landing.sub.touchdown_point",
        points: 85,
        score: 85,
        band: "good",
        value: "579 m · Ziel 400 m · Δ +179 m",
        rationale_key: "landing.rat.in_tdz",
        tip_key: "landing.tip.in_tdz",
        skipped: false,
      },
    ],
    erwartet: {
      wort: "hervorragend",
      deckel: null,
      marken: [],
      pille: "✓ STABLE GATE",
      gruende: [],
    },
  },
];
