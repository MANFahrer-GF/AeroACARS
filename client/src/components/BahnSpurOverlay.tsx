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

import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { Projektion } from "../lib/runwayProjection";
import { aussenkanteHalbM, randstelle } from "../lib/bahnQuerMassstab";
import { bandFarbeFuer, gruppiere, type Ausfahrt } from "./bahnHilfen";
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
  /**
   * Die genutzte Ausfahrt — EINE Quelle (`genutzteAusfahrt` in bahnHilfen,
   * ab 2.1.8 vom Client gemeldet). `ausfahrten` ist dazu schon auf einen
   * Namen je Abzweigung zusammengefasst (`ausfahrtenFuerAnzeige`).
   */
  genutzt?: Ausfahrt | null;
  /** Zeichenbereich in x — Texte und Pfeile bleiben darin. */
  bildVonX: number;
  bildBisX: number;
  /** Zahl mit dem Dezimalzeichen der Sprache (de/it Komma, en Punkt). */
  dez: (n: number, stellen?: number) => string;
  /** Texte der Draufsicht selbst (Aufsetzzone, Räumpunkt …) — die neuen
   *  Beschriftungen hier weichen ihnen aus. */
  fremdeTexte?: Kasten[];
}

/** Ein Textkasten wie in der Lesbarkeitsprüfung (`RunwayLesbarkeit.test`). */
export type Kasten = { x: number; y: number; b: number; h: number };

/** Kasten eines Textes — Breite 0,62 em je Zeichen (wie `gruppiere`). */
export function textKasten(
  x: number,
  grundlinie: number,
  text: string,
  schrift: number,
  anker: "start" | "middle" | "end",
): Kasten {
  const b = text.length * schrift * 0.62;
  return { x: anker === "middle" ? x - b / 2 : anker === "end" ? x - b : x, y: grundlinie - schrift * 0.8, b, h: schrift * 1.15 };
}

const ueberlappt = (a: Kasten, b: Kasten) =>
  !(a.x + a.b <= b.x || b.x + b.b <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y);

type P = { x: number; y: number };

/**
 * Die gezeichnete Spur: nach dem Räumpunkt abgeschnitten, sobald sie das Bild
 * verlässt (eine Ausfahrt ist kein Ausbrechen); davor geklemmt — ein
 * Ausbrechen bleibt im Bild. `raus`: die erste Probe ausserhalb danach.
 */
export function spurImBild(
  samples: Array<{ laengs_m: number; quer_m: number }>,
  clearanceM: number | null,
  sichtbarM: number,
) {
  const spur: Array<{ laengs_m: number; quer_m: number; geklemmt: boolean }> = [];
  let raus: { laengs_m: number; quer_m: number } | null = null;
  for (const s of samples) {
    const nachRaeum = clearanceM != null && s.laengs_m > clearanceM;
    if (nachRaeum && Math.abs(s.quer_m) > sichtbarM) {
      raus = s;
      break;
    }
    spur.push({ ...s, geklemmt: Math.abs(s.quer_m) > sichtbarM });
  }
  return { spur, raus };
}

export interface BahnHinweis {
  rolle: "randstelle-text" | "aufgesetzt-neben" | "laeuft-weiter" | "hinweis-ohne-spur";
  text: string;
  farbe: string;
  fett: boolean;
}

/**
 * Die Textzeilen unter der Draufsicht — als TEXT (HTML), nicht im Bild:
 * Sie brechen am Handy um (Sichtprüfung 22.10.2026).
 *
 * „Spur läuft weiter — real bis 102 m“ ist entfallen (Entscheidung 5,
 * #1936): Es las sich als Länge, gemeint war der Abstand quer. Wo die Spur
 * das Bild verlässt, steht jetzt ein Pfeil im Bild („weiter auf Rollweg B“).
 */
export function bahnHinweise(
  p: BahnSpurOverlayProps,
  t: (k: string, o?: Record<string, unknown>) => string,
): BahnHinweis[] {
  const fmt = (n: number) => p.dez(n, 1);
  const seite = (q: number) => t(q < 0 ? "runway_v2.b.seite_links" : "runway_v2.b.seite_rechts");
  if (p.samples.length < 2) {
    return [{ rolle: "hinweis-ohne-spur", text: t("runway_v2.b.ohne_spur"), farbe: "#fbbf24", fett: false }];
  }
  const aus: BahnHinweis[] = [];
  const rs =
    p.trackWidthM != null && p.trackWidthM > 0
      ? randstelle(p.samples, p.halbeBahnM, p.trackWidthM, p.bewertungsEndeM, p.minEdgeClearanceM)
      : null;
  if (rs && (p.mitRandabstand !== false || p.unbefestigt)) {
    const draussen = rs.randabstand_m < 0 && !p.unbefestigt;
    aus.push({
      rolle: "randstelle-text",
      text: p.unbefestigt
        ? t("runway_v2.b.gras", { m: fmt(Math.abs(rs.quer_m)), seite: seite(rs.quer_m) })
        : draussen
        ? t("runway_v2.b.rad_draussen", { m: fmt(-rs.randabstand_m), seite: seite(rs.quer_m), laengs: rs.laengs_m.toFixed(0) })
        : t("runway_v2.b.knappste", { m: fmt(rs.randabstand_m), laengs: rs.laengs_m.toFixed(0) }),
      farbe: draussen ? p.tokens.tdSevere : "#cbd5e1",
      fett: true,
    });
  }
  if (p.aufgesetztNebenM != null && p.aufgesetztNebenM > 0)
    aus.push({
      rolle: "aufgesetzt-neben",
      text: t("runway_v2.b.aufgesetzt_neben", { m: fmt(p.aufgesetztNebenM) }),
      farbe: p.tokens.tdSevere,
      fett: true,
    });
  const { spur } = spurImBild(p.samples, p.clearanceM, p.halbeBahnM + p.randM);
  if (spur.some((s) => s.geklemmt))
    aus.push({
      rolle: "laeuft-weiter",
      text: t("runway_v2.b.geklemmt", { m: fmt(Math.max(...spur.map((s) => Math.abs(s.quer_m)))) }),
      farbe: "#fbbf24",
      fett: false,
    });
  return aus;
}

/** Die Hinweiszeilen als HTML unter der Grafik. */
export function BahnSpurHinweise({ zeilen }: { zeilen: BahnHinweis[] }) {
  if (!zeilen.length) return null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2, fontFamily: "monospace", fontSize: "0.8rem", padding: "4px 8px", overflowWrap: "anywhere" }}>
      {zeilen.map((z) => (
        <div key={z.rolle} data-rolle={z.rolle} style={{ color: z.farbe, fontWeight: z.fett ? 700 : 400 }}>
          {z.text}
        </div>
      ))}
    </div>
  );
}

/** Breite eines Textes in SVG-Einheiten — dieselbe Schätzung wie `gruppiere`. */
const breite = (text: string, schrift: number) => text.length * schrift * 0.62;

/** Farbe der Marke 2 — dieselben Schwellen wie das Band (`bandFarbeFuer`). */
export function marke2Farbe(
  tokens: BahnSpurOverlayProps["tokens"],
  randabstandM: number | null,
  unbefestigt: boolean | undefined,
): string {
  if (unbefestigt || randabstandM == null) return "#94a3b8";
  return bandFarbeFuer(tokens, null, randabstandM);
}

export function BahnSpurOverlay(p: BahnSpurOverlayProps) {
  const { t } = useTranslation();
  const clipId = `bahn-${useId().replace(/:/g, "")}`;
  const sichtbarM = p.halbeBahnM + p.randM;
  const y = (q: number) => p.mitteY + Math.max(-sichtbarM, Math.min(sichtbarM, q)) * p.pxProQuerM;
  const x = (m: number) => p.projektion.mAbBahnanfangZuX(m);
  const kanteOben = y(-p.halbeBahnM);
  const kanteUnten = y(p.halbeBahnM);
  const lang = p.randM * p.pxProQuerM;
  const fmt = (n: number) => p.dez(n, 1);
  // Dunkler Rand um die neuen Beschriftungen, damit sie auf Band und
  // Grünstreifen lesbar bleiben. Im Druck (Schrift-Untergrenze gesetzt)
  // schmaler: 3 Einheiten verschmierten dort die Schrift (PDF #1936).
  const halo = {
    stroke: "#0c1628",
    strokeWidth: p.sf(0) > 0 ? 1.2 : 3,
    paintOrder: "stroke" as const,
    strokeLinejoin: "round" as const,
  };
  /** x eines Textes so setzen, dass er ganz im Zeichenbereich liegt. */
  const imBild = (xx: number, text: string, schrift: number, anker: "start" | "middle" | "end") => {
    const b = breite(text, schrift);
    const links = anker === "middle" ? xx - b / 2 : anker === "end" ? xx - b : xx;
    const verschiebung = Math.max(0, p.bildVonX - links) - Math.max(0, links + b - p.bildBisX);
    return xx + verschiebung;
  };

  // Kantenangaben IM Grünstreifen am Bahnanfang, direkt an der Kante
  // (Thomas 22.10.2026: keine eigene Spalte rechts, „Mitte“ entfällt — die
  // gestrichelte Mittellinie spricht für sich). Die Ausfahrtsnamen stehen
  // in der äusseren Hälfte des Streifens, diese Angaben in der inneren.
  const skala = (
    <g data-rolle="querskala" data-legende="beschriftung">
      {[-p.halbeBahnM, p.halbeBahnM].map((q) => (
        <text
          key={q}
          data-kante={q < 0 ? "links" : "rechts"}
          x={p.projektion.bahnAnfangX + 4}
          y={q < 0 ? kanteOben - 3 : kanteUnten + p.sf(9.5) - 0.5}
          // 9,5: Schriftgrösse 9 tragen nur die Ausfahrtsnamen (QS zählt sie).
          fontSize={p.sf(9.5)}
          fill="#C9D2E0"
          fontFamily="monospace"
        >
          {t(q < 0 ? "runway_v2.b.skala_kante_links" : "runway_v2.b.skala_kante_rechts", { m: fmt(Math.abs(q)) })}
        </text>
      ))}
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
            data-legende="ersatz"
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
      </g>
    );
  }

  const aussenM = p.trackWidthM != null && p.trackWidthM > 0 ? aussenkanteHalbM(p.trackWidthM) : 0;

  const { spur, raus } = spurImBild(p.samples, p.clearanceM, sichtbarM);
  const achse: P[] = spur.map((s) => ({ x: x(s.laengs_m), y: y(s.quer_m) }));
  // Das Band senkrecht zur gezeichneten Linie abgesetzt — wie bis 2.1.7
  // (Thomas: so ist die Kurve zu sehen; quer abgesetzt wurde sie in der
  // Ausfahrt zur Linie gequetscht).
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
  const mitRand = rs != null && (p.mitRandabstand !== false || !!p.unbefestigt);

  const tdX = p.projektion.mToX(p.tdM);
  // Marke 1 sitzt AUF dem Aufsetzpunkt der Draufsicht (nur die Ziffer).
  const marken: Array<{ n: number; x: number; y: number; farbe: string | null; legende: string }> = [];
  // Marke 2 dort, wo die Ereignisliste den Grösstversatz nennt (dieselbe
  // Suche, `stelleDesVersatzes`) — sonst nennen Bild und Text zwei Orte.
  // Farbe nach dem Randabstand (Entscheidung 3): grün viel, gelb eng, rot
  // Rad neben der Bahn — dieselben Schwellen wie das Band.
  if (p.mitVersatzMarke !== false && p.maxLateralOffsetM != null) {
    const st = stelleDesVersatzes(p.samples, p.maxLateralOffsetM, p.bewertungsEndeM);
    if (st)
      marken.push({
        n: 2,
        x: x(st.probe.laengs_m),
        y: y(st.probe.quer_m),
        farbe: marke2Farbe(p.tokens, mitRand ? rs!.randabstand_m : null, p.unbefestigt),
        legende: "marke2",
      });
  }
  if (p.clearanceM != null && p.clearanceSide != null)
    marken.push({ n: 3, x: x(p.clearanceM), y: p.clearanceSide === "left" ? kanteOben : kanteUnten, farbe: p.tokens.rollout, legende: "marke3" });
  else if (
    achse.length > 0 &&
    // Dieselbe Bedingung wie der Listeneintrag: Räumpunkt ohne Seite, oder
    // ohne Räumpunkt und Überrollen ein Spurende abseits des Grösstversatzes.
    (p.clearanceM != null ||
      ((p.overrunM ?? 0) <= 0 &&
        !(p.maxLateralOffsetM != null && Math.abs(p.samples[p.samples.length - 1]!.quer_m - p.maxLateralOffsetM) < 0.5)))
  )
    marken.push({
      n: p.endpunktNummer ?? 3,
      x: achse[achse.length - 1]!.x,
      y: achse[achse.length - 1]!.y,
      farbe: p.tokens.rollout,
      legende: p.clearanceM != null ? "marke3" : "endpunkt",
    });
  if ((p.overrunM ?? 0) > 0)
    marken.push({ n: 4, x: p.projektion.bahnEndeX, y: p.mitteY, farbe: p.tokens.tdSevere, legende: "marke4" });
  // Marken überdecken sich nie (#1738): Liegt eine Marke näher als ihr
  // Durchmesser an einer früheren (oder am Aufsetzpunkt mit Marke 1), rückt
  // sie zur Bahnmitte hin nach.
  {
    const belegt: P[] = [{ x: tdX, y: y(p.tdOffsetM) }];
    for (const m of marken) {
      const richtung = m.y <= p.mitteY ? 1 : -1;
      for (let n = 0; n < 8 && belegt.some((b) => Math.hypot(b.x - m.x, b.y - m.y) < 20); n++) m.y += richtung * 20;
      belegt.push({ x: m.x, y: m.y });
    }
  }
  const istGenutzt = (a: Ausfahrt) =>
    p.genutzt != null && a.name === p.genutzt.name && a.seite === p.genutzt.seite && a.laengs_m === p.genutzt.laengs_m;

  // Alle Ausfahrten tragen ihren Namen (Inventar §3) — ein Name je
  // Abzweigung (Entscheidung 2), dicht liegende zusammengefasst
  // (`gruppiere`: höchstens zwei Kennungen, der Rest GEZÄHLT).
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
    p.genutzt != null && g.seite === p.genutzt.seite && g.name.split(/[/ ]/).includes(p.genutzt.name);

  // Wo die Spur das Bild verlässt (Entscheidung 5): ein kleiner Pfeil an
  // genau der Stelle, „weiter auf Rollweg B“ — der Name aus derselben
  // Quelle wie Kopf und Lupe. Quer über den Grünstreifen hinaus …
  let pfeil: { x: number; y: number; richtung: "oben" | "unten" | "rechts" } | null = null;
  const letzte = spur[spur.length - 1];
  if (raus && letzte) {
    const kante = Math.sign(raus.quer_m) * sichtbarM;
    const tt = (kante - letzte.quer_m) / (raus.quer_m - letzte.quer_m || 1);
    const mx = x(letzte.laengs_m + Math.max(0, Math.min(1, tt)) * (raus.laengs_m - letzte.laengs_m));
    if (mx >= p.bildVonX && mx <= p.bildBisX) pfeil = { x: mx, y: y(kante), richtung: raus.quer_m < 0 ? "oben" : "unten" };
  } else if (letzte && !p.projektion.gezoomt && p.projektion.mAbBahnanfangZuX(letzte.laengs_m) >= p.bildBisX - 0.5) {
    // … oder längs hinter dem Platz hinter dem Bahnende.
    pfeil = { x: p.bildBisX, y: y(letzte.quer_m), richtung: "rechts" };
  }
  // Liegt der helle Name der genutzten Ausfahrt an der Austrittsstelle,
  // steht der Pfeil ÜBER (bzw. unter) ihm — nicht darauf.
  if (pfeil && pfeil.richtung !== "rechts") {
    const g = gruppen.find(istGenutzteGruppe);
    if (g && Math.abs(x(g.laengs_m) - pfeil.x) < 24)
      pfeil = { ...pfeil, y: pfeil.y + (pfeil.richtung === "oben" ? -1 : 1) * (p.sf(9) + 6) };
  }
  const pfeilText = p.genutzt
    ? t("runway_v2.b.weiter_rollweg", { name: p.genutzt.name })
    : t("runway_v2.b.weiter_ohne_name");
  const genutzteGruppe = gruppen.find(istGenutzteGruppe);

  // Freie Plätze für die neuen Beschriftungen (Pfeil, Marke 2, Dreieck):
  // nicht auf anderen Texten, Marken oder dem Band, nicht aus dem Bild.
  type Platz = { x: number; y: number; anker: "start" | "middle" | "end" };
  const belegt: Kasten[] = [
    ...(p.fremdeTexte ?? []),
    ...gruppen.map((g) => {
      const oben = g.seite === "left";
      const y0 = oben ? kanteOben : kanteUnten;
      const hell = istGenutzteGruppe(g);
      const gy = hell ? (oben ? y0 - lang - 4 : y0 + lang + p.sf(9) + 2) : oben ? y0 - lang + p.sf(9) + 1 : y0 + lang - 4;
      return textKasten(imBild(x(g.laengs_m), g.name, p.sf(9), "middle"), gy, g.name, p.sf(9), "middle");
    }),
    { x: tdX - 11, y: y(p.tdOffsetM) - 11, b: 22, h: 22 },
    ...marken.map((m) => ({ x: m.x - 10, y: m.y - 10, b: 20, h: 20 })),
  ];
  const bandPunkte = [...links, ...rechts];
  const setze = (text: string, schrift: number, plaetze: Platz[]): Platz => {
    const frei = (q: Platz) => {
      const k = textKasten(q.x, q.y, text, schrift, q.anker);
      return (
        k.x >= p.bildVonX &&
        k.x + k.b <= p.bildBisX &&
        !belegt.some((b) => ueberlappt(k, b)) &&
        !bandPunkte.some((q2) => q2.x >= k.x && q2.x <= k.x + k.b && q2.y >= k.y && q2.y <= k.y + k.h)
      );
    };
    const q = plaetze.find(frei) ?? plaetze[0]!;
    belegt.push(textKasten(q.x, q.y, text, schrift, q.anker));
    return q;
  };

  const endeX = p.messEndeM != null ? x(p.messEndeM) : null;
  const marke2 = marken.find((m) => m.n === 2);
  const marke2Text =
    marke2 && mitRand && !p.unbefestigt
      ? rs!.randabstand_m < 0
        ? t("runway_v2.b.marke2_neben", { m: fmt(-rs!.randabstand_m) })
        : t("runway_v2.b.marke2_kante", { m: fmt(rs!.randabstand_m) })
      : null;
  // Plätze in dieser Reihenfolge: Pfeil (an seine Stelle gebunden), Marke 2,
  // Dreieck.
  const PFEIL_S = 7;
  const pfeilSchrift = p.sf(9.5);
  const pfeilPlatz = pfeil
    ? (() => {
        const s0 = PFEIL_S;
        // Neben dem Pfeil, nicht auf dem hellen Namen des Stummels.
        const nameRand = genutzteGruppe ? x(genutzteGruppe.laengs_m) - breite(genutzteGruppe.name, p.sf(9)) / 2 : pfeil.x;
        const links = Math.min(pfeil.x - s0 - 3, nameRand - 5);
        const rechtsX = pfeil.x + s0 + 3;
        const yNah =
          pfeil.richtung === "oben" ? pfeil.y - s0 - 4 : pfeil.richtung === "unten" ? pfeil.y + s0 + pfeilSchrift + 2 : pfeil.y - s0 - 4;
        const yFern = pfeil.richtung === "unten" ? yNah + pfeilSchrift + 3 : yNah - pfeilSchrift - 3;
        return setze(pfeilText, pfeilSchrift, [
          { x: links, y: yNah, anker: "end" },
          { x: rechtsX, y: yNah, anker: "start" },
          { x: links, y: yFern, anker: "end" },
          { x: rechtsX, y: yFern, anker: "start" },
          { x: pfeil.x, y: yFern, anker: "middle" },
        ]);
      })()
    : null;
  const marke2Schrift = p.sf(10);
  const marke2Platz =
    marke2 && marke2Text
      ? (() => {
          // Direkt neben der Marke (Entscheidung 3) — zuerst zur Bahnmitte
          // hin knapp neben dem Band, dann zur Kante hin, dann seitlich.
          const q = p.samples.reduce((a, b) => (Math.abs(x(b.laengs_m) - marke2.x) < Math.abs(x(a.laengs_m) - marke2.x) ? b : a)).quer_m;
          const unter = Math.max(marke2.y + 9, y(q + aussenM)) + marke2Schrift + 2;
          const ueber = Math.min(marke2.y - 9, y(q - aussenM)) - 4;
          const innen = q < 0 ? [unter, ueber] : [ueber, unter];
          const weiter = (yy: number) => yy + (yy > marke2.y ? 1 : -1) * (marke2Schrift + 3);
          return setze(marke2Text, marke2Schrift, [
            { x: marke2.x, y: innen[0]!, anker: "middle" },
            { x: marke2.x, y: innen[1]!, anker: "middle" },
            { x: marke2.x + 13, y: marke2.y + 3.8, anker: "start" },
            { x: marke2.x - 13, y: marke2.y + 3.8, anker: "end" },
            { x: marke2.x, y: weiter(innen[0]!), anker: "middle" },
            { x: marke2.x, y: weiter(innen[1]!), anker: "middle" },
            { x: marke2.x + 11, y: innen[0]!, anker: "start" },
            { x: marke2.x - 11, y: innen[0]!, anker: "end" },
          ]);
        })()
      : null;
  const endeText = t("runway_v2.b.bewertung_bis_hier");
  // 9,5 statt 9: Schriftgrösse 9 tragen nur die Ausfahrtsnamen (QS zählt sie daran).
  const endeSchrift = p.sf(9.5);
  const endePlatz =
    endeX != null
      ? setze(endeText, endeSchrift, [
          { x: endeX + 9, y: kanteUnten + 9, anker: "start" },
          { x: endeX - 9, y: kanteUnten + 9, anker: "end" },
          { x: endeX + 30, y: kanteUnten + 9, anker: "start" },
          { x: endeX - 30, y: kanteUnten + 9, anker: "end" },
          { x: endeX + 9, y: kanteUnten + lang + endeSchrift + 2, anker: "start" },
          { x: endeX - 9, y: kanteUnten + lang + endeSchrift + 2, anker: "end" },
        ])
      : null;

  // Die Spur selbst — vor dem Bewertungsende kräftig, danach deutlich
  // blasser (Entscheidung 4): Dort zählt sie nicht mehr.
  const spurTeil = (nach: boolean) => (
    <g clipPath={endeX != null ? `url(#${clipId}-${nach ? "nach" : "vor"})` : undefined} opacity={nach ? 0.35 : 1}>
      <path
        data-rolle={nach ? "band-nach" : "band"}
        data-legende="band"
        d={flaeche}
        fill={p.bandFarbe}
        fillOpacity={nach ? 0.15 : 0.45}
        stroke="none"
      />
      <polyline data-rolle={nach ? "rad-links-nach" : "rad-links"} data-legende="band" points={pts(links)} fill="none" stroke={p.bandFarbe} strokeWidth={1.4} />
      <polyline data-rolle={nach ? "rad-rechts-nach" : "rad-rechts"} data-legende="band" points={pts(rechts)} fill="none" stroke={p.bandFarbe} strokeWidth={1.4} />
      <polyline
        data-rolle={nach ? "spur-mitte-nach" : "spur-mitte"}
        data-legende="mitte"
        points={pts(achse)}
        fill="none"
        stroke="#e2e8f0"
        strokeWidth={1}
        opacity={0.7}
      />
    </g>
  );
  // Die Messpunkte selbst (Inventar §3) — je einmal, nach dem
  // Bewertungsende blasser.
  const messpunkte = (
    <g data-legende="mitte">
      {achse.map((q, i) => (
        <circle
          key={`p${i}`}
          cx={q.x.toFixed(1)}
          cy={q.y.toFixed(1)}
          r="1.8"
          fill="#e2e8f0"
          opacity={endeX != null && q.x > endeX ? 0.3 : 0.8}
        />
      ))}
    </g>
  );

  return (
    <g data-rolle="spur-overlay">
      {skala}
      {endeX != null && (
        <defs>
          {/* Als Pfad, nicht als Rechteck: Prüfungen suchen die Bahn als
              grösstes Rechteck im Bild. */}
          <clipPath id={`${clipId}-vor`}>
            <path d={`M -10 -10 H ${endeX.toFixed(1)} V 100000 H -10 Z`} />
          </clipPath>
          <clipPath id={`${clipId}-nach`}>
            <path d={`M ${endeX.toFixed(1)} -10 H 100000 V 100000 H ${endeX.toFixed(1)} Z`} />
          </clipPath>
        </defs>
      )}
      {(p.ausfahrten ?? []).map((a, i) => {
        const ax = x(a.laengs_m);
        if (ax < p.projektion.bahnAnfangX || ax > p.projektion.bahnEndeX) return null;
        const oben = a.seite === "left";
        const y0 = oben ? kanteOben : kanteUnten;
        return (
          <rect
            key={`a${i}`}
            data-rolle="ausfahrt"
            data-legende="rollwege"
            x={ax - 4}
            y={oben ? y0 - lang : y0}
            width={8}
            height={lang}
            fill={p.tokens.rollweg}
            opacity={istGenutzt(a) ? 0.95 : 0.4}
          />
        );
      })}
      {gruppen.map((g, i) => {
        const oben = g.seite === "left";
        const y0 = oben ? kanteOben : kanteUnten;
        const hell = istGenutzteGruppe(g);
        return (
          <text
            key={`n${i}`}
            data-rolle="ausfahrt-name"
            x={imBild(x(g.laengs_m), g.name, p.sf(9), "middle")}
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
          data-legende="durchgang"
          points={pts(d.map((s) => ({ x: x(s.laengs_m), y: y(s.quer_m) })))}
          fill="none"
          stroke="#a78bfa"
          strokeWidth={2}
          strokeDasharray="5 4"
        />
      ))}
      {spurTeil(false)}
      {endeX != null && spurTeil(true)}
      {messpunkte}
      {p.messEndeM != null && endeX != null && (
        <g data-legende="dreieck">
          <path data-rolle="mess-ende" d={`M ${endeX} ${kanteUnten + 2} l -6 10 l 12 0 z`} fill="#fbbf24" />
          {/* Beschriftung direkt daneben (Entscheidung 4). */}
          {endePlatz && (
            <text
              data-rolle="mess-ende-text"
              x={endePlatz.x}
              y={endePlatz.y}
              textAnchor={endePlatz.anker}
              fontSize={endeSchrift}
              fill="#fbbf24"
              fontFamily="monospace"
              {...halo}
            >
              {endeText}
            </text>
          )}
        </g>
      )}
      {rs && (
        // Die knappste Stelle: nur noch die Zahl an Marke 2 — der gelbe
        // senkrechte Strich und die gestrichelte Linie ins Leere sind
        // entfallen (Entscheidung 3). Die Daten bleiben für Tests lesbar.
        <g data-rolle="randstelle" data-laengs={rs.laengs_m.toFixed(0)} data-randabstand={rs.randabstand_m.toFixed(2)}>
          {marke2 && marke2Text && marke2Platz && (
            <text
              data-rolle="marke2-text"
              x={marke2Platz.x}
              y={marke2Platz.y}
              textAnchor={marke2Platz.anker}
              fontSize={marke2Schrift}
              fontWeight={700}
              fill={marke2.farbe!}
              fontFamily="monospace"
              {...halo}
            >
              {marke2Text}
            </text>
          )}
        </g>
      )}
      {pfeil && pfeilPlatz && (() => {
        // Pfeilspitze als Fläche (kein Pfeilzeichen: B612 Mono kennt es nicht).
        const s = PFEIL_S;
        const spitze =
          pfeil.richtung === "oben"
            ? `${pfeil.x},${pfeil.y - 2 * s} ${pfeil.x - s},${pfeil.y - s} ${pfeil.x + s},${pfeil.y - s}`
            : pfeil.richtung === "unten"
            ? `${pfeil.x},${pfeil.y + 2 * s} ${pfeil.x - s},${pfeil.y + s} ${pfeil.x + s},${pfeil.y + s}`
            : `${pfeil.x},${pfeil.y} ${pfeil.x - s},${pfeil.y - s} ${pfeil.x - s},${pfeil.y + s}`;
        return (
          <g data-rolle="weiter-pfeil" data-legende="pfeil">
            <polygon points={spitze} fill="#e2e8f0" />
            <text
              x={pfeilPlatz.x}
              y={pfeilPlatz.y}
              textAnchor={pfeilPlatz.anker}
              fontSize={pfeilSchrift}
              fill="#e2e8f0"
              fontFamily="monospace"
              {...halo}
            >
              {pfeilText}
            </text>
          </g>
        );
      })()}
      {marken.map((m) => (
        <g key={`m${m.n}`} data-marke={m.n} data-legende={m.legende} data-legende-farbe={m.farbe ?? undefined}>
          {m.farbe != null && <circle cx={m.x} cy={m.y} r={9} fill={m.farbe} stroke="#0c1628" strokeWidth={1.5} />}
          <text x={m.x} y={m.y + 3.8} textAnchor="middle" fontSize={p.sf(11)} fontWeight={700} fill="#0B0F17">
            {m.n}
          </text>
        </g>
      ))}
    </g>
  );
}
