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
  const skin = useV2Skin();
  const TOKENS = skin.tokens;
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
  const lengthM = Math.max(500, props.length_m);
  const ddsM = props.displaced_threshold_m ?? 0;
  const ddsActive = ddsM > 0;
  const totalVisualM = lengthM + ddsM;

  // thresholdX = Pixel-Position des Landethresholds.
  //   ohne DDS: thresholdX == padX (Bahn-Anfang IS Threshold)
  //   mit DDS:  thresholdX > padX (DDS-Bereich beansprucht erste ddsM)
  const thresholdX = padX + (ddsM / totalVisualM) * innerW;

  // Meter → X-Pixel. Eingabe m ist Distanz VOM LANDETHRESHOLD (signed).
  // Negative m → vor dem Threshold (= in der DDS-Zone).
  const mToX = (m: number) =>
    thresholdX +
    (Math.max(-ddsM, Math.min(lengthM, m)) / totalVisualM) * innerW;

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
      style={{ width: "100%", display: "flex", flexDirection: "column", gap: 12 }}
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
        }}
      >
        <svg
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="xMidYMid meet"
          style={{ width: "100%", height: "auto", display: "block" }}
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
              {/* DDS-Label am UNTEREN Rand der Zone — sodass es nicht
                  mit der AUFSETZZONE-Beschriftung kollidiert die am
                  oberen Rand der TDZ-Box rechts davon sitzt. */}
              <text
                x={(padX + thresholdX) / 2}
                y={rwyBot - 12}
                textAnchor="middle"
                fontSize="12"
                fill="#fecaca"
                fontWeight="800"
                fontFamily="monospace"
                stroke="#7c1d1d"
                strokeWidth="2.5"
                paintOrder="stroke"
              >
                {t("runway_v2.dds_prefix")} {ddsM.toFixed(0)} m
              </text>
              <text
                x={(padX + thresholdX) / 2}
                y={rwyBot - 26}
                textAnchor="middle"
                fontSize="11"
                fill="#fecaca"
                fontWeight="700"
                fontFamily="monospace"
                stroke="#7c1d1d"
                strokeWidth="2.5"
                paintOrder="stroke"
              >
                {t("runway_v2.dds_forbidden")}
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
          {tdzEndX != null && tdzEndX > thresholdX + 24 && (
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
              <text
                x={thresholdX + 24 + (tdzEndX - thresholdX - 24) / 2}
                y={rwyTop + 18}
                fontSize="12"
                fill={TOKENS.tdzStroke}
                fontWeight="700"
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
          {aimX != null && (
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
              <text
                x={aimX}
                y={rwyTop - 32}
                textAnchor="middle"
                fontSize="13"
                fill={TOKENS.aimMarker}
                fontWeight="700"
                fontFamily="monospace"
              >
                {t("runway_v2.aim_point_prefix")} {props.aim_point_m?.toFixed(0)} m
              </text>
              <text
                x={aimX}
                y={rwyTop - 19}
                textAnchor="middle"
                fontSize="10"
                fill={TOKENS.aimMarker}
                fontFamily="monospace"
                opacity="0.85"
              >
                {t("runway_v2.aim_subtitle")}
              </text>
              <title>{t("runway_v2.tooltip_aim_point", { m: props.aim_point_m?.toFixed(0) })}</title>
            </g>
          )}

          {/* Rollout-Linie (Glow + Solid). */}
          {exitX != null && (
            <g>
              <line
                x1={tdX}
                y1={tdY}
                x2={exitX}
                y2={tdY}
                stroke={TOKENS.rolloutGlow}
                strokeWidth="14"
              />
              <line
                x1={tdX}
                y1={tdY}
                x2={exitX}
                y2={tdY}
                stroke={TOKENS.rollout}
                strokeWidth="3"
                opacity="0.75"
              />
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
          {Math.abs(props.td_centerline_offset_m) > 0.5 && (() => {
            const isLeftOffset = props.td_centerline_offset_m < 0;
            // Pfeil-Group direkt unter der Bahn — über dem TD-Distanz-Label.
            const arrowY = rwyBot + 22;
            const arrowLen = 56;
            // Pfeil-Richtung folgt dem Offset (LINKS-Offset → Pfeil zeigt
            // nach links). ABER: wenn der TD-Dot sehr nah am linken oder
            // rechten SVG-Rand sitzt, würde das Label aus dem SVG raus
            // gerendert und abgeschnitten werden. In dem Fall klappen
            // wir die ganze Group horizontal um (Pfeil zeigt auf die
            // gegenüberliegende Seite des Dots, Label sitzt drin).
            const labelW = 110; // grobe Pixel-Breite "6.6 m LINKS"
            const wouldClipLeft = isLeftOffset && (tdX - arrowLen / 2 - labelW) < 0;
            const wouldClipRight = !isLeftOffset && (tdX + arrowLen / 2 + labelW) > W;
            const flipped = wouldClipLeft || wouldClipRight;
            // arrowDir = wo die Spitze hin zeigt (true = nach links)
            const arrowDir = flipped ? !isLeftOffset : isLeftOffset;
            const ax1 = arrowDir ? tdX + arrowLen / 2 : tdX - arrowLen / 2;
            const ax2 = arrowDir ? tdX - arrowLen / 2 : tdX + arrowLen / 2;
            const isLeft = arrowDir;
            return (
              <g>
                {/* Dünne Anker-Linie vom TD-Dot zum Pfeil */}
                <line
                  x1={tdX}
                  y1={tdY}
                  x2={tdX}
                  y2={arrowY - 8}
                  stroke={dotColor}
                  strokeWidth="1"
                  strokeDasharray="2,3"
                  opacity="0.5"
                />
                {/* Großer Pfeil-Schaft */}
                <line
                  x1={ax1}
                  y1={arrowY}
                  x2={ax2}
                  y2={arrowY}
                  stroke={dotColor}
                  strokeWidth="3.5"
                />
                {/* Große Pfeilspitze.
                    v0.19.x FIX: die Basis-/Spitze-Punkte waren vertauscht —
                    die Spitze (der Einzelpunkt) lag auf der SCHAFT-Seite von
                    `ax2`, die flache Basis ragte darüber hinaus. Eine
                    Pfeilspitze zeigt in die Richtung, in der IHR Einzelpunkt
                    am weitesten liegt; mit vertauschten Punkten zeigte der
                    Pfeil zurück zum TD-Punkt statt in die gelabelte
                    LINKS/RECHTS-Richtung — das genaue Gegenteil vom
                    Text-Label daneben. */}
                <polygon
                  points={
                    isLeft
                      ? `${ax2 + 10},${arrowY - 8} ${ax2},${arrowY} ${ax2 + 10},${arrowY + 8}`
                      : `${ax2 - 10},${arrowY - 8} ${ax2},${arrowY} ${ax2 - 10},${arrowY + 8}`
                  }
                  fill={dotColor}
                />
                {/* Großes Label neben dem Pfeil — Label-Text zeigt die
                    Offset-Richtung (LINKS/RECHTS), unabhängig davon ob
                    der Pfeil aus Platzgründen geklappt wurde. */}
                <text
                  x={isLeft ? ax2 - 14 : ax2 + 14}
                  y={arrowY + 5}
                  fontSize="15"
                  fill={dotColor}
                  fontWeight="800"
                  fontFamily="monospace"
                  textAnchor={isLeft ? "end" : "start"}
                >
                  {Math.abs(props.td_centerline_offset_m).toFixed(1)} m {isLeftOffset ? t("runway_v2.centerline_left") : t("runway_v2.centerline_right")}
                </text>
              </g>
            );
          })()}

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

          {/* Bremspunkt (40 kt) — adaptive Label-Platzierung:
              - normaler Fall (Rollout ≥ ~80 px) → Labels ÜBER dem Dot,
                gestapelt, mit ausreichend Abstand zum Kreis (kein
                Overlap mit der Circle r=11)
              - kurzer Rollout (Labels würden mit dem TD-Dot Bereich
                kollidieren) → Labels RECHTS vom Bremspunkt-Dot
              Reines Overlap-Hygiene-Detail, kein User-sichtbares
              Verhalten ändert sich beim normalen Fall. */}
          {exitX != null && (() => {
            // 3-Modi-Anti-Overlap:
            // 1. "right": Wenn Rollout sehr kurz (< 80 px) → Labels rechts
            //    vom Bremspunkt-Dot, nicht drüber (sonst crashen sie in
            //    den TD-Dot-Bereich).
            // 2. "below": Wenn der TD-Dot durch großen XTD-Offset weit
            //    oben sitzt (tdY < rwyTop + 60), würden Labels über dem
            //    Dot in die AUFSETZZONE-Beschriftung crashen → Labels
            //    UNTER den Bremspunkt-Dot.
            // 3. "above": Standard-Fall, Labels über dem Dot.
            const exitGap = exitX - tdX;
            const mode: "right" | "below" | "above" =
              exitGap < 80
                ? "right"
                : tdY < rwyTop + 60
                ? "below"
                : "above";
            const textProps = {
              fill: TOKENS.exitDot,
              fontWeight: "800" as const,
              fontFamily: "monospace",
              stroke: "#0c1628",
              strokeWidth: "3",
              paintOrder: "stroke" as const,
            };
            return (
              <g>
                <circle cx={exitX} cy={tdY} r="11" fill={TOKENS.exitDot} opacity="0.25" />
                <circle cx={exitX} cy={tdY} r="6" fill={TOKENS.exitDot} stroke="#0c1628" strokeWidth="1.5" />
                {mode === "right" && (
                  <>
                    <text x={exitX + 18} y={tdY - 4} textAnchor="start" fontSize="14" {...textProps}>
                      {t("runway_v2.bremspunkt_title")}
                    </text>
                    <text x={exitX + 18} y={tdY + 12} textAnchor="start" fontSize="13" {...textProps}>
                      40 kt
                    </text>
                  </>
                )}
                {mode === "below" && (
                  <>
                    <text x={exitX} y={tdY + 22} textAnchor="middle" fontSize="14" {...textProps}>
                      {t("runway_v2.bremspunkt_title")}
                    </text>
                    <text x={exitX} y={tdY + 38} textAnchor="middle" fontSize="13" {...textProps}>
                      40 kt
                    </text>
                  </>
                )}
                {mode === "above" && (
                  <>
                    <text x={exitX} y={tdY - 36} textAnchor="middle" fontSize="14" {...textProps}>
                      {t("runway_v2.bremspunkt_title")}
                    </text>
                    <text x={exitX} y={tdY - 20} textAnchor="middle" fontSize="13" {...textProps}>
                      40 kt
                    </text>
                  </>
                )}
                <title>{t("runway_v2.tooltip_brake_point")}</title>
              </g>
            );
          })()}

          {/* RWY-Designator (groß links) — die Landerichtung. */}
          <text
            x={padX / 2 - 4}
            y={rwyCl + 10}
            textAnchor="middle"
            fontSize="28"
            fill="#f1f5f9"
            fontWeight="800"
            fontFamily="monospace"
          >
            {props.runway_ident}
          </text>

          {/* Gegen-RWY-Designator + Bahnlänge rechts. Gegen-Designator
              zeigt klar dass die Bahn da endet (= Gegen-Richtung,
              z. B. RWY 32 ↔ RWY 14). Plus Bahn-Gesamtlänge darunter. */}
          <text
            x={W - padX / 2 + 8}
            y={rwyCl - 2}
            textAnchor="middle"
            fontSize="20"
            fill="#94a3b8"
            fontWeight="700"
            fontFamily="monospace"
            opacity="0.85"
          >
            {oppositeRunway(props.runway_ident)}
          </text>
          <text
            x={W - padX / 2 + 8}
            y={rwyCl + 18}
            textAnchor="middle"
            fontSize="11"
            fill="#64748b"
            fontFamily="monospace"
          >
            {props.length_m.toFixed(0)} m
          </text>

          {/* (Landerichtungs-Pfeil entfernt — die neuen End-Streifen
              + der Gegen-RWY-Designator zeigen das Bahn-Ende
              eindeutiger als der Pfeil es konnte.) */}

          {/* TD-Label unter dem Dot — nur Distanz. L/R wird durch den
              großen L/R-Pfeil oben dargestellt. Bei Offset < 0.5 m
              steht hier zusätzlich "auf CL". */}
          <g>
            <text
              x={tdX}
              y={rwyBot + 46}
              textAnchor="middle"
              fontSize="13"
              fill={dotColor}
              fontWeight="700"
              fontFamily="monospace"
            >
              TD {props.td_distance_from_threshold_m.toFixed(0)} m
              {Math.abs(props.td_centerline_offset_m) < 0.5 ? " · " + t("runway_v2.auf_cl") : ""}
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
                    fontSize="10"
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
      </div>

      {/* ─── 3. LEGENDE ─────────────────────────────────────────────── */}
      <div
        style={{
          display: "flex",
          gap: 18,
          flexWrap: "wrap",
          fontSize: "0.78rem",
          opacity: 0.85,
          padding: "0 4px",
        }}
      >
        <LegendItem swatch={TOKENS.threshold} label={t("runway_v2.legend_threshold")} />
        {tdzEndX && <LegendItem swatch={TOKENS.tdzStroke} label={t("runway_v2.legend_tdz")} />}
        {aimX && <LegendItem swatch={TOKENS.aimMarker} label={t("runway_v2.legend_aim")} />}
        <LegendDot color={dotColor} label={t("runway_v2.legend_td")} />
        {exitX && <LegendDot color={TOKENS.exitDot} label={t("runway_v2.legend_brakepoint")} />}
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
            tone={
              bahnUsedPct > 100
                ? "bad"
                : bahnUsedPct > skin.thresholds.bahn_auslastung_warn_above
                ? "warn"
                : "good"
            }
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
        <FlugzeugBar props={props} />
      </div>

      {glossaryOpen && (
        <GlossaryModal onClose={() => setGlossaryOpen(false)} />
      )}
    </section>
  );
}

// ─── Kleine UI-Helpers ──────────────────────────────────────────────

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
