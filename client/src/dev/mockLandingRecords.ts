// Dev-only Mock-LandingRecords für RunwayDiagram-Design-Iteration.
// Aktiviert über das versteckte "Preview"-Tab (nur in `npm run tauri dev`
// sichtbar dank `import.meta.env.DEV`). Im Production-Build ungenutzt.
//
// 4 Varianten decken die wichtigsten visuellen Cases ab:
//   * MS713-Anchor  — OLBA RWY 17, leicht left of CL, Aim −80 m short
//   * Perfect       — TDZ-Treffer, Aim ±0, CL=0
//   * Long Landing  — TDZ verfehlt, Aim +500 m past
//   * DDS-Violation — Touchdown vor displaced threshold (OLBA RWY 35)

import type { LandingRecord } from "../components/LandingPanel";

const NOW_ISO = "2026-05-13T17:42:00Z";

function baseRecord(): LandingRecord {
  return {
    pirep_id: "preview-mock",
    touchdown_at: NOW_ISO,
    recorded_at: NOW_ISO,
    flight_number: "MS713",
    airline_icao: "MSR",
    dpt_airport: "HECA",
    arr_airport: "OLBA",
    touchdown_airport: "OLBA",
    touchdown_airport_source: "runway_match",
    touchdown_distance_to_destination_nm: 0,
    touchdown_nearest_distance_nm: null,
    aircraft_registration: "SU-GCC",
    aircraft_icao: "B738",
    aircraft_title: "Boeing 737-800 PMDG",
    sim_kind: "X-PLANE",

    score_numeric: 82,
    score_label: "smooth",
    grade_letter: "A",

    landing_rate_fpm: -194,
    landing_peak_vs_fpm: -210,
    landing_g_force: 1.32,
    landing_peak_g_force: 1.52,
    landing_pitch_deg: 4.2,
    landing_bank_deg: 0.3,
    landing_speed_kt: 142,
    landing_heading_deg: 172,
    landing_weight_kg: 62500,
    touchdown_sideslip_deg: 0.4,
    bounce_count: 0,

    headwind_kt: 8,
    crosswind_kt: -2,

    approach_vs_stddev_fpm: 65,
    approach_bank_stddev_deg: 2.1,
    rollout_distance_m: 1100,

    planned_block_fuel_kg: 8800,
    planned_burn_kg: 4500,
    planned_tow_kg: 65800,
    planned_ldw_kg: 61300,
    planned_zfw_kg: 56200,
    actual_trip_burn_kg: 4620,
    fuel_efficiency_kg_diff: 120,
    fuel_efficiency_pct: 2.7,
    takeoff_weight_kg: 66000,
    takeoff_fuel_kg: 9100,
    landing_fuel_kg: 4480,
    block_fuel_kg: 8800,

    runway_match: {
      airport_ident: "OLBA",
      runway_ident: "17",
      surface: "ASP",
      length_ft: 10663,
      centerline_distance_m: -6.6,
      centerline_distance_abs_ft: 21.65,
      side: "LEFT",
      touchdown_distance_from_threshold_ft: 1050,
      source: "navigraph",
      nav_cycle: "2604",
      true_course_deg: 176.94,
      displaced_threshold_ft: 0,
      tch_expected_ft: 49,
      glideslope_angle_deg: 3.0,
    },
    touchdown_profile: [],
    approach_samples: [],

    ux_version: 1,
    forensics_version: 2,
    landing_confidence: "high",
    landing_source: "vs_at_impact",
    sub_scores: [],

    runway_geometry_trusted: true,
    runway_geometry_reason: null,

    accident: false,

    // v0.8.0 assessment fields
    td_distance_from_threshold_m: 320,
    td_in_tdz: true,
    td_third: 1,
    td_tdz_length_m: 900,
    aim_delta_m: -80,
    aim_class: "short_of_aim",
    aim_point_m: 400,
    tch_actual_ft: 47,
    tch_delta_ft: -2,
    tch_class: "on_profile",
    pre_displaced_threshold: false,
  };
}

// ─── v1.7.0 Bahndisziplin — die zehn Pflichtvarianten aus Spec §11 ────
//
// Sie sind kein Nebenprodukt, sondern die Stufe, an der die Bandgrenzen aus
// §4 und §5.4 entschieden werden: Am Schreibtisch lässt sich nicht sinnvoll
// festlegen, was 3 m Randabstand gegenüber 15 m kosten soll — man muss es
// nebeneinander sehen.

import ECHTE_SPUREN from "./echteSpuren.json";
import AUSFAHRTEN from "./ausfahrten.json";

/**
 * Die echten Rollspuren aus dem Bestand, nach PIREP.
 *
 * # Warum echte Spuren und keine konstruierten
 *
 * Der erste Entwurf interpolierte zwischen vier Stützstellen. Das ergab
 * Geraden mit Knicken — und damit ein Bild, das die Anzeige besser aussehen
 * liess, als sie ist: Eine echte Spur ist nie gerade. Sie schwankt, sie
 * korrigiert, sie hat Rauschen. Genau daran entscheidet sich, ob das Band in
 * der Queransicht lesbar bleibt.
 *
 * Die Daten stammen aus den Flug-Protokollen auf dem VPS, exportiert mit
 * demselben Messfenster, das der Client fährt (ab dem Aufsetzen, bis 60 kt
 * oder 10° Kursabweichung). Elf bis siebenunddreissig Abtastpunkte je
 * Landung — das ist die Auflösung, die im Feld ankommt.
 */
interface EchteSpur {
  pirep: string;
  punkte: Array<{ laengs_m: number; quer_m: number }>;
  raeum: {
    /** Beginn des Ausschwenkens — Grenze der Bewertung. */
    m: number;
    kt: number | null;
    /** Überschreitung der Bahnkante — hier ist die Bahn geräumt. */
    kante_m?: number;
    seite: "left" | "right";
  } | null;
}
const SPUR_NACH_PIREP: Record<string, EchteSpur> = Object.fromEntries(
  (ECHTE_SPUREN as EchteSpur[]).map((e) => [e.pirep, e]),
);

/**
 * Konstruierte Spuren für die drei Fälle, für die es im Bestand keine
 * Landung gibt: Graspiste, Wasser, sehr kurze Bahn.
 *
 * Sie sind bewusst als das gekennzeichnet, was sie sind. Damit sie sich von
 * den echten nicht unterscheiden lassen, tragen sie deren Auflösung (etwa
 * dreissig Meter Abstand) und deren Rauschen — eine glatte Linie wäre eine
 * Behauptung darüber, wie ruhig ein Flugzeug rollt.
 */
function bastelSpur(
  stuetzen: Array<[laengs: number, quer: number]>,
  rauschen = 0.4,
): Array<{ laengs_m: number; quer_m: number }> {
  const out: Array<{ laengs_m: number; quer_m: number }> = [];
  let z = 12345; // fester Startwert: die Demo muss reproduzierbar sein
  const zufall = () => {
    z = (z * 1103515245 + 12345) % 2147483648;
    return z / 2147483648 - 0.5;
  };
  for (let i = 0; i < stuetzen.length - 1; i++) {
    const [l0, q0] = stuetzen[i]!;
    const [l1, q1] = stuetzen[i + 1]!;
    const schritte = Math.max(1, Math.round((l1 - l0) / 30));
    for (let k = 0; k < schritte; k++) {
      const f = k / schritte;
      // Weiche Überblendung statt Knick an der Stützstelle.
      const g = f * f * (3 - 2 * f);
      out.push({
        laengs_m: Math.round((l0 + (l1 - l0) * f) * 10) / 10,
        quer_m: Math.round((q0 + (q1 - q0) * g + zufall() * rauschen) * 100) / 100,
      });
    }
  }
  const letzte = stuetzen[stuetzen.length - 1]!;
  out.push({ laengs_m: letzte[0], quer_m: letzte[1] });
  return out;
}

/** Graspiste: kurz, schmal, mit mehr seitlichem Spiel. */
function grasSpur() {
  return bastelSpur([[110, -1.2], [260, 2.6], [420, -0.4], [560, -2.1]], 0.7);
}

/** Wasserlandung: kaum Bremsweg, langer Auslauf. */
function wasserSpur() {
  return bastelSpur([[180, 0.8], [420, -1.6], [700, -0.5]], 0.5);
}

/** Kurze Bahn: früh aufgesetzt, zügig geräumt. */
function kurzeBahnSpur() {
  return bastelSpur([[140, 0.5], [330, 3.4], [520, 1.1], [700, -0.6]], 0.35);
}

/** Holt eine echte Spur. Wirft, wenn sie fehlt — eine stumm leere Demo-
 *  Variante wäre schlimmer als ein Fehler beim Bauen. */
function echteSpur(pirep: string): EchteSpur {
  const s = SPUR_NACH_PIREP[pirep];
  if (!s || s.punkte.length < 5) {
    throw new Error(`Keine echte Spur für ${pirep} in echteSpuren.json`);
  }
  return s;
}

/** Setzt die Bahndisziplin-Felder eines Datensatzes in einem Zug. */
function bahn(
  r: LandingRecord,
  o: {
    breite?: number;
    spur?: number;
    spann?: number;
    /** PIREP einer echten Spur aus `echteSpuren.json`. */
    spurVon?: string;
    /** Oder eine eigene Spur, wenn es keine echte gibt (Gras, Wasser). */
    punkte?: Array<{ laengs_m: number; quer_m: number }>;
    belag?: boolean | null;
    /** Nicht mehr gesetzt — der Räumpunkt kommt aus dem Spurende. */
    raeumM?: number | null;
    raeumKt?: number | null;
    raeumSeite?: "left" | "right" | null;
    overrun?: number | null;
  },
): LandingRecord {
  const breite = o.breite ?? 45;
  const spurweite = o.spur ?? null;
  const quelle = o.spurVon ? echteSpur(o.spurVon) : null;
  const punkte = quelle ? quelle.punkte : o.punkte ?? [];
  // Der grösste Versatz zählt nur bis zum Räumpunkt.
  //
  // Danach ist das Flugzeug auf dem Weg zur Ausfahrt, und dort sind
  // vierzig Meter neben der Mittellinie normal, nicht auffällig. Rechnet man
  // die ganze Spur, ist der „grösste Versatz" immer die Ausfahrt selbst —
  // die Marke ② sitzt dann auf der Marke ③, und die Bewertung würde ein
  // reguläres Abrollen als Fehler zählen.
  //
  // Der Client macht es genauso: `bahn_max_querversatz_m` wird nur
  // fortgeschrieben, solange das Messfenster offen ist.
  // Für die Bewertung zählt der Beginn des Ausschwenkens, nicht die Kante.
  const raeumGrenze = quelle?.raeum?.m ?? Infinity;
  const gewertet = punkte.filter((x) => x.laengs_m < raeumGrenze);
  const basis = gewertet.length > 0 ? gewertet : punkte;
  const max =
    basis.length > 0
      ? basis.reduce((a, b) => (Math.abs(b.quer_m) > Math.abs(a.quer_m) ? b : a)).quer_m
      : null;
  r.runway_width_m = breite;
  r.track_width_m = spurweite;
  r.track_width_source = spurweite != null ? "type_table" : null;
  r.wingspan_m = o.spann ?? null;
  r.lateral_samples = punkte;
  r.max_lateral_offset_m = max;
  // Derselbe Ausdruck wie in `bahn_felder` auf der Rust-Seite: halbe
  // Bahnbreite minus das äussere Rad. Weicht die Demo hier ab, zeigt sie
  // etwas anderes als das Produkt.
  r.min_edge_clearance_m =
    max != null && spurweite != null
      ? breite / 2 - (Math.abs(max) + spurweite / 2)
      : null;
  r.surface_paved = o.belag === undefined ? true : o.belag;
  // Der Räumpunkt ist das ENDE der Spur, nicht ein freier Wert.
  //
  // Dort, wo das Messfenster zuging, hat das Flugzeug die Bahn verlassen —
  // beides ist derselbe Moment. Ein erfundener Räumpunkt hinter dem letzten
  // Spurpunkt erzeugt in der Grafik eine Marke im Nichts: Die Spur endet auf
  // der Mittellinie, und dreihundert Meter weiter sitzt „Bahn geräumt" an der
  // Kante, ohne dass irgendetwas dazwischen liegt. Das Flugzeug wäre dorthin
  // gesprungen.
  // Bei echten Spuren stammt er aus der Messung: die letzte Überschreitung
  // der Bahnkante, nach der das Flugzeug draussen bleibt. Bei konstruierten
  // Spuren (Gras, Wasser, kurze Bahn) aus den Angaben der Variante.
  if (quelle) {
    // „Bahn geräumt" ist die KANTE, nicht der Beginn des Ausschwenkens.
    // Beides in ein Feld zu legen war der Fehler: Die Spur wurde dann schon
    // mitten auf der Bahn gestrichelt gezeichnet, weil das Ausschwenken
    // dort begann — und eine gestrichelte Linie auf der Bahn ist nicht zu
    // erklären.
    r.clearance_point_m = quelle.raeum?.kante_m ?? quelle.raeum?.m ?? null;
    r.scoring_cutoff_m = quelle.raeum?.m ?? null;
    r.clearance_speed_kt = quelle.raeum?.kt ?? null;
    r.clearance_side = quelle.raeum?.seite ?? null;
  } else {
    const letzter = punkte.length ? punkte[punkte.length - 1]!.laengs_m : null;
    r.clearance_point_m = o.raeumSeite != null ? letzter : null;
    r.clearance_speed_kt = o.raeumSeite != null ? (o.raeumKt ?? null) : null;
    r.clearance_side = o.raeumSeite ?? null;
  }
  r.overrun_m = o.overrun ?? null;
  // Ausfahrten aus der OSM-Bodenkarte, nach Platz und Bahn. Sie machen die
  // Bewertung nachvollziehbar: Man sieht, welche Ausfahrt vor der genutzten
  // lag und wie weit davor.
  const bahnSchluessel = `${r.runway_match!.airport_ident}/${r.runway_match!.runway_ident}`;
  r.runway_exits =
    (AUSFAHRTEN as Record<string, Array<{ name: string; laengs_m: number; seite: "left" | "right" }>>)[
      bahnSchluessel
    ] ?? null;
  // Marke ① muss auf dem Band sitzen. Im Protokoll liegt der erste
  // Bodenpunkt bis zu zweihundert Meter hinter dem erkannten Aufsetzer
  // (die Positionen kommen mit rund einem Hertz, der Aufsetzer aus dem
  // 50-Hz-Sampler). Im Client faengt die Aufzeichnung dagegen mit der
  // Landephase an -- fuer die Demo wird das hier nachgezogen.
  if (punkte.length > 0) {
    r.td_distance_from_threshold_m = punkte[0]!.laengs_m;
    r.runway_match!.touchdown_distance_from_threshold_ft =
      punkte[0]!.laengs_m / 0.3048;
    r.runway_match!.centerline_distance_m = punkte[0]!.quer_m;
  }
  return r;
}

export type MockKey =
  | "ms713"
  | "perfect"
  | "long_landing"
  | "dds_violation"
  | "ourairports_fallback"
  | "pre_v080"
  | "d_mittig"
  | "d_kante"
  | "d_daneben"
  | "d_overrun"
  | "d_gras"
  | "d_ohne_spurweite"
  | "d_wasser"
  | "d_kurze_bahn";

export interface MockOption {
  key: MockKey;
  label: string;
  hint: string;
  build: () => LandingRecord;
}

export const MOCK_LANDING_OPTIONS: MockOption[] = [
  {
    key: "ms713",
    label: "MS713 (OLBA 17, 6.6 m left, aim short −80 m)",
    hint: "Real-Anchor aus dem v0.8.0-Bug-Report. Navigraph-Source, TDZ-Treffer, Aim leicht zu kurz.",
    build: baseRecord,
  },
  {
    key: "perfect",
    label: "Perfect Landing (OLBA 17, on centerline, aim ±0)",
    hint: "Idealwerte: CL=0 m, Aim exact, TCH on profile, TDZ-Treffer.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.centerline_distance_m = 0.2;
      r.runway_match!.centerline_distance_abs_ft = 0.66;
      r.runway_match!.side = "CENTER";
      r.runway_match!.touchdown_distance_from_threshold_ft = 1312;
      r.td_distance_from_threshold_m = 400;
      r.aim_delta_m = 0;
      r.aim_class = "perfect";
      r.tch_actual_ft = 49;
      r.tch_delta_ft = 0;
      r.score_numeric = 96;
      r.score_label = "smooth";
      r.grade_letter = "A";
      return r;
    },
  },
  {
    key: "long_landing",
    label: "Long Landing (OLBA 17, +500 m past aim, outside TDZ)",
    hint: "Pilot setzt erst bei 900 m past threshold auf — TDZ verfehlt, Aim +500 m → Long-Landing-Pill.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.centerline_distance_m = 3.5;
      r.runway_match!.centerline_distance_abs_ft = 11.48;
      r.runway_match!.side = "RIGHT";
      r.runway_match!.touchdown_distance_from_threshold_ft = 2953;
      r.td_distance_from_threshold_m = 900;
      r.td_in_tdz = false;
      r.td_third = 2;
      r.aim_delta_m = 500;
      r.aim_class = "long_landing";
      r.tch_actual_ft = 75;
      r.tch_delta_ft = 26;
      r.tch_class = "high";
      r.score_numeric = 58;
      r.score_label = "acceptable";
      r.grade_letter = "C";
      return r;
    },
  },
  {
    key: "ourairports_fallback",
    label: "OurAirports-Fallback (VPS nicht erreichbar — orange Warnung)",
    hint: "Source=ourairports_fallback, TCH/DDS null. Diagram zeigt Fallback-Warnhinweis im Header + Datenquellen-Card.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.source = "ourairports_fallback";
      r.runway_match!.nav_cycle = null;
      r.runway_match!.displaced_threshold_ft = null;
      r.runway_match!.tch_expected_ft = null;
      r.tch_actual_ft = null;
      r.tch_delta_ft = null;
      r.tch_class = null;
      r.pre_displaced_threshold = null;
      return r;
    },
  },
  {
    key: "pre_v080",
    label: "Pre-v0.8.0 Legacy (alle v0.8.0-Felder null — graceful degrade)",
    hint: "Alter PIREP von vor v0.8.0. Keine TDZ, kein Aim, keine TCH-Card — aber Basis-Geometrie + TD bleibt sichtbar.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.source = null;
      r.runway_match!.nav_cycle = null;
      r.runway_match!.displaced_threshold_ft = null;
      r.runway_match!.tch_expected_ft = null;
      r.td_distance_from_threshold_m = null;
      r.runway_match!.touchdown_distance_from_threshold_ft = 0;
      r.td_in_tdz = null;
      r.td_third = null;
      r.td_tdz_length_m = null;
      r.aim_delta_m = null;
      r.aim_class = null;
      r.aim_point_m = null;
      r.tch_actual_ft = null;
      r.tch_delta_ft = null;
      r.tch_class = null;
      r.pre_displaced_threshold = null;
      return r;
    },
  },
  {
    key: "dds_violation",
    label: "DDS Violation (OLBA 35, touchdown vor displaced threshold)",
    hint: "OLBA RWY 35 hat 2690 ft (820 m) displaced. Pilot setzt 50 m VOR Landing-Threshold auf → illegal.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.runway_ident = "35";
      r.runway_match!.length_ft = 10663;
      r.runway_match!.centerline_distance_m = -1.2;
      r.runway_match!.centerline_distance_abs_ft = 3.94;
      r.runway_match!.side = "LEFT";
      r.runway_match!.touchdown_distance_from_threshold_ft = -164; // ~ -50 m
      r.runway_match!.displaced_threshold_ft = 2690;
      r.runway_match!.true_course_deg = 356.94;
      r.runway_match!.tch_expected_ft = 50;
      r.td_distance_from_threshold_m = -50;
      r.td_in_tdz = false;
      r.td_third = 1;
      r.aim_delta_m = -450;
      r.aim_class = "severe";
      r.tch_actual_ft = 18;
      r.tch_delta_ft = -32;
      r.tch_class = "below_profile";
      r.pre_displaced_threshold = true;
      r.score_numeric = 32;
      r.score_label = "hard";
      r.grade_letter = "F";
      return r;
    },
  },

  // ─── v1.7.0 Bahndisziplin — Spec §11 ────────────────────────────────
  //
  // Die Nummern entsprechen der Liste in der Spezifikation. Fünf und neun
  // decken die vorhandenen Varianten `dds_violation` und `pre_v080` ab.
  {
    key: "d_mittig",
    label: "① Mittig, in der Aufsetzzone — EDDH 23, Fenix A319",
    hint: "Echte Spur (a3V0DXnWr6054VO6, 37 Messpunkte): Der Normalfall. Die Spur bleibt über den ganzen Rollweg innerhalb von 3 m um die Mittellinie — 100 Punkte auf der Disziplin-Achse.",
    build: () =>
      bahn(rwyEDDH(baseRecord()), {
        breite: 46,
        spur: 7.59,
        spann: 35.8,
        spurVon: "a3V0DXnWr6054VO6",
        raeumM: 1795,
        raeumKt: 58,
        raeumSeite: "left",
      }),
  },
  {
    key: "d_kante",
    label: "② Deutlich aussermittig — EDDH 05, A220-300",
    hint: "Echte Spur (0Ab3v9EvNN1LKZ8z, 27 Messpunkte): wandert bis 13,4 m nach rechts und kommt zurück. Das äussere Rad bleibt gut 5 m von der Kante — noch kein Fehler, aber sichtbar aussermittig.",
    build: () =>
      // Die Bahn muss zur Spur passen: Diese Landung fand auf der 05 statt,
      // nicht auf der 23. Sonst zeigt die Ausfahrtenliste die Rollwege der
      // Gegenrichtung -- und die Laengspositionen waeren gespiegelt.
      bahn(rwyEDDH(baseRecord(), "05"), {
        breite: 46,
        spur: 6.0,
        spann: 35.1,
        spurVon: "0Ab3v9EvNN1LKZ8z",
        raeumM: 1830,
        raeumKt: 61,
        raeumSeite: "right",
      }),
  },
  {
    key: "d_daneben",
    label: "③ Rad neben der befestigten Bahn — EDDH 23, Fenix A320",
    hint: "Echte Spur (raKOnJD1XgNbP06q, 23.07., 20 Messpunkte): 26,9 m Versatz auf einer 46-m-Bahn, äusseres Rad 7,6 m im Gras. 20 Punkte. Die Bahngeometrie wurde am 23.08. gegen OSM gegengeprüft — der Versatz ist echt.",
    build: () =>
      bahn(rwyEDDH(baseRecord()), {
        breite: 46,
        spur: 7.59,
        spann: 35.8,
        spurVon: "raKOnJD1XgNbP06q",
        raeumM: null,
      }),
  },
  {
    key: "d_overrun",
    label: "④ Über das Bahnende hinaus — EDDL 05R, A321",
    hint: "Echte Spur (zR4a18JGxVKZ84de, 21 Messpunkte), der Überroll-Wert ist konstruiert: Im ganzen Bestand von 802 Landungen ist niemand über das Bahnende geschossen. 0 Punkte, unabhängig von allem Seitlichen — die Prüfung läuft VOR den seitlichen Regeln.",
    build: () =>
      bahn(rwyEDDL05R(baseRecord()), {
        breite: 45,
        spur: 7.59,
        spann: 35.8,
        spurVon: "zR4a18JGxVKZ84de",
        overrun: 84,
      }),
  },
  {
    key: "d_gras",
    label: "⑥ Graspiste — seitliche Bewertung ausgesetzt (EDXF, C172)",
    hint: "Konstruiert — im Bestand gibt es keine Graslandung. Auf Gras ist der Rand fliessend, die Queransicht entfällt sichtbar mit Grund. Aufsetzpunkt und Bahnende werden weiter bewertet.",
    build: () => {
      const r = bahn(rwyKlein(baseRecord(), "EDXF", "08", 2296, 30), {
        breite: 30,
        spur: 2.5,
        spann: 11.0,
        punkte: grasSpur(),
        belag: false,
      });
      r.runway_match!.surface = "GRS";
      r.aircraft_icao = "C172";
      r.aircraft_title = "Cessna 172 Skyhawk";
      return r;
    },
  },
  {
    key: "d_ohne_spurweite",
    label: "⑦ Spurweite unbekannt — Verzicht sichtbar (EDDH 23)",
    hint: "Echte Spur (y75RLelRGWq7ogA3), aber ein Muster ohne Eintrag in der Typtabelle. Ohne Spurweite lässt sich die Lage der Räder nicht bestimmen — der Verzicht steht da, statt eines geratenen Werts.",
    build: () => {
      const r = bahn(rwyEDDH(baseRecord()), {
        breite: 46,
        spur: undefined,
        spurVon: "y75RLelRGWq7ogA3",
      });
      r.aircraft_icao = "ZZZZ";
      r.aircraft_title = "Ein Muster ohne Eintrag";
      return r;
    },
  },
  {
    key: "d_wasser",
    label: "⑧ Wasserlandung — keine Bahn, keine Kante",
    hint: "Konstruiert — im Bestand gibt es keine Wasserlandung. Weder befestigte Fläche noch Kante: Alles Seitliche entfällt, die Landung wird trotzdem bewertet.",
    build: () => {
      const r = bahn(rwyKlein(baseRecord(), "FA12", "18W", 3000, 60), {
        breite: 60,
        spur: 3.3,
        spann: 14.6,
        punkte: wasserSpur(),
        belag: false,
      });
      r.runway_match!.surface = "WATER";
      r.aircraft_icao = "DHC2";
      r.aircraft_title = "DHC-2 Beaver Amphibian";
      return r;
    },
  },
  {
    key: "d_kurze_bahn",
    label: "⑩ Sehr kurze Bahn — Aufsetzzone = erstes Drittel (EDXB 26, C208)",
    hint: "Konstruiert, mit der Auflösung echter Daten. Unter 1200 m gibt es keine Aufsetzzone nach Annex 14 — die Zone wird zum ersten Drittel, der Zielpunkt rückt von 400 m auf 300 m.",
    build: () => {
      const r = bahn(rwyKlein(baseRecord(), "EDXB", "26", 900, 23), {
        breite: 23,
        spur: 3.6,
        spann: 15.88,
        punkte: kurzeBahnSpur(),
        raeumM: 700,
        raeumKt: 30,
        raeumSeite: "right",
      });
      r.aircraft_icao = "C208";
      r.aircraft_title = "Cessna 208B Grand Caravan";
      r.td_distance_from_threshold_m = 140;
      r.td_in_tdz = true;
      r.td_third = 1;
      r.aim_point_m = 300;
      r.aim_delta_m = -160;
      return r;
    },
  },
];

// ─── Bahn-Vorlagen für die Disziplin-Varianten ────────────────────────

/** EDDH — die Bahn aus der Gegenprobe: 46 m breit, versetzte Schwelle. */
function rwyEDDH(r: LandingRecord, richtung: "23" | "05" = "23"): LandingRecord {
  r.runway_match!.airport_ident = "EDDH";
  r.runway_match!.runway_ident = richtung;
  r.runway_match!.surface = "ASP";
  r.runway_match!.length_ft = 10663;
  // Versetzte Schwelle — genau die Werte, die die Gegenprobe am 23.08.2026
  // gegen OSM und OurAirports bestätigt hat: 512 ft für die 23, 978 ft für
  // die 05. Zusammen 454 m, und genau um diesen Betrag ist die
  // Navigraph-Geometrie kürzer als die Bahnlänge.
  r.runway_match!.displaced_threshold_ft = richtung === "23" ? 512 : 978;
  r.runway_match!.true_course_deg = richtung === "23" ? 230.21 : 50.2;
  r.arr_airport = "EDDH";
  r.touchdown_airport = "EDDH";
  r.aircraft_icao = "A321";
  r.aircraft_title = "FenixA321 CFM SL SC";
  return r;
}

/** EDDL 05R — 2997 m, 45 m breit, 300 m versetzte Schwelle. */
function rwyEDDL05R(r: LandingRecord): LandingRecord {
  r.runway_match!.airport_ident = "EDDL";
  r.runway_match!.runway_ident = "05R";
  r.runway_match!.surface = "CON";
  r.runway_match!.length_ft = 9833;
  r.runway_match!.displaced_threshold_ft = 984;
  r.runway_match!.true_course_deg = 52.5;
  r.arr_airport = "EDDL";
  r.touchdown_airport = "EDDL";
  r.aircraft_icao = "A321";
  r.aircraft_title = "FenixA321 IAE WF SC";
  return r;
}

/** Eine kleine Bahn mit frei wählbarer Länge und Breite. */
function rwyKlein(
  r: LandingRecord,
  icao: string,
  ident: string,
  laengeM: number,
  breiteM: number,
): LandingRecord {
  r.runway_match!.airport_ident = icao;
  r.runway_match!.runway_ident = ident;
  r.runway_match!.length_ft = Math.round(laengeM / 0.3048);
  r.runway_match!.displaced_threshold_ft = 0;
  r.arr_airport = icao;
  r.touchdown_airport = icao;
  void breiteM; // die Breite steht in `runway_width_m`, nicht im Match
  return r;
}
