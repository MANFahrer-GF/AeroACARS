// Mapper TouchdownDto.payload → RunwayDiagramV2Props (Webapp-Pfad).
// Spec: docs/spec/runway-diagram-v2.contract.md §Mapping aus TouchdownDto.payload.
// Wire-Felder verifiziert gegen aeroacars-live recorder + Pilot-Client
// aeroacars-mqtt/src/lib.rs am 2026-05-13.

import type {
  RunwayDiagramV2Props,
  AimClass,
  TchClass,
} from "../../components/RunwayDiagramV2";
import { aufsetzIasAusPayload, bewerteteGAusPayload } from "./helfer";

const FT_TO_M = 0.3048;

/**
 * Ein Sub-Score, so wie er im PIREP-Payload steht.
 *
 * Nur die Felder, die hier gebraucht werden — der volle Typ liegt in
 * `landingScoring.ts`.
 */
export interface SubScoreLike {
  key: string;
  skipped?: boolean;
  reason?: string | null;
}

/**
 * Der Grund, aus dem die Bahn-Achse nicht gewertet hat.
 *
 * # Warum das hier steht und nicht im Payload
 *
 * Der Pilot-Client publiziert den Touchdown, BEVOR die Bewertung läuft —
 * `lateral_skip_reason` ist dort zwangsläufig leer. Der Recorder ergänzt
 * später ausschliesslich `sub_scores`.
 *
 * In Runde 21 stand an dieser Stelle die Behauptung, die Webapp lese den
 * Grund „ohnehin aus den `sub_scores`". Sie tat es nicht: Der Mapper las
 * das Direktfeld, das nie ankommt. Die Korrektur wirkte im Pilot-Client
 * und in der Webapp gar nicht — dort zeichnete die Grafik weiter Zahlen,
 * die die Bewertung verworfen hatte.
 *
 * Eine Behauptung im Kommentar ist keine Prüfung.
 */
function skipGrundAusSubScores(
  subScores: SubScoreLike[] | null | undefined,
): string | null {
  if (!Array.isArray(subScores)) return null;
  const bahn = subScores.find((s) => s.key === "rollout" && s.skipped);
  return bahn?.reason ?? null;
}

export function mapTouchdownPayloadToV2Props(
  payload: Record<string, unknown> | null | undefined,
  subScores?: SubScoreLike[] | null,
): RunwayDiagramV2Props | null {
  if (!payload) return null;
  const pl = payload;

  // Webapp-Mapper sieht "navdata_source" als Wire-Feld vom Recorder.
  // Pre-v0.8.0 Touchdowns haben es nicht → source = null.
  const sourceRaw = pl.navdata_source as string | null | undefined;
  const source: "navigraph" | "ourairports_fallback" | null =
    sourceRaw === "navigraph"
      ? "navigraph"
      : sourceRaw === "ourairports_fallback"
      ? "ourairports_fallback"
      : null;

  const airportIdent =
    (pl.runway_match_icao as string | null) ??
    (pl.airport as string | null) ??
    "—";
  const runwayIdent =
    (pl.runway_match_ident as string | null) ??
    (pl.runway as string | null) ??
    "—";

  // length_m ist im Diagramm die NUTZBARE Landebahn (hinter der
  // versetzten Schwelle) — der Versatz kommt als eigener Bereich davor
  // dazu. Das Wire-Feld `runway_length_m` ist dagegen die volle
  // Bahnlaenge in Metern (siehe lib.rs:runway_length_m), also muss der
  // Versatz hier abgezogen werden.
  //
  // Bis v1.6.7 fiel das nicht auf, weil `runway_displaced_threshold_ft`
  // im Wire-Format immer 0 war (der DFD-Export steckt den Versatz seit
  // AIRAC 2608 in die Geometrie). Seit der Client den echten Wert
  // meldet, haette das Diagramm die Bahn um genau diesen Versatz zu
  // lang gezeichnet — bei EDDH 33 um 446 m. Der Pilot-Client rechnet an
  // seiner Stelle schon immer so (`runwayDiagramV2Mapper.ts` dort).
  const displaced_m_roh =
    ((pl.runway_displaced_threshold_ft as number | null) ?? 0) * FT_TO_M;
  const length_roh = (pl.runway_length_m as number | null) ?? 0;
  const length_m =
    displaced_m_roh > 0 && length_roh - displaced_m_roh > 0
      ? length_roh - displaced_m_roh
      : length_roh;

  // Wenn weder runway_match_icao noch runway_match_ident vorhanden:
  // Diagram macht keinen Sinn → null zurück.
  if (pl.runway_match_icao == null && pl.runway_match_ident == null) {
    return null;
  }

  const td_distance_from_threshold_m =
    (pl.td_distance_from_threshold_m as number | null) ??
    (pl.runway_match_distance_m as number | null) ??
    null;
  const td_centerline_offset_m = (pl.runway_match_centerline_offset_m as number | null) ?? null;
  // Ohne Bahnlänge, Aufsetzpunkt oder Versatz (sehr alte Payloads) keine
  // Grafik — nicht „0 m Bahn" und Aufsetzen exakt an Schwelle/Mittellinie
  // erfinden (QS 06.10.2026, Codex). Wie der Datensatz (`bahnTreffer`).
  if (length_roh <= 0 || td_distance_from_threshold_m == null || td_centerline_offset_m == null) {
    return null;
  }

  // ⚠ Eine veraltete Spur ist keine Spur. Der Client meldet
  // `bahn_spur_veraltet: true`, wenn der Bahnbezug NACH dem Ausrollen
  // gewechselt hat (Navdaten-/Szenerie-Upgrade) und die Spur den alten
  // Bezug traegt; die Werte selbst bleiben in der Zeile (der Recorder
  // loescht nichts). Sie hier zu zeichnen hiesse, eine Querachse zur
  // falschen Bahn als Messwert zu zeigen (Codex, 03.09.2026). Die Anzeige
  // sagt dann „fuer diesen Flug nicht erfasst".
  const spurVeraltet = pl.bahn_spur_veraltet === true;
  const plSpur: Record<string, unknown> = spurVeraltet ? {} : pl;
  return {
    airport_ident: airportIdent,
    airport_name: null, // Webapp resolved über separate airport-cache, hier neutral
    runway_ident: runwayIdent,
    length_m,
    // Seit QS 06.10.2026 sendet der Client den Belag mit (`runway_surface`)
    // — der Client zeigt ihn im Kopf der Grafik; ältere Flüge: keiner.
    surface: (pl.runway_surface as string | null) ?? null,
    source,
    nav_cycle: (pl.navdata_cycle as string | null) ?? null,
    displaced_threshold_m: displaced_m_roh,
    // ⚠ Der Nullpunkt der SPURWERTE — nicht die versetzte Schwelle.
    //
    // Ob beide zusammenfallen, entscheidet die Datenquelle: Steckt der
    // Versatz schon in der Navdaten-Geometrie, sind sie identisch. Am
    // Bestand gemessen (30.08.2026) ist das bei 8 von 19 Flügen mit
    // versetzter Schwelle der Fall. Wer hier `displaced_threshold_m`
    // einsetzt, verschiebt dort die ganze Spur um bis zu 300 m.
    //
    // Fehlt das Feld (Flüge vor v1.7.12), wird nichts verschoben.
    spur_nullpunkt_versatz_m:
      (pl.spur_nullpunkt_versatz_m as number | null) ?? null,
    td_distance_from_threshold_m,
    td_centerline_offset_m,
    td_in_tdz: (pl.td_in_tdz as boolean | null) ?? null,
    td_third: (pl.td_third as 1 | 2 | 3 | null) ?? null,
    td_tdz_length_m: (pl.td_tdz_length_m as number | null) ?? null,
    aim_point_m: (pl.aim_point_m as number | null) ?? null,
    aim_delta_m: (pl.aim_delta_m as number | null) ?? null,
    aim_class: (pl.aim_class as AimClass | null) ?? null,
    tch_actual_ft: (pl.tch_actual_ft as number | null) ?? null,
    tch_expected_ft: (pl.runway_tch_expected_ft as number | null) ?? null,
    tch_delta_ft: (pl.tch_delta_ft as number | null) ?? null,
    tch_class: (pl.tch_class as TchClass | null) ?? null,
    // v1.8.1 (Client): Raederhoehe ueber der Schwelle + FAA-Hoehengruppe.
    // Fehlen bei aelteren Landungen — die Anzeige faellt dann auf die alte
    // Fassung zurueck.
    tch_rad_ft: (pl.tch_rad_ft as number | null) ?? null,
    tch_hoehengruppe: (pl.tch_hoehengruppe as number | null) ?? null,
    pre_displaced_threshold:
      (pl.pre_displaced_threshold as boolean | null) ?? null,
    rollout_m: (pl.rollout_distance_m as number | null) ?? null,

    // ── v1.7.0 Bahndisziplin ─────────────────────────────────────────
    //
    // Diese Werte werden hier NICHT gerechnet, sondern nur gelesen. Sie
    // entstehen im Pilot-Client aus dem 5-Hz-Rollout-Fenster, das nur er
    // sieht; eine zweite Herleitung aus den Positionsdaten des Servers
    // (Grundtakt 3 s) käme zwangsläufig auf andere Zahlen. Zwei Zahlen
    // für dieselbe Landung sind schlimmer als eine fehlende.
    //
    // Fehlen sie — Flüge von vor v1.7.0, oder Stratos-Landungen über die
    // Brücke, die keine Rollspur liefern —, bleibt es bei null, und die
    // Anzeige sagt „für diesen Flug nicht erfasst". Sie malt dann keine
    // leere Querachse, die wie ein Messwert aussieht.
    clearance_point_m: (plSpur.clearance_point_m as number | null) ?? null,
    scoring_cutoff_m: (plSpur.scoring_cutoff_m as number | null) ?? null,
    // Wo das Messfenster schloss (unter 60 kt) — NICHT dasselbe wie
    // `scoring_cutoff_m` (Kurswechsel). Ab Client v1.7.4; aeltere Fluege
    // fuehren es nicht, dort faellt die Anzeige auf den Kurswechsel
    // zurueck, also auf das Verhalten von vorher.
    mess_ende_laengs_m: (plSpur.mess_ende_laengs_m as number | null) ?? null,
    clearance_speed_kt: (plSpur.clearance_speed_kt as number | null) ?? null,
    clearance_side: (plSpur.clearance_side as "left" | "right" | null) ?? null,
    track_width_m: (pl.track_width_m as number | null) ?? null,
    track_width_source:
      (pl.track_width_source as "type_table" | "aircraft_file" | null) ?? null,
    wingspan_m: (pl.wingspan_m as number | null) ?? null,
    runway_width_m: (pl.runway_width_m as number | null) ?? null,
    ausrichtung_grund:
      (Array.isArray(subScores)
        ? (subScores as Array<{ key: string; skipped?: boolean | null; rationale_key?: string | null }>).find(
            (s) => s.key === "alignment" && !s.skipped,
          )?.rationale_key?.replace(/^landing\.rat\./, "")
        : null) ?? null,
    min_edge_clearance_m: (plSpur.min_edge_clearance_m as number | null) ?? null,
    max_lateral_offset_m: (plSpur.max_lateral_offset_m as number | null) ?? null,
    lateral_samples:
      (plSpur.lateral_samples as Array<{ laengs_m: number; quer_m: number }> | null) ??
      null,
    // Frühere Durchgänge (durchgestartet), vom Client ab v1.7.40. Ältere
    // Flüge ergänzt `LandingAnalysis` aus dem überholten Datensatz.
    vorherige_durchgaenge:
      (plSpur.vorherige_durchgaenge as Array<{
        lateral_samples: Array<{ laengs_m: number; quer_m: number }>;
      }> | null) ?? null,
    // Der Grund, aus dem die seitliche Bewertung entfiel — aus der
    // BEWERTUNG, nicht hier hergeleitet.
    //
    // Beim Touchdown-Publish ist er noch nicht bekannt: Die Achse rechnet
    // erst danach. Der Wert kommt dann aus den `sub_scores` des PIREP,
    // die `landingScoring.ts` ohnehin liest.
    // Aus den `sub_scores` — das Direktfeld ist beim Touchdown-Publish
    // immer leer, siehe `skipGrundAusSubScores`.
    // Zwischenstand-Kennzeichen: `undefined` heisst „Flug von vor
    // dieser Fassung", nur ein ausdrueckliches `false` meldet sich.
    rollout_final: (pl as Record<string, unknown>).rollout_final as
      | boolean
      | undefined,
    lateral_skip_reason:
      skipGrundAusSubScores(subScores) ??
      ((pl.lateral_skip_reason as string | null) ?? null),
    surface_paved: (pl.surface_paved as boolean | null) ?? null,
    overrun_m: (pl.overrun_m as number | null) ?? null,
    // Die Ausfahrten kommen DOCH über die Leitung.
    //
    // Der erste Anlauf liess sie hier auf null: Sie stehen in der
    // OSM-Bodenkarte, die der Server ohnehin hat, also könnte er sie
    // selbst rechnen. Genau das soll er nicht — zwei Herleitungen
    // derselben Grösse driften auseinander, und die Anzeige muss auf
    // beiden Seiten dieselbe sein. Solange der Server sie nicht rechnete,
    // zeigte die Webapp schlicht nie Ausfahrten.
    // Die Ausfahrten reisen als Ganzes — der `verlauf` gehoert dazu.
    //
    // Er steht hier ausdruecklich, obwohl die Zuweisung ihn ohnehin
    // mitnimmt: Eine Typangabe, die ein vorhandenes Feld verschweigt,
    // ist eine Einladung, es beim Aufraeumen zu entfernen. Aus ihm
    // zeichnet die Queransicht den Korridor des genommenen Rollwegs.
    // Bahnansicht 2.1.8: genutzte Ausfahrt und Quelle der Rollwege (Client).
    clearance_exit: (pl.clearance_exit as string | null) ?? null,
    runway_exits_quelle: (pl.runway_exits_quelle as string | null) ?? null,
    runway_exits:
      (pl.runway_exits as Array<{
        name: string;
        laengs_m: number;
        seite: "left" | "right";
        verlauf?: Array<{ laengs_m: number; quer_m: number }>;
      }> | null) ?? null,
    // Aircraft-Daten aus dem TouchdownPayload — alle als optional
    // gefasst, weil pre-v0.5-Touchdowns sie nicht alle tragen.
    aircraft_icao: (pl.aircraft_icao as string | null) ?? null,
    aircraft_title: (pl.aircraft_title as string | null) ?? null,
    aircraft_registration: (pl.aircraft_registration as string | null) ?? null,
    landing_weight_kg: (pl.landing_weight_kg as number | null) ?? null,
    planned_ldw_kg: (pl.planned_ldw_kg as number | null) ?? null,
    // QS 06.10.2026: 0 heißt beim Client „keine IAS" — leer, wie im Datensatz.
    landing_speed_kt: aufsetzIasAusPayload(pl),
    landing_pitch_deg: (pl.pitch_deg as number | null) ?? null,
    // PIREP-Wert des Clients (Profilpunkt am nächsten an t = 0) vor dem
    // Touchdown-Wert — wie der Datensatz (QS 06.10.2026, W4).
    landing_bank_deg:
      (typeof pl.landing_bank_deg === "number" && Number.isFinite(pl.landing_bank_deg) ? pl.landing_bank_deg : null) ??
      (pl.bank_deg as number | null) ??
      null,
    // Der gewertete G-Wert unter seinem eigenen Namen.
    //
    // Er stand bisher NUR im Feld darunter, weil die Anzeige damals nur
    // eins von beiden kannte. Der Wert im Bild aendert sich dadurch
    // nicht — die Anzeige zieht ohnehin den gewerteten vor —, aber die
    // Felder heissen jetzt, was sie enthalten.
    //
    // QS 06.10.2026: derselbe Wert wie im Client-Datensatz — bei X-Plane ab
    // 1.6.2 trägt `scored_g_load` die auf MSFS umgerechnete Punkte-Zahl,
    // nicht den gemessenen Wert (siehe `bewerteteGAusPayload`).
    // Seit QS 06.10.2026 sendet der Client die gemessene Zahl im PIREP
    // (`landing_scored_g_force`, vom Recorder in den Touchdown übernommen).
    landing_scored_g_force:
      (typeof pl.landing_scored_g_force === "number" ? pl.landing_scored_g_force : null) ??
      bewerteteGAusPayload(pl),
    // Wie der Client-Mapper: die Spitzen-G unter ihrem Namen (die Anzeige
    // zieht die bewertete ohnehin vor) — nicht die bewertete als Spitze.
    landing_peak_g_force: (pl.peak_g_load as number | null) ?? null,
    headwind_kt: (pl.headwind_kt as number | null) ?? null,
    crosswind_kt: (pl.crosswind_kt as number | null) ?? null,
    locale: "de",
  };
}
