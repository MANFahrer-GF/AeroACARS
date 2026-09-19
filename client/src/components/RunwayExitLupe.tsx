// Lupe „Abrollen im echten Massstab" — die Ausfahrt als Draufsicht.
//
// # Warum es diese Ansicht gibt
//
// Die Queransicht zeigt die ganze Bahn in einer Zeile. Dafür ist ihre
// Querachse überhöht (bei 3000 × 45 m rund zwölffach), und das ist für die
// Frage „wie nah an der Kante" richtig. Für die Ausfahrt ist es falsch: Ein
// Abrollen mit 30° wird dort als fast rechter Winkel gezeichnet. Thomas
// (19.09.2026): „Mir ist wichtig, dass das Abrollen gut dargestellt wird …
// aber in der Realität mit den Daten sieht das immer komisch aus."
//
// Hier gilt deshalb EIN Massstab für beide Achsen. Der Ausschnitt reicht
// von gut zweihundert Metern vor dem Räumpunkt bis hinter ihn — genug, um
// das Einlenken zu sehen, klein genug, dass eine Bahn nicht zum Strich wird.
// Das war der Fehler der verworfenen Draufsicht vom 11.09.: die ganze Bahn
// in voller Länge, sechs Pixel hoch.
//
// Vor dem Bau an neun echten Landungen geprüft (EHRD, EDLW, EDDL, EDDW,
// EGFF, LTFJ, EDDF, LEBB, VHHH): flache Schnellabrollwege wie rechtwinklige
// Abbieger lesen sich so, wie sie gefahren wurden.

import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Ausfahrt } from "./RunwayCrossSection";
import { weicherPfad } from "./RunwayCrossSection";

export interface LupeProps {
  /** Die bereinigte Spur, ab Bahnanfang gemessen. */
  samples: Array<{ laengs_m: number; quer_m: number }>;
  runwayWidthM: number;
  trackWidthM: number | null;
  /** Räumpunkt ab Bahnanfang — der Anker des Ausschnitts. */
  clearanceM?: number | null;
  clearanceSide?: "left" | "right" | null;
  ausfahrten?: Ausfahrt[] | null;
  /** Farbe des Bandes — dieselbe wie in der Queransicht. */
  bandFarbe: string;
  width: number;
  schriftMindest?: number;
  tokens: { tarmac: string; rollout: string; rollweg: string; rollwegRand: string };
}

/** Wie weit vor und hinter dem Anker der Ausschnitt reicht, in Metern. */
const VOR_M = 220;
const NACH_M = 110;
/** Grösste Höhe der Zeichenfläche — darüber wird der Massstab kleiner. */
const MAX_H = 400;
/** Weiter als so weit neben der Mitte wird nichts gezeichnet. */
const QUER_GRENZE_UEBER_KANTE_M = 150;
/**
 * Breite eines Rollwegs. Die Bodenkarte führt nur die Mittellinie; 23 m
 * ist die übliche Breite eines Schnellabrollwegs (dieselbe Annahme wie der
 * Korridor der Queransicht bis v1.7.39).
 */
const ROLLWEG_BREITE_M = 23;
const RAND = 14;
/** Über der Zeichenfläche: Titel, darunter die Namen der linken Rollwege. */
const KOPF = 44;
/** Unter der Zeichenfläche: Namen der rechten Rollwege, Richtung, Massstab. */
const FUSS = 58;

type P = { x: number; y: number };

type LQ = { laengs_m: number; quer_m: number };

/**
 * Schneidet einen Linienzug auf das Rechteck [l0, l1] × [q0, q1]
 * (Liang–Barsky je Abschnitt). Liefert die Stücke, die im Rechteck liegen;
 * ein Linienzug, der hinaus- und wieder hineinläuft, ergibt mehrere.
 */
function linieZuschneiden(
  zug: LQ[],
  l0: number,
  l1: number,
  q0: number,
  q1: number,
): LQ[][] {
  const stuecke: LQ[][] = [];
  let akt: LQ[] = [];
  const gleich = (a: LQ, b: LQ) => Math.abs(a.laengs_m - b.laengs_m) < 1e-6 && Math.abs(a.quer_m - b.quer_m) < 1e-6;
  for (let i = 0; i + 1 < zug.length; i++) {
    const a = zug[i]!;
    const b = zug[i + 1]!;
    const dl = b.laengs_m - a.laengs_m;
    const dq = b.quer_m - a.quer_m;
    let t0 = 0;
    let t1 = 1;
    const kanten: Array<[number, number]> = [
      [-dl, a.laengs_m - l0],
      [dl, l1 - a.laengs_m],
      [-dq, a.quer_m - q0],
      [dq, q1 - a.quer_m],
    ];
    let drin = true;
    for (const [pp, qq] of kanten) {
      if (pp === 0) {
        if (qq < 0) drin = false;
      } else {
        const r = qq / pp;
        if (pp < 0) t0 = Math.max(t0, r);
        else t1 = Math.min(t1, r);
      }
    }
    if (!drin || t0 > t1) {
      if (akt.length >= 2) stuecke.push(akt);
      akt = [];
      continue;
    }
    const s = { laengs_m: a.laengs_m + t0 * dl, quer_m: a.quer_m + t0 * dq };
    const e = { laengs_m: a.laengs_m + t1 * dl, quer_m: a.quer_m + t1 * dq };
    if (akt.length === 0 || !gleich(akt[akt.length - 1]!, s)) {
      if (akt.length >= 2) stuecke.push(akt);
      akt = [s];
    }
    akt.push(e);
    // Verlässt der Abschnitt das Rechteck, endet das Stück hier.
    if (t1 < 1) {
      if (akt.length >= 2) stuecke.push(akt);
      akt = [];
    }
  }
  if (akt.length >= 2) stuecke.push(akt);
  return stuecke.filter((st) => st.length >= 2 && !gleich(st[0]!, st[st.length - 1]!));
}

/**
 * Die Lupe. Gibt `null` zurück, wenn es kein Abrollen zu zeigen gibt —
 * weder ein Räumpunkt noch eine Spur, die die Bahn verlässt.
 */
export function RunwayExitLupe(p: LupeProps) {
  const { t } = useTranslation();
  const id = useId().replace(/:/g, "");
  const sf = (g: number) => Math.max(g, p.schriftMindest ?? 0);
  const halbeBahn = p.runwayWidthM / 2;
  if (!Number.isFinite(halbeBahn) || halbeBahn <= 0 || p.samples.length < 2) return null;

  // Anker: der Räumpunkt, sonst der erste Punkt jenseits der Kante.
  const draussen = p.samples.find((s) => Math.abs(s.quer_m) > halbeBahn);
  const anker = p.clearanceM ?? draussen?.laengs_m ?? null;
  if (anker == null) return null;

  const von = anker - VOR_M;
  const bis = anker + NACH_M;
  const quergrenze = halbeBahn + QUER_GRENZE_UEBER_KANTE_M;
  const imFenster = (s: { laengs_m: number; quer_m: number }) =>
    s.laengs_m >= von - 30 && s.laengs_m <= bis + 30 && Math.abs(s.quer_m) <= quergrenze;

  const spur = p.samples.filter(imFenster);
  if (spur.length < 2) return null;
  const halbeSpur = (p.trackWidthM ?? 0) / 2;

  // Rollwege genau auf den Ausschnitt — was darüber hinausragt, läge unter
  // den Namen am Rand.
  //
  // Geschnitten, nicht gefiltert: OSM führt Schnellabrollwege oft als EINE
  // lange Gerade aus zwei Punkten. Punkte zu filtern ließ davon einen übrig,
  // und der Rollweg fehlte ganz (QS 19.09.2026, derselbe Fehler, den
  // `ausfahrten.rs` im Client schon mit Liang–Barsky behoben hat).
  const rollwege = (p.ausfahrten ?? []).flatMap((a) =>
    linieZuschneiden(a.verlauf ?? [], von, bis, -quergrenze, quergrenze).map((v) => ({ a, v })),
  );


  // Hervorgehoben wird der Rollweg, auf den die Spur gefahren ist. Die
  // Seite kommt aus der Bewertung; fehlt sie (EWG9503: `clearance_side`
  // leer), zeigt die Spur selbst, wohin es ging — sie endet neben der Bahn.
  const letzter = spur[spur.length - 1]!;
  const seite =
    p.clearanceSide ??
    (Math.abs(letzter.quer_m) > halbeBahn ? (letzter.quer_m < 0 ? "left" : "right") : null);
  // Die NÄCHSTE Ausfahrt, nicht jede im Fenster: In München liegen B7
  // (2.259 m) und B6 (2.368 m) beide innerhalb von 120 m um den Räumpunkt
  // 2.345 m — genommen wurde B6 (DLH369, siehe RunwayQS.test.tsx).
  const genutzteAusfahrt =
    seite == null
      ? null
      : ((p.ausfahrten ?? [])
          .filter((a) => a.seite === seite && Math.abs(a.laengs_m - anker) < 120)
          .sort((a, b) => Math.abs(a.laengs_m - anker) - Math.abs(b.laengs_m - anker))[0] ??
        null);
  const genutzt = (a: Ausfahrt) =>
    genutzteAusfahrt != null &&
    a.name === genutzteAusfahrt.name &&
    a.seite === genutzteAusfahrt.seite &&
    a.laengs_m === genutzteAusfahrt.laengs_m;

  // Senkrechter Ausschnitt: Bahn, Spur und der GENOMMENE Rollweg.
  //
  // Nicht alle Rollwege: In EDDL kreuzt K3 die Bahn und reicht 170 m zu
  // beiden Seiten. Er zog den Ausschnitt auf 340 m Höhe, der Massstab
  // schrumpfte, und die Ausfahrt wurde klein (Vorschau #1431, 19.09.2026).
  // Andere Rollwege werden gezeichnet, soweit sie ins Bild fallen.
  const genommen = rollwege.find((r) => genutzt(r.a));
  const quers = [
    -halbeBahn,
    halbeBahn,
    ...spur.map((s) => s.quer_m - halbeSpur),
    ...spur.map((s) => s.quer_m + halbeSpur),
    ...(genommen?.v.map((v) => v.quer_m) ?? []),
  ];
  // Luft mindestens eine halbe Rollwegbreite, damit ein Rollweg am Rand
  // nicht über die Zeichenfläche hinausragt.
  const luft = ROLLWEG_BREITE_M / 2 + 3;
  const qMin = Math.max(-quergrenze, Math.min(...quers) - luft);
  const qMax = Math.min(quergrenze, Math.max(...quers) + luft);

  // EIN Massstab für beide Achsen — das ist die ganze Aussage dieser Ansicht.
  const innenB = p.width - 2 * RAND;
  const k = Math.min(innenB / (bis - von), MAX_H / (qMax - qMin));
  const zeichenB = (bis - von) * k;
  const x0 = RAND + (innenB - zeichenB) / 2;
  const y0 = KOPF;
  const H = y0 + (qMax - qMin) * k + FUSS;
  const x = (m: number) => x0 + (m - von) * k;
  // „oben = links in Landerichtung", wie in beiden anderen Ansichten.
  const y = (q: number) => y0 + (q - qMin) * k;
  const xy = (s: { laengs_m: number; quer_m: number }): P => ({ x: x(s.laengs_m), y: y(s.quer_m) });

  const band = (achse: P[], halbPx: number) => {
    const rand = (v: 1 | -1) =>
      achse.map((q, i) => {
        const a = achse[Math.max(0, i - 1)]!;
        const b = achse[Math.min(achse.length - 1, i + 1)]!;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const l = Math.hypot(dx, dy) || 1;
        return { x: q.x + (v * -dy * halbPx) / l, y: q.y + (v * dx * halbPx) / l };
      });
    const links = rand(-1);
    const rechts = rand(1).reverse();
    const ende = rechts[0];
    if (!ende) return null;
    return {
      flaeche: `${weicherPfad(links)} L ${ende.x.toFixed(1)} ${ende.y.toFixed(1)} ${weicherPfad(rechts, 0.5, true)} Z`,
      links: weicherPfad(links),
      rechts: weicherPfad(rechts),
    };
  };

  const achse = spur.map(xy);
  const spurBand = halbeSpur > 0 ? band(achse, halbeSpur * k) : null;


  // Namen, die sich nicht überdecken. In Frankfurt liegen M17, M19 und M21
  // wenige Meter auseinander (#1426) — übereinander gesetzt war keiner
  // lesbar. Der genommene Rollweg wird zuerst gesetzt und gewinnt; ein
  // Name, der einen gesetzten überdecken würde, entfällt. Die Queransicht
  // darüber führt alle Namen.
  const namenOhneUeberlappung: Array<{ a: Ausfahrt; xx: number }> = [];
  for (const a of [...(p.ausfahrten ?? [])]
    .filter((a) => a.laengs_m >= von && a.laengs_m <= bis)
    .sort((m, n) => Number(genutzt(n)) - Number(genutzt(m)))) {
    const xx = x(a.laengs_m);
    const breite = (n: string) => n.length * sf(10) * 0.62;
    const stoesst = namenOhneUeberlappung.some(
      (b) =>
        b.a.seite === a.seite &&
        Math.abs(b.xx - xx) < (breite(a.name) + breite(b.a.name)) / 2 + 4,
    );
    if (!stoesst) namenOhneUeberlappung.push({ a, xx });
  }

  const bahnOben = y(-halbeBahn);
  const bahnUnten = y(halbeBahn);
  const skala = [50, 100, 25].find((m) => m * k <= zeichenB / 3) ?? 10;
  const clip = `lupe-${id}`;

  const hatRollweg = rollwege.length > 0;
  const legende: Array<{ farbe: string; text: string; flaeche?: boolean }> = [
    {
      farbe: p.bandFarbe,
      text: t("runway_v2.lupe_legend_spur", {
        defaultValue: "Spur der Hauptfahrwerke, Punkte = Messungen",
      }),
    },
    ...(hatRollweg
      ? [
          {
            farbe: p.tokens.rollweg,
            flaeche: true,
            text: t("runway_v2.lupe_legend_rollweg", {
              defaultValue: "Rollweg aus OpenStreetMap (23 m breit angenommen) — kräftig: der genommene",
            }),
          },
        ]
      : []),
    ...(p.clearanceM != null && p.clearanceSide != null
      ? [
          {
            farbe: p.tokens.rollout,
            text: t("runway_v2.lupe_legend_raeum", { defaultValue: "③ hier hat die Spur die Bahn verlassen" }),
          },
        ]
      : []),
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
    <svg
      viewBox={`0 0 ${p.width} ${H.toFixed(0)}`}
      width="100%"
      role="img"
      aria-label={t("runway_v2.lupe_aria", {
        defaultValue: "Abrollen im echten Massstab: Spur und Rollweg als Draufsicht",
      })}
      style={{ display: "block" }}
      data-ansicht="lupe"
    >
      <defs>
        <clipPath id={clip}>
          <rect x={x0} y={y0} width={zeichenB} height={(qMax - qMin) * k} />
        </clipPath>
      </defs>

      <text x={0} y={14} fontSize={sf(10.5)} letterSpacing={1.4} fill="#8B95A8">
        {t("runway_v2.lupe_title", { defaultValue: "ABROLLEN — IM ECHTEN MASSSTAB" })}
      </text>
      <text x={p.width} y={14} fontSize={sf(10)} textAnchor="end" fill="#66707E">
        {t("runway_v2.lupe_scale_note", {
          defaultValue: "längs und quer gleicher Massstab · Winkel wie gefahren",
        })}
      </text>

      <g clipPath={`url(#${clip})`}>
        {/* Rollwege UNTER der Bahn: Wo sie die Bahn kreuzen, gilt die Bahn. */}
        {rollwege.map(({ a, v }, i) => {
          const b = band(v.map(xy), (ROLLWEG_BREITE_M / 2) * k);
          if (!b) return null;
          const an = genutzt(a);
          return (
            <path
              key={`${a.name}-${a.seite}-${i}`}
              d={b.flaeche}
              fill={p.tokens.rollweg}
              fillOpacity={an ? 0.24 : 0.1}
              stroke={p.tokens.rollwegRand}
              strokeOpacity={an ? 0.8 : 0.35}
              strokeWidth={1}
              strokeDasharray="6 5"
            />
          );
        })}
        <rect
          x={x0}
          y={bahnOben}
          width={zeichenB}
          height={bahnUnten - bahnOben}
          fill={p.tokens.tarmac}
        />
        {[bahnOben, bahnUnten].map((yy, i) => (
          <line key={i} x1={x0} y1={yy} x2={x0 + zeichenB} y2={yy} stroke="#C9D2E0" strokeWidth={1.4} opacity={0.8} />
        ))}
        <line x1={x0} y1={y(0)} x2={x0 + zeichenB} y2={y(0)} stroke="#6C7A8F" strokeDasharray="16 12" />

        {spurBand && (
          <>
            <path d={spurBand.flaeche} fill={p.bandFarbe} fillOpacity={0.2} />
            <path d={spurBand.links} fill="none" stroke={p.bandFarbe} strokeWidth={2} />
            <path d={spurBand.rechts} fill="none" stroke={p.bandFarbe} strokeWidth={2} />
          </>
        )}
        <path d={weicherPfad(achse)} fill="none" stroke={p.bandFarbe} strokeWidth={1.2} strokeOpacity={0.6} />
        {achse.map((q, i) => (
          <circle key={i} cx={q.x} cy={q.y} r={2} fill={p.bandFarbe} fillOpacity={0.9} />
        ))}
      </g>

      {/* Rollwegnamen AUSSERHALB der Zeichenfläche, auf ihrer Seite — wie
          in der Queransicht (§8.6.3: keine Beschriftung auf der Fläche). */}
      {namenOhneUeberlappung.map(({ a, xx }, i) => {
          const oben = a.seite === "left";
          const an = genutzt(a);
          return (
            <text
              key={`n-${a.name}-${a.seite}-${i}`}
              x={xx}
              y={oben ? y0 - 8 : y0 + (qMax - qMin) * k + 16}
              textAnchor="middle"
              fontSize={sf(10)}
              fontWeight={an ? 700 : 400}
              fill={an ? p.tokens.rollwegRand : "#7C8698"}
            >
              {a.name}
            </text>
          );
        })}

      {/* Marke ③ — dieselbe Bedingung wie in der Queransicht: nur mit Seite. */}
      {p.clearanceM != null && p.clearanceSide != null && (
        <g>
          <circle
            cx={x(p.clearanceM)}
            cy={p.clearanceSide === "left" ? bahnOben : bahnUnten}
            r={9}
            fill={p.tokens.rollout}
          />
          <text
            x={x(p.clearanceM)}
            y={(p.clearanceSide === "left" ? bahnOben : bahnUnten) + 3.8}
            textAnchor="middle"
            fontSize={sf(11)}
            fontWeight={600}
            fill="#0B0F17"
          >
            3
          </text>
        </g>
      )}

      {/* Massstab und Landerichtung — ohne sie ist „echter Massstab" eine Behauptung. */}
      <text x={x0} y={H - 8} fontSize={sf(9.5)} fill="#7C8698">
        {t("runway_v2.lupe_direction", { defaultValue: "Landerichtung →" })}
      </text>
      <line
        x1={x0 + zeichenB - skala * k}
        y1={H - 20}
        x2={x0 + zeichenB}
        y2={H - 20}
        stroke="#C9D2E0"
        strokeWidth={2}
      />
      <text x={x0 + zeichenB - (skala * k) / 2} y={H - 6} fontSize={sf(9.5)} textAnchor="middle" fill="#C9D2E0">
        {skala} m
      </text>
    </svg>
      <div style={{ fontSize: "0.76rem", color: "#94a3b8", lineHeight: 1.5 }} data-zeile="so-liest-du">
        {t("runway_v2.lupe_erklaerung", {
          defaultValue:
            "Ausschnitt um die Stelle, an der die Bahn verlassen wurde — längs und quer im selben Maßstab, also mit den echten Winkeln. So ist das Flugzeug abgerollt.",
        })}
        {!hatRollweg &&
          ` ${t("runway_v2.lupe_ohne_rollweg", {
            defaultValue:
              "Rollwege liegen für diesen Flug nicht vor (Client vor v1.7.40 oder keine Bodenkarte für den Platz).",
          })}`}
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "5px 18px", fontSize: "0.72rem", color: "#94a3b8" }}>
        {legende.map((e) => (
          <span key={e.text} style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span
              style={{
                display: "inline-block",
                width: 11,
                height: e.flaeche ? 11 : 3,
                borderRadius: 2,
                background: e.farbe,
                opacity: e.flaeche ? 0.6 : 1,
              }}
            />
            {e.text}
          </span>
        ))}
      </div>
    </div>
  );
}