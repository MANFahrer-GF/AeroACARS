// Queransicht der Bahn — der gefahrene Streifen im Massstab der Bahnbreite.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.3, §8.5, §8.6.
// Referenzgrafik: `docs/spec/assets/v1.7.0-bahndisziplin-referenz.html`.
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
import type { BahnZoom } from "../lib/useBahnZoom";

/** Eine Ausfahrt: wo ein Rollweg die Bahnkante trifft. */
export interface Ausfahrt {
  /** Kennung des Rollwegs, z. B. `S4`. */
  name: string;
  /** Distanz ab der Landeschwelle, in Metern. */
  laengs_m: number;
  /** Auf welcher Seite der Bahn. */
  seite: "left" | "right";
  /**
   * Wie der Rollweg wirklich verläuft — in Bahn-Koordinaten.
   *
   * Kommt seit v1.7.4 aus den Bodendaten mit (`ausfahrten.rs`). Ältere
   * Flüge haben ihn nicht; dann bleibt es beim Stummel.
   */
  verlauf?: Array<{ laengs_m: number; quer_m: number }> | null;
}

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
  /** Räumpunkt: wo die Spur die Bahnkante überschreitet. */
  clearanceM?: number | null;
  /** Wo die Bewertung endet (Beginn des Ausschwenkens). */
  scoringCutoffM?: number | null;
  /** Wo das Messfenster schloss — siehe `messEnde` weiter unten. */
  messEndeM?: number | null;
  clearanceSide?: "left" | "right" | null;
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
  /** Kleinster Randabstand des äusseren Rades. Negativ = neben der Bahn. */
  minEdgeClearanceM?: number | null;
  /** Grösster Versatz — für die Marke ②. */
  maxLateralOffsetM?: number | null;
  /** Strecke über das Bahnende hinaus — für die Marke ④. */
  overrunM?: number | null;
  /** Rollwege, die die Bahn treffen (OSM). Optional. */
  ausfahrten?: Ausfahrt[] | null;
  /** ICAO-Typcode — steht mit der Spurweite im Kopf der Ansicht. */
  aircraftIcao?: string | null;
  /** Breite des SVG in Benutzereinheiten (muss der Längsansicht gleichen). */
  width: number;
  /** Derselbe Zoom wie die Längsansicht. Beide Ansichten, ein Zustand. */
  zoom?: BahnZoom;
  tokens: {
    tarmac: string;
    tarmacBorder: string;
    centerline: string;
    rollout: string;
    tdPerfect: string;
    tdWarn: string;
    tdSevere: string;
    /** Der genutzte Rollweg — Flaeche und Rand. Siehe `runwayV2Skin.ts`. */
    rollweg: string;
    rollwegRand: string;
  };
}

/**
 * Höhe der Grafik.
 *
 * §8.6.6: „Lieber höher als enger." Der erste Entwurf war 210 hoch — bei
 * 1200 Einheiten Breite ein Streifen von 80 Pixeln für die Bahn, in dem eine
 * Spur von drei Metern Versatz nicht mehr von der Mittellinie zu
 * unterscheiden war. Die Ansicht existiert aber genau dafür. Die
 * Referenzgrafik gibt der Bahnfläche 176 Einheiten; dieselbe Höhe steht hier.
 */
const H = 390;
/** Oberkante der befestigten Fläche. */
const BAHN_TOP = 74;
/**
 * Unterkante der befestigten Fläche.
 *
 * 264 statt 176 Einheiten Bahnhöhe. Der Grund ist das Seitenverhältnis:
 * Bei 1130 Einheiten Bildbreite wirkt alles Senkrechte klein, und die
 * Spurweite eines A320 — 7,59 m auf einer 46-m-Bahn, also 16,5 % — kam auf
 * neunundzwanzig Pixel. Rechnerisch richtig, aber nicht ablesbar: Dass da
 * sieben Meter stehen, sah man ihnen nicht an.
 *
 * Mit 264 Einheiten sind es dreiundvierzig Pixel. Das Verhältnis bleibt
 * unverändert — die Ansicht bekommt nur mehr Raum, wie es §8.6.6 vorsieht
 * („lieber höher als enger").
 */
const BAHN_BOT = 338;
/**
 * Bahnbreite, die die volle Höhe füllt — der feste Massstab der Ansicht.
 *
 * # Warum es diesen Wert gibt
 *
 * Bis hierher skalierte die Ansicht **immer** auf die Bahnbreite: Jede
 * Bahn füllte die Höhe, egal ob dreiundzwanzig oder sechzig Meter breit.
 * Das Band der Radspuren stand damit in jeder Grafik in einem anderen
 * Massstab — und drehte das Verhältnis um. Gemessen am 23.08.2026 über
 * die Demo-Varianten:
 *
 *     C208   Spur 3,6 m   Bahn 23 m   ->   Band 41,3 px
 *     B738   Spur 5,7 m   Bahn 45 m   ->   Band 33,6 px
 *
 * Die Cessna bekam ein breiteres Band als die 737. Thomas hat es gesehen
 * („5,7 B738 und 3 m bei einer Challenger sieht die Rollspur gleich breit
 * aus") — es war nicht gleich breit, es war verkehrt herum.
 *
 * Sechzig Meter, weil fünfundneunzig Prozent aller Bahnen im
 * Navdatenbestand darunter liegen (85 058 Bahnen: Median 30 m, 75 % ≤ 45,
 * 95 % ≤ 60). Breitere werden gestaucht — das betrifft eine von zwanzig,
 * und dort ist die Bahn so breit, dass es auf ein paar Pixel nicht ankommt.
 */
const REFERENZ_BREITE_M = 60;
/**
 * Kleinste gezeichnete Bahnhöhe.
 *
 * Ohne sie waere eine 9-m-Graspiste vierzig Pixel hoch — Band, Spur und
 * Randabstand liessen sich darin nicht mehr auseinanderhalten. Wo diese
 * Grenze greift, ist die Ansicht gedehnt, und die Kopfzeile sagt es.
 */
const MIN_BAHN_H = 120;
/** Höhe des Grünstreifens beidseits. */
const GRUEN_H = 13;
/** Abstand der Querskala-Striche in Metern. */
const SKALA_SCHRITT_M = 10;
/**
 * X der Skalenachse, links ausserhalb der Bahn.
 *
 * Achtundvierzig statt dreissig: Die äussersten Werte tragen die Einheit
 * („23 m"), und rechtsbündig an der Achse begann dieser Text bei x = −2 —
 * zwei Pixel ausserhalb des Zeichenbereichs. Die Lesbarkeitsprüfung hatte
 * ihn nicht gemeldet, weil sie die Textbreite zu knapp schätzte.
 */
const SKALA_X = 48;

/**
 * Queransicht. Gibt `null` zurück, wenn die Bahnbreite fehlt — eine geratene
 * Breite wäre eine Behauptung über die Kante, an der die Bewertung hängt.
 */
export function RunwayCrossSection(p: QueransichtProps) {
  const { t } = useTranslation();
  const schriftMindest = p.schriftMindest ?? 0;
  const sf = (g: number) => Math.max(g, schriftMindest);
  if (!Number.isFinite(p.runwayWidthM) || p.runwayWidthM <= 0) return null;

  const halbeBahnM = p.runwayWidthM / 2;
  const mitteY = (BAHN_TOP + BAHN_BOT) / 2;

  // ── Fester Massstab statt „jede Bahn füllt die Höhe" ─────────────────
  //
  // Siehe `REFERENZ_BREITE_M`. Die Bahn bekommt so viel Höhe, wie ihr
  // zusteht; schmale Bahnen werden schmal gezeichnet, und das Band der
  // Radspuren ist zwischen zwei Landungen vergleichbar.
  //
  // `dehnung` ist der Faktor, um den eine sehr schmale Bahn dennoch
  // aufgezogen wurde, damit sie lesbar bleibt. Er steht in der Kopfzeile —
  // eine Grafik, die stillschweigend dehnt, behauptet einen Massstab, den
  // sie nicht einhält.
  const bahnHRoh = (p.runwayWidthM / REFERENZ_BREITE_M) * (BAHN_BOT - BAHN_TOP);
  const bahnH = Math.min(BAHN_BOT - BAHN_TOP, Math.max(MIN_BAHN_H, bahnHRoh));
  const dehnung = bahnH / bahnHRoh;
  const bahnTop = mitteY - bahnH / 2;
  const bahnBot = mitteY + bahnH / 2;
  const pxProQuerM = bahnH / 2 / halbeBahnM;
  /** Wie weit über die Kante hinaus gezeichnet wird. */
  const sichtbarM = halbeBahnM + GRUEN_H / pxProQuerM;

  // ── Die Seitenkonvention, an EINER Stelle ────────────────────────────
  //
  // §8.6: „oben = links in Landerichtung", verbindlich für beide Ansichten.
  // Die Längsansicht ist eine Draufsicht mit Landerichtung nach rechts; dort
  // liegt links vom Flugzeug zwangsläufig oben. Intern bleibt `quer > 0 =
  // rechts` — die Umrechnung geschieht hier, einmal, nicht verstreut über die
  // Mapper. Im ersten Entwurf war die Skala mathematisch beschriftet (+ oben),
  // damit lag dieselbe Seite in einer Ansicht oben und in der anderen unten.
  //
  // Begrenzt auf den sichtbaren Streifen: Ohne diese Schranke läuft eine Spur
  // mit unmöglichem Messwert aus dem Bild — der EDDL-Fall lag bei 52,6 m auf
  // einer 45-m-Bahn.
  const querZuY = (quer_m: number) => {
    const begrenzt = Math.max(-sichtbarM, Math.min(sichtbarM, quer_m));
    return mitteY + begrenzt * pxProQuerM;
  };

  const ueberhoehung = pxProQuerM / p.projektion.pxProMeter;
  const halbeSpurM = (p.trackWidthM ?? 0) / 2;
  const punkte = p.samples.filter(
    (s) => Number.isFinite(s.laengs_m) && Number.isFinite(s.quer_m),
  );

  const xy = (s: { laengs_m: number; quer_m: number }) => ({
    x: p.projektion.mToX(s.laengs_m),
    y: querZuY(s.quer_m),
  });
  const achsePunkte = punkte.map((s) => xy(s));

  // ── Das Band der Radspuren — senkrecht zur KURVE, nicht zur X-Achse ──
  //
  // Die Spurweite eines Flugzeugs ist konstant. Trägt man sie als reinen
  // Y-Versatz auf, wirkt das Band trotzdem schmaler, sobald die Spur steil
  // verläuft: Beim Ausschwenken zur Ausfahrt steigt die Kurve über wenige
  // Meter Länge um dutzende Meter Querversatz, und die senkrechte
  // Projektion des Bandes schrumpft entsprechend.
  //
  // Optisch liest sich das als „das Flugzeug wird kleiner" — was es nicht
  // tut. Der Versatz wird deshalb entlang der **Normalen zur Kurve**
  // aufgetragen, mit konstanter Pixelbreite. Das ist näher an der
  // Wirklichkeit als die verzerrte Projektion, denn die Verzerrung stammt
  // allein aus der Überhöhung der Querachse.
  const halbeSpurPx = halbeSpurM * pxProQuerM;
  /**
   * Ein Band konstanter Breite entlang einer Kurve.
   *
   * Aus der Spur-Berechnung herausgezogen, weil der Rollweg dasselbe
   * braucht: Auch er darf beim Ausschwenken nicht schmaler werden, nur
   * weil die Querachse überhöht ist. Zwei Fassungen desselben
   * Gedankengangs würden gegeneinander driften.
   */
  const bandRand = (
    achse: Array<{ x: number; y: number }>,
    halbPx: number,
    vorzeichen: 1 | -1,
  ) =>
    achse.map((q, i) => {
      const vor = achse[Math.max(0, i - 1)]!;
      const nach = achse[Math.min(achse.length - 1, i + 1)]!;
      const dx = nach.x - vor.x;
      const dy = nach.y - vor.y;
      const len = Math.hypot(dx, dy) || 1;
      // Normale = Tangente um 90° gedreht.
      //
      // In x geklemmt: Bei einer steilen Kurve am Bahnende hat die Normale
      // eine nennenswerte x-Komponente und schiebt den Bandrand über die
      // Bahnfläche hinaus. Das ist nicht nur ein Zeichenfehler — dort ist
      // keine Bahn mehr, und die Beschriftung am rechten Rand lag darauf.
      //
      // ⚠ Dieselbe Klemmung, die in der Querrichtung eine falsche Aussage
      // erzeugte (siehe `amRandAbschneiden`). Hier gemessen, statt sie
      // anzunehmen: Über alle Demo-Varianten trifft der Anschlag nur
      // `d_overrun`, dort neun von 138 Bandpunkten am Bahnende. Neun
      // Punkte sind ein kurzer Stummel an einer Stelle, die ohnehin als
      // Überrollen ausgewiesen ist — die Querrichtung hatte NEUNZIG.
      //
      // Deshalb bleibt es hier bei der Klemmung. Sollte sich das ändern,
      // ist der Weg derselbe: in Metern schneiden, vor der Umrechnung.
      return {
        x: Math.max(
          p.projektion.bahnAnfangX,
          Math.min(p.projektion.bahnEndeX, q.x + (vorzeichen * -dy * halbPx) / len),
        ),
        y: q.y + (vorzeichen * dx * halbPx) / len,
      };
    });
  // Das Band folgt derselben Regel wie die Linie: es endet am Bildrand.
  //
  // Ohne den Schnitt lief es mit seinem geklemmten Ende unter die
  // Kantenlinie und lag dort als grünes Rechteck neben den Ausfahrten —
  // an einer Stelle, an der keine Bahn mehr ist. Die Linie allein zu
  // schneiden reicht nicht: Das Band ist das Auffälligere von beiden.
  const spurSichtbar = amRandAbschneiden(punkte, sichtbarM);
  const achseSichtbar = spurSichtbar.map((q) => xy(q));
  const versetzt = (vorzeichen: 1 | -1) =>
    bandRand(achseSichtbar, halbeSpurPx, vorzeichen);
  const linksPunkte = versetzt(-1);
  const rechtsPunkte = versetzt(1);

  const bandPfad =
    spurSichtbar.length >= 2 && halbeSpurM > 0
      ? `${weicherPfad(linksPunkte)} L ${rechtsPunkte
          .slice()
          .reverse()
          .map((q) => `${q.x.toFixed(1)} ${q.y.toFixed(1)}`)
          .join(" L ")} Z`
      : null;

  // ── Gewerteter und ungewerteter Teil der Spur ────────────────────────
  //
  // Ab dem Räumpunkt wird nicht mehr gewertet — die seitliche Lage hängt
  // dort an der Ausfahrt, nicht mehr am Piloten. Aufgezeichnet wird sie
  // trotzdem, sonst endet die Spur mitten auf der Bahn und die Marke „Bahn
  // geräumt" sässe ohne Verbindung daneben.
  //
  // Der Unterschied muss sichtbar sein: durchgezogen bis zum Räumpunkt,
  // abgesetzt danach. Sonst liest sich der ungewertete Teil wie Teil der
  // Bewertung. Ein Punkt Überlappung, damit keine Lücke entsteht.
  // Der Strich wechselt an der KANTE, nicht an der Bewertungsgrenze.
  //
  // Auf der Bahn ist die Spur durchgezogen — sie ist dort gemessen und
  // sichtbar, unabhängig davon, ob der Abschnitt in die Note eingeht. Erst
  // jenseits der Kante wird sie abgesetzt: Dort ist das Flugzeug nicht mehr
  // auf der Bahn, und die Ansicht zeigt nur noch, wohin es gerollt ist.
  const trennIdx =
    p.clearanceM != null
      ? Math.max(1, punkte.findIndex((s) => s.laengs_m >= p.clearanceM!))
      : punkte.length;
  const gewertet = trennIdx >= punkte.length ? achsePunkte : achsePunkte.slice(0, trennIdx + 1);
  const mittelPfad = gewertet.length >= 2 ? weicherPfad(gewertet) : null;
  // Die Spur endet am Bildrand — sie legt sich nicht an ihn.
  //
  // # Der Befund (Thomas, 26.08.2026): „nach der RWY wieder so ein nicht
  // passender grüner Verlauf"
  //
  // `querZuY` begrenzt auf den sichtbaren Streifen. Das verhindert, dass
  // eine Spur aus dem Bild läuft — aber jeder Punkt jenseits der Grenze
  // landet auf DERSELBEN Höhe. Bei DLH369 zog das Flugzeug nach dem
  // Räumen bis 107,8 m nach rechts; die Ansicht zeigt rund 33. Von
  // vierundneunzig Punkten lagen **neunzig** exakt auf der Kantenlinie,
  // gezeichnet als fünfundvierzig Pixel waagerechter Lauf.
  //
  // Das ist keine Ungenauigkeit, sondern eine falsche Aussage: Es liest
  // sich, als wäre das Flugzeug die Bahnkante entlanggerollt. Wohin es
  // wirklich gefahren ist, zeigt die Längsansicht.
  // In METERN abschneiden, dann erst umrechnen — siehe `amRandAbschneiden`.
  const danachRoh =
    trennIdx >= punkte.length ? [] : punkte.slice(trennIdx);
  const danachSichtbar = amRandAbschneiden(danachRoh, sichtbarM).map((s) => xy(s));
  const nachPfad =
    danachSichtbar.length >= 2 ? weicherPfad(danachSichtbar) : null;

  // ── Farbe des Bandes — dieselbe Rangfolge wie die Bewertung ──────────
  //
  // `sub_bahndisziplin` prüft das Überrollen VOR allen seitlichen Regeln
  // und vergibt dafür null Punkte. Die Farbe muss derselben Ordnung folgen,
  // sonst zeigt das Bild grün, während die Note rot ist: Bei Variante ④
  // liegt der seitliche Randabstand bei 17,2 m — vorbildlich — und das
  // Flugzeug ist trotzdem über das Bahnende geschossen.
  //
  // Erst danach zählt die seitliche Lage.
  const rand = p.minEdgeClearanceM;
  const ueberrollt = (p.overrunM ?? 0) > 0;
  const bandFarbe = ueberrollt
    ? p.tokens.tdSevere
    : rand == null
    ? p.tokens.rollout
    : rand < 0
    ? p.tokens.tdSevere
    : rand < 3
    ? p.tokens.tdWarn
    : p.tokens.tdPerfect;

  // ── Marken ───────────────────────────────────────────────────────────
  const marken: Array<{ n: number; x: number; y: number; farbe: string }> = [
    {
      n: 1,
      x: p.projektion.mToX(p.touchdownM),
      y: querZuY(p.touchdownOffsetM),
      farbe: p.tokens.tdPerfect,
    },
  ];
  const maxOff = p.maxLateralOffsetM;
  // Nur im gewerteten Teil suchen: Nach dem Räumpunkt gibt es Querwerte,
  // die betragsmässig grösser sind als der gemeldete Höchstwert — das
  // Abrollen selbst. Ohne diese Grenze findet die Suche dort einen Punkt,
  // und die Marke ② landet auf der Marke ③.
  // Die Marke des grössten Versatzes gehört in den GEMESSENEN Teil.
  //
  // Und der endet frueher als der gewertete: Das Messfenster schliesst
  // unter sechzig Knoten, der Kurswechsel kommt spaeter. Bei DLH369
  // (EDDM 26L) lagen 600 Meter dazwischen — und in diesen 600 Metern
  // laeuft die Spur durch Querwerte, die dem gemeldeten Hoechstwert
  // naeher kommen als der Punkt, an dem er wirklich gemessen wurde.
  // Die Marke wandert dann dorthin und behauptet eine Stelle, an der
  // gar nicht mehr gemessen wurde.
  //
  // Ohne das Feld (Client vor v1.7.4) bleibt es beim Kurswechsel: Das
  // ist die alte, etwas zu weite Grenze — aber besser als die Kante.
  const bewertungsEnde = p.messEndeM ?? p.scoringCutoffM ?? p.clearanceM;
  const gewerteteP = punkte.filter(
    (s) => bewertungsEnde == null || s.laengs_m < bewertungsEnde,
  );
  const maxProbe =
    maxOff != null && gewerteteP.length
      ? gewerteteP.reduce((a, b) =>
          Math.abs(b.quer_m - maxOff) < Math.abs(a.quer_m - maxOff) ? b : a,
        )
      : null;
  if (maxProbe) {
    marken.push({
      n: 2,
      x: p.projektion.mToX(maxProbe.laengs_m),
      y: querZuY(maxProbe.quer_m),
      farbe: p.tokens.tdWarn,
    });
  }
  if (p.clearanceM != null) {
    marken.push({
      n: 3,
      x: p.projektion.mToX(p.clearanceM),
      y: querZuY(p.clearanceSide === "left" ? -halbeBahnM : halbeBahnM),
      farbe: p.tokens.rollout,
    });
  }
  // Ende der Spur — auch ohne Ausfahrt.
  //
  // Eine Spur, die einfach aufhört, lässt den Leser fragen, wo das Flugzeug
  // geblieben ist. Es gibt immer einen Endpunkt: entweder die Ausfahrt
  // (Marke ③) oder die Stelle, an der es auf Rollgeschwindigkeit war und
  // nicht mehr gemessen wurde.
  // Kein zusätzlicher Endpunkt, wenn schon ein Überrollen markiert ist:
  // Die Marke ④ sitzt am Bahnende, und dort endet auch die Spur — zwei
  // Marken übereinander, und die Liste nennt denselben Punkt zweimal.
  if (p.clearanceM == null && (p.overrunM ?? 0) <= 0 && punkte.length >= 2) {
    const letzter = punkte[punkte.length - 1]!;
    const x = p.projektion.mToX(letzter.laengs_m);
    const y = querZuY(letzter.quer_m);
    // Nur, wenn sie nicht mit einer bestehenden Marke zusammenfällt.
    //
    // Bei einer Landung, deren grösster Versatz erst kurz vor dem Ende
    // auftritt, sitzen beide Marken übereinander — zwei Ziffern im selben
    // Kreis, und die Liste darunter nennt denselben Punkt zweimal. Der
    // Endpunkt ist dann keine zusätzliche Aussage.
    const MIN_ABSTAND_PX = 22;
    const belegt = marken.some((m) => Math.hypot(m.x - x, m.y - y) < MIN_ABSTAND_PX);
    if (!belegt) {
      marken.push({ n: marken.length + 1, x, y, farbe: "#94a3b8" });
    }
  }

  // Überrollen: die Marke sitzt AM Bahnende, nicht dahinter — dort endet der
  // Zeichenbereich. Sie muss sein: Die Liste darunter führt den Eintrag ④,
  // und eine Nummer in der Liste ohne Marke im Bild lässt den Leser suchen.
  if (p.overrunM != null && p.overrunM > 0) {
    marken.push({
      n: 4,
      x: p.projektion.bahnEndeX - 10,
      y: mitteY,
      farbe: p.tokens.tdSevere,
    });
  }

  // ── Ausfahrten ───────────────────────────────────────────────────────
  //
  // §8.6: „nur als **Stummel** am Bahnrand, niemals als vollständige
  // Rollwege. Bei 15-facher Überhöhung wäre ein 30°-Schnellabrollweg fast
  // senkrecht gezeichnet — das wäre eine Behauptung, die der Massstab nicht
  // hergibt. Der Stummel markiert die Position, mehr nicht."
  //
  // Sie machen die Bewertung erst nachvollziehbar: Man sieht, welche Ausfahrt
  // vor der genutzten lag und wie weit davor.
  const genutzt = (a: Ausfahrt) =>
    p.clearanceM != null &&
    p.clearanceSide === a.seite &&
    Math.abs(a.laengs_m - p.clearanceM) < 120;
  // Ausfahrten an derselben Stelle werden zu einer Marke zusammengefasst.
  //
  // Bei EDDL treffen K3 und L6 die Bahn beide bei 358 m, K2 und L3 beide bei
  // 2230 m — zwei Beschriftungen an derselben x-Position, die einander
  // ueberdecken. Die Referenzgrafik loest das mit `S5/S6`, und genau das
  // passiert hier: Ein Stummel, ein Name aus beiden.
  // ── Der genutzte Rollweg, als Korridor unter der Spur ────────────────
  //
  // §8.6 verbot Rollwege in dieser Ansicht: „Bei 15-facher Überhöhung wäre
  // ein 30°-Schnellabrollweg fast senkrecht gezeichnet — das wäre eine
  // Behauptung, die der Massstab nicht hergibt."
  //
  // Die Regel war richtig für einen Rollweg ALLEIN. Zusammen mit der Spur
  // beantwortet er eine andere Frage — und die beantwortet er richtig:
  // Beide sind gleich überhöht, also ist ablesbar, OB die Räder im
  // Rollweg blieben. Nur der absolute Winkel bleibt unlesbar, und der
  // steht deshalb als Zahl in der Marke.
  //
  // Feldbefund Thomas zu DLH369 (EDDM 26L, 25.08.2026): „auf B6 abgerollt,
  // aber das Abrollen sieht auf der Darstellung ganz anders aus." Gemessen
  // 19,4°, B6 selbst 23,7°, gezeichnet 80,3°.
  //
  // Gezeichnet wird NUR der genutzte Rollweg. Alle nebeneinander wären ein
  // Liniengewirr, und die Frage lautet „bin ich meinem Rollweg gefolgt",
  // nicht „welche gibt es".
  const rollwegKorridor = ((): string | null => {
    // Die NAECHSTE, nicht die erste.
    //
    // `find` nahm die erste Ausfahrt, die ins Fenster passt — und das
    // Fenster ist 120 Meter breit. In Muenchen liegen darin zwei: B7 bei
    // 2.259 m und B6 bei 2.368 m. Thomas' Raeumpunkt war 2.345 m, also
    // 23 Meter von B6 und 86 von B7 entfernt — gezeichnet worden waere
    // B7, weil sie in der nach Laengs sortierten Liste vorne steht.
    //
    // Das ist genau der Einwand, mit dem diese ganze Arbeit angefangen
    // hat: „auf B6 abgerollt, aber das Abrollen sieht ganz anders aus."
    const genutzteAusfahrt = (p.ausfahrten ?? [])
      .filter((a) => genutzt(a) && (a.verlauf?.length ?? 0) >= 2)
      .sort(
        (x, y) =>
          Math.abs(x.laengs_m - (p.clearanceM ?? 0)) -
          Math.abs(y.laengs_m - (p.clearanceM ?? 0)),
      )[0];
    if (!genutzteAusfahrt?.verlauf) return null;
    const achse = genutzteAusfahrt.verlauf
      .filter(
        (v) =>
          Number.isFinite(v.laengs_m) &&
          Number.isFinite(v.quer_m) &&
          v.laengs_m >= 0 &&
          v.laengs_m <= p.projektion.lengthM &&
          Math.abs(v.quer_m) <= sichtbarM,
      )
      .map((v) => xy({ laengs_m: v.laengs_m, quer_m: v.quer_m }));
    if (achse.length < 2) return null;
    // 23 m: die uebliche Breite eines Schnellabrollwegs. Die Bodenkarte
    // fuehrt Rollwege als Mittellinie, eine echte Breite steht dort nicht.
    const halbRollwegPx = (23 / 2) * pxProQuerM;
    const oben = bandRand(achse, halbRollwegPx, -1);
    const unten = bandRand(achse, halbRollwegPx, 1);
    return `${weicherPfad(oben)} L ${unten
      .slice()
      .reverse()
      .map((q) => `${q.x.toFixed(1)} ${q.y.toFixed(1)}`)
      .join(" L ")} Z`;
  })();

  const ausfahrten = gruppiere(
    (p.ausfahrten ?? []).filter(
      (a) =>
        Number.isFinite(a.laengs_m) &&
        a.laengs_m >= 0 &&
        a.laengs_m <= p.projektion.lengthM,
    ),
    p.projektion,
    // Dieselbe Groesse, mit der die Namen weiter unten gesetzt werden.
    // Steht sie hier anders, fasst die Anzeige nach einem Mass zusammen
    // und zeichnet nach einem anderen.
    sf(9),
  );

  const gruenTop = bahnTop - GRUEN_H;
  const gruenBot = bahnBot;
  const idGruen = "quer-gruen";

  return (
    <svg
      // Nicht `onWheel` — React bindet Rad-Ereignisse passiv, dort ist
      // `preventDefault()` wirkungslos und der Browser zoomt die Seite mit.
      ref={p.zoom?.radAnschluss}
      onMouseDown={p.zoom?.aufZiehStart}
      onMouseMove={p.zoom?.aufZiehen}
      onMouseUp={p.zoom?.aufZiehEnde}
      onMouseLeave={p.zoom?.aufZiehEnde}
      viewBox={`0 0 ${p.width} ${H}`}
      width="100%"
      role="img"
      aria-label={t("runway_v2.cross_section_aria", {
        defaultValue: "Queransicht der Bahn mit dem gefahrenen Streifen",
      })}
      style={{
        display: "block",
        cursor: p.zoom?.zieht ? "grabbing" : p.zoom?.gezoomt ? "grab" : "default",
      }}
    >
      <defs>
        <pattern
          id={idGruen}
          width="7"
          height="7"
          patternTransform="rotate(45)"
          patternUnits="userSpaceOnUse"
        >
          <line x1="0" y1="0" x2="0" y2="7" stroke="#3F6B4A" strokeWidth="3" opacity=".5" />
        </pattern>
      </defs>

      {/* Überschrift und Massstabsangabe — beide ausserhalb der Flächen. */}
      <text x={0} y={14} fontSize={sf(10.5)} letterSpacing={1.4} fill="#8B95A8">
        {t("runway_v2.cross_title", { defaultValue: "QUER — WO DIE RÄDER LIEFEN" })}
      </text>
      {/* Muster und Spurweite — ohne sie ist das Band eine Linie ohne
          Bedeutung. Die Breite ist massstäblich zur Bahn, aber ob 29 Pixel
          nun sieben oder vierzehn Meter sind, sieht man ihr nicht an. */}
      <text x={p.width / 2} y={14} fontSize={sf(10.5)} textAnchor="middle" fill="#9AA5B5">
        {p.trackWidthM != null
          ? t("runway_v2.cross_aircraft", {
              defaultValue: "{{typ}} · Spurweite {{m}} m",
              typ: p.aircraftIcao ?? "",
              m: p.trackWidthM.toFixed(1),
            }).trim()
          : t("runway_v2.cross_aircraft_unknown", {
              defaultValue: "Spurweite nicht bekannt",
            })}
      </text>
      {/* Die Überhöhung quer zur Länge — und, falls sie greift, die
          Dehnung der Bahnbreite selbst.

          Ohne den zweiten Teil behauptet die Grafik einen festen Massstab,
          den sie bei sehr schmalen Bahnen nicht einhält: Eine 9-m-Piste
          waere nach Massstab vierzig Pixel hoch und darin nicht mehr
          lesbar, also wird sie aufgezogen. Das darf sie — aber nicht
          stillschweigend. */}
      <text x={p.width} y={14} fontSize={sf(10)} textAnchor="end" fill="#66707E">
        {dehnung > 1.02
          ? t("runway_v2.cross_exaggeration_stretched", {
              defaultValue:
                "quer {{f}}× überhöht · schmale Bahn {{d}}× aufgezogen",
              f: ueberhoehung.toFixed(1),
              d: dehnung.toFixed(1),
            })
          : t("runway_v2.cross_exaggeration", {
              defaultValue: "quer {{f}}× überhöht · in sich maßstäblich",
              f: ueberhoehung.toFixed(1),
            })}
      </text>

      {/* Ausfahrten: Beschriftung ausserhalb, Stummel am Rand. */}
      {ausfahrten.map((a, i) => {
        const x = p.projektion.mToX(a.laengs_m);
        const oben = a.seite === "left";
        const an = genutzt(a);
        return (
          <g key={`${a.name}-${a.seite}-${i}`}>
            <text
              x={x}
              y={oben ? 34 : H - 6}
              textAnchor="middle"
              fontSize={sf(9)}
              fill={an ? bandFarbe : "#4E6350"}
              fontWeight={an ? 600 : 400}
            >
              {a.name}
            </text>
            <line
              x1={x}
              y1={oben ? gruenTop : gruenBot + GRUEN_H}
              x2={x}
              y2={oben ? gruenTop - 12 : gruenBot + GRUEN_H + 12}
              stroke={an ? bandFarbe : "#4E6350"}
              strokeWidth={an ? 2.6 : 1.6}
            />
          </g>
        );
      })}

      {/* Grünstreifen beidseits — die Fläche neben der Bahn. */}
      <rect
        x={p.projektion.bahnAnfangX}
        y={gruenTop}
        width={p.projektion.bahnEndeX - p.projektion.bahnAnfangX}
        height={GRUEN_H}
        fill={`url(#${idGruen})`}
      />
      <rect
        x={p.projektion.bahnAnfangX}
        y={gruenBot}
        width={p.projektion.bahnEndeX - p.projektion.bahnAnfangX}
        height={GRUEN_H}
        fill={`url(#${idGruen})`}
      />

      {/* Die befestigte Fläche. */}
      <rect
        x={p.projektion.bahnAnfangX}
        y={bahnTop}
        width={p.projektion.bahnEndeX - p.projektion.bahnAnfangX}
        height={bahnH}
        fill={p.tokens.tarmac}
      />
      {/* Zone vor der Landeschwelle — hier darf nicht aufgesetzt werden. */}
      {p.projektion.ddsM > 0 && (
        <>
          <rect
            x={p.projektion.bahnAnfangX}
            y={bahnTop}
            width={p.projektion.thresholdX - p.projektion.bahnAnfangX}
            height={bahnH}
            fill="#2E1614"
          />
          <line
            x1={p.projektion.thresholdX}
            y1={bahnTop}
            x2={p.projektion.thresholdX}
            y2={bahnBot}
            stroke={p.tokens.tdSevere}
            strokeWidth={1.5}
            opacity={0.75}
          />
        </>
      )}

      {/* Kanten betont — das ist die Linie, an der die Note hängt. */}
      {[bahnTop, bahnBot].map((y, i) => (
        <line
          key={i}
          x1={p.projektion.bahnAnfangX}
          y1={y}
          x2={p.projektion.bahnEndeX}
          y2={y}
          stroke="#C9D2E0"
          strokeWidth={1.6}
          opacity={0.8}
        />
      ))}

      {/* Mittellinie. */}
      <line
        x1={p.projektion.thresholdX}
        y1={mitteY}
        x2={p.projektion.bahnEndeX}
        y2={mitteY}
        stroke="#6C7A8F"
        strokeWidth={1}
        strokeDasharray="16 12"
      />

      {/* Der gefahrene Streifen: Fläche zwischen den Rädern, Achse betont. */}
      {/* Kontur um das Band: Ohne sie verläuft die Fläche bei 22 % Deckung
          im Untergrund, und die Spurweite — die eigentliche Aussage dieser
          Ansicht — ist nicht abzulesen. Die Ränder SIND die Radspuren. */}
      {/* Der genutzte Rollweg — UNTER der Spur, damit sie oben liegt.
          Nur eine Fläche mit gestricheltem Rand: Er ist der Untergrund, auf
          dem die Spur läuft, nicht selbst eine Messung. */}
      {rollwegKorridor && (
        <path
          d={rollwegKorridor}
          fill={p.tokens.rollweg}
          fillOpacity={0.22}
          stroke={p.tokens.rollwegRand}
          strokeWidth={1.1}
          strokeOpacity={0.75}
          strokeDasharray="6 5"
          strokeLinejoin="round"
          pointerEvents="none"
        />
      )}
      {bandPfad && (
        <path
          d={bandPfad}
          fill={bandFarbe}
          fillOpacity={0.22}
          stroke={bandFarbe}
          strokeWidth={1}
          strokeOpacity={0.65}
          strokeLinejoin="round"
        />
      )}
      {mittelPfad && (
        <path
          d={mittelPfad}
          fill="none"
          stroke={bandFarbe}
          strokeWidth={2.4}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      )}
      {nachPfad && (
        <path
          d={nachPfad}
          fill="none"
          stroke={bandFarbe}
          strokeWidth={2}
          strokeDasharray="5 4"
          opacity={0.55}
          strokeLinecap="round"
        />
      )}
      {/* Die Messpunkte selbst — sonst ist nicht zu sehen, worauf die Kurve
          beruht. Ein geglätteter Verlauf ohne sichtbare Stützstellen sieht
          aus wie ein Modell; er ist aber eine Messung. */}
      {punkte
        // Punkte ausserhalb des Streifens liegen alle auf derselben
        // Höhe — eine Perlenkette auf der Kantenlinie, die eine Fahrt
        // entlang der Kante behauptet. Sie gehören nicht ins Bild.
        .filter((s) => Math.abs(s.quer_m) <= sichtbarM)
        .map((s, i) => (
          <circle
            key={i}
            cx={p.projektion.mToX(s.laengs_m)}
            cy={querZuY(s.quer_m)}
            r={1.8}
            fill={bandFarbe}
            fillOpacity={0.9}
          />
        ))}

      {/* Der gezeichnete Ausfahrt-Bogen ist entfallen.

          Er war ein Behelf aus der Zeit, als die Aufzeichnung an der
          Bahnkante endete: Die Spur brach dort ab, und ein gestrichelter
          Bogen deutete an, wohin es weiterging. Seit die Spur bis zum
          Übergang in den Rollweg durchläuft, zeigt sie das selbst — und
          zwar gemessen statt gezeichnet.

          Zwei Linien für dieselbe Aussage sind eine zu viel, und die
          erfundene wäre die auffälligere gewesen. */}

      {/* Marken — nur Ziffern, der Text steht in der Liste darunter (§8.5). */}
      {marken.map((m) => (
        <g key={m.n}>
          <circle cx={m.x} cy={m.y} r={9} fill={m.farbe} />
          <text
            x={m.x}
            y={m.y + 3.8}
            textAnchor="middle"
            fontSize={sf(11)}
            fontWeight={600}
            fill="#0B0F17"
          >
            {m.n}
          </text>
        </g>
      ))}

      {/* Skalenachse links, ausserhalb der Bahn. */}
      <g stroke="#4A5769" strokeWidth={1}>
        <line x1={SKALA_X} y1={bahnTop} x2={SKALA_X} y2={bahnBot} />
        {skalaWerte(halbeBahnM).map((m) => (
          <line
            key={m}
            x1={SKALA_X - (m === 0 || Math.abs(m) >= halbeBahnM ? 5 : 3)}
            y1={querZuY(m)}
            x2={SKALA_X + (m === 0 || Math.abs(m) >= halbeBahnM ? 5 : 3)}
            y2={querZuY(m)}
          />
        ))}
      </g>
      <g fill="#7C8698" fontSize={sf(9)} textAnchor="end">
        {skalaWerte(halbeBahnM).map((m) => (
          <text
            key={m}
            x={SKALA_X - 10}
            y={querZuY(m) + 3}
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {Math.abs(m) >= halbeBahnM ? `${Math.abs(m).toFixed(0)} m` : Math.abs(m)}
          </text>
        ))}
      </g>
      {/* Linksbuendig am Bildrand, nicht rechtsbuendig an der Skalenachse:
          „RECHTS" ist breiter als der Platz links davon, und rechtsbuendig
          begann das Wort bei x = -11 -- ausserhalb des Zeichenbereichs
          (§8.6.2). Aufgefallen erst in der Kollisionspruefung. */}
      <text
        x={0}
        y={gruenTop - 4}
        fontSize={sf(9.5)}
        fontWeight={600}
        fill="#9AA5B5"
      >
        {t("runway_v2.side_left", { defaultValue: "LINKS" })}
      </text>
      <text
        x={0}
        y={gruenBot + GRUEN_H + 11}
        fontSize={sf(9.5)}
        fontWeight={600}
        fill="#9AA5B5"
      >
        {t("runway_v2.side_right", { defaultValue: "RECHTS" })}
      </text>

      {/* Eine Bemassung der Spurbreite im Bild wäre naheliegend — und
          verletzt §8.6.3: „Keine Beschriftung auf der Bahnfläche."
          Der Doppelpfeil sass zwangsläufig dort, wo die Spur ist, also
          mitten auf der Bahn. Die Lesbarkeitsprüfung hat ihn gemeldet.

          Die Breite ist stattdessen auf drei Wegen ablesbar: Das Band hat
          eine Kontur, der Kopf nennt Muster und Spurweite, und der
          Grössenvergleich unter der Grafik stellt sie neben Bahnbreite und
          Spannweite. */}

      {/* Kanten und Mitte rechts benannt — sonst muss man die Skala lesen,
          um zu wissen, welche Linie die Kante ist.

          Kurz gehalten („Kante", nicht „Kante links"): Rechts bleiben nach
          der Bahnfläche siebzig Einheiten, und die Seite steht ohnehin
          links an der Skala. „Kante links" lief bei sechzehn Einheiten
          Abstand über den Rand hinaus. */}
      <text x={p.projektion.bahnEndeX + 10} y={bahnTop + 4} fontSize={sf(9.5)} fill="#8B95A8">
        {t("runway_v2.edge_left", { defaultValue: "Kante links" })}
      </text>
      <text x={p.projektion.bahnEndeX + 10} y={mitteY + 4} fontSize={sf(9.5)} fill="#66707E">
        {t("runway_v2.centre", { defaultValue: "Mitte" })}
      </text>
      <text x={p.projektion.bahnEndeX + 10} y={bahnBot + 4} fontSize={sf(9.5)} fill="#8B95A8">
        {t("runway_v2.edge_right", { defaultValue: "Kante rechts" })}
      </text>
    </svg>
  );
}

/**
 * Weicher Pfad durch eine Punktfolge (Catmull-Rom, in Bézier übersetzt).
 *
 * # Warum geglättet und nicht Punkt-zu-Punkt
 *
 * Die Messpunkte liegen zehn bis dreissig Meter auseinander. Verbindet man
 * sie mit Geraden, entsteht ein Polygonzug mit sichtbaren Knicken — und der
 * liest sich wie eine Folge abrupter Lenkbewegungen, die es nie gab. Ein
 * Flugzeug auf der Bahn beschreibt Kurven, keine Ecken.
 *
 * # Warum das keine erfundenen Daten sind
 *
 * Die Kurve läuft **exakt durch jeden gemessenen Punkt** — das ist die
 * Eigenschaft von Catmull-Rom, die sie hier qualifiziert. Zwischen den
 * Punkten interpoliert sie, aber sie verschiebt keinen. Eine Glättung, die
 * Messpunkte verfehlt (etwa ein gleitender Mittelwert), wäre eine Aussage
 * über Werte, die so nicht gemessen wurden.
 */
export function weicherPfad(
  punkte: Array<{ x: number; y: number }>,
  spannung = 0.5,
): string {
  if (punkte.length === 0) return "";
  if (punkte.length === 1) return `M ${punkte[0]!.x} ${punkte[0]!.y}`;
  if (punkte.length === 2) {
    return `M ${punkte[0]!.x} ${punkte[0]!.y} L ${punkte[1]!.x} ${punkte[1]!.y}`;
  }
  const teile: string[] = [`M ${r(punkte[0]!.x)} ${r(punkte[0]!.y)}`];
  for (let i = 0; i < punkte.length - 1; i++) {
    const p0 = punkte[Math.max(0, i - 1)]!;
    const p1 = punkte[i]!;
    const p2 = punkte[i + 1]!;
    const p3 = punkte[Math.min(punkte.length - 1, i + 2)]!;
    const c1x = p1.x + ((p2.x - p0.x) * spannung) / 6;
    const c1y = p1.y + ((p2.y - p0.y) * spannung) / 6;
    const c2x = p2.x - ((p3.x - p1.x) * spannung) / 6;
    const c2y = p2.y - ((p3.y - p1.y) * spannung) / 6;
    teile.push(`C ${r(c1x)} ${r(c1y)}, ${r(c2x)} ${r(c2y)}, ${r(p2.x)} ${r(p2.y)}`);
  }
  return teile.join(" ");
}

function r(n: number): string {
  return n.toFixed(1);
}

/**
 * Wie weit zwei Ausfahrtsnamen auseinanderstehen muessen.
 *
 * # Warum das keine feste Zahl mehr ist
 *
 * Vorher standen hier achtzehn Pixel — die Breite eines zweistelligen
 * Namens. Das stimmt fuer „B6" und stimmt nicht mehr, sobald das
 * Zusammenfassen selbst den Namen verbreitert: „B2/B1" ist fuenf Zeichen
 * und damit knapp achtundzwanzig Pixel breit. In Muenchen (EDDM 26L, 14
 * Ausfahrten) lag „B3" achtzehn Pixel neben „B2/B1" — nach der alten
 * Regel weit genug, auf dem Bild uebereinander.
 *
 * Zwei mittig gesetzte Beschriftungen beruehren sich, wenn ihr
 * Mittenabstand unter die halbe Summe ihrer Breiten faellt. Genau das
 * rechnet diese Funktion, plus etwas Luft.
 *
 * Der Faktor 0,62 ist dieselbe Annahme, mit der die Lesbarkeitspruefung
 * (`RunwayLesbarkeit.test.tsx`) die Textkaesten aufspannt — beide muessen
 * dieselbe Breite meinen, sonst prueft der Test etwas anderes, als die
 * Anzeige baut.
 */
const ZEICHENBREITE = 0.62;
const LUFT_PX = 3;

function mindestAbstandPx(links: string, rechts: string, schrift: number): number {
  const halb = (n: string) => (n.length * schrift * ZEICHENBREITE) / 2;
  return halb(links) + halb(rechts) + LUFT_PX;
}

/**
 * Fasst Ausfahrten zusammen, deren Stummel sich sonst ueberdecken.
 *
 * Massgeblich ist der **Pixelabstand**, nicht der Meterabstand: Auf einer
 * kurzen Bahn liegen dieselben zwanzig Meter viel weiter auseinander als auf
 * einer langen. Wie viele Pixel noetig sind, haengt am Namen selbst —
 * siehe `mindestAbstandPx`.
 */
function gruppiere(
  liste: Ausfahrt[],
  proj: Projektion,
  schrift: number,
): Ausfahrt[] {
  // Intern wird GEZAEHLT, nicht an Zeichenketten herumgeschnitten.
  //
  // # Warum das der Umbau wert war
  //
  // Die erste Fassung baute den Namen bei jedem Verbinden neu aus dem
  // vorherigen Text zusammen und las das „+N" wieder heraus. Das geht
  // gut, solange der Text von dieser Funktion stammt — ein Rollweg, der
  // in OSM selbst „A+1" heisst, wurde aber als „A und ein weiterer"
  // gelesen. Aus „A+1", „A/B" und „C" wurde „A/B +2": vier behauptete
  // Ausfahrten, drei tatsaechliche.
  //
  // Der Name wird deshalb erst ganz am Schluss gesetzt.
  type Haufen = { a: Ausfahrt; namen: string[]; label: string };
  const machen = (a: Ausfahrt): Haufen => ({
    a,
    namen: a.name ? [a.name] : [],
    label: a.name,
  });
  // Der angezeigte Text — und damit der Platzbedarf — haengt am Stand
  // des Haufens. Beides muss zusammen fortgeschrieben werden, sonst
  // misst die Kollisionspruefung eine Beschriftung, die es nicht gibt.
  const setzen = (h: Haufen) => {
    h.label = beschriftung(h.namen);
  };
  const legen = (ziel: Haufen, quelle: Haufen) => {
    ziel.namen.push(...quelle.namen);
    setzen(ziel);
  };
  const kollidiert = (x: Haufen, y: Haufen) =>
    x.a.seite === y.a.seite &&
    Math.abs(proj.mToX(x.a.laengs_m) - proj.mToX(y.a.laengs_m)) <
      mindestAbstandPx(x.label, y.label, schrift);

  const haufen: Haufen[] = [];
  for (const a of [...liste].sort((x, y) => x.laengs_m - y.laengs_m)) {
    const h = machen(a);
    const nachbar = haufen.find((b) => kollidiert(b, h));
    if (nachbar) legen(nachbar, h);
    else haufen.push(h);
  }

  // Nachziehen, bis nichts mehr kollidiert.
  //
  // # Warum ein Durchgang nicht reicht
  //
  // Der erste Durchgang prueft jede Ausfahrt gegen den Stand, den die
  // Liste in DIESEM Moment hat. Verbindet er spaeter zwei Namen, wird die
  // Beschriftung breiter — und kann etwas ueberdecken, das vorher weit
  // genug entfernt lag und schon abgelegt war.
  //
  // Genau so entstand der Befund in Muenchen (EDDM 26L): B3 stand fest,
  // dann wuchs der Nachbar von „B2" auf „B2/B1" und schob sich darunter.
  // Niemand hat B3 je wieder angesehen.
  //
  // Die Schranke ist kein Sicherheitsnetz, sondern eine Aussage: jede
  // Runde legt zwei Haufen zusammen, also kann es nie mehr Runden geben
  // als Haufen.
  for (let runde = 0; runde < haufen.length; runde++) {
    let i = -1;
    let j = -1;
    for (let k = 0; k < haufen.length && i < 0; k++) {
      for (let l = k + 1; l < haufen.length; l++) {
        if (kollidiert(haufen[k]!, haufen[l]!)) {
          i = k;
          j = l;
          break;
        }
      }
    }
    if (i < 0) break;
    legen(haufen[i]!, haufen[j]!);
    haufen.splice(j, 1);
  }

  return haufen.map((h) => ({ ...h.a, name: h.label }));
}

/**
 * Aus den Namen eines Haufens eine Beschriftung machen.
 *
 * Hoechstens zwei Kennungen ausschreiben; was darueber hinausgeht, wird
 * GEZAEHLT, nicht verschwiegen. Gemessen ueber alle 660 Bahnen mit
 * Bodenkarte (23.08.2026) trifft das 38 Ausfahrten — in Frankfurt und
 * Koeln liegen drei Rollwege innerhalb weniger Meter an der Bahn. Vorher
 * stand dort „R11/M19" und R13 fehlte, ohne dass die Grafik es
 * andeutete: Wer nach der Ausfahrt sucht, die er genommen hat, findet
 * sie nicht und haelt die Karte fuer unvollstaendig.
 *
 * Gleiche Namen zaehlen einmal. In OSM ist ein Rollweg oft in mehrere
 * Stuecke geteilt, die alle „B6" heissen — daraus „B6 +4" zu machen
 * waere schlicht falsch.
 *
 * # Bekannte Grenze
 *
 * Traegt ein Rollweg den Schraegstrich im eigenen Namen, wird die
 * Beschriftung mehrdeutig: „X/Y" und „Z" ergeben „X/Y/Z", was sich wie
 * drei Kennungen liest. Die ZAHL stimmt trotzdem — es sind zwei Namen —,
 * und jede Kennung ist zu sehen. Ein anderes Trennzeichen wuerde das
 * aufloesen, aber das Bild ist so abgenommen; gemessen ueber alle 199
 * Flughaefen mit Bodenkarte kommt der Fall selten vor. Bewusst so
 * gelassen, nicht uebersehen.
 */
function beschriftung(namen: string[]): string {
  const eindeutig = [...new Set(namen)];
  const rest = Math.max(0, eindeutig.length - 2);
  const kopf = eindeutig.slice(0, 2).join("/");
  return rest > 0 ? `${kopf} +${rest}` : kopf;
}

/**
 * Eine Spur dort enden lassen, wo sie das Bild verlässt.
 *
 * Gerechnet wird in METERN, vor der Umrechnung in Bildkoordinaten. Das
 * ist der Punkt: `querZuY` begrenzt bereits auf den sichtbaren Streifen,
 * und danach liegt jeder Punkt jenseits der Grenze scheinbar innerhalb —
 * ein Schnitt auf den umgerechneten Werten schneidet nichts ab. Genau so
 * ist mein erster Anlauf ins Leere gelaufen, mit unverändertem Bild.
 *
 * Behalten wird, was innerhalb liegt; beim Austritt kommt ein Punkt
 * genau auf der Grenze dazu, damit die Linie den Rand berührt statt
 * davor aufzuhören. Danach ist Schluss — auch wenn die Spur später
 * zurückkäme: Eine Linie, die über die Lücke hinweg weiterläuft,
 * verbindet zwei Stellen, zwischen denen nichts gezeichnet ist.
 */
function amRandAbschneiden(
  spur: Array<{ laengs_m: number; quer_m: number }>,
  sichtbarM: number,
): Array<{ laengs_m: number; quer_m: number }> {
  const aus: Array<{ laengs_m: number; quer_m: number }> = [];
  for (let i = 0; i < spur.length; i++) {
    const q = spur[i]!;
    if (Math.abs(q.quer_m) <= sichtbarM) {
      aus.push(q);
      continue;
    }
    const vorher = spur[i - 1];
    if (vorher && Math.abs(vorher.quer_m) <= sichtbarM) {
      const grenze = q.quer_m > 0 ? sichtbarM : -sichtbarM;
      const t = (grenze - vorher.quer_m) / (q.quer_m - vorher.quer_m);
      aus.push({
        laengs_m: vorher.laengs_m + t * (q.laengs_m - vorher.laengs_m),
        quer_m: grenze,
      });
    }
    break;
  }
  return aus;
}

/**
 * Die Werte der Querskala, symmetrisch um die Mittellinie.
 *
 * Der äusserste Wert ist immer die Kante selbst — sie ist die Linie, an der
 * die Bewertung hängt, und muss deshalb beziffert sein.
 */
function skalaWerte(halbeBahnM: number): number[] {
  const werte: number[] = [0];
  // Mindestabstand zur Kante: Bei einer 46-m-Bahn liegt die Kante bei 23 m,
  // und der Zehnerschritt 20 stand nur drei Meter davor -- elf Pixel, in
  // denen zwei Beschriftungen uebereinanderlagen. Ein halber Schritt ist die
  // Untergrenze, unter der zwei Werte nicht mehr getrennt lesbar sind.
  const frei = SKALA_SCHRITT_M / 2;
  for (let m = SKALA_SCHRITT_M; m < halbeBahnM - frei; m += SKALA_SCHRITT_M) {
    werte.push(-m, m);
  }
  werte.push(-halbeBahnM, halbeBahnM);
  return werte;
}
