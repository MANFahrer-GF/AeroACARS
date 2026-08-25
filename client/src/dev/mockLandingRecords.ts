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

/**
 * Graspiste: kurz, schmal, mit mehr seitlichem Spiel — und am Ende
 * heruntergerollt.
 *
 * Bei 30 m Bahnbreite liegt die Kante bei 15 m. Die Spur läuft am Schluss
 * darüber hinaus, sonst behauptet die Marke „Bahn geräumt" etwas, das im
 * Bild nicht zu sehen ist.
 */
function grasSpur() {
  return bastelSpur(
    [[110, -1.2], [260, 2.6], [420, -0.4], [520, -4.0], [575, -16.0], [600, -34.0]],
    0.7,
  );
}

/**
 * Wasserlandung: kaum Bremsweg, langer Auslauf, dann seitlich zum Steg.
 *
 * Auch hier läuft die Spur am Ende über die gedachte Kante — ein
 * Wasserlandeplatz hat zwar keine befestigte Fläche, aber ein Ende hat der
 * Auslauf trotzdem, und die Anzeige muss zeigen, wohin.
 */
function wasserSpur() {
  return bastelSpur(
    [[180, 0.8], [420, -1.6], [700, -0.5], [820, -8.0], [900, -26.0], [960, -44.0]],
    0.5,
  );
}

/**
 * Überrollen: spät aufgesetzt auf kurzer Bahn, die Spur läuft über das
 * Bahnende hinaus.
 *
 * Die 1700-m-Bahn ist bei 1700 zu Ende; die Spur geht bis 1784 — das sind
 * die 84 m, die als `overrun_m` gemeldet werden. Beides muss
 * zusammenpassen, sonst zeigt das Bild etwas anderes als die Zahl daneben.
 */
function overrunSpur() {
  return bastelSpur(
    [
      [780, 0.8],
      [1100, -2.4],
      [1400, -1.0],
      [1700, 1.6],
      [1784, 2.2],
    ],
    0.3,
  );
}

/**
 * Kurze Bahn: früh aufgesetzt, dann nach rechts von der Bahn gerollt.
 *
 * Die alte Fassung endete bei 700 m mit 0,6 m Versatz — mitten auf der
 * Bahn — und behauptete trotzdem „Bahn geräumt · rechts". Die Marke stand
 * dadurch im Nichts, und man konnte nicht sehen, wie das Flugzeug dorthin
 * gekommen sein soll.
 *
 * Bei 23 m Bahnbreite liegt die Kante bei 11,5 m; die Spur läuft bis 31 m.
 */
function kurzeBahnSpur() {
  return bastelSpur(
    [[140, 0.5], [330, 3.4], [520, 1.1], [640, 4.0], [700, 13.0], [740, 31.0]],
    0.35,
  );
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
    /**
     * ICAO-Typ, wenn er von dem der Bahn-Vorlage abweicht.
     *
     * Muss zur Spurweite passen: Ein Kopf, der „A321 · Spurweite 6,0 m"
     * zeigt, widerspricht sich selbst — 6,0 m ist der A220-Wert.
     */
    icao?: string;
    /** Anzeigename des Musters. */
    titel?: string;
    raeumKt?: number | null;
    raeumSeite?: "left" | "right" | null;
    overrun?: number | null;
  },
): LandingRecord {
  const breite = o.breite ?? 45;
  const spurweite = o.spur ?? null;
  const quelle = o.spurVon ? echteSpur(o.spurVon) : null;
  const punkte = quelle ? quelle.punkte : o.punkte ?? [];
  // ── Reihenfolge: erst die Grenzen, dann der Versatz ────────────────
  //
  // Der grösste Versatz zählt nur bis zum Beginn des Ausschwenkens. Wer
  // ihn vorher rechnet, bekommt bei konstruierten Spuren den Wert der
  // Ausfahrt — vierzig Meter neben der Mittellinie, wo das normal ist.
  const halbeBahn = breite / 2;
  let kante: { laengs_m: number; quer_m: number } | null = null;
  for (let i = 1; i < punkte.length; i++) {
    if (
      Math.abs(punkte[i]!.quer_m) > halbeBahn &&
      Math.abs(punkte[i - 1]!.quer_m) <= halbeBahn &&
      punkte.slice(i).every((x) => Math.abs(x.quer_m) > halbeBahn)
    ) {
      kante = punkte[i]!;
      break;
    }
  }
  let cutoff: number | null = null;
  if (kante) {
    let j = punkte.indexOf(kante);
    while (j > 0 && Math.abs(punkte[j - 1]!.quer_m) < Math.abs(punkte[j]!.quer_m)) {
      j--;
    }
    cutoff = punkte[j]!.laengs_m;
  }
  // Bei echten Spuren stammen beide Werte aus der Messung im Export.
  const kanteM = quelle ? (quelle.raeum?.kante_m ?? null) : (kante?.laengs_m ?? null);
  const cutoffM = quelle ? (quelle.raeum?.m ?? null) : cutoff;

  const gewertet = punkte.filter((x) => cutoffM == null || x.laengs_m < cutoffM);
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
  r.overrun_m = o.overrun ?? null;
  if (o.icao) r.aircraft_icao = o.icao;
  if (o.titel) r.aircraft_title = o.titel;
  // Räumpunkt und Bewertungsgrenze — beide oben bestimmt, hier gesetzt.
  //
  // „Bahn geräumt" ist die KANTE. Die Bewertungsgrenze liegt davor, beim
  // Beginn des Ausschwenkens. Beides in ein Feld zu legen war der Fehler,
  // der die Spur schon mitten auf der Bahn gestrichelt zeichnete.
  r.clearance_point_m = kanteM;
  r.scoring_cutoff_m = cutoffM;
  r.clearance_side =
    (quelle?.raeum?.seite ?? (kante ? (kante.quer_m > 0 ? "right" : "left") : null)) ?? null;
  // Die Fahrt gehört zu der Stelle, an der sie gemessen wurde.
  //
  // `raeum.kt` ist die Geschwindigkeit beim KURSWECHSEL (`raeum.m`), nicht
  // an der Kante. Liegen beide auseinander, gehört sie nicht an
  // `clearance_point_m` — die Spur trägt für die neue Stelle keine
  // Geschwindigkeit.
  //
  // Genau derselbe Fehler war im Client schon behoben (`bahn_felder`,
  // 25-m-Toleranz) und stand hier noch. Ein Beispiel aus den echten
  // Daten: Räumpunkt 1264,7 m bei 57,9 kt, Kante bei 1901,0 m — dort ist
  // das Flugzeug längst langsamer.
  const KANTE_TOLERANZ_M = 25;
  const raeumM = quelle?.raeum?.m ?? null;
  const gemesseneFahrt = quelle?.raeum?.kt ?? o.raeumKt ?? null;
  r.clearance_speed_kt =
    kanteM != null &&
    gemesseneFahrt != null &&
    raeumM != null &&
    Math.abs(kanteM - raeumM) < KANTE_TOLERANZ_M
      ? gemesseneFahrt
      : null;

  // ── Aufsetzzone und Zielpunkt nach denselben Regeln wie der Client ──
  //
  // Sonst trägt jede Variante die Werte der Vorlage weiter, und die passen
  // nur zu deren Bahn. Aufgefallen an ⑩: „AUFSETZZONE (TDZ) 900 m" auf
  // einer 900-m-Bahn — die Zone wäre so lang wie die ganze Bahn gewesen.
  //
  // Die Regeln stehen in `runway_assessment` (ICAO Annex 14):
  //   * Aufsetzzone = min(900 m, Länge / 3); unter 1200 m Bahnlänge gibt
  //     es GAR KEINE Markierung.
  //   * Zielpunkt 400 m ab 2400 m Bahnlänge, sonst 300 m (FAA AIM 8-9-1).
  const lda = (r.runway_match!.length_ft - (r.runway_match!.displaced_threshold_ft ?? 0)) * 0.3048;
  r.td_tdz_length_m = lda >= 1200 ? Math.min(900, lda / 3) : null;
  r.aim_point_m = lda >= 2400 ? 400 : 300;
  r.td_in_tdz =
    r.td_tdz_length_m != null
      ? r.td_distance_from_threshold_m! > 0 &&
        r.td_distance_from_threshold_m! <= r.td_tdz_length_m
      : null;
  r.td_third = (Math.min(3, Math.floor((r.td_distance_from_threshold_m! / lda) * 3) + 1) ||
    1) as 1 | 2 | 3;
  r.aim_delta_m = Math.round(r.td_distance_from_threshold_m! - r.aim_point_m);
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
  | "dlh369"
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

/**
 * Der Grund, aus dem die Bewertung die seitliche Lage verwirft.
 *
 * Im Betrieb kommt er aus den `sub_scores` — die Achse hat entschieden.
 * Die Demo hat keine Bewertung, also wird er hier aus denselben
 * Bedingungen abgeleitet, die `sub_bahndisziplin` prüft.
 *
 * # Warum am Ende und nicht in `bahn()`
 *
 * Der Belag wird von manchen Varianten NACH dem Aufbau gesetzt
 * (`r.runway_match.surface = "WATER"`). Eine Ableitung mitten im Aufbau
 * sah deshalb noch „GRS" statt „WATER" und vergab `unpaved_runway` an
 * eine Wasserlandung.
 *
 * Deshalb läuft sie hier, nach jedem `build()` — und zwar über den
 * Wrapper unten, damit keine Variante sie vergessen kann.
 *
 * # Für Tests, die Rohwerte ändern
 *
 * Wer nach dem `build()` an `surface_paved` oder `track_width_m` dreht,
 * muss sie erneut aufrufen — im Betrieb läuft die Bewertung ja auch nach
 * den Daten. Sonst zeigt die Anzeige den Grund vom Ausgangszustand.
 */
export function skipGrundAbleiten(r: LandingRecord): LandingRecord {
  // Wasser am BELAG-Code erkennen, nicht an der Bahnbreite: Ein
  // Wasserlandeplatz hat durchaus eine. `belag_aus_angabe` prüft
  // dasselbe Präfix.
  // Ein Datensatz ohne v1.7.0-Daten wurde nie mit dieser Achse bewertet —
  // für ihn gibt es keinen Grund, sondern gar keine Bewertung. Ohne diese
  // Schranke bekam ein alter Flug „surface_unknown" verpasst, und die
  // Anzeige nannte das statt „für diesen Flug nicht erfasst".
  const traegtBahndaten =
    r.runway_width_m != null ||
    r.track_width_m != null ||
    r.clearance_point_m != null ||
    (r.lateral_samples?.length ?? 0) > 0;
  if (!traegtBahndaten) {
    r.lateral_skip_reason = null;
    return r;
  }

  const belag = (r.runway_match?.surface ?? "").toUpperCase();
  const istWasser = belag.startsWith("WAT");
  // Reihenfolge wie in der Achse: Belag vor Spurweite.
  r.lateral_skip_reason =
    r.surface_paved === false
      ? istWasser
        ? "water_runway"
        : "unpaved_runway"
      : r.surface_paved == null
        ? "surface_unknown"
        : r.track_width_m == null
          ? "track_width_unknown"
          : null;
  return r;
}

const ROH_OPTIONEN: MockOption[] = [
  {
    key: "dlh369",
    label: "DLH369 (EDDM 26L, A220, Ausfahrt B6) — ECHTE Messwerte",
    hint:
      "Thomas' Befund vom 25.08.2026: Die Marke „Größter Versatz\" stand " +
      "bei 12,9 m / 1.646 m, während die Spur noch geradeaus bis 15,2 m " +
      "bei 1.907 m weiterlief — das Messfenster schliesst unter 60 kt. " +
      "Ausserdem sieht die Ausfahrt bei 16,6facher Überhöhung wie ein " +
      "80-Grad-Knick aus; gemessen sind es 19,4°, B6 selbst hat 23,7°.",
    build: () => {
      const r = baseRecord();
      r.runway_match!.airport_ident = "EDDM";
      r.runway_match!.runway_ident = "26L";
      r.runway_match!.surface = "ASP";
      r.runway_match!.length_ft = 13123;
      r.runway_match!.displaced_threshold_ft = 0;
      r.arr_airport = "EDDM";
      r.touchdown_airport = "EDDM";
      // ⚠ Der Höchstwert wird HIER gesetzt, nicht aus der Spur gerechnet.
      //
      // `bahn()` bildet ihn sonst aus den Punkten bis zum Räumpunkt und
      // käme auf 15,2 m bei 1.907 m. Der echte Client meldet 12,9 m bei
      // 1.646 m, weil sein Messfenster unter 60 kt schliesst — genau
      // diesen Unterschied soll die Vorschau ZEIGEN, nicht glattbügeln.
      // Wo das Messfenster schloss — HERGELEITET, nicht gegriffen.
      //
      // Die alte Meldung führt die Zahl noch nicht (dafür ist das Feld
      // neu). Sie lässt sich aber aus der Spur einklemmen: bis 1.656,5 m
      // bleibt die Spur unter dem gemeldeten Höchstwert von 12,881 m,
      // bei 1.668,0 m steht sie auf 13,0 m. Hätte das Fenster dort noch
      // offen gestanden, hätte der Client 13,0 gemeldet statt 12,881.
      // Der Schluss lag also in diesen 11,5 Metern; 1.662 ist die Mitte.
      const messEnde = 1662.0;
      const rec = bahn(r, {
        breite: 60,
        spur: 6,
        spann: 35.1,
        icao: "BCS3",
        titel: "BCS3",
        raeumKt: 48.8,
        raeumSeite: "right",
        punkte: [
        { laengs_m: 699.0, quer_m: -3.7 },
        { laengs_m: 711.9, quer_m: -3.8 },
        { laengs_m: 723.6, quer_m: -3.9 },
        { laengs_m: 733.6, quer_m: -4.0 },
        { laengs_m: 744.5, quer_m: -4.1 },
        { laengs_m: 754.8, quer_m: -4.2 },
        { laengs_m: 766.0, quer_m: -4.3 },
        { laengs_m: 776.9, quer_m: -4.4 },
        { laengs_m: 787.4, quer_m: -4.5 },
        { laengs_m: 800.0, quer_m: -4.7 },
        { laengs_m: 810.6, quer_m: -4.8 },
        { laengs_m: 821.5, quer_m: -4.9 },
        { laengs_m: 835.8, quer_m: -5.0 },
        { laengs_m: 848.4, quer_m: -5.2 },
        { laengs_m: 859.5, quer_m: -5.3 },
        { laengs_m: 872.8, quer_m: -5.4 },
        { laengs_m: 886.4, quer_m: -5.5 },
        { laengs_m: 898.9, quer_m: -5.7 },
        { laengs_m: 913.0, quer_m: -5.8 },
        { laengs_m: 924.6, quer_m: -5.9 },
        { laengs_m: 935.2, quer_m: -6.0 },
        { laengs_m: 945.3, quer_m: -6.1 },
        { laengs_m: 958.0, quer_m: -6.3 },
        { laengs_m: 968.2, quer_m: -6.4 },
        { laengs_m: 980.4, quer_m: -6.5 },
        { laengs_m: 993.0, quer_m: -6.6 },
        { laengs_m: 1003.2, quer_m: -6.7 },
        { laengs_m: 1015.6, quer_m: -6.8 },
        { laengs_m: 1026.3, quer_m: -6.9 },
        { laengs_m: 1039.1, quer_m: -7.0 },
        { laengs_m: 1050.8, quer_m: -7.1 },
        { laengs_m: 1063.6, quer_m: -7.3 },
        { laengs_m: 1076.5, quer_m: -7.4 },
        { laengs_m: 1087.7, quer_m: -7.5 },
        { laengs_m: 1100.2, quer_m: -7.6 },
        { laengs_m: 1111.9, quer_m: -7.7 },
        { laengs_m: 1124.2, quer_m: -7.8 },
        { laengs_m: 1135.5, quer_m: -7.9 },
        { laengs_m: 1147.6, quer_m: -8.1 },
        { laengs_m: 1159.3, quer_m: -8.2 },
        { laengs_m: 1171.4, quer_m: -8.3 },
        { laengs_m: 1182.8, quer_m: -8.4 },
        { laengs_m: 1194.7, quer_m: -8.5 },
        { laengs_m: 1206.3, quer_m: -8.6 },
        { laengs_m: 1217.0, quer_m: -8.7 },
        { laengs_m: 1228.2, quer_m: -8.8 },
        { laengs_m: 1239.2, quer_m: -8.9 },
        { laengs_m: 1250.8, quer_m: -9.0 },
        { laengs_m: 1261.1, quer_m: -9.1 },
        { laengs_m: 1271.9, quer_m: -9.2 },
        { laengs_m: 1283.5, quer_m: -9.3 },
        { laengs_m: 1295.9, quer_m: -9.5 },
        { laengs_m: 1306.8, quer_m: -9.6 },
        { laengs_m: 1317.5, quer_m: -9.7 },
        { laengs_m: 1328.4, quer_m: -9.8 },
        { laengs_m: 1340.5, quer_m: -9.9 },
        { laengs_m: 1351.1, quer_m: -10.0 },
        { laengs_m: 1361.5, quer_m: -10.1 },
        { laengs_m: 1372.5, quer_m: -10.2 },
        { laengs_m: 1383.1, quer_m: -10.3 },
        { laengs_m: 1395.2, quer_m: -10.4 },
        { laengs_m: 1407.0, quer_m: -10.5 },
        { laengs_m: 1417.4, quer_m: -10.6 },
        { laengs_m: 1427.7, quer_m: -10.7 },
        { laengs_m: 1439.5, quer_m: -10.8 },
        { laengs_m: 1451.5, quer_m: -10.9 },
        { laengs_m: 1461.9, quer_m: -11.0 },
        { laengs_m: 1473.6, quer_m: -11.1 },
        { laengs_m: 1485.3, quer_m: -11.2 },
        { laengs_m: 1495.8, quer_m: -11.3 },
        { laengs_m: 1507.6, quer_m: -11.5 },
        { laengs_m: 1518.8, quer_m: -11.6 },
        { laengs_m: 1529.2, quer_m: -11.7 },
        { laengs_m: 1539.2, quer_m: -11.8 },
        { laengs_m: 1549.8, quer_m: -11.8 },
        { laengs_m: 1559.9, quer_m: -11.9 },
        { laengs_m: 1569.9, quer_m: -12.0 },
        { laengs_m: 1581.5, quer_m: -12.1 },
        { laengs_m: 1592.7, quer_m: -12.3 },
        { laengs_m: 1603.8, quer_m: -12.4 },
        { laengs_m: 1614.6, quer_m: -12.5 },
        { laengs_m: 1624.8, quer_m: -12.6 },
        { laengs_m: 1634.7, quer_m: -12.6 },
        { laengs_m: 1645.9, quer_m: -12.8 },
        { laengs_m: 1656.5, quer_m: -12.8 },
        { laengs_m: 1668.0, quer_m: -13.0 },
        { laengs_m: 1678.4, quer_m: -13.1 },
        { laengs_m: 1689.2, quer_m: -13.2 },
        { laengs_m: 1700.2, quer_m: -13.3 },
        { laengs_m: 1710.9, quer_m: -13.4 },
        { laengs_m: 1721.5, quer_m: -13.5 },
        { laengs_m: 1732.3, quer_m: -13.6 },
        { laengs_m: 1743.5, quer_m: -13.7 },
        { laengs_m: 1754.8, quer_m: -13.8 },
        { laengs_m: 1765.1, quer_m: -13.9 },
        { laengs_m: 1776.9, quer_m: -14.0 },
        { laengs_m: 1787.9, quer_m: -14.1 },
        { laengs_m: 1798.9, quer_m: -14.2 },
        { laengs_m: 1809.5, quer_m: -14.3 },
        { laengs_m: 1821.1, quer_m: -14.4 },
        { laengs_m: 1831.9, quer_m: -14.5 },
        { laengs_m: 1842.7, quer_m: -14.6 },
        { laengs_m: 1853.7, quer_m: -14.7 },
        { laengs_m: 1864.6, quer_m: -14.8 },
        { laengs_m: 1875.3, quer_m: -14.9 },
        { laengs_m: 1886.3, quer_m: -15.0 },
        { laengs_m: 1897.1, quer_m: -15.1 },
        { laengs_m: 1907.4, quer_m: -15.2 },
        { laengs_m: 1918.4, quer_m: -15.2 },
        { laengs_m: 1928.9, quer_m: -15.2 },
        { laengs_m: 1939.6, quer_m: -15.0 },
        { laengs_m: 1949.9, quer_m: -14.5 },
        { laengs_m: 1960.7, quer_m: -14.0 },
        { laengs_m: 1971.1, quer_m: -13.5 },
        { laengs_m: 1981.5, quer_m: -13.0 },
        { laengs_m: 1991.8, quer_m: -12.5 },
        { laengs_m: 2002.1, quer_m: -12.0 },
        { laengs_m: 2012.3, quer_m: -11.6 },
        { laengs_m: 2022.9, quer_m: -11.1 },
        { laengs_m: 2033.3, quer_m: -10.6 },
        { laengs_m: 2044.3, quer_m: -10.0 },
        { laengs_m: 2054.6, quer_m: -9.5 },
        { laengs_m: 2064.5, quer_m: -9.1 },
        { laengs_m: 2074.8, quer_m: -8.6 },
        { laengs_m: 2084.7, quer_m: -8.1 },
        { laengs_m: 2094.8, quer_m: -7.6 },
        { laengs_m: 2106.2, quer_m: -7.1 },
        { laengs_m: 2117.5, quer_m: -6.5 },
        { laengs_m: 2128.5, quer_m: -6.0 },
        { laengs_m: 2139.3, quer_m: -5.5 },
        { laengs_m: 2150.7, quer_m: -4.9 },
        { laengs_m: 2161.8, quer_m: -4.4 },
        { laengs_m: 2172.5, quer_m: -3.9 },
        { laengs_m: 2183.3, quer_m: -3.2 },
        { laengs_m: 2193.1, quer_m: -2.4 },
        { laengs_m: 2202.5, quer_m: -1.5 },
        { laengs_m: 2211.9, quer_m: -0.5 },
        { laengs_m: 2221.1, quer_m: 0.6 },
        { laengs_m: 2230.3, quer_m: 1.8 },
        { laengs_m: 2239.2, quer_m: 3.1 },
        { laengs_m: 2248.9, quer_m: 4.8 },
        { laengs_m: 2257.7, quer_m: 6.5 },
        { laengs_m: 2265.4, quer_m: 8.1 },
        { laengs_m: 2273.1, quer_m: 9.7 },
        { laengs_m: 2281.6, quer_m: 11.7 },
        { laengs_m: 2289.6, quer_m: 13.7 },
        { laengs_m: 2296.7, quer_m: 15.6 },
        { laengs_m: 2303.4, quer_m: 17.4 },
        { laengs_m: 2310.8, quer_m: 19.6 },
        { laengs_m: 2317.8, quer_m: 21.6 },
        { laengs_m: 2324.6, quer_m: 23.7 },
        { laengs_m: 2331.0, quer_m: 25.8 },
        { laengs_m: 2337.1, quer_m: 27.8 },
        { laengs_m: 2344.7, quer_m: 30.3 },
        { laengs_m: 2351.3, quer_m: 32.7 },
        { laengs_m: 2357.4, quer_m: 34.9 },
        { laengs_m: 2363.4, quer_m: 37.1 },
        { laengs_m: 2369.4, quer_m: 39.4 },
        { laengs_m: 2375.9, quer_m: 41.9 },
        { laengs_m: 2382.2, quer_m: 44.4 },
        { laengs_m: 2388.9, quer_m: 47.2 },
        { laengs_m: 2394.8, quer_m: 49.6 },
        { laengs_m: 2399.8, quer_m: 51.8 },
        { laengs_m: 2405.5, quer_m: 54.3 },
        { laengs_m: 2411.4, quer_m: 56.9 },
        { laengs_m: 2416.8, quer_m: 59.2 },
        { laengs_m: 2422.4, quer_m: 61.7 },
        { laengs_m: 2428.3, quer_m: 64.3 },
        { laengs_m: 2433.6, quer_m: 66.7 },
        { laengs_m: 2439.2, quer_m: 69.2 },
        { laengs_m: 2444.4, quer_m: 71.6 },
        { laengs_m: 2449.8, quer_m: 74.2 },
        { laengs_m: 2455.4, quer_m: 77.0 },
        { laengs_m: 2460.6, quer_m: 79.6 },
        { laengs_m: 2465.6, quer_m: 82.3 },
        { laengs_m: 2470.0, quer_m: 84.6 },
        { laengs_m: 2474.3, quer_m: 86.9 },
        { laengs_m: 2478.5, quer_m: 89.2 },
        { laengs_m: 2483.8, quer_m: 92.0 },
        { laengs_m: 2488.5, quer_m: 94.5 },
        { laengs_m: 2492.8, quer_m: 96.8 },
        { laengs_m: 2498.2, quer_m: 99.7 },
        { laengs_m: 2503.2, quer_m: 102.4 },
        { laengs_m: 2508.0, quer_m: 104.9 },
        { laengs_m: 2513.3, quer_m: 107.8 },
        ],
      });
      rec.max_lateral_offset_m = -12.881;
      rec.mess_ende_laengs_m = messEnde;
      return rec;
    },
  },
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
      // Die Aufsetzzone gehoert zur LANDEBAHN, nicht zur Bahnflaeche:
      // 10663 ft minus 2690 ft Versatz sind 2430 m, davon ein Drittel
      // ergibt 810 m. Der Wert der Vorlage (900 m) galt fuer die
      // unversetzte Bahn und war hier zu gross.
      r.td_tdz_length_m = 810;
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
        raeumKt: 58,
        icao: "A319",
        titel: "FenixA319 IAE WF SD",
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
        raeumKt: 61,
        icao: "BCS3",
        titel: "A220-300",
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
        icao: "A320",
        titel: "FenixA320 CFM WF",
      }),
  },
  {
    key: "d_overrun",
    label: "④ Über das Bahnende hinaus — EDLW 24, B738",
    hint: "Vollständig konstruiert: Im ganzen Bestand von 802 Landungen ist niemand über das Bahnende geschossen, es gibt dafür also keine echte Spur. Die Bahn ist mit 1700 m kurz genug, dass der Fall plausibel wird — spät aufgesetzt, die Spur läuft bis über das Bahnende hinaus. 0 Punkte, unabhängig von allem Seitlichen: Die Überroll-Prüfung läuft VOR den seitlichen Regeln.",
    build: () => {
      // Die frühere Fassung nahm die echte EDDL-Spur und setzte einen
      // Überroll-Wert daneben. Das passte nicht zusammen: Jene Spur endet
      // bei 1711 m und schwenkt dort zur Ausfahrt, während die Bahn
      // 2697 m lang ist — ein Überrollen tausend Meter vor dem Bahnende.
      // Eine Demo-Variante, deren Bild der eigenen Beschriftung
      // widerspricht, ist schlimmer als keine.
      const r = bahn(rwyKlein(baseRecord(), "EDLW", "24", 1700, 45), {
        breite: 45,
        spur: 5.72,
        spann: 34.32,
        punkte: overrunSpur(),
        overrun: 84,
      });
      r.aircraft_icao = "B738";
      r.aircraft_title = "737-800 PAX";
      return r;
    },
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
      return r;
    },
  },
];

/**
 * Die Varianten, wie die Demo sie benutzt.
 *
 * Jedes `build()` läuft durch `skipGrundAbleiten` — ein Wrapper statt
 * vierzehn Aufrufen, damit keine Variante ihn vergessen kann.
 */
export const MOCK_LANDING_OPTIONS: MockOption[] = ROH_OPTIONEN.map((o) => ({
  ...o,
  build: () => skipGrundAbleiten(o.build()),
}));

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
