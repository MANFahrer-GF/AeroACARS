// Queransicht der Bahn — der gefahrene Streifen im Massstab der Bahnbreite.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.3, §8.5, §8.6.
//
// # Was diese Ansicht beantwortet
//
// Die Längsansicht zeigt, **wo** entlang der Bahn etwas passiert ist. Sie kann
// nicht zeigen, **wie nah an der Kante** — dafür ist ihre Querachse gestaucht.
// Genau daran hängt aber die Bewertung: Ob ein Rad die befestigte Fläche
// verlassen hat, entscheidet über 100 oder 20 Punkte.
//
// # Warum die Achse überhöht ist — und warum das ehrlich bleibt
//
// Bei 3189 m Länge und 45 m Breite wäre eine massstabsgetreue Queransicht ein
// Strich. Die Querachse ist deshalb rund 15-fach überhöht, aber **in sich
// massstäblich**: Die Spurweite nimmt exakt den Anteil der Bahnbreite ein, den
// sie in Wirklichkeit einnimmt. Der Faktor steht sichtbar in der Grafik, damit
// niemand die Überhöhung für die Wirklichkeit hält.
//
// # Kein Flugzeugumriss
//
// Ein Umriss in der Bahnfläche verdeckt genau den Spurverlauf, den die Ansicht
// zeigen soll. Das Grössenverhältnis steht stattdessen als Balkenvergleich
// darunter — dort ragt die Spannweite sichtbar über die Bahnbreite hinaus, und
// erst dadurch versteht man, warum die Fahrspur so schmal wirkt.

import { useTranslation } from "react-i18next";
import type { Projektion } from "../lib/runwayProjection";

export interface QueransichtProps {
  /** Die **gemeinsame** Projektion der Längsansicht — nicht eine zweite. */
  projektion: Projektion;
  /** Bahnbreite in Metern. Ohne sie ist keine Queransicht möglich. */
  runwayWidthM: number;
  /** Spurweite des Hauptfahrwerks in Metern. */
  trackWidthM: number | null;
  /** Der gefahrene Streifen. */
  samples: Array<{ laengs_m: number; quer_m: number }>;
  /** Aufsetzpunkt: Distanz ab Schwelle und Versatz. */
  touchdownM: number;
  touchdownOffsetM: number;
  /** Räumpunkt, falls die Bahn über eine Ausfahrt verlassen wurde. */
  clearanceM?: number | null;
  clearanceSide?: "left" | "right" | null;
  /** Kleinster Randabstand des äusseren Rades. Negativ = neben der Bahn. */
  minEdgeClearanceM?: number | null;
  /** Grösster Versatz — für die Marke ②. */
  maxLateralOffsetM?: number | null;
  /** Strecke über das Bahnende hinaus — für die Marke ④. */
  overrunM?: number | null;
  /** Breite des SVG in Benutzereinheiten (muss der Längsansicht gleichen). */
  width: number;
  tokens: {
    tarmac: string;
    tarmacBorder: string;
    centerline: string;
    rollout: string;
    tdPerfect: string;
    tdWarn: string;
    tdSevere: string;
  };
}

/** Höhe der Grafik. Grosszügig — §8.6.6: lieber höher als enger. */
const H = 210;
/** Oberkante der Grünfläche. */
const GRUEN_TOP = 42;
/** Unterkante der Grünfläche. */
const GRUEN_BOT = 162;
/** Wie viel Grünstreifen beidseits der Bahn gezeigt wird, in Metern. */
const GRUEN_M = 12;

/**
 * Queransicht. Gibt `null` zurück, wenn die Bahnbreite fehlt — eine geratene
 * Breite wäre eine Behauptung über die Kante, an der die Bewertung hängt.
 */
export function RunwayCrossSection(p: QueransichtProps) {
  const { t } = useTranslation();
  if (!Number.isFinite(p.runwayWidthM) || p.runwayWidthM <= 0) return null;

  const halbeBahnM = p.runwayWidthM / 2;
  const sichtbarM = halbeBahnM + GRUEN_M;
  const mitteY = (GRUEN_TOP + GRUEN_BOT) / 2;
  const pxProQuerM = (GRUEN_BOT - GRUEN_TOP) / 2 / sichtbarM;

  // ── Die Seitenkonvention, an EINER Stelle ────────────────────────────
  //
  // §8.6: „oben = links in Landerichtung", verbindlich für beide Ansichten.
  // Die Längsansicht ist eine Draufsicht mit Landerichtung nach rechts; dort
  // liegt links vom Flugzeug zwangsläufig oben. Intern bleibt `quer > 0 =
  // rechts` — die Umrechnung geschieht hier, einmal, nicht verstreut über die
  // Mapper. Im ersten Entwurf war die Skala mathematisch beschriftet (+ oben),
  // damit lag dieselbe Seite in einer Ansicht oben und in der anderen unten.
  //
  // Begrenzt auf den sichtbaren Streifen. Ohne diese Schranke laeuft eine
  // Spur mit unmoeglichem Messwert aus dem Bild -- der EDDL-Fall lag bei
  // 52,6 m Versatz auf einer 45-m-Bahn, das waere hundert Pixel oberhalb des
  // Zeichenbereichs gelandet. Die Bewertung ueberspringt solche Werte
  // (`implausible_lateral_track`), gezeichnet werden sie trotzdem: Wer sie
  // sieht, erkennt sofort, dass die Bahnzuordnung nicht stimmen kann.
  const querZuY = (quer_m: number) => {
    const begrenzt = Math.max(-sichtbarM, Math.min(sichtbarM, quer_m));
    return mitteY + begrenzt * pxProQuerM;
  };

  const bahnTop = querZuY(-halbeBahnM);
  const bahnBot = querZuY(halbeBahnM);

  // Überhöhungsfaktor — gehört sichtbar in die Grafik (§8.6).
  const ueberhoehung = pxProQuerM / p.projektion.pxProMeter;

  // ── Das Band der Radspuren ───────────────────────────────────────────
  //
  // §8.5: als **gefülltes Band**, nicht als drei Linien. Drei getrennte
  // Linien laufen bei steilen Abschnitten optisch auseinander und sehen aus,
  // als würden sie einander kreuzen; ein Band liest sich sofort als der
  // befahrene Streifen.
  const halbeSpurM = (p.trackWidthM ?? 0) / 2;
  const punkte = p.samples.filter(
    (s) => Number.isFinite(s.laengs_m) && Number.isFinite(s.quer_m),
  );
  const bandPfad =
    punkte.length >= 2
      ? [
          ...punkte.map(
            (s, i) =>
              `${i === 0 ? "M" : "L"} ${p.projektion.mToX(s.laengs_m).toFixed(1)} ${querZuY(
                s.quer_m - halbeSpurM,
              ).toFixed(1)}`,
          ),
          ...punkte
            .slice()
            .reverse()
            .map(
              (s) =>
                `L ${p.projektion.mToX(s.laengs_m).toFixed(1)} ${querZuY(
                  s.quer_m + halbeSpurM,
                ).toFixed(1)}`,
            ),
          "Z",
        ].join(" ")
      : null;

  const mittelPfad =
    punkte.length >= 2
      ? punkte
          .map(
            (s, i) =>
              `${i === 0 ? "M" : "L"} ${p.projektion.mToX(s.laengs_m).toFixed(1)} ${querZuY(
                s.quer_m,
              ).toFixed(1)}`,
          )
          .join(" ")
      : null;

  // Farbe des Bandes nach dem kleinsten Randabstand — dieselbe Aussage wie
  // die Note, nicht eine zweite.
  const rand = p.minEdgeClearanceM;
  const bandFarbe =
    rand == null
      ? p.tokens.rollout
      : rand < 0
      ? p.tokens.tdSevere
      : rand < 3
      ? p.tokens.tdWarn
      : p.tokens.tdPerfect;

  // ── Marken ───────────────────────────────────────────────────────────
  const marken: Array<{ n: number; x: number; y: number }> = [
    {
      n: 1,
      x: p.projektion.mToX(p.touchdownM),
      y: querZuY(p.touchdownOffsetM),
    },
  ];
  const maxOff = p.maxLateralOffsetM;
  const maxProbe =
    maxOff != null && punkte.length
      ? punkte.reduce((a, b) =>
          Math.abs(b.quer_m - maxOff) < Math.abs(a.quer_m - maxOff) ? b : a,
        )
      : null;
  if (maxProbe) {
    marken.push({
      n: 2,
      x: p.projektion.mToX(maxProbe.laengs_m),
      y: querZuY(maxProbe.quer_m),
    });
  }
  if (p.clearanceM != null) {
    marken.push({
      n: 3,
      x: p.projektion.mToX(p.clearanceM),
      y: querZuY(p.clearanceSide === "left" ? -halbeBahnM : halbeBahnM),
    });
  }
  // Ueberrollen: die Marke sitzt AM Bahnende, nicht dahinter -- dort endet
  // der Zeichenbereich. Sie muss sein: Die Liste darunter fuehrt den
  // Eintrag ④, und eine Nummer in der Liste ohne Marke im Bild laesst den
  // Leser suchen (aufgefallen beim Rendern der Varianten am 23.08.).
  if (p.overrunM != null && p.overrunM > 0) {
    marken.push({ n: 4, x: p.projektion.bahnEndeX - 10, y: mitteY });
  }

  return (
    <svg
      viewBox={`0 0 ${p.width} ${H}`}
      width="100%"
      role="img"
      aria-label={t("runway_v2.cross_section_aria", {
        defaultValue: "Queransicht der Bahn mit dem gefahrenen Streifen",
      })}
      style={{ display: "block" }}
    >
      {/* Grünstreifen beidseits — die Fläche neben der Bahn. */}
      <rect
        x={p.projektion.bahnAnfangX}
        y={GRUEN_TOP}
        width={p.projektion.bahnEndeX - p.projektion.bahnAnfangX}
        height={GRUEN_BOT - GRUEN_TOP}
        fill="rgba(34,197,94,0.10)"
      />
      {/* Die befestigte Fläche. */}
      <rect
        x={p.projektion.bahnAnfangX}
        y={bahnTop}
        width={p.projektion.bahnEndeX - p.projektion.bahnAnfangX}
        height={bahnBot - bahnTop}
        fill={p.tokens.tarmac}
        stroke={p.tokens.tarmacBorder}
        strokeWidth={1}
      />
      {/* Mittellinie. */}
      <line
        x1={p.projektion.bahnAnfangX}
        y1={mitteY}
        x2={p.projektion.bahnEndeX}
        y2={mitteY}
        stroke={p.tokens.centerline}
        strokeWidth={1}
        strokeDasharray="10 8"
        opacity={0.6}
      />

      {/* Der gefahrene Streifen. */}
      {bandPfad && (
        <path d={bandPfad} fill={bandFarbe} fillOpacity={0.35} stroke="none" />
      )}
      {mittelPfad && (
        <path
          d={mittelPfad}
          fill="none"
          stroke={bandFarbe}
          strokeWidth={1.5}
          strokeLinejoin="round"
        />
      )}

      {/* Marken — nur Ziffern, der Text steht in der Liste darunter (§8.5). */}
      {marken.map((m) => (
        <g key={m.n}>
          <circle
            cx={m.x}
            cy={m.y}
            r={9}
            fill="rgba(15,23,42,0.92)"
            stroke={bandFarbe}
            strokeWidth={1.5}
          />
          <text
            x={m.x}
            y={m.y + 3.5}
            textAnchor="middle"
            fontSize={10}
            fontWeight={700}
            fill="#e2e8f0"
          >
            {m.n}
          </text>
        </g>
      ))}

      {/* Skala: LINKS / RECHTS statt Vorzeichen — ein Pilot denkt in Seiten. */}
      <text
        x={p.projektion.bahnAnfangX - 8}
        y={GRUEN_TOP + 12}
        textAnchor="end"
        fontSize={10}
        fill="#94a3b8"
      >
        {t("runway_v2.side_left", { defaultValue: "LINKS" })}
      </text>
      <text
        x={p.projektion.bahnAnfangX - 8}
        y={GRUEN_BOT - 4}
        textAnchor="end"
        fontSize={10}
        fill="#94a3b8"
      >
        {t("runway_v2.side_right", { defaultValue: "RECHTS" })}
      </text>
      <text
        x={p.projektion.bahnAnfangX - 8}
        y={mitteY + 3.5}
        textAnchor="end"
        fontSize={10}
        fill="#64748b"
      >
        0
      </text>

      {/* Bahnbreite bemasst, ausserhalb der Fläche (§8.6.3). */}
      <text
        x={p.projektion.bahnEndeX + 8}
        y={mitteY - 4}
        fontSize={10}
        fill="#94a3b8"
      >
        {p.runwayWidthM.toFixed(0)} m
      </text>
      <text
        x={p.projektion.bahnEndeX + 8}
        y={mitteY + 9}
        fontSize={9}
        fill="#64748b"
      >
        {t("runway_v2.cross_exaggeration", {
          defaultValue: "{{f}}× überhöht",
          f: ueberhoehung.toFixed(0),
        })}
      </text>
    </svg>
  );
}
