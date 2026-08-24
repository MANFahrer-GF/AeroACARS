// Runway Diagram v2 — Display-Only Polish nach v0.8.0.
//
// Spec: docs/spec/runway-diagram-v2.contract.md
//
// Pure-Display-Component, KEIN neues Scoring, KEINE neuen Wire-Felder.
// Nutzt ausschließlich existing v0.8.0-Felder aus LandingRecord oder
// TouchdownDto.payload (Wire-symmetrisch). Pilot-Client liest aus
// `landings.json` (lokal persistiert, kein VPS-Fetch nötig). Webapp
// kann später dieselbe Component importieren.
//
// Layout (4 Bereiche full-width):
//   1. Header — Airport/RWY/Length/Source + Hilfe-Button
//   2. SVG-Diagramm (viewBox 1200x320, responsive)
//   3. Legende
//   4. 4 Detail-Karten (Aufsetz-Bewertung / Position / Anflug-Profil / Datenquelle)

import { useMemo, useState } from "react";
import { erzeugeProjektion } from "../lib/runwayProjection";
import { useBahnZoom } from "../lib/useBahnZoom";
import { RunwayDisciplinePanel } from "./RunwayDisciplinePanel";
import { useTranslation } from "react-i18next";
import { GlossaryModal } from "./RunwayGlossaryModal";
import { useV2Skin } from "./SkinContext";

// ─── Public types ───────────────────────────────────────────────────

export type AimClass =
  | "perfect"
  | "short_of_aim"
  | "past_aim"
  | "long_landing"
  | "severe";

export type TchClass =
  | "on_profile"
  | "slightly_low"
  | "slightly_high"
  | "high"
  | "below_profile";

export interface RunwayDiagramV2Props {
  /**
   * Mindest-Schriftgrösse in SVG-Einheiten — für den Druck.
   *
   * Auf Papier skaliert das SVG auf die Spaltenbreite (`width: 100%`),
   * und jede Schrift darin schrumpft mit. Gemessen am 24.08.2026 landeten
   * die Beschriftungen im A4-Bericht bei **3,6 bis 4,4 pt**; lesbar ist
   * Druck etwa ab 6 pt. Die halbe Grafik war auf dem Ausdruck nicht zu
   * entziffern, ohne dass irgendwo etwas fehlgeschlagen wäre.
   *
   * # Warum eine Untergrenze und kein Faktor
   *
   * Der erste Versuch multiplizierte alles mit 1,9. Die kleinen Zeilen
   * wurden lesbar — und die grossen sprengten das Bild: Die Bahnkennungen
   * (28 Einheiten) und die Längenangabe (20) liefen aus dem viewBox
   * heraus und übereinander, 78 Kollisionen. Sie waren nie das Problem;
   * sie drucken schon bei 8 bis 11 pt.
   *
   * Eine Untergrenze hebt nur an, was zu klein ist, und lässt den Rest
   * in Ruhe. Die Reihenfolge der Grössen bleibt erhalten.
   */
  schriftMindest?: number;

  airport_ident: string;
  airport_name?: string | null;
  runway_ident: string;
  length_m: number;
  surface?: string | null;
  source: "navigraph" | "ourairports_fallback" | null;
  nav_cycle?: string | null;
  displaced_threshold_m?: number;
  td_distance_from_threshold_m: number;
  td_centerline_offset_m: number;
  td_in_tdz?: boolean | null;
  td_third?: 1 | 2 | 3 | null;
  td_tdz_length_m?: number | null;
  aim_point_m?: number | null;
  aim_delta_m?: number | null;
  aim_class?: AimClass | null;
  tch_actual_ft?: number | null;
  tch_expected_ft?: number | null;
  tch_delta_ft?: number | null;
  tch_class?: TchClass | null;
  pre_displaced_threshold?: boolean | null;
  rollout_m?: number | null;

  // Optional Aircraft-Daten für die Landeeinschätzung. Wenn nichts
  // gesetzt → FLUGZEUG-Pill wird nicht gerendert.
  // ── v1.7.0 Bahndisziplin (siehe docs/spec/runway-diagram-v2.contract.md) ──
  /**
   * Wo die Bahn verlassen wurde — die Stelle, an der die Spur die Bahnkante
   * überschreitet und nicht zurückkommt. Das ist „Bahn geräumt".
   */
  clearance_point_m?: number | null;
  /**
   * Wo die **Bewertung** endet: der Beginn des Ausschwenkens zur Ausfahrt.
   *
   * Nicht dasselbe wie `clearance_point_m` und deshalb ein eigenes Feld.
   * Ein Flugzeug zieht Hunderte Meter vor der Kante nach aussen; dieser
   * Teil gehört zum Abrollen und darf nicht als seitlicher Versatz
   * gewertet werden. Gezeichnet wird die Spur dort aber weiter
   * durchgezogen — sie ist gemessen, sie ist auf der Bahn, und eine
   * gestrichelte Linie mitten auf der Bahn wäre nicht zu erklären.
   */
  scoring_cutoff_m?: number | null;
  clearance_speed_kt?: number | null;
  clearance_side?: "left" | "right" | null;
  track_width_m?: number | null;
  track_width_source?: "type_table" | "aircraft_file" | null;
  /** Spannweite in Metern — für den Grössenvergleich unter der Grafik. */
  wingspan_m?: number | null;
  /** Bahnbreite in Metern — Grundlage der Queransicht. */
  runway_width_m?: number | null;
  /**
   * Rollwege, die die Bahn treffen (OpenStreetMap-Bodenkarte).
   *
   * Machen die Bewertung nachvollziehbar: Man sieht, welche Ausfahrt vor der
   * genutzten lag und wie weit davor. Optional — ohne sie zeigt die
   * Queransicht einfach keine Stummel.
   */
  runway_exits?: Array<{ name: string; laengs_m: number; seite: "left" | "right" }> | null;
  min_edge_clearance_m?: number | null;
  max_lateral_offset_m?: number | null;
  lateral_samples?: Array<{ laengs_m: number; quer_m: number }> | null;
  /** Warum die seitliche Bewertung entfiel — der Grund aus der BEWERTUNG. */
  lateral_skip_reason?: string | null;
  /**
   * Steht das Ausrollen fest — oder ist das ein Zwischenstand?
   *
   * `touchdown_complete` geht rund neun Sekunden nach dem Aufsetzen raus;
   * da rollt das Flugzeug noch. Bleibt die Finalisierung aus, sind alle
   * Bahnwerte vorläufig — und sahen bis dahin aus wie fertige.
   *
   * EDDB 06L am 24.08.2026: 482 m Ausrollstrecke (das wären 0,42 g), eine
   * Spur, die mitten auf der 3600-m-Bahn aufhört, kein Räumpunkt. Nichts
   * davon war falsch gemessen — es war nur noch nicht fertig.
   *
   * `undefined` = Flug von vor dieser Fassung; dann wird nichts behauptet.
   */
  rollout_final?: boolean;
  surface_paved?: boolean | null;
  overrun_m?: number | null;

  aircraft_icao?: string | null;
  aircraft_title?: string | null;
  aircraft_registration?: string | null;
  landing_weight_kg?: number | null;
  planned_ldw_kg?: number | null;
  landing_speed_kt?: number | null;
  landing_pitch_deg?: number | null;
  landing_bank_deg?: number | null;
  landing_peak_g_force?: number | null;
  /** v0.12.3 (LE9): EMA-scored G — shown/coloured instead of the raw
   *  peak when present. */
  landing_scored_g_force?: number | null;
  headwind_kt?: number | null;
  crosswind_kt?: number | null;

  locale?: "de" | "en" | "it";
}

// ─── Visual tokens ───────────────────────────────────────────────────

// TOKENS-Konstante entfernt — Werte kommen jetzt aus useV2Skin() und sind
// pro Render zur Laufzeit verfügbar. So kann der VPS-Skin die Werte
// hot-tauschen ohne Pilot-Client-Release.

function tdColor(p: RunwayDiagramV2Props, tokens: { tdSevere: string; tdPerfect: string; tdWarn: string; tdAcceptable: string }): string {
  if (p.pre_displaced_threshold === true) return tokens.tdSevere;
  switch (p.aim_class) {
    case "perfect":
    case "past_aim":
    case "short_of_aim":
      return tokens.tdPerfect;
    case "long_landing":
      return tokens.tdWarn;
    case "severe":
      return tokens.tdSevere;
    default:
      return tokens.tdAcceptable;
  }
}

// ─── Component ───────────────────────────────────────────────────────

export function RunwayDiagramV2(props: RunwayDiagramV2Props) {
  // Auf dem Bildschirm 0 (wirkungslos), im Druck die Untergrenze.
  const schriftMindest = props.schriftMindest ?? 0;
  const sf = (g: number) => Math.max(g, schriftMindest);
  /**
   * Zeilenabstand einer gestapelten Beschriftung — folgt der Schrift.
   *
   * Die Abstände standen als feste Zahlen im Layout (11 bzw. 13
   * Einheiten), abgestimmt auf die Schriftgrössen von damals. Sobald
   * der Druck die Schrift anhebt, sitzen die Zeilen ineinander:
   * „BAHN GERÄUMT" lag auf „2296 m · Ausfahrt D9 rechts". Gemessen an
   * der Demo waren es fünfzehn solche Paare.
   */
  const zeile = (g: number) => sf(g) * 1.18;
  const skin = useV2Skin();
  const TOKENS = skin.tokens;
  const display = skin.display;
  const { t } = useTranslation();
  const [glossaryOpen, setGlossaryOpen] = useState(false);

  const W = skin.geometry.svgWidth;
  const H = skin.geometry.svgHeight;
  const padX = skin.geometry.rwyPaddingX;
  const padY = skin.geometry.rwyPaddingY;
  const innerW = W - 2 * padX;
  const innerH = H - 2 * padY;
  const rwyTop = padY;
  const rwyBot = padY + innerH;
  const rwyCl = (rwyTop + rwyBot) / 2;

  // Bahn-Geometrie.
  // - lengthM: nutzbare LANDE-Bahn (= nach dem displaced threshold)
  // - ddsM: Länge der pre-threshold-Zone (DDS) vor dem Landethreshold
  // - totalVisualM: gesamte physische Bahn (DDS + Lande-Bereich)
  // Das tarmac-Rect spannt die gesamte physische Bahn ab; mToX(0) liegt
  // beim Landethreshold (= NICHT am linken Rand der Bahn bei DDS > 0).
  // v1.6.8-QS3: der Riegel schuetzt nur noch gegen unbrauchbare Werte
  // (0, negativ, NaN) — er ueberschreibt keine echte kurze Bahn mehr.
  //
  // Dieselbe Untergrenze steckte frueher auch in der Prozent-Rechnung und
  // machte dort aus einer knappen Landung eine komfortable (v0.19.x-Fix,
  // siehe Test „ignores the SVG-geometry floor"). Im Bild blieb sie
  // stehen — bis die versetzten Schwellen dazukamen: 19 Bahnen rutschen
  // durch den Abzug unter 500 m nutzbare Laenge (EDKU, EDXZ, EDNG,
  // LOAD …), und dort haette das Bild eine Bahn gezeichnet, die es nicht
  // gibt, mit dem Aufsetzpunkt an der falschen Stelle. Kurze Plaetze sind
  // ein unterstuetzter Fall, kein Datenfehler.
  // Untergrenze 100 m: tief genug, dass keine echte Bahn sie beruehrt (die
  // kuerzeste mit versetzter Schwelle in den Navdaten hat 292 m nutzbare
  // Laenge), hoch genug, dass ein kaputter Kleinstwert die Zeichnung nicht
  // entarten laesst — bei 0,5 m bildete `mToX` jeden Meter auf ein
  // Vielfaches der Bahnbreite ab. Die alten 500 m waren dafuer zu grob:
  // sie ueberschrieben echte kurze Plaetze (Review-Befund).
  // v1.7.0: Die Projektion kommt aus `lib/runwayProjection` -- dieselbe
  // Funktion, die die Queransicht benutzt. Vorher stand sie hier inline, und
  // die Queransicht haette eine zweite gebraucht. Genau daraus entsteht die
  // Fehlerklasse aus Spec §8.4: zwei Stellen, die dasselbe rechnen sollen,
  // driften auseinander -- im ersten Entwurf stand der Aim-Marker 209 m falsch.
  // Zoom — EIN Zustand für beide Ansichten. Getrennte Zustände wären der
  // Fehler, gegen den §8.4 die gemeinsame Projektion vorschreibt: Zwei
  // Ansichten, die nicht mehr fluchten, sind schlimmer als eine.
  const zoom = useBahnZoom(
    -(props.displaced_threshold_m ?? 0),
    Number.isFinite(props.length_m) ? Math.max(100, props.length_m) : 500,
  );
  const projektion = erzeugeProjektion({
    lengthM: props.length_m,
    ddsM: props.displaced_threshold_m ?? 0,
    padX,
    innerW,
    sichtVonM: zoom.vonM,
    sichtBisM: zoom.bisM,
  });
  const lengthM = projektion.lengthM;
  const ddsM = projektion.ddsM;
  const ddsActive = ddsM > 0;

  // thresholdX = Pixel-Position des Landethresholds.
  //   ohne DDS: thresholdX == padX (Bahn-Anfang IS Threshold)
  //   mit DDS:  thresholdX > padX (DDS-Bereich beansprucht erste ddsM)
  const thresholdX = projektion.thresholdX;

  // Meter → X-Pixel. Eingabe m ist Distanz VOM LANDETHRESHOLD (signed).
  // Negative m → vor dem Threshold (= in der DDS-Zone).
  const mToX = projektion.mToX;

  // Centerline-Offset → Y. ±widthM/2 → ±(innerH/2 - safetyMargin).
  // widthM = 45 m typisch, aber wir stretchen für Sichtbarkeit (sonst
  // wäre ±2 m visuell unsichtbar).
  const widthM = 45;
  const yMaxOffset = innerH / 2 - 20;
  const clampedOffset = Math.max(
    -widthM / 2,
    Math.min(widthM / 2, props.td_centerline_offset_m),
  );
  const tdY = rwyCl + (clampedOffset / (widthM / 2)) * yMaxOffset;
  const tdX = mToX(props.td_distance_from_threshold_m);
  // Halbe Breite der TD-Beschriftung, geschaetzt aus Zeichenzahl und
  // Schriftgroesse (13 px, Monospace ~0,6 em je Zeichen). Nur fuer die
  // Klemmung am Rand -- auf den Pixel kommt es dabei nicht an.
  const tdLabelHalb =
    (`TD ${props.td_distance_from_threshold_m.toFixed(0)} m`.length + 14) * 13 * 0.6 * 0.5;
  const dotColor = tdColor(props, TOKENS);

  // Skala-Ticks anhand Bahn-Länge: 0/300/600/900/1200/1500/1800/2400 etc.
  const scaleTicks = useMemo(() => {
    const candidates = [0, 300, 600, 900, 1200, 1500, 1800, 2100, 2400, 3000, 3600, 4200];
    return candidates.filter((d) => d <= lengthM);
  }, [lengthM]);

  // Aim-Marker only when known + within bahn.
  const aimX =
    props.aim_point_m != null && props.aim_point_m > 0
      ? mToX(props.aim_point_m)
      : null;

  // TDZ-Box only when length covers the marker.
  const tdzEndX =
    props.td_tdz_length_m != null && props.td_tdz_length_m > 0
      ? mToX(props.td_tdz_length_m)
      : null;

  // Rollout-Endpunkt
  const exitDistM =
    props.rollout_m != null
      ? Math.min(lengthM, props.td_distance_from_threshold_m + props.rollout_m)
      : null;
  const exitX = exitDistM != null ? mToX(exitDistM) : null;
  // ⚠ `exitDistM` (aus `rollout_m`) und `clearance_point_m` sind ZWEI
  // Quellen fuer dasselbe Ende, und sie stimmen nicht ueberein:
  // `rollout_m` ist die gefahrene Strecke bis zum Stillstand, der
  // Raeumpunkt die Stelle, an der die Bahn verlassen wurde. Wer beides
  // zeichnet, bekommt eine Linie, die ueber ihre eigene Endmarke
  // hinauslaeuft. Die Linie endet deshalb IMMER am Raeumpunkt, sobald
  // einer bekannt ist -- siehe `rolloutEndeX`.

  // ── Räumpunkt (v1.7.0) ─────────────────────────────────────────────
  //
  // Spec §8.3: „Räumpunkt statt Bremspunkt; die gestrichelte Spur danach
  // folgt der ECHTEN Ausfahrtsrichtung."
  //
  // Der Unterschied zum alten Bremspunkt ist nicht kosmetisch. Der
  // Bremspunkt behauptete, bei 40 kt sei etwas Bewertbares passiert — was
  // von der Anweisung des Lotsen abhängt, nicht vom Piloten. Der Räumpunkt
  // dagegen ist eine Messung: Hier hat das Flugzeug die Bahn verlassen,
  // hier endet das Messfenster, ab hier wird nichts mehr gewertet.
  const raeumM = props.clearance_point_m ?? null;
  const raeumX = raeumM != null ? mToX(raeumM) : null;
  /**
   * Wo die Ausroll-Linie endet — eine Groesse, nicht zwei.
   *
   * Der Raeumpunkt hat Vorrang: Er ist gemessen und traegt die Endmarke.
   * Nur wenn keiner vorliegt (Fluege vor v1.7.0), endet die Linie an der
   * Ausrollstrecke.
   */
  const rolloutEndeX = raeumX ?? exitX;
  // Die UNGEKLEMMTE Endstelle. `exitDistM` ist fuer die Geometrie an der
  // Bahnlaenge gekappt; fuer die Beschriftung ist genau der Unterschied
  // die Aussage: Endet die Aufzeichnung hinter dem Bahnende, gibt es
  // keine Stelle AUF der Bahn, die man nennen koennte.
  const ausrollEndeM =
    props.rollout_m != null
      ? props.td_distance_from_threshold_m + props.rollout_m
      : null;
  const ausrollEndeUeberBahn = ausrollEndeM != null && ausrollEndeM > lengthM;
  // Die Ausfahrt, über die geräumt wurde — für die Beschriftung. Nur wenn
  // die Seite feststeht: Ohne Seite gibt es keine eindeutige Zuordnung,
  // und einen Namen zu raten wäre schlimmer als keiner (§8.6).
  const raeumAusfahrt =
    raeumM != null && props.clearance_side != null
      ? (props.runway_exits ?? [])
          .filter((e) => e.seite === props.clearance_side)
          .map((e) => ({ e, d: Math.abs(e.laengs_m - raeumM) }))
          .filter((x) => x.d < 120)
          .sort((a, b) => a.d - b.d)[0]?.e ?? null
      : null;

  // Bahn-Auslastung.
  //
  // v0.20.0: dieselbe Formel wie die rollout-Kachel in LandingPanel
  // (`buildRolloutValueLabel`): used = max(td + rollout, rollout), Nenner ist
  // die LDA. Vorher wich das hier zweifach ab — Nenner war die physische
  // Laenge (siehe Mapper-Kommentar), und die Klemmung auf 100 % verschwieg
  // einen Overrun, den die Kachel daneben offen auswies. `props.length_m`
  // IST die LDA (siehe Mapper-Kommentar in runwayDiagramV2Mapper.ts).
  //
  // v0.19.x FIX: die Klemmung auf 500 m — bewusst als Schutz gegen eine
  // degenerierte SVG-Geometrie bei fehlenden/kaputten Daten gedacht — war
  // hier trotzdem als Nenner im Einsatz (`lengthM`, NICHT `props.length_m`).
  // Für echte Kurzbahnen (Busch-/VFR-Landeplätze unter 500 m LDA, ein von
  // AeroACARS ausdrücklich unterstützter Fall) rechnete die Auslastung
  // gegen eine fiktiv aufgeblähte Bahn und zeigte einen zu NIEDRIGEN
  // Prozentwert — eine wirklich knappe Landung auf einer 300-m-Piste sah
  // entspannter aus als sie war. Die SVG-Geometrie darf die Klemmung
  // weiter nutzen (`lengthM`), aber der Score-Nenner nimmt jetzt den
  // echten, ungeklemmten Wert.
  const bahnUsedPct =
    props.rollout_m != null && props.length_m > 0
      ? (Math.max(
          props.td_distance_from_threshold_m + props.rollout_m,
          props.rollout_m,
        ) /
          props.length_m) *
        100
      : null;

  // Source-Label — neutral Wording per Spec §Akzeptanz (Lizenz-Vorsicht):
  // UI sagt "VPS Navdata (AIRAC X)" statt direkt "Navigraph".
  const sourceLabel = (() => {
    if (props.source === "navigraph") {
      return `${t("runway_v2.source_vps_navdata")} (AIRAC ${props.nav_cycle ?? "?"}) ✓`;
    }
    if (props.source === "ourairports_fallback") {
      return t("runway_v2.source_ourairports_fallback");
    }
    return t("runway_v2.source_ourairports_legacy");
  })();

  return (
    <section
      className="rwy-v2"
      aria-label="Landebahn-Analyse"
      style={{
        width: "100%",
        display: "flex",
        flexDirection: "column",
        gap: 12,
        // Drei Regeln, damit die Anzeige in jeden Platz passt, den sie
        // bekommt — §8.6.5 verbietet waagerechtes Scrollen.
        //
        // `box-sizing: border-box` ist die entscheidende: Das Stylesheet gibt
        // der Sektion links und rechts je 19 Pixel Innenabstand, und unter
        // `content-box` kommen die zur Breite DAZU. Gemessen: berechnete
        // Breite 603 Pixel, tatsächliche 641 — achtunddreissig zu viel, und
        // der Container schnitt sie ab.
        //
        // `min-width: 0` bricht die Vorgabe auf, mit der Flex-Kinder ihre
        // Inhaltsbreite erzwingen; `max-width: 100%` deckelt den Rest.
        boxSizing: "border-box",
        minWidth: 0,
        maxWidth: "100%",
      }}
    >
      {/* ─── 1. HEADER ─────────────────────────────────────────────── */}
      <header
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 16,
          padding: "12px 16px",
          background: "rgba(255,255,255,0.04)",
          borderRadius: 8,
          borderTop: "2px solid rgba(34,197,94,0.5)",
        }}
      >
        <div>
          {/* v2.x: H3-Titel "Landebahn-Analyse" entfernt — die Component
              wird im Webapp-Card und im Pilot-Client-LandingPanel
              jeweils schon mit demselben Titel gewrappt. Wäre doppelt
              gemoppelt. Das 🛬-Icon wandert vor den Airport. */}
          <div
            style={{
              fontSize: "1.0rem",
              lineHeight: 1.55,
              display: "flex",
              alignItems: "baseline",
              gap: 6,
              flexWrap: "wrap",
            }}
          >
            <span style={{ fontSize: "1.1rem" }}>🛬</span>
            <strong style={{ fontSize: "1.05rem" }}>{props.airport_ident}</strong>
            {props.airport_name ? <span>({props.airport_name})</span> : null}
            <span style={{ opacity: 0.5 }}>·</span>
            <strong style={{ fontSize: "1.05rem" }}>{t("runway_v2.rwy_label_prefix")} {props.runway_ident}</strong>
            <span style={{ opacity: 0.5 }}>·</span>
            <span>{props.length_m.toFixed(0)} m</span>
            {/* Die Breite gehoert in den Kopf: Sie ist der Massstab der
                Queransicht und die Groesse, an der „Rad neben der Bahn"
                haengt. Wer die Note nachvollziehen will, braucht sie. */}
            {props.runway_width_m != null && props.runway_width_m > 0 && (
              <>
                <span aria-hidden>·</span>
                <span>
                  {t("runway_v2.width_label", {
                    defaultValue: "{{m}} m breit",
                    m: props.runway_width_m.toFixed(0),
                  })}
                </span>
              </>
            )}
            {props.surface ? (
              <>
                <span style={{ opacity: 0.5 }}>·</span>
                <span>{t(surfaceLabelKey(props.surface)) || props.surface}</span>
              </>
            ) : null}
          </div>
          <div
            style={{
              fontSize: "0.82rem",
              opacity: props.source === "ourairports_fallback" ? 0.95 : 0.7,
              marginTop: 4,
              color:
                props.source === "ourairports_fallback" ? "#fbbf24" : undefined,
            }}
          >
            {t("runway_v2.data_source")}: {sourceLabel}
          </div>
        </div>
        <button
          type="button"
          // `nur-bildschirm`: Bedienelemente gehören nicht aufs Papier.
          // Im Druck (Bericht-Export) stand hier ein Knopf, den niemand
          // drücken kann — siehe die Regel in App.css.
          className="bahn-nur-bildschirm"
          onClick={() => setGlossaryOpen(true)}
          aria-label="Begriffe erklärt — Glossar öffnen"
          style={{
            padding: "6px 12px",
            background: "rgba(255,255,255,0.06)",
            border: "1px solid rgba(255,255,255,0.18)",
            borderRadius: 6,
            color: "inherit",
            cursor: "pointer",
            fontSize: "0.85rem",
            whiteSpace: "nowrap",
          }}
        >
          ⓘ {t("runway_v2.glossary_open")}
        </button>
      </header>

      {/* ─── 2. SVG-DIAGRAMM ───────────────────────────────────────── */}
      <div
        style={{
          width: "100%",
          background: "rgba(0,0,0,0.25)",
          borderRadius: 8,
          padding: "12px 8px 4px 8px",
          // Ohne `border-box` kommen die sechzehn Pixel Innenabstand zur
          // Breite dazu: Der Wrapper wurde 634 statt 618 breit, und sein
          // Container scrollte waagerecht. §8.6.5 verbietet genau das.
          boxSizing: "border-box",
          maxWidth: "100%",
          overflowX: "hidden",
        }}
      >
        <svg
          // Nicht `onWheel`: React bindet Rad-Ereignisse passiv, und dort
          // ist `preventDefault()` wirkungslos — der Browser hätte die
          // ganze Seite mitgezoomt. Siehe `radAnschluss`.
          ref={zoom.radAnschluss}
          onMouseDown={zoom.aufZiehStart}
          onMouseMove={zoom.aufZiehen}
          onMouseUp={zoom.aufZiehEnde}
          onMouseLeave={zoom.aufZiehEnde}
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="xMidYMid meet"
          style={{
            width: "100%",
            height: "auto",
            display: "block",
            cursor: zoom.zieht ? "grabbing" : zoom.gezoomt ? "grab" : "default",
          }}
          role="img"
          aria-label="Bahn-Geometrie mit Aufsetzpunkt"
        >
          {/* Tarmac */}
          <rect
            x={padX}
            y={rwyTop}
            width={innerW}
            height={innerH}
            fill={TOKENS.tarmac}
            stroke={TOKENS.tarmacBorder}
            strokeWidth="1"
          />

          {/* DDS Pre-Threshold-Zone — die ERSTEN ddsM Meter der Bahn,
              VOR dem Landethreshold. Wird ROT gezeichnet (Landung
              verboten) mit Chevron-Hatch (= echte Bahn-Markierung).
              Liegt zwischen padX (Bahn-Anfang) und thresholdX (Landethreshold). */}
          {ddsActive && (
            <g>
              <defs>
                <pattern
                  id="dds-chevron"
                  patternUnits="userSpaceOnUse"
                  width="14"
                  height="14"
                  patternTransform="rotate(60)"
                >
                  <line x1="0" y1="0" x2="0" y2="14" stroke={TOKENS.ddsBorder} strokeWidth="2.5" />
                </pattern>
              </defs>
              <rect
                x={padX + 2}
                y={rwyTop + 4}
                width={Math.max(0, thresholdX - padX - 2)}
                height={innerH - 8}
                fill={TOKENS.ddsZone}
              />
              <rect
                x={padX + 2}
                y={rwyTop + 4}
                width={Math.max(0, thresholdX - padX - 2)}
                height={innerH - 8}
                fill="url(#dds-chevron)"
                opacity="0.6"
              />
              <rect
                x={padX + 2}
                y={rwyTop + 4}
                width={Math.max(0, thresholdX - padX - 2)}
                height={innerH - 8}
                fill="none"
                stroke={TOKENS.ddsBorder}
                strokeDasharray="4,4"
                strokeWidth="1.2"
              >
                <title>{t("runway_v2.tooltip_pre_threshold", { m: ddsM.toFixed(0) })}</title>
              </rect>
              {/* Beschriftung UNTER der Bahn, nicht darin.
                  §8.6.3: „Keine Beschriftung auf der Bahnfläche, ausser den
                  Bahnkennungen."

                  Vorher stand sie mittig in der Verbotszone. Bei EDDH 23 ist
                  diese Zone 156 m lang — bei 3250 m Bahn rund 51 Pixel breit,
                  während „LANDUNG VERBOTEN" bei elf Punkt Schriftgrösse
                  siebenundneunzig Pixel braucht. Der Text ragte also fast um
                  das Doppelte über seine eigene Zone hinaus und lag dabei auf
                  der roten Schraffur: rot auf rot, unlesbar. Ein Konturrand
                  (`stroke` + `paintOrder`) sollte das auffangen und machte es
                  eher schlimmer.

                  Die Referenzgrafik setzt beide Zeilen unter die Bahn. Dort
                  ist beliebig viel Platz, und der Zusammenhang zur Zone bleibt
                  über die gemeinsame Mitte erhalten. */}
              {/* Linksbuendig am Bahnanfang, nicht mittig in der Zone.

                  Mittig ist nur solange richtig, wie die Zone schmal ist.
                  Bei OLBA 35 ist sie 820 m lang, ihre Mitte liegt damit weit
                  in der Bahn hinein — und genau dort steht die
                  Versatz-Beschriftung des Aufsetzpunkts. Am Bahnanfang
                  verankert ist die Position unabhaengig von der Zonenbreite. */}
              <text
                x={padX}
                y={rwyBot + 18}
                fontSize={sf(9.5)}
                fill={TOKENS.tdSevere}
                fontFamily="monospace"
              >
                {t("runway_v2.dds_forbidden")}
              </text>
              <text
                x={padX}
                y={rwyBot + 18 + zeile(9.5)}
                fontSize={sf(9.5)}
                fill={TOKENS.tdSevere}
                fontFamily="monospace"
              >
                {t("runway_v2.dds_prefix")} {ddsM.toFixed(0)} m
              </text>
            </g>
          )}

          {/* Landethreshold-Streifen — am Ort thresholdX (= 0m from
              landing threshold). Bei aktivem DDS verschoben nach
              rechts. */}
          <g>
            {Array.from({ length: 8 }, (_, i) => (
              <rect
                key={i}
                x={thresholdX + 4}
                y={rwyTop + 4 + (i * (innerH - 8)) / 8}
                width={20}
                height={(innerH - 8) / 8 - 2}
                fill={TOKENS.threshold}
              />
            ))}
            {/* Senkrechte Solid-Line links der Chevrons — markiert
                eindeutig "ab HIER fängt das landbare Stück an". */}
            <line
              x1={thresholdX}
              y1={rwyTop + 4}
              x2={thresholdX}
              y2={rwyBot - 4}
              stroke="rgba(255,255,255,0.9)"
              strokeWidth="2"
            />
            <title>{t("runway_v2.tooltip_threshold")}</title>
          </g>

          {/* Bahn-Ende rechts — gespiegelte 8 weiße Streifen + solides
              weißes End-Band. Macht visuell klar dass die Bahn HIER
              aufhört und nicht endlos weiterläuft (User-Befund). */}
          <g>
            {Array.from({ length: 8 }, (_, i) => (
              <rect
                key={i}
                x={padX + innerW - 24}
                y={rwyTop + 4 + (i * (innerH - 8)) / 8}
                width={20}
                height={(innerH - 8) / 8 - 2}
                fill={TOKENS.threshold}
                opacity="0.7"
              />
            ))}
            <rect
              x={padX + innerW - 2}
              y={rwyTop + 4}
              width={4}
              height={innerH - 8}
              fill="rgba(255,255,255,0.9)"
            />
            <title>{t("runway_v2.tooltip_runway_end")}</title>
          </g>

          {/* TDZ-Box — gelbe Schraffur als Bereichs-Indikator + dünner
              Rahmen + Label. Die diagonale Schraffur soll visuell
              vermitteln "hier soll der Touchdown rein". */}
          {tdzEndX != null && tdzEndX > thresholdX + 24 && display.show_aufsetzzone_box && (
            <g>
              <defs>
                <pattern
                  id="tdz-hatch"
                  patternUnits="userSpaceOnUse"
                  width="10"
                  height="10"
                  patternTransform="rotate(45)"
                >
                  <line
                    x1="0"
                    y1="0"
                    x2="0"
                    y2="10"
                    stroke={TOKENS.tdzStroke}
                    strokeWidth="2"
                  />
                </pattern>
              </defs>
              <rect
                x={thresholdX + 24}
                y={rwyTop + 30}
                width={tdzEndX - thresholdX - 24}
                height={innerH - 60}
                fill="url(#tdz-hatch)"
                opacity="0.55"
              />
              <rect
                x={thresholdX + 24}
                y={rwyTop + 30}
                width={tdzEndX - thresholdX - 24}
                height={innerH - 60}
                fill={TOKENS.tdzFill}
                stroke={TOKENS.tdzStroke}
                strokeDasharray="6,5"
                strokeWidth="1"
              >
                <title>{t("runway_v2.tooltip_tdz", { m: props.td_tdz_length_m?.toFixed(0) })}</title>
              </rect>
              {/* Klammer OBERHALB der Bahn, Beschriftung darueber.

                  §8.3: „Aufsetzzone als gefuellte Flaeche mit Klammer
                  oberhalb der Bahn, nicht als zarte Schraffur darin — sie
                  ging auf dem dunklen Grund unter."
                  §8.6.3: „Keine Beschriftung auf der Bahnflaeche."

                  Vorher stand der Text in der Flaeche, gelb auf gelber
                  Schraffur. Die Klammer macht ausserdem sichtbar, WO die Zone
                  anfaengt und aufhoert — das leistet eine Schraffur ohne
                  Randmarken nicht. */}
              <line
                x1={thresholdX}
                y1={rwyTop - 14}
                x2={tdzEndX}
                y2={rwyTop - 14}
                stroke={TOKENS.tdzStroke}
                strokeWidth="1.5"
              />
              <line
                x1={thresholdX}
                y1={rwyTop - 14}
                x2={thresholdX}
                y2={rwyTop - 6}
                stroke={TOKENS.tdzStroke}
                strokeWidth="1.5"
              />
              <line
                x1={tdzEndX}
                y1={rwyTop - 14}
                x2={tdzEndX}
                y2={rwyTop - 6}
                stroke={TOKENS.tdzStroke}
                strokeWidth="1.5"
              />
              <text
                x={(thresholdX + tdzEndX) / 2}
                y={rwyTop - 20}
                fontSize={sf(11)}
                fill={TOKENS.tdzStroke}
                fontWeight="600"
                fontFamily="monospace"
                textAnchor="middle"
              >
                {t("runway_v2.aufsetzzone_prefix")} {props.td_tdz_length_m?.toFixed(0)} m
              </text>
            </g>
          )}

          {/* Centerline (gestrichelt). */}
          <line
            x1={thresholdX + 28}
            y1={rwyCl}
            x2={padX + innerW - 6}
            y2={rwyCl}
            stroke={TOKENS.centerline}
            strokeWidth="1.6"
            strokeDasharray={TOKENS.centerlineDashArray}
          />

          {/* Aim-Point — ICAO Annex 14 §5.2.6: GENAU ZWEI breite
              Streifen, symmetrisch zur Centerline. Ein Streifen liegt
              direkt OBERHALB der CL, einer direkt UNTERHALB. (Frühere
              v2-Version hatte 4 kleine Quadrate in 2×2 = falsche
              "Stufen"-Optik — User-Befund 2026-05-13.) Streifen-Breite
              hier 24 px (entspricht ~50 m Real-Länge, ICAO gibt 30–60 m
              je nach Bahn). */}
          {aimX != null && display.show_aim_marker && (
            <g>
              <rect
                x={aimX - 12}
                y={rwyCl - 22}
                width={24}
                height={18}
                fill={TOKENS.aimMarker}
                opacity="0.95"
              />
              <rect
                x={aimX - 12}
                y={rwyCl + 4}
                width={24}
                height={18}
                fill={TOKENS.aimMarker}
                opacity="0.95"
              />
              {/* Pfeilspitze + Label oberhalb der Bahn — zeigt explizit
                  dass die zwei großen gelben Streifen die Aim-Point-
                  Markierungen sind (wie auf echten Runways gemalt). */}
              <polygon
                points={`${aimX - 7},${rwyTop - 14} ${aimX + 7},${rwyTop - 14} ${aimX},${rwyTop - 4}`}
                fill={TOKENS.aimMarker}
              />
              {/* Beschriftung UNTER der Bahn — der Platz darueber gehoert
                  jetzt der Aufsetzzonen-Klammer, und der Zielpunkt liegt
                  IN der Aufsetzzone: Beides oben haette einander
                  ueberdeckt. Die Referenzgrafik ordnet es ebenso an. */}
              <line
                x1={aimX}
                y1={rwyBot + 2}
                x2={aimX}
                y2={rwyBot + 9}
                stroke={TOKENS.aimMarker}
                strokeWidth="1.5"
              />
              <text
                x={aimX}
                y={rwyBot + 20}
                textAnchor="middle"
                fontSize={sf(10.5)}
                fill={TOKENS.aimMarker}
                fontWeight="600"
                fontFamily="monospace"
              >
                {t("runway_v2.aim_point_prefix")} {props.aim_point_m?.toFixed(0)} m
              </text>
              {/* Hier stand bis 23.08.2026 eine zweite Zeile
                  „↓ Soll-Aufsetz-Stelle". Sie kam aus der Webapp-Fassung
                  und wurde beim Zusammenführen der beiden Anzeigen
                  mitgenommen, damit nichts lautlos verschwindet.

                  Thomas hat sie in der Demo gesehen und gefragt, ob das
                  nicht doppelt sei. Es war doppelt UND falsch: Auf den
                  Aim-Point wird gezielt, aufgesetzt wird durch den Flare
                  typisch 50–150 m dahinter — der Tooltip an genau diesem
                  Marker sagt es selbst. Eine Beschriftung, die dem
                  Erklärtext daneben widerspricht, ist schlimmer als keine.

                  Was der Marker bedeutet, steht im Tooltip und im
                  Glossar. */}
              <title>{t("runway_v2.tooltip_aim_point", { m: props.aim_point_m?.toFixed(0) })}</title>
            </g>
          )}

          {/* Ausroll-Linie (Schein + Kern) — endet an `rolloutEndeX`. */}
          {rolloutEndeX != null && rolloutEndeX > tdX && (
            <g>
              <line
                x1={tdX}
                y1={tdY}
                x2={rolloutEndeX}
                y2={tdY}
                stroke={TOKENS.rolloutGlow}
                strokeWidth="14"
              />
              <line
                x1={tdX}
                y1={tdY}
                x2={rolloutEndeX}
                y2={tdY}
                stroke={TOKENS.rollout}
                strokeWidth="3"
                opacity="0.75"
              />
            </g>
          )}

          {/* Titel der Ansicht — beide Ansichten tragen einen, sonst ist
              beim Blick auf zwei Bahnrechtecke nicht klar, was welche zeigt. */}
          <text x={padX} y={16} fontSize={sf(10.5)} letterSpacing={1.4} fill="#8B95A8">
            {t("runway_v2.laengs_title", {
              defaultValue: "LÄNGS — WO AUFGESETZT, WO GERÄUMT",
            })}
          </text>

          {/* „BAHN-ENDE" unter der Sperrfläche am Bahnende — das
              Gegenstück zur Verbotszone vorne (§8.3). Ohne Beschriftung
              liest sich der rote Streifen rechts wie ein zweiter
              Landeverbotsbereich. */}
          <text
            x={padX + innerW}
            y={rwyBot + 18}
            textAnchor="end"
            fontSize={sf(9.5)}
            fill={TOKENS.tdSevere}
            fontFamily="monospace"
          >
            {t("runway_v2.runway_end", { defaultValue: "BAHN-ENDE" })}
          </text>

          {/* Ende der Ausrollstrecke, wenn KEIN Räumpunkt bekannt ist.

              Eine Linie ohne Endpunkt hoert im Nichts auf, und der Leser
              fragt sich, wo das Flugzeug geblieben ist. Der alte
              Bremspunkt-Marker leistete das nebenbei; er ist mit v1.7.0
              entfallen, weil seine AUSSAGE („bei 40 kt ist etwas Bewertbares
              passiert") nicht haltbar war. Das Ende der Linie zu markieren
              ist etwas anderes: Es behauptet nichts, es zeigt, wo die
              Aufzeichnung endet.

              Bei Fluegen ab v1.7.0 uebernimmt das der Raeumpunkt darunter.
              Bei aelteren gibt es nur `rollout_m` — dann steht hier die
              Marke, ohne Geschwindigkeit und ohne Seite, weil beides nicht
              erfasst wurde. */}
          {raeumX == null && rolloutEndeX != null && (
            <g>
              <path
                d={`M ${rolloutEndeX} ${tdY - 9} l 8 9 l -8 9 l -8 -9 z`}
                fill={TOKENS.rollout}
                opacity="0.85"
              />
              <line
                x1={rolloutEndeX}
                y1={rwyTop - 30}
                x2={rolloutEndeX}
                y2={tdY - 13}
                stroke={TOKENS.rollout}
                strokeWidth="1"
                opacity="0.35"
              />
              {/* Am Bildrand geklemmt — dieselbe Regel wie bei der
                  TD-Beschriftung: Eine mittig gesetzte Zeile, die einem
                  beweglichen Punkt folgt, läuft am Rand hinaus. Bei einem
                  Ausrollende nahe der Bahnkante endete sie bei x = 1201,
                  einen Pixel ausserhalb. */}
              <text
                x={Math.min(
                  Math.max(rolloutEndeX, padX + 70),
                  padX + innerW - 70,
                )}
                y={rwyTop - 36}
                textAnchor="middle"
                fontSize={sf(10)}
                fill="#8B95A8"
                fontFamily="monospace"
              >
                {t("runway_v2.rollout_end", { defaultValue: "AUSROLLEN ENDE" })}
                {/* Die STELLE, nicht die gefahrene Strecke.

                    Vorher stand hier `rollout_m` — die Strecke ab dem
                    Aufsetzpunkt. Auf einer Achse, deren Lineal und deren
                    andere Marken („TD 780 m", „BAHN GERAEUMT · 700 m")
                    durchweg Stellen ab der Schwelle nennen, liest sich
                    diese eine Zahl zwangslaeufig auch als Stelle. Bei
                    einem Aufsetzpunkt von 780 m und 1100 m Ausrollen
                    stand die Marke am Bahnende und war mit „1100 m"
                    beschriftet, waehrend das Lineal darunter 1500 m
                    zeigte. Die gefahrene Strecke steht weiter in der
                    Kennzahlen-Zeile, wo sie hingehoert. */}
                {ausrollEndeUeberBahn
                  ? ` · ${t("runway_v2.rollout_end_beyond", {
                      defaultValue: "hinter dem Bahnende",
                    })}`
                  : exitDistM != null
                    ? ` · ${exitDistM.toFixed(0)} m`
                    : ""}
              </text>
            </g>
          )}

          {/* Räumpunkt: Raute auf der Bahn, Beschriftung darüber, und die
              gestrichelte Spur in die ECHTE Ausfahrtsrichtung.

              Die Richtung ist keine Zierde: Sie steht nur da, wenn zwei
              unabhängige Größen sie bestätigen — Kursänderung UND
              Querbewegung (§8.6). Fehlt `clearance_side`, läuft die Spur
              gerade weiter, statt eine Seite zu behaupten. */}
          {raeumX != null && (
            <g>
              {(() => {
                // Nach oben = links in Landerichtung, dieselbe Konvention
                // wie in der Queransicht. Ohne bekannte Seite: waagerecht.
                const dy =
                  props.clearance_side === "left"
                    ? -1
                    : props.clearance_side === "right"
                    ? 1
                    : 0;
                const ende = Math.min(raeumX + 90, W - padX / 2);
                const bogen = `M ${raeumX} ${tdY} C ${raeumX + 32} ${tdY + dy * 3}, ${
                  raeumX + 58
                } ${tdY + dy * 14}, ${ende} ${tdY + dy * 42}`;
                return (
                  <path
                    d={bogen}
                    fill="none"
                    stroke={TOKENS.rollout}
                    strokeWidth="1.5"
                    strokeDasharray="3 4"
                    opacity="0.45"
                  />
                );
              })()}
              <path
                d={`M ${raeumX} ${tdY - 11} l 10 11 l -10 11 l -10 -11 z`}
                fill={TOKENS.rollout}
              />
              <line
                x1={raeumX}
                y1={rwyTop - 44}
                x2={raeumX}
                y2={tdY - 15}
                stroke={TOKENS.rollout}
                strokeWidth="1"
                opacity="0.45"
              />
              <text
                x={raeumX}
                y={rwyTop - 52}
                textAnchor="middle"
                fontSize={sf(11)}
                fontWeight="600"
                fill={TOKENS.rollout}
                fontFamily="monospace"
              >
                {t("runway_v2.cleared_title", { defaultValue: "BAHN GERÄUMT" })}
              </text>
              <text
                x={raeumX}
                y={rwyTop - 52 + zeile(11)}
                textAnchor="middle"
                fontSize={sf(10)}
                fill="#8B95A8"
                fontFamily="monospace"
              >
                {[
                  `${raeumM!.toFixed(0)} m`,
                  props.clearance_speed_kt != null
                    ? `${props.clearance_speed_kt.toFixed(0)} kt`
                    : null,
                  raeumAusfahrt
                    ? t("runway_v2.cleared_via", {
                        defaultValue: "Ausfahrt {{name}} {{seite}}",
                        name: raeumAusfahrt.name,
                        seite: t(
                          props.clearance_side === "left"
                            ? "runway_v2.side_left_word"
                            : "runway_v2.side_right_word",
                          { defaultValue: props.clearance_side === "left" ? "links" : "rechts" },
                        ),
                      })
                    : props.clearance_side != null
                    ? t(
                        props.clearance_side === "left"
                          ? "runway_v2.side_left_word"
                          : "runway_v2.side_right_word",
                        { defaultValue: props.clearance_side === "left" ? "links" : "rechts" },
                      )
                    : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </text>
            </g>
          )}

          {/* (Frühere "Bahn verbleibend X m" + Doppelpfeil-Annotation
              entfernt — war redundant mit der "Bahn-Auslastung X %"-
              Pill unter dem Diagramm und wirkte visuell laut.
              Die Bahn-Ende-Streifen rechts zeigen die Bahn-Grenze
              schon eindeutig, die Pills tragen die Zahlen.) */}

          {/* Offset-Indikator: großer Pfeil + Label UNTER der Bahn, mit
              dünner Anker-Linie zum TD-Dot. Bewusst außerhalb der
              Bahn-Fläche, damit es nicht hinter den AIM-Quadraten
              verschwindet wenn TD und Aim-Position fast übereinander
              liegen. Nur wenn |offset| > 0.5 m. */}
          {/* Der L/R-Pfeil unter der Bahn ist entfallen.

              Er zeigte den Versatz als waagerechten Doppelpfeil von der
              Mittellinie zum Aufsetzpunkt, mit der Meterzahl daneben — und
              lief dabei regelmaessig in die TD-Beschriftung hinein, weil
              beide unter der Bahn an fast derselben Stelle sitzen. Bei
              einem Versatz von wenigen Metern war der Pfeil ausserdem so
              kurz, dass links und rechts nicht zu unterscheiden waren.

              Die Aussage steht jetzt IN der TD-Zeile, als Wort: „TD 320 m ·
              6,6 m links". Ein Wort ist eindeutig, braucht keinen Platz
              neben dem Aufsetzpunkt und kann mit nichts kollidieren. */}

          {/* Touchdown-Punkt — Doppel-Glow + Solid Dot. */}
          <g>
            <circle cx={tdX} cy={tdY} r="22" fill={dotColor} opacity="0.10" />
            <circle cx={tdX} cy={tdY} r="14" fill={dotColor} opacity="0.22" />
            <circle cx={tdX} cy={tdY} r="9" fill={dotColor} stroke="#0c1628" strokeWidth="2" />
            <title>
              {t("runway_v2.tooltip_touchdown", {
                distance: props.td_distance_from_threshold_m.toFixed(0),
                beforeAfter: t(
                  props.td_distance_from_threshold_m < 0
                    ? "runway_v2.tooltip_word_before"
                    : "runway_v2.tooltip_word_after",
                ),
                lateral: Math.abs(props.td_centerline_offset_m).toFixed(1),
                side: t(
                  props.td_centerline_offset_m > 0.5
                    ? "runway_v2.tooltip_word_right"
                    : props.td_centerline_offset_m < -0.5
                    ? "runway_v2.tooltip_word_left"
                    : "runway_v2.tooltip_word_on",
                ),
              })}
            </title>
          </g>

          {/* Der Marker „Bremspunkt 40 kt" ist mit v1.7.0 ERSATZLOS
              entfallen — so steht es im Vertrag
              (docs/spec/runway-diagram-v2.contract.md, Abschnitt v1.7.0).

              Warum: Er behauptete eine Aussage, die die Achse nicht mehr
              trifft. Wie stark jemand bremst, hängt an der Anweisung des
              Lotsen, am Verkehr dahinter und an der Lage der Ausfahrten —
              alles Dinge, die der Recorder nicht kennt. Wer bis zum Ende
              der Bahn rollen soll, bremst nicht auf 40 kt herunter.

              Er nahm ausserdem den Platz oberhalb der Bahn ein, den die
              Aufsetzzonen-Klammer braucht, und brachte dafür eine
              dreistufige Ausweichlogik für seine eigenen Beschriftungen
              mit. Wo die Ausfahrten stehen, zeigt jetzt die Queransicht. */}

          {/* RWY-Designator (groß links) — die Landerichtung. */}
          <text
            x={padX / 2 - 4}
            y={rwyCl + 10}
            textAnchor="middle"
            fontSize={sf(28)}
            fill="#f1f5f9"
            fontWeight="800"
            fontFamily="monospace"
          >
            {props.runway_ident}
          </text>

          {/* Gegen-RWY-Designator + Bahnlänge rechts. Gegen-Designator
              zeigt klar dass die Bahn da endet (= Gegen-Richtung,
              z. B. RWY 32 ↔ RWY 14). Plus Bahn-Gesamtlänge darunter. */}
          {display.show_opposite_runway && (
            <text
              x={W - padX / 2 + 8}
              y={rwyCl - 2}
              textAnchor="middle"
              fontSize={sf(20)}
              fill="#94a3b8"
              fontWeight="700"
              fontFamily="monospace"
              opacity="0.85"
            >
              {oppositeRunway(props.runway_ident)}
            </text>
          )}
          {display.show_bahn_length && (
            <text
              x={W - padX / 2 + 8}
              y={rwyCl + 18}
              textAnchor="middle"
              fontSize={sf(11)}
              fill="#64748b"
              fontFamily="monospace"
            >
              {props.length_m.toFixed(0)} m
            </text>
          )}

          {/* (Landerichtungs-Pfeil entfernt — die neuen End-Streifen
              + der Gegen-RWY-Designator zeigen das Bahn-Ende
              eindeutiger als der Pfeil es konnte.) */}

          {/* TD-Label unter dem Dot — nur Distanz. L/R wird durch den
              großen L/R-Pfeil oben dargestellt. Bei Offset < 0.5 m
              steht hier zusätzlich "auf CL". */}
          <g>
            {/* Am Bildrand geklemmt: Bei einem Aufsetzpunkt direkt auf der
                Schwelle steht die Beschriftung mittig ueber x = padX, und
                ihre linke Haelfte liegt dann ausserhalb des Zeichenbereichs
                (§8.6.2). Dieselbe Klemmung braucht jede mittig gesetzte
                Beschriftung, die einem beweglichen Punkt folgt. */}
            <text
              x={Math.min(
                Math.max(tdX, padX + tdLabelHalb),
                padX + innerW - tdLabelHalb,
              )}
              y={rwyBot + 46}
              textAnchor="middle"
              fontSize={sf(13)}
              fill={dotColor}
              fontWeight="700"
              fontFamily="monospace"
            >
              TD {props.td_distance_from_threshold_m.toFixed(0)} m
              {Math.abs(props.td_centerline_offset_m) < 0.5
                ? " · " + t("runway_v2.auf_cl")
                : ` · ${Math.abs(props.td_centerline_offset_m).toFixed(1)} m ${t(
                    props.td_centerline_offset_m < 0
                      ? "runway_v2.side_left_word"
                      : "runway_v2.side_right_word",
                    { defaultValue: props.td_centerline_offset_m < 0 ? "links" : "rechts" },
                  )}`}
            </text>
          </g>

          {/* Distanz-Skala unter der Bahn. */}
          <g>
            <line
              x1={padX}
              y1={rwyBot + 62}
              x2={padX + innerW}
              y2={rwyBot + 62}
              stroke="rgba(255,255,255,0.25)"
              strokeWidth="1"
            />
            {scaleTicks.map((d) => {
              const x = mToX(d);
              return (
                <g key={d}>
                  <line
                    x1={x}
                    y1={rwyBot + 57}
                    x2={x}
                    y2={rwyBot + 67}
                    stroke="rgba(255,255,255,0.5)"
                    strokeWidth="1.2"
                  />
                  <text
                    x={x}
                    y={rwyBot + 80}
                    textAnchor="middle"
                    fontSize={sf(10)}
                    fill="rgba(255,255,255,0.55)"
                    fontFamily="monospace"
                  >
                    {d} m
                  </text>
                </g>
              );
            })}
          </g>
        </svg>

        {/* Bedienung: Der Hinweis steht nur da, solange nicht gezoomt ist —
            danach erklärt sich der Zustand selbst, und der Platz gehört dem
            Zurücksetzen. */}
        <div
          // Zoom-Hinweis und -Knöpfe: auf Papier sinnlos. Sie hatten
          // keinen Klassennamen und waren deshalb für das Druck-CSS
          // unerreichbar — im exportierten Bericht stand mitten in der
          // Grafik „Strg + Mausrad zoomt · Ziehen verschieben" mit zwei
          // Knöpfen daneben.
          className="bahn-nur-bildschirm"
          style={{
            display: "flex",
            justifyContent: "flex-end",
            alignItems: "center",
            gap: 10,
            fontSize: "0.72rem",
            color: "#64748b",
            marginTop: 4,
          }}
        >
          <span>
            {zoom.gezoomt
              ? `${projektion.sichtVonM.toFixed(0)}–${projektion.sichtBisM.toFixed(0)} m`
              : t("runway_v2.zoom_hint", {
                  defaultValue: "Strg + Mausrad zoomt · Ziehen verschiebt",
                })}
          </span>
          {/* Knöpfe für alle, die nicht mit Tastatur und Rad hantieren
              wollen. Sie zoomen auf die Mitte des Ausschnitts. */}
          <button
            type="button"
            onClick={() => zoom.stufe(-1)}
            disabled={!zoom.gezoomt}
            title={t("runway_v2.zoom_out", { defaultValue: "Weiter weg" })}
            style={zoomKnopf(!zoom.gezoomt)}
          >
            −
          </button>
          <button
            type="button"
            onClick={() => zoom.stufe(1)}
            title={t("runway_v2.zoom_in", { defaultValue: "Näher" })}
            style={zoomKnopf(false)}
          >
            +
          </button>
          {zoom.gezoomt && (
            <button
              type="button"
              onClick={zoom.zuruecksetzen}
              style={{ ...zoomKnopf(false), padding: "2px 8px" }}
            >
              {t("runway_v2.zoom_reset", { defaultValue: "Ganze Bahn" })}
            </button>
          )}
        </div>

        {/* ─── 2b. QUERANSICHT + EREIGNISSE + GROESSENVERGLEICH ────────
            v1.7.0, Spec §8.3. Im SELBEN Container wie die Laengsansicht,
            damit beide dieselbe Breite haben und die Kanten fluchten -- der
            Aufsetzpunkt oben muss senkrecht ueber der Marke unten liegen.
            Zwei getrennte SVGs statt eines grossen: So kann aus der einen
            Ansicht nichts in die andere ragen (§8.6.4), und der Zwischenraum
            bleibt ohne Zutun frei. */}
        <div style={{ marginTop: 14 }}>
          <RunwayDisciplinePanel
            props={props}
            projektion={projektion}
            zoom={zoom}
            width={W}
            tokens={{
              tarmac: TOKENS.tarmac,
              tarmacBorder: TOKENS.tarmacBorder,
              centerline: TOKENS.centerline,
              rollout: TOKENS.rollout,
              tdPerfect: TOKENS.tdPerfect,
              tdWarn: TOKENS.tdWarn,
              tdSevere: TOKENS.tdSevere,
            }}
          />
        </div>
      </div>

      {/* ─── 3. LEGENDE ─────────────────────────────────────────────── */}
      <div
        style={{
          display: "flex",
          gap: 18,
          flexWrap: "wrap",
          minWidth: 0,
          fontSize: "0.78rem",
          opacity: 0.85,
          padding: "0 4px",
        }}
      >
        <LegendItem swatch={TOKENS.threshold} label={t("runway_v2.legend_threshold")} />
        {tdzEndX && display.show_aufsetzzone_box && <LegendItem swatch={TOKENS.tdzStroke} label={t("runway_v2.legend_tdz")} />}
        {aimX && display.show_aim_marker && <LegendItem swatch={TOKENS.aimMarker} label={t("runway_v2.legend_aim")} />}
        <LegendDot color={dotColor} label={t("runway_v2.legend_td")} />
        {ddsActive && <LegendItem swatch={TOKENS.ddsBorder} label={t("runway_v2.legend_pre_threshold")} />}
      </div>

      {/* ─── 4. DETAIL-PILLS ─────────────────────────────────────────
          v2.2 Layout-Switch: vom 3-Box-Layout (jeweils mehrere Rows
          drin) auf atomare Pill-Cards (1 Stat pro Pill) wie im
          Pilot-Client-Legacy-Layout. Macht den Block kompakter und
          info-dichter — Piloten finden den gewünschten Wert schneller. */}
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 8,
        }}
      >
        <Pill label={t("runway_v2.pill_bahn")} value={`${props.airport_ident}/${props.runway_ident}${props.surface ? ` (${t(surfaceLabelKey(props.surface)) || props.surface})` : ""}`} />
        <Pill label={t("runway_v2.pill_laenge")} value={`${props.length_m.toFixed(0)} m`} />
        <Pill
          label={t("runway_v2.pill_hinter_schwelle")}
          value={`${props.td_distance_from_threshold_m.toFixed(0)} m`}
          tone={
            props.pre_displaced_threshold === true
              ? "bad"
              : props.td_distance_from_threshold_m < 0
              ? "bad"
              : props.td_distance_from_threshold_m > skin.thresholds.hinter_schwelle_warn_above
              ? "warn"
              : "good"
          }
        />
        <Pill
          label={t("runway_v2.pill_mittellinie")}
          value={
            Math.abs(props.td_centerline_offset_m) < 0.5
              ? t("runway_v2.auf_cl")
              : `${Math.abs(props.td_centerline_offset_m).toFixed(1)} m ${
                  props.td_centerline_offset_m > 0 ? t("runway_v2.centerline_right") : t("runway_v2.centerline_left")
                }`
          }
          tone={
            Math.abs(props.td_centerline_offset_m) < skin.thresholds.centerline_warn_above
              ? "good"
              : Math.abs(props.td_centerline_offset_m) < skin.thresholds.centerline_bad_above
              ? "warn"
              : "bad"
          }
        />
        {props.rollout_m != null && (
          <Pill label={t("runway_v2.pill_ausrollstrecke")} value={`${props.rollout_m.toFixed(0)} m`} />
        )}
        {bahnUsedPct != null && (
          <Pill
            label={t("runway_v2.pill_bahn_auslastung")}
            // v0.20.0: ueber 100 % heisst Overrun — das war vorher auf 100
            // geklemmt und las sich als exakt volle Bahn. Jenseits von 200 %
            // ist die Zahl aber keine Aussage mehr ueber die Landung, sondern
            // ueber eine kaputte Bahn-Zuordnung (Buschflug, Fehl-Match) — dann
            // lieber ">200 %" zeigen als eine praezise wirkende Absurditaet.
            value={
              bahnUsedPct > 200 ? "> 200 %" : `${bahnUsedPct.toFixed(0)} %`
            }
            // v1.7.0: KEINE Bewertungsfarbe mehr auf der Auslastung.
            //
            // Die Achse bewertet nicht mehr, wie viel Bahn jemand gebraucht
            // hat -- das haengt an der Anweisung des Lotsen, am Verkehr
            // dahinter und an der Lage der Ausfahrten, alles Dinge, die der
            // Recorder nicht kennt. Eine gelbe Pill neben einer Landung mit
            // voller Punktzahl waere ein Widerspruch, den niemand aufloesen
            // kann: Der Wert bleibt als Information stehen, die Wertung faellt.
            //
            // Ueber 100 % bleibt rot -- das ist kein Auslastungsgrad mehr,
            // sondern ein Ueberrollen, und das IST ein Kriterium (Spec §5.4).
            tone={bahnUsedPct > 100 ? "bad" : "neutral"}
          />
        )}
        {props.td_in_tdz != null && (
          <Pill
            label={t("runway_v2.pill_tdz")}
            value={
              props.td_in_tdz
                ? `✓ ${props.td_third ? t(thirdLabelKey(props.td_third)) : t("runway_v2.tdz_hit_marker")}`
                : `✗ ${props.td_third ? t(thirdLabelKey(props.td_third)) : t("runway_v2.tdz_missed_marker")}`
            }
            tone={props.td_in_tdz ? "good" : "warn"}
          />
        )}
        {props.aim_class && props.aim_delta_m != null && props.aim_point_m != null && (
          <Pill
            label={t("runway_v2.pill_aim_point")}
            value={`${props.aim_point_m.toFixed(0)} m · Δ ${props.aim_delta_m >= 0 ? "+" : ""}${props.aim_delta_m.toFixed(0)} m · ${t(aimClassLabelKey(props.aim_class))}`}
            tone={aimTone(props.aim_class)}
          />
        )}
        {props.tch_actual_ft != null && props.tch_class && (
          <Pill
            label={t("runway_v2.pill_tch")}
            value={`${props.tch_actual_ft.toFixed(0)} ft${props.tch_delta_ft != null ? ` · Δ ${props.tch_delta_ft >= 0 ? "+" : ""}${props.tch_delta_ft.toFixed(0)} ft` : ""} · ${t(tchClassLabelKey(props.tch_class))}`}
            tone={tchTone(props.tch_class)}
          />
        )}
        {props.pre_displaced_threshold === true && (
          <Pill
            label={t("runway_v2.pill_pre_threshold")}
            value={t("runway_v2.pill_pre_threshold_value")}
            tone="bad"
          />
        )}
        <Pill
          label={t("runway_v2.pill_navdata")}
          value={
            props.source === "navigraph"
              ? `${t("runway_v2.source_vps_navdata")} · AIRAC ${props.nav_cycle ?? "?"}`
              : props.source === "ourairports_fallback"
              ? t("runway_v2.source_ourairports_fallback_short")
              : t("runway_v2.source_ourairports_legacy")
          }
          tone={
            props.source === "navigraph"
              ? "good"
              : props.source === "ourairports_fallback"
              ? "warn"
              : "neutral"
          }
        />
        {display.show_flugzeug_bar && <FlugzeugBar props={props} />}
      </div>

      {glossaryOpen && (
        <GlossaryModal onClose={() => setGlossaryOpen(false)} />
      )}
    </section>
  );
}

// ─── Kleine UI-Helpers ──────────────────────────────────────────────

/** Einheitlicher Stil für die Zoom-Knöpfe. */
function zoomKnopf(aus: boolean): React.CSSProperties {
  return {
    background: "rgba(255,255,255,0.06)",
    border: "1px solid rgba(255,255,255,0.15)",
    borderRadius: 4,
    color: aus ? "#475569" : "#cbd5e1",
    fontSize: "0.8rem",
    lineHeight: 1,
    padding: "3px 8px",
    cursor: aus ? "default" : "pointer",
  };
}

// FlugzeugBar — eine Pill-Höhen-große Box, voll Breite (flex 1 1 100%),
// die ALLE Aircraft-Daten inline trägt. Wenn die Werte für eine Zeile
// zu lang sind, wrappen sie via flex-wrap auf eine zweite Zeile —
// die Pill bleibt damit auf einer Bildschirm-Zeile so lang wie nötig,
// nicht "höher" durch gestapelte Rows.
function FlugzeugBar({ props }: { props: RunwayDiagramV2Props }) {
  const { t } = useTranslation();
  const skin = useV2Skin();
  const has =
    props.aircraft_icao ||
    props.aircraft_title ||
    props.landing_weight_kg != null ||
    props.landing_speed_kt != null ||
    props.landing_peak_g_force != null ||
    props.landing_scored_g_force != null ||
    props.headwind_kt != null ||
    props.crosswind_kt != null;
  if (!has) return null;

  // Sub-Stat-Items mit optionaler Tone-Farbe.
  type Item = { label: string; value: string; color?: string };
  const items: Item[] = [];

  // Aircraft-Header
  const acName = props.aircraft_title || props.aircraft_icao;
  if (acName) {
    items.push({ label: t("runway_v2.flugzeug_type"), value: String(acName) });
  }
  if (props.aircraft_registration) {
    items.push({ label: t("runway_v2.flugzeug_reg"), value: props.aircraft_registration });
  }

  // Landegewicht ± Plan
  if (props.landing_weight_kg != null) {
    const realT = (props.landing_weight_kg / 1000).toFixed(1);
    if (props.planned_ldw_kg != null) {
      const deltaT = (props.landing_weight_kg - props.planned_ldw_kg) / 1000;
      const sign = deltaT >= 0 ? "+" : "";
      items.push({
        label: t("runway_v2.flugzeug_weight"),
        value: `${realT} t (Δ ${sign}${deltaT.toFixed(1)} t)`,
      });
    } else {
      items.push({ label: t("runway_v2.flugzeug_weight"), value: `${realT} t` });
    }
  }

  // TD-IAS
  if (props.landing_speed_kt != null) {
    items.push({ label: t("runway_v2.flugzeug_iast"), value: `${props.landing_speed_kt.toFixed(0)} kt` });
  }

  // Pitch / Bank
  if (props.landing_pitch_deg != null || props.landing_bank_deg != null) {
    const p = props.landing_pitch_deg?.toFixed(1) ?? "—";
    const b = props.landing_bank_deg?.toFixed(1) ?? "—";
    const tailStrike =
      props.landing_pitch_deg != null && props.landing_pitch_deg < skin.thresholds.pitch_bad_below;
    const bankWarn =
      props.landing_bank_deg != null &&
      Math.abs(props.landing_bank_deg) > skin.thresholds.bank_warn_above;
    items.push({
      label: t("runway_v2.flugzeug_pb"),
      value: `${p}° / ${b}°`,
      color: tailStrike ? "#ef4444" : bankWarn ? "#fbbf24" : undefined,
    });
  }

  // G-Kraft — v0.12.3 (LE9): den gescorten (EMA) Wert zeigen/färben,
  // sonst Fallback auf den rohen Peak.
  {
    const g = props.landing_scored_g_force ?? props.landing_peak_g_force ?? null;
    if (g != null) {
      items.push({
        label: t("runway_v2.flugzeug_peakg"),
        value: `${g.toFixed(2)} g`,
        color:
          g >= skin.thresholds.peak_g_bad
            ? "#ef4444"
            : g >= skin.thresholds.peak_g_warn
            ? "#fbbf24"
            : "#22c55e",
      });
    }
  }

  // Wind
  if (props.headwind_kt != null || props.crosswind_kt != null) {
    const parts: string[] = [];
    const hw = props.headwind_kt;
    const xw = props.crosswind_kt;
    if (hw != null) {
      parts.push(hw >= 0 ? `HW ${Math.abs(hw).toFixed(0)}` : `TW ${Math.abs(hw).toFixed(0)}`);
    }
    if (xw != null && Math.abs(xw) >= 1) {
      const side = xw > 0 ? "R" : "L";
      parts.push(`XW ${Math.abs(xw).toFixed(0)} ${side}`);
    }
    const xwAbs = xw != null ? Math.abs(xw) : 0;
    const isTw = hw != null && hw < -skin.thresholds.tailwind_bad;
    const color =
      xwAbs > skin.thresholds.crosswind_bad || isTw
        ? "#ef4444"
        : xwAbs > skin.thresholds.crosswind_warn
        ? "#fbbf24"
        : undefined;
    items.push({ label: t("runway_v2.flugzeug_wind"), value: parts.join(" "), color });
  }

  return (
    <div
      style={{
        padding: "8px 12px",
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.10)",
        borderRadius: 8,
        display: "flex",
        flexDirection: "column",
        gap: 2,
        // flex 999 1 0: basis=0 zwingt Bar dazu, exakt den Restplatz
        // bündig bis zur rechten Container-Kante auszufüllen (statt
        // bei flex-basis auto am Content-Ende zu stoppen). Damit
        // alignment mit den Pills der Zeile darüber.
        flex: "999 1 0",
        minWidth: 320,
      }}
    >
      <div
        style={{
          fontSize: "0.68rem",
          fontWeight: 700,
          letterSpacing: 1.1,
          textTransform: "uppercase",
          opacity: 0.65,
        }}
      >
        {t("runway_v2.flugzeug_label")}
      </div>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: "2px 12px",
          fontSize: "0.88rem",
          fontWeight: 700,
          alignItems: "baseline",
        }}
      >
        {items.map((it, i) => (
          <span key={i}>
            <span style={{ opacity: 0.55, fontWeight: 600, marginRight: 4 }}>
              {it.label}
            </span>
            <span style={{ color: it.color ?? "#e2e8f0" }}>{it.value}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

// Atomare Stat-Pille — 1 Label + 1 Value, optionale Tone-Farbe am Wert.
// Ersetzt das alte 3-Box-DetailCard-Layout (v2.2).
function Pill({
  label,
  value,
  tone = "neutral",
}: {
  label: string;
  value: string;
  tone?: "good" | "warn" | "bad" | "neutral";
}) {
  const valueColor =
    tone === "good"
      ? "#22c55e"
      : tone === "warn"
      ? "#fbbf24"
      : tone === "bad"
      ? "#ef4444"
      : "#e2e8f0";
  return (
    <div
      style={{
        padding: "8px 12px",
        background: "rgba(255,255,255,0.04)",
        border: "1px solid rgba(255,255,255,0.10)",
        borderRadius: 8,
        display: "flex",
        flexDirection: "column",
        gap: 2,
        minWidth: 110,
        // v2.x: maxWidth weg + flex 1 1 auto → Pills wachsen
        // proportional zum Restplatz ihrer Zeile, sodass jede Zeile
        // bündig bis zur Container-Kante reicht. Damit AIM-POINT-
        // Pill oben und FlugzeugBar unten an derselben x-Position
        // enden.
        flex: "1 1 auto",
      }}
    >
      <div
        style={{
          fontSize: "0.68rem",
          fontWeight: 700,
          letterSpacing: 1.1,
          textTransform: "uppercase",
          opacity: 0.65,
        }}
      >
        {label}
      </div>
      <div style={{ fontSize: "0.95rem", fontWeight: 700, color: valueColor }}>
        {value}
      </div>
    </div>
  );
}

function LegendItem({ swatch, label }: { swatch: string; label: string }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      <span
        style={{
          width: 14,
          height: 8,
          background: swatch,
          display: "inline-block",
          borderRadius: 2,
        }}
      />
      {label}
    </span>
  );
}

function LegendDot({ color, label }: { color: string; label: string }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      <span
        style={{
          width: 10,
          height: 10,
          background: color,
          borderRadius: 999,
          display: "inline-block",
        }}
      />
      {label}
    </span>
  );
}

// ─── Pure label helpers ─────────────────────────────────────────────

// Gegen-Bahn-Designator. RWY 32 ↔ RWY 14, RWY 24L ↔ RWY 06R, ...
function oppositeRunway(ident: string): string {
  const m = ident.match(/^(\d{1,2})([LRC]?)$/i);
  if (!m) return "?";
  const num = parseInt(m[1]!, 10);
  if (Number.isNaN(num) || num < 1 || num > 36) return "?";
  let opp = num + 18;
  if (opp > 36) opp -= 36;
  const suffix = m[2]?.toUpperCase() ?? "";
  const oppSuffix = suffix === "L" ? "R" : suffix === "R" ? "L" : suffix;
  return String(opp).padStart(2, "0") + oppSuffix;
}

// Helper liefern i18n-Keys (nicht direkt Strings) — Caller löst via t() auf.
function surfaceLabelKey(s: string): string {
  const map: Record<string, string> = {
    ASP: "runway_v2.surface_asp",
    CON: "runway_v2.surface_con",
    GRV: "runway_v2.surface_grv",
    GRS: "runway_v2.surface_grs",
    DIRT: "runway_v2.surface_dirt",
    TURF: "runway_v2.surface_turf",
  };
  return map[s.toUpperCase()] ?? "";
}

function thirdLabelKey(n: 1 | 2 | 3): string {
  return n === 1 ? "runway_v2.third_1" : n === 2 ? "runway_v2.third_2" : "runway_v2.third_3";
}

function aimClassLabelKey(c: AimClass): string {
  switch (c) {
    case "perfect":
      return "runway_v2.aim_perfect";
    case "short_of_aim":
      return "runway_v2.aim_short";
    case "past_aim":
      return "runway_v2.aim_past";
    case "long_landing":
      return "runway_v2.aim_long_landing";
    case "severe":
      return "runway_v2.aim_severe";
  }
}

function aimTone(c: AimClass): "good" | "warn" | "bad" {
  if (c === "perfect" || c === "past_aim" || c === "short_of_aim") return "good";
  if (c === "long_landing") return "warn";
  return "bad";
}

function tchClassLabelKey(c: TchClass): string {
  switch (c) {
    case "on_profile":
      return "runway_v2.tch_on_profile";
    case "slightly_low":
      return "runway_v2.tch_slightly_low";
    case "slightly_high":
      return "runway_v2.tch_slightly_high";
    case "high":
      return "runway_v2.tch_high";
    case "below_profile":
      return "runway_v2.tch_below_profile";
  }
}

function tchTone(c: TchClass): "good" | "warn" | "bad" {
  if (c === "on_profile") return "good";
  if (c === "slightly_low" || c === "slightly_high") return "warn";
  return "bad";
}
