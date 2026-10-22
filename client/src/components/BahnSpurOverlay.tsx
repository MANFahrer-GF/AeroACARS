// Bahnansicht Variante B (Thomas 22.10.2026, Spec v1.7.0 §8.3) — die echte
// Spur in der Draufsicht. Draufsicht und Queransicht sind EINE überhöhte
// Ansicht; alles, was bis dahin nur die Queransicht zeigte, steht hier.
//
// Das Band reicht bis zur REIFEN-AUSSENKANTE (Spurweite/2 + halbes
// Radpaket), weil genau diese Kante bewertet wird (`spurweite.rs`,
// `aussenkante_halb_aus_spur`). Die beiden Bandkanten sind die Linien der
// äusseren Räder; sie tragen `data-rolle="rad-links"` / `"rad-rechts"`,
// damit der Render-Test die GEZEICHNETE Geometrie gegen die gezeichnete
// Bahnkante prüfen kann.
//
// Das Band läuft durch bis zur Ausfahrt, wird nie schmaler und endet nie an
// einer Heuristik. Vor dem Räumpunkt wird es am Bildrand GEKLEMMT, nicht
// abgeschnitten — ein Abkommen bleibt sichtbar.
//
// Seitenkonvention wie überall (§8.6): oben = links in Landerichtung,
// intern quer > 0 = rechts.

import { useTranslation } from "react-i18next";
import type { Projektion } from "../lib/runwayProjection";
import { aussenkanteHalbM, randstelle } from "../lib/bahnQuerMassstab";
import { gruppiere, type Ausfahrt } from "./bahnHilfen";
import { stelleDesVersatzes } from "./RunwayDisciplinePanel";

export interface BahnSpurOverlayProps {
  projektion: Projektion;
  mitteY: number;
  pxProQuerM: number;
  halbeBahnM: number;
  /** Wie weit neben der Kante noch gezeichnet wird (Grünstreifen), in m. */
  randM: number;
  samples: Array<{ laengs_m: number; quer_m: number }>;
  trackWidthM: number | null;
  /** Ende des bewerteten Teils (frühestes von mess_ende, cutoff, Räumpunkt). */
  bewertungsEndeM: number | null;
  /** Gespeicherter kleinster Randabstand — die Zahl im Bild, wenn vorhanden. */
  minEdgeClearanceM?: number | null;
  clearanceM: number | null;
  clearanceSide: "left" | "right" | null;
  overrunM: number | null;
  tdM: number;
  tdOffsetM: number;
  /**
   * „aufgesetzt X m neben der Bahn“ — nur gesetzt, wenn die Achse
   * Ausrichtung `off_runway_surface` meldet (Entscheidung 11). Sonst null.
   */
  aufgesetztNebenM?: number | null;
  /** Ende der alten Ausroll-Linie, falls es keine Spur gibt. */
  ersatzEndeX: number | null;
  bandFarbe: string;
  tokens: { rollout: string; tdPerfect: string; tdWarn: string; tdSevere: string; rollweg: string };
  sf: (g: number) => number;
  ausfahrten?: Ausfahrt[] | null;
  /** Ende des bewerteten Teils für die Marke ▲ (frühestes Ende, B3). */
  messEndeM?: number | null;
  durchgaenge?: Array<Array<{ laengs_m: number; quer_m: number }>>;
  /** y für die erste Textzeile unter der Bahn. */
  textY: number;
  /** Unbefestigte Bahn: der Rand wird nicht bewertet (§7.1) — nichts behaupten. */
  unbefestigt?: boolean;
  /** Marke ② nur, wenn die Ereignisliste sie führt (`versatzUnglaubwuerdig`). */
  mitVersatzMarke?: boolean;
  /** Randabstand als Zahl nur, wenn die Kante ihn trägt (`kanteTraegtNicht`). */
  mitRandabstand?: boolean;
  /** Nummer des Spurendes ohne Räumseite — dieselbe wie in der Liste (`endpunktNummer`). */
  endpunktNummer?: number;
  /** Gespeicherter Grösstversatz — die Liste führt kein Spurende, wenn es dort liegt. */
  maxLateralOffsetM?: number | null;
}

type P = { x: number; y: number };

/** Zeilenabstand der Textzeilen unter der Bahn, in Schriftgrössen. */
const ZEILE = 14;

export function BahnSpurOverlay(p: BahnSpurOverlayProps) {
  const { t } = useTranslation();
  const sichtbarM = p.halbeBahnM + p.randM;
  const y = (q: number) => p.mitteY + Math.max(-sichtbarM, Math.min(sichtbarM, q)) * p.pxProQuerM;
  const x = (m: number) => p.projektion.mAbBahnanfangZuX(m);
  const kanteOben = y(-p.halbeBahnM);
  const kanteUnten = y(p.halbeBahnM);
  const fmt = (n: number) => n.toFixed(1).replace(".", ",");
  const seite = (q: number) => t(q < 0 ? "runway_v2.b.seite_links" : "runway_v2.b.seite_rechts");

  // Querskala am rechten Bildrand — die Überhöhung bleibt ablesbar
  // (Inventar §3: Querskala, Kanten-Beschriftung; vorher Queransicht).
  const skalaX = p.projektion.bahnEndeX + 6;
  const skala = (
    <g data-rolle="querskala">
      {[-p.halbeBahnM, 0, p.halbeBahnM].map((q) => (
        <g key={q}>
          <line x1={skalaX} y1={y(q)} x2={skalaX + 6} y2={y(q)} stroke="#8B95A8" strokeWidth={1} />
          <text x={skalaX + 9} y={y(q) + 3.5} fontSize={p.sf(9.5)} fill="#C9D2E0" fontFamily="monospace">
            {q === 0 ? "0" : `${fmt(Math.abs(q))} m`}
          </text>
        </g>
      ))}
      <text x={skalaX + 9} y={kanteOben - 8} fontSize={p.sf(9.5)} fill="#8B95A8" fontFamily="monospace">
        {t("runway_v2.b.kante")}
      </text>
      {/* Seiten in Landerichtung, wie früher am Rand der Queransicht. */}
      <text x={skalaX + 9} y={kanteOben - 8 - p.sf(9.5) - 5} fontSize={p.sf(9.5)} fontWeight={600} fill="#9AA5B5">
        {t("runway_v2.side_left", { defaultValue: "LINKS" })}
      </text>
      <text x={skalaX + 9} y={y(sichtbarM) + p.sf(9.5) + 2} fontSize={p.sf(9.5)} fontWeight={600} fill="#9AA5B5">
        {t("runway_v2.side_right", { defaultValue: "RECHTS" })}
      </text>
    </g>
  );

  // Ohne Spur: die alte Linie, aber gestrichelt und mit Grund — eine
  // durchgezogene Linie sähe aus wie eine Messung „immer mittig".
  if (p.samples.length < 2) {
    const tdX = p.projektion.mToX(p.tdM);
    const tdY = y(p.tdOffsetM);
    return (
      <g data-rolle="ohne-spur">
        {skala}
        {p.ersatzEndeX != null && p.ersatzEndeX > tdX && (
          <line
            data-rolle="ersatz-linie"
            x1={tdX}
            y1={tdY}
            x2={p.ersatzEndeX}
            y2={tdY}
            stroke={p.tokens.rollout}
            strokeWidth={2}
            strokeDasharray="6 6"
            opacity={0.7}
          />
        )}
        <text
          data-rolle="hinweis-ohne-spur"
          x={p.projektion.bahnAnfangX}
          y={p.textY}
          fontSize={p.sf(11)}
          fill="#fbbf24"
          fontFamily="monospace"
        >
          {t("runway_v2.b.ohne_spur")}
        </text>
      </g>
    );
  }

  const aussenM = p.trackWidthM != null && p.trackWidthM > 0 ? aussenkanteHalbM(p.trackWidthM) : 0;

  // Nach dem Räumpunkt wird abgeschnitten, sobald die Spur das Bild
  // verlässt (eine Ausfahrt ist kein Ausbrechen); davor wird geklemmt —
  // ein Ausbrechen soll im Bild bleiben, nicht verschwinden.
  const spur: Array<{ laengs_m: number; quer_m: number; geklemmt: boolean }> = [];
  // Wie weit die Spur hinter dem Bildrand real noch reicht (nach dem Räumen).
  let ausserBildM: number | null = null;
  for (let i = 0; i < p.samples.length; i++) {
    const s = p.samples[i]!;
    const nachRaeum = p.clearanceM != null && s.laengs_m > p.clearanceM;
    if (nachRaeum && Math.abs(s.quer_m) > sichtbarM) {
      ausserBildM = Math.max(...p.samples.slice(i).map((r) => Math.abs(r.quer_m)));
      break;
    }
    spur.push({ ...s, geklemmt: Math.abs(s.quer_m) > sichtbarM });
  }
  const achse: P[] = spur.map((s) => ({ x: x(s.laengs_m), y: y(s.quer_m) }));
  const halbPx = aussenM * p.pxProQuerM;
  const rand = (v: 1 | -1): P[] =>
    achse.map((q, i) => {
      const a = achse[Math.max(0, i - 1)]!;
      const b = achse[Math.min(achse.length - 1, i + 1)]!;
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const l = Math.hypot(dx, dy) || 1;
      return { x: q.x + (v * -dy * halbPx) / l, y: q.y + (v * dx * halbPx) / l };
    });
  const links = rand(-1);
  const rechts = rand(1);
  const pts = (a: P[]) => a.map((q) => `${q.x.toFixed(1)},${q.y.toFixed(1)}`).join(" ");
  const flaeche = `M ${pts(links).replace(/ /g, " L ")} L ${pts(rechts.slice().reverse()).replace(/ /g, " L ")} Z`;

  const rs =
    p.trackWidthM != null && p.trackWidthM > 0
      ? randstelle(p.samples, p.halbeBahnM, p.trackWidthM, p.bewertungsEndeM, p.minEdgeClearanceM)
      : null;
  const draussen = rs != null && rs.randabstand_m < 0 && !p.unbefestigt;
  const rsX = rs ? x(rs.laengs_m) : 0;
  const rsRadY = rs ? y(rs.quer_m) + Math.sign(rs.quer_m || 1) * aussenM * p.pxProQuerM : 0;
  const rsKanteY = rs ? (rs.quer_m < 0 ? kanteOben : kanteUnten) : 0;

  const tdX = p.projektion.mToX(p.tdM);
  // Marke 1 sitzt AUF dem Aufsetzpunkt der Draufsicht (nur die Ziffer) —
  // ein zweiter Kreis an derselben Stelle wäre eine verdeckte Marke.
  const marken: Array<{ n: number; x: number; y: number; farbe: string | null }> = [];
  // Ziffer 1 zeichnet die Draufsicht selbst auf ihren Aufsetzpunkt (sonst
  // läge der Punkt über der Ziffer).
  // Marke 2 dort, wo die Ereignisliste den Grösstversatz nennt (dieselbe
  // Suche, `stelleDesVersatzes`) — sonst nennen Bild und Text zwei Orte.
  if (p.mitVersatzMarke !== false && p.maxLateralOffsetM != null) {
    const st = stelleDesVersatzes(p.samples, p.maxLateralOffsetM, p.bewertungsEndeM);
    if (st) marken.push({ n: 2, x: x(st.probe.laengs_m), y: y(st.probe.quer_m), farbe: p.tokens.tdWarn });
  }
  if (p.clearanceM != null && p.clearanceSide != null)
    marken.push({ n: 3, x: x(p.clearanceM), y: p.clearanceSide === "left" ? kanteOben : kanteUnten, farbe: p.tokens.rollout });
  else if (
    achse.length > 0 &&
    // Dieselbe Bedingung wie der Listeneintrag: Räumpunkt ohne Seite, oder
    // ohne Räumpunkt und Überrollen ein Spurende abseits des Grösstversatzes.
    (p.clearanceM != null ||
      ((p.overrunM ?? 0) <= 0 &&
        !(p.maxLateralOffsetM != null && Math.abs(p.samples[p.samples.length - 1]!.quer_m - p.maxLateralOffsetM) < 0.5)))
  )
    // Spurende ohne bekannte Räumseite — wie früher die Queransicht, mit
    // der Nummer der Ereignisliste.
    marken.push({ n: p.endpunktNummer ?? 3, x: achse[achse.length - 1]!.x, y: achse[achse.length - 1]!.y, farbe: p.tokens.rollout });
  if ((p.overrunM ?? 0) > 0) marken.push({ n: 4, x: p.projektion.bahnEndeX, y: p.mitteY, farbe: p.tokens.tdSevere });
  const genutzt =
    p.clearanceM != null && p.clearanceSide != null
      ? (p.ausfahrten ?? [])
          .filter((a) => a.seite === p.clearanceSide && Math.abs(a.laengs_m - p.clearanceM!) < 120)
          .sort((a, b) => Math.abs(a.laengs_m - p.clearanceM!) - Math.abs(b.laengs_m - p.clearanceM!))[0] ?? null
      : null;

  // Alle Ausfahrten tragen ihren Namen (Inventar §3) — zusammengefasst wie
  // früher in der Queransicht (`gruppiere`): höchstens zwei Kennungen, der
  // Rest GEZÄHLT („R11/M19 +2“), nie verschwiegen.
  const gruppen = gruppiere(
    (p.ausfahrten ?? []).filter(
      (a) =>
        Number.isFinite(a.laengs_m) &&
        a.laengs_m >= 0 &&
        a.laengs_m <= p.projektion.lengthM + p.projektion.spurVersatzM,
    ),
    p.projektion,
    p.sf(9),
  );
  const istGenutzteGruppe = (g: Ausfahrt) =>
    genutzt != null && g.seite === genutzt.seite && g.name.split(/[/ ]/).includes(genutzt.name);

  const letzteProbe = p.samples[p.samples.length - 1]!;
  const laeuftWeiter = x(letzteProbe.laengs_m) > p.projektion.bahnEndeX;
  const geklemmtMax = spur.some((s) => s.geklemmt) ? Math.max(...spur.map((s) => Math.abs(s.quer_m))) : null;

  const zeilenY = (i: number) => p.textY + i * p.sf(ZEILE);
  const textX = (lx: number, halbBreite: number) =>
    Math.min(Math.max(lx, p.projektion.bahnAnfangX + halbBreite), p.projektion.bahnEndeX - halbBreite);

  return (
    <g data-rolle="spur-overlay">
      {skala}
      {(p.ausfahrten ?? []).map((a, i) => {
        const ax = x(a.laengs_m);
        if (ax < p.projektion.bahnAnfangX || ax > p.projektion.bahnEndeX) return null;
        const oben = a.seite === "left";
        const y0 = oben ? kanteOben : kanteUnten;
        const lang = p.randM * p.pxProQuerM;
        return (
          <rect
            key={`a${i}`}
            data-rolle="ausfahrt"
            x={ax - 4}
            y={oben ? y0 - lang : y0}
            width={8}
            height={lang}
            fill={p.tokens.rollweg}
            opacity={genutzt === a ? 0.95 : 0.4}
          />
        );
      })}
      {gruppen.map((g, i) => {
        const ax = x(g.laengs_m);
        const oben = g.seite === "left";
        const y0 = oben ? kanteOben : kanteUnten;
        const lang = p.randM * p.pxProQuerM;
        const hell = istGenutzteGruppe(g);
        return (
          <text
            key={`n${i}`}
            data-rolle="ausfahrt-name"
            x={ax}
            // IM Grünstreifen, nicht darunter — dort stehen Bahnende,
            // Aim-Point und Aufsetz-Beschriftung. Die genutzte Ausfahrt
            // steht AUSSERHALB: durch ihren Stummel läuft die Spur hinaus.
            y={
              hell
                ? oben ? y0 - lang - 4 : y0 + lang + p.sf(9) + 2
                : oben ? y0 - lang + p.sf(9) + 1 : y0 + lang - 4
            }
            textAnchor="middle"
            fontSize={p.sf(9)}
            fontWeight={hell ? 700 : 400}
            fill={hell ? "#7dd3fc" : "#94a3b8"}
            fontFamily="monospace"
          >
            {g.name}
          </text>
        );
      })}
      {(p.durchgaenge ?? []).map((d, i) => (
        <polyline
          key={`d${i}`}
          data-durchgang={i + 1}
          data-rolle="durchgang"
          points={pts(d.map((s) => ({ x: x(s.laengs_m), y: y(s.quer_m) })))}
          fill="none"
          stroke="#a78bfa"
          strokeWidth={2}
          strokeDasharray="5 4"
        />
      ))}
      <path data-rolle="band" d={flaeche} fill={p.bandFarbe} fillOpacity={0.45} stroke="none" />
      <polyline data-rolle="rad-links" points={pts(links)} fill="none" stroke={p.bandFarbe} strokeWidth={1.4} />
      <polyline data-rolle="rad-rechts" points={pts(rechts)} fill="none" stroke={p.bandFarbe} strokeWidth={1.4} />
      <polyline data-rolle="spur-mitte" points={pts(achse)} fill="none" stroke="#e2e8f0" strokeWidth={1} opacity={0.7} />
      {/* Die Messpunkte selbst (Inventar §3, vorher Queransicht). */}
      {achse.map((q, i) => (
        <circle key={`p${i}`} cx={q.x.toFixed(1)} cy={q.y.toFixed(1)} r="1.8" fill="#e2e8f0" opacity={0.8} />
      ))}
      {p.messEndeM != null && (
        <path data-rolle="mess-ende" d={`M ${x(p.messEndeM)} ${kanteUnten + 2} l -6 10 l 12 0 z`} fill="#fbbf24" />
      )}
      {rs && (p.mitRandabstand !== false || p.unbefestigt) && (
        <g data-rolle="randstelle" data-laengs={rs.laengs_m.toFixed(0)} data-randabstand={rs.randabstand_m.toFixed(2)}>
          <line x1={rsX} y1={rsKanteY} x2={rsX} y2={rsRadY} stroke={draussen ? p.tokens.tdSevere : p.tokens.tdWarn} strokeWidth={3} />
          <line
            x1={rsX}
            // Unterhalb des Grünstreifens beginnen — dort stehen die
            // Ausfahrtsnamen.
            y1={Math.max(rsKanteY, rsRadY, kanteUnten + p.randM * p.pxProQuerM)}
            x2={rsX}
            y2={zeilenY(0) - p.sf(12)}
            stroke={draussen ? p.tokens.tdSevere : "#8B95A8"}
            strokeWidth={1}
            strokeDasharray="2 3"
          />
          <text
            data-rolle="randstelle-text"
            x={textX(rsX, 180)}
            y={zeilenY(0)}
            textAnchor="middle"
            fontSize={p.sf(12)}
            fontWeight={700}
            fill={draussen ? p.tokens.tdSevere : "#cbd5e1"}
            fontFamily="monospace"
          >
            {p.unbefestigt
              ? t("runway_v2.b.gras", { m: fmt(Math.abs(rs.quer_m)), seite: seite(rs.quer_m) })
              : draussen
              ? t("runway_v2.b.rad_draussen", { m: fmt(-rs.randabstand_m), seite: seite(rs.quer_m), laengs: rs.laengs_m.toFixed(0) })
              : t("runway_v2.b.knappste", { m: fmt(rs.randabstand_m), laengs: rs.laengs_m.toFixed(0) })}
          </text>
        </g>
      )}
      {p.aufgesetztNebenM != null && p.aufgesetztNebenM > 0 && (
        <text
          data-rolle="aufgesetzt-neben"
          x={textX(tdX, 150)}
          y={zeilenY(1)}
          textAnchor="middle"
          fontSize={p.sf(12)}
          fontWeight={700}
          fill={p.tokens.tdSevere}
          fontFamily="monospace"
        >
          {t("runway_v2.b.aufgesetzt_neben", { m: fmt(p.aufgesetztNebenM) })}
        </text>
      )}
      {(geklemmtMax != null || laeuftWeiter || ausserBildM != null) && (
        <text
          data-rolle="laeuft-weiter"
          data-annotation="spur-ausser-bild"
          x={p.projektion.bahnEndeX}
          y={zeilenY(2)}
          textAnchor="end"
          fontSize={p.sf(10)}
          fill="#fbbf24"
          fontFamily="monospace"
        >
          {[
            geklemmtMax != null ? t("runway_v2.b.geklemmt", { m: fmt(geklemmtMax) }) : null,
            laeuftWeiter ? t("runway_v2.b.laeuft_weiter", { m: letzteProbe.laengs_m.toFixed(0) }) : null,
            ausserBildM != null
              ? t("runway_v2.spur_ausser_bild", {
                  defaultValue: "Spur läuft weiter — real bis {{m}} m",
                  m: ausserBildM.toFixed(0),
                })
              : null,
          ]
            .filter(Boolean)
            .join(" · ")}
        </text>
      )}
      {marken.map((m) => (
        <g key={`m${m.n}`} data-marke={m.n}>
          {m.farbe != null && <circle cx={m.x} cy={m.y} r={9} fill={m.farbe} stroke="#0c1628" strokeWidth={1.5} />}
          <text x={m.x} y={m.y + 3.8} textAnchor="middle" fontSize={p.sf(11)} fontWeight={700} fill="#0B0F17">
            {m.n}
          </text>
        </g>
      ))}
    </g>
  );
}
