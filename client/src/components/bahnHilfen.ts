// Gemeinsame Hilfen der Bahnansicht (Ausfahrten, Bandfarbe, Glättung,
// Beschriftungsgruppen). Bis 22.10.2026 lagen sie in `RunwayCrossSection`;
// die Queransicht ist mit Variante B entfallen, die Hilfen nutzen
// Draufsicht (`BahnSpurOverlay`), Panel und Lupe weiter — unverändert.

import type { Projektion } from "../lib/runwayProjection";

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

/** Farbe früherer Durchgänge — deutlich getrennt von Grün/Gelb/Rot/Cyan der Wertung. */
export const DURCHGANG_FARBE = "#a78bfa";

/**
 * Farbe des Radspur-Bandes. Eine Stelle für Queransicht und Lupe — zwei
 * Fassungen derselben Rangfolge würden auseinanderlaufen.
 */
export function bandFarbeFuer(
  tokens: { rollout: string; tdPerfect: string; tdWarn: string; tdSevere: string },
  overrunM: number | null | undefined,
  minEdgeClearanceM: number | null | undefined,
): string {
  if ((overrunM ?? 0) > 0) return tokens.tdSevere;
  if (minEdgeClearanceM == null) return tokens.rollout;
  if (minEdgeClearanceM < 0) return tokens.tdSevere;
  if (minEdgeClearanceM < 3) return tokens.tdWarn;
  return tokens.tdPerfect;
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
  // Ohne führendes "M": für den Anschluss an einen bereits offenen Pfad
  // (siehe `bandRand`-Nutzung unten) — sonst würde ein zweites "M" den
  // Pfad in zwei getrennte Teilstücke zerreissen.
  ohneAnfangM = false,
): string {
  if (punkte.length === 0) return "";
  if (punkte.length === 1) {
    return ohneAnfangM ? "" : `M ${r(punkte[0]!.x)} ${r(punkte[0]!.y)}`;
  }
  if (punkte.length === 2) {
    const anfang = ohneAnfangM ? "L" : `M ${r(punkte[0]!.x)} ${r(punkte[0]!.y)} L`;
    return `${anfang} ${r(punkte[1]!.x)} ${r(punkte[1]!.y)}`;
  }
  const teile: string[] = ohneAnfangM ? [] : [`M ${r(punkte[0]!.x)} ${r(punkte[0]!.y)}`];
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
export function gruppiere(
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

// ── Eine Quelle für den Ausfahrtnamen (Bahnansicht 2.1.8, #1936) ─────────
//
// Bis 2.1.7 suchten Kopf („BAHN GERÄUMT … Ausfahrt B“), Draufsicht
// (hervorgehobener Stummel „B/S1“) und Lupe („B“) die genutzte Ausfahrt
// je selbst — drei Suchen, zwei Antworten. Seit 2.1.8 bestimmt der Client
// sie einmal (`clearance_exit`); für ältere Flüge rechnet DIESE Funktion
// einmal nach derselben Regel wie der Client (`ausfahrten.rs`).

/** Wie weit zwei Ausfahrten derselben Seite auseinander liegen dürfen und
 *  noch dieselbe Abzweigung sind (wie `DIESELBE_ABZWEIGUNG_M` im Client):
 *  derselbe Stützpunkt. Getrennte Rollwege wenige Meter auseinander
 *  (Frankfurt R11/M19/R13) bleiben getrennt — sie werden gezählt. */
const DIESELBE_ABZWEIGUNG_M = 0.5;

/**
 * Doppelnamen an derselben Abzweigung → EIN Name (Thomas, 22.10.2026):
 * `bevorzugt` (der Name, den der Client gemeldet hat), sonst der kürzere,
 * bei Gleichstand der alphabetisch erste. Der Client bevorzugt zusätzlich
 * den Namen der Bodenkarte; die liegt der Anzeige nicht vor.
 */
export function doppelnamenZusammenfassen<A extends Ausfahrt>(liste: A[], bevorzugt?: string | null): A[] {
  const rang = (a: A): [number, number, string] => [a.name === bevorzugt ? 0 : 1, [...a.name].length, a.name];
  const kleiner = (x: [number, number, string], y: [number, number, string]) =>
    x[0] !== y[0] ? x[0] < y[0] : x[1] !== y[1] ? x[1] < y[1] : x[2] < y[2];
  const aus: A[] = [];
  for (const a of liste) {
    const i = aus.findIndex((b) => b.seite === a.seite && Math.abs(b.laengs_m - a.laengs_m) <= DIESELBE_ABZWEIGUNG_M);
    if (i < 0) aus.push(a);
    else if (kleiner(rang(a), rang(aus[i]!))) aus[i] = a;
  }
  return aus;
}

export interface AusfahrtQuelle {
  runway_exits?: Ausfahrt[] | null;
  clearance_exit?: string | null;
  clearance_point_m?: number | null;
  clearance_side?: "left" | "right" | null;
}

/** Die Ausfahrten, wie alle Ansichten sie zeigen: ein Name je Abzweigung. */
export function ausfahrtenFuerAnzeige(p: AusfahrtQuelle): Ausfahrt[] {
  return doppelnamenZusammenfassen(p.runway_exits ?? [], p.clearance_exit);
}

/**
 * Die genutzte Ausfahrt — für Kopf, Draufsicht, Lupe, Pfeil und PDF.
 *
 * Mit `clearance_exit` (ab 2.1.8): genau dieser Name, an seinem Stummel
 * nächst dem Räumpunkt. Ohne (älter): auf der Räumseite, höchstens 120 m
 * vom Räumpunkt, die nächste — wie der Client (DLH369 EDDM: B6, nicht B7).
 * Ohne Räumseite kein Name: Eine geratene Seite wäre schlimmer als keine.
 */
export function genutzteAusfahrt(p: AusfahrtQuelle): Ausfahrt | null {
  const raeum = p.clearance_point_m;
  if (raeum == null || p.clearance_side == null) return null;
  const kandidaten = ausfahrtenFuerAnzeige(p)
    .filter((a) => a.seite === p.clearance_side && Math.abs(a.laengs_m - raeum) < 120)
    .sort((a, b) => Math.abs(a.laengs_m - raeum) - Math.abs(b.laengs_m - raeum));
  // Ein gemeldeter Name, der hier nicht (mehr) liegt — etwa nach einer
  // späteren Bahnkorrektur —, wird nicht erfunden: dann die Regel.
  return kandidaten.find((a) => a.name === p.clearance_exit) ?? kandidaten[0] ?? null;
}

/**
 * Woher die Rollwege stammen — ab 2.1.8 meldet es der Client
 * (`runway_exits_quelle`). Davor ist es nicht gespeichert: `null`, die
 * Legende sagt „Quelle nicht gespeichert“ statt eine zu raten (bis 2.1.7
 * stand pauschal „OSM“, bei #1936 falsch). Nicht aus der Bahn-Herkunft
 * geschlossen: Die kennt nur der Payload der Webapp, nicht der Datensatz
 * des Clients — beide Seiten zeigten sonst Verschiedenes.
 */
export function rollwegQuelle(p: { runway_exits_quelle?: string | null }): "szenerie" | "osm" | null {
  return p.runway_exits_quelle === "szenerie" || p.runway_exits_quelle === "osm" ? p.runway_exits_quelle : null;
}
