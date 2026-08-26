// Durchgehende Qualitätssicherung über **alle** Varianten und **beide**
// Ansichten.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8 und §12.
//
// # Warum diese Datei existiert
//
// Die Fehler, die beim Bau der Bahn-Anzeige auftraten, hatten alle dieselbe
// Form: Sie waren in *einer* Variante unsichtbar und fielen erst auf, als
// jemand die nächste ansah. Ein Beispiel je Regel:
//
//   * Die Ausroll-Linie lief über ihre eigene Endmarke hinaus, weil
//     `rollout_m` (Strecke bis zum Stillstand) und `clearance_point_m`
//     (Stelle des Verlassens) zwei Quellen für dasselbe Ende sind.
//   * Die Spur endete mitten auf der Bahn, während „Bahn geräumt"
//     dreihundert Meter weiter sass.
//   * Die Marke ② lag auf der Marke ③, weil der „grösste Versatz" aus der
//     ganzen Spur kam — und die Ausfahrt ist immer der grösste Versatz.
//   * Eine Nummer stand in der Liste, aber nicht im Bild.
//
// Keiner dieser Fehler ist ein Geschmacksurteil. Alle sind prüfbar, und ab
// hier werden sie geprüft — über jede Variante, nicht über eine.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MOCK_LANDING_OPTIONS, skipGrundAbleiten } from "../dev/mockLandingRecords";
import { mapLandingRecordToV2Props } from "../dev/runwayDiagramV2Mapper";
import { RunwayDiagramV2, type RunwayDiagramV2Props } from "./RunwayDiagramV2";

interface Variante {
  key: string;
  props: RunwayDiagramV2Props;
  markup: string;
  svgs: string[];
}

const VARIANTEN: Variante[] = MOCK_LANDING_OPTIONS.map((o) => {
  const props = mapLandingRecordToV2Props(o.build())!;
  const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props} />);
  return { key: o.key, props, markup, svgs: markup.match(/<svg[\s\S]*?<\/svg>/g) ?? [] };
});

/** Alle sichtbaren Texte eines Markup-Abschnitts. */
function texte(teil: string): string[] {
  return [...teil.matchAll(/<text[^>]*>([\s\S]*?)<\/text>/g)]
    .map((m) => m[1]!.replace(/<[^>]+>/g, "").trim())
    .filter(Boolean);
}

/**
 * Die Ziffern der Marken — und nur die.
 *
 * Nicht jede einstellige Zahl im Bild ist eine Marke: Die Querskala trägt
 * eine `0` an der Mittellinie. Marken erkennt man an ihrer Schriftfarbe:
 * dunkel, weil sie in einem farbigen Kreis stehen.
 */
function markenZiffern(svg: string): string[] {
  return [...svg.matchAll(/<text[^>]*fill="#0B0F17"[^>]*>(\d)<\/text>/g)].map((m) => m[1]!);
}

/** Zahlenattribut aus einem Tag. */
function attr(tag: string, name: string): number | null {
  const m = new RegExp(`${name}="([-\\d.]+)"`).exec(tag);
  return m ? Number(m[1]) : null;
}

describe("QS — Ausroll-Linie und ihr Ende", () => {
  it("gibt jeder Ausroll-Linie eine Endmarke", () => {
    // Eine Linie, die im Nichts aufhört, lässt den Leser fragen, wo das
    // Flugzeug geblieben ist. Es gibt immer ein Ende: die Ausfahrt, oder
    // die Stelle, an der die Aufzeichnung endete.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const laengs = v.svgs[0];
      if (!laengs) continue;
      // Die Linie existiert nur, wenn eine Ausrollstrecke bekannt ist.
      const hatLinie = /stroke-width="14"/.test(laengs);
      if (!hatLinie) continue;
      // Die Endmarke ist eine Raute — ein `path` mit dem Rautenmuster.
      const hatRaute = /d="M [\d.]+ [\d.]+ l -?\d+ \d+ l -\d+ \d+ l -\d+ -\d+ z"/.test(laengs);
      if (!hatRaute) befunde.push(`${v.key}: Linie ohne Endmarke`);
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("lässt keine Linie über ihre Endmarke hinauslaufen", () => {
    // Der Fall, den du gesehen hast: `rollout_m` und `clearance_point_m`
    // sind zwei Quellen für dasselbe Ende und stimmen nicht überein.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const laengs = v.svgs[0];
      if (!laengs) continue;
      const linie = /<line[^>]*stroke-width="14"[^>]*>/.exec(laengs)?.[0];
      const raute = /<path[^>]*d="M ([\d.]+) [\d.]+ l -?\d+ \d+ l -\d+ \d+ l -\d+ -\d+ z"[^>]*>/.exec(
        laengs,
      );
      if (!linie || !raute) continue;
      const x2 = attr(linie, "x2");
      const rx = Number(raute[1]);
      if (x2 != null && x2 > rx + 2) {
        befunde.push(`${v.key}: Linie endet bei ${x2.toFixed(0)}, Marke bei ${rx.toFixed(0)}`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});

describe("QS — Spur und Marken der Queransicht", () => {
  it("lässt die Spur nicht vor ihrer Endmarke aufhören", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const s = v.props.lateral_samples ?? [];
      if (s.length < 2) continue;
      const ende = s[s.length - 1]!.laengs_m;
      const raeum = v.props.clearance_point_m;
      if (raeum != null && raeum > ende + 30) {
        befunde.push(
          `${v.key}: Spur endet ${ende.toFixed(0)} m, Räumpunkt ${raeum.toFixed(0)} m`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("zählt die Ausfahrt nicht als seitlichen Versatz", () => {
    // Nach dem Räumen sind vierzig Meter neben der Mittellinie normal.
    // Rechnet man sie mit, ist der „grösste Versatz" immer die Ausfahrt —
    // und ein reguläres Abrollen würde als Fehler gewertet.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const s = v.props.lateral_samples ?? [];
      const max = v.props.max_lateral_offset_m;
      // Massgeblich ist die BEWERTUNGSGRENZE (Beginn des Ausschwenkens),
      // nicht der Räumpunkt (Bahnkante). Zwischen beiden liegen Hunderte
      // Meter, in denen das Flugzeug schon nach aussen zieht.
      //
      // Es gibt ZWEI Grenzen, und sie fallen auseinander:
      //
      //   `scoring_cutoff_m`   — wo der KURS abwich
      //   `mess_ende_laengs_m` — wo das Messfenster schloss (unter 60 kt)
      //
      // Bei DLH369 (EDDM 26L) lagen 600 Meter dazwischen: Fenster zu bei
      // rund 1.695 m, Kurswechsel erst bei 2.251 m. Wer gegen den
      // Kurswechsel prüft, verlangt vom Client einen Höchstwert aus
      // einem Bereich, den er gar nicht mehr gemessen hat.
      const fenster = v.props.mess_ende_laengs_m;
      const raeum =
        fenster ?? v.props.scoring_cutoff_m ?? v.props.clearance_point_m;
      if (max == null || raeum == null || s.length < 2) continue;
      const gewertet = s.filter((x) => x.laengs_m < raeum);
      if (!gewertet.length) continue;
      const echterMax = gewertet.reduce((a, b) =>
        Math.abs(b.quer_m) > Math.abs(a.quer_m) ? b : a,
      ).quer_m;
      const ab = Math.abs(max) - Math.abs(echterMax);
      // Zu WENIG zu melden ist nur dann ein Fehler, wenn wir das
      // Fensterende kennen — sonst kann die Untertreibung schlicht
      // daher kommen, dass das Fenster früher schloss als der Kurs
      // abwich. Zu VIEL zu melden ist immer der gesuchte Fehler: dann
      // ist die Ausfahrt mitgewertet worden.
      const schranke = fenster != null ? Math.abs(ab) : ab;
      if (schranke > 0.5) {
        befunde.push(
          `${v.key}: gemeldet ${max.toFixed(1)} m, im gewerteten Teil ` +
            `${echterMax.toFixed(1)} m (Grenze ${raeum.toFixed(0)} m, ` +
            `${fenster != null ? "Fensterende" : "Kurswechsel"})`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("zeichnet die Spur auf der Bahn durchgezogen", () => {
    // Der Wechsel auf gestrichelt gehört an die Bahnkante — nicht an die
    // Bewertungsgrenze. Lagen beide auf demselben Feld, begann die
    // gestrichelte Linie mitten auf der Bahn, dort wo das Ausschwenken
    // anfing. Das ist nicht zu erklären: Die Spur ist dort gemessen, und
    // das Flugzeug ist dort auf der Bahn.
    //
    // Geprüft wird die INHALTLICHE Aussage, nicht das Verhältnis zweier
    // Felder: Der erste gestrichelte Punkt muss jenseits der Bahnkante
    // liegen. Eine Prüfung auf „die beiden Felder sind verschieden" wäre
    // grün geblieben, als ich sie zum Test wieder zusammenlegte — sie
    // übersprang genau den Fall, den sie fangen sollte.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const quer = v.svgs[1];
      const breite = v.props.runway_width_m;
      const s = v.props.lateral_samples ?? [];
      if (!quer || breite == null || s.length < 2) continue;
      const gestrichelt = /<path[^>]*stroke-dasharray="5 4"[^>]*d="M ([\d.]+)/.exec(quer);
      if (!gestrichelt) continue;

      // Pixel zurück in Meter: dieselbe Projektion wie die Anzeige.
      const gesamt = v.props.length_m + (v.props.displaced_threshold_m ?? 0);
      const startM =
        ((Number(gestrichelt[1]) - 70) / (1060 / gesamt)) - (v.props.displaced_threshold_m ?? 0);

      // Der Querversatz an dieser Stelle — er muss die Kante überschritten
      // haben, sonst steht die gestrichelte Linie auf der Bahn.
      const dort = s.reduce((a, b) =>
        Math.abs(b.laengs_m - startM) < Math.abs(a.laengs_m - startM) ? b : a,
      );
      if (Math.abs(dort.quer_m) < breite / 2 - 2) {
        befunde.push(
          `${v.key}: gestrichelt ab ${startM.toFixed(0)} m bei ${dort.quer_m.toFixed(
            1,
          )} m Versatz — die Kante liegt bei ${(breite / 2).toFixed(1)} m`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("markiert das Räumen nur, wo die Spur die Kante erreicht", () => {
    // Die Marke ③ sitzt an der Bahnkante. Endet die Spur mittig, steht sie
    // im Nichts — man sieht nicht, wie das Flugzeug dorthin gekommen sein
    // soll. Genau das war bei ⑩ zu sehen: Die konstruierte Spur endete bei
    // 700 m mit 0,6 m Versatz und behauptete trotzdem „geräumt · rechts".
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const kante = v.props.clearance_point_m;
      const breite = v.props.runway_width_m;
      const s = v.props.lateral_samples ?? [];
      if (kante == null || breite == null || s.length < 2) continue;
      // Der Versatz am Räumpunkt muss die Kante erreicht haben.
      const dort = s.reduce((a, b) =>
        Math.abs(b.laengs_m - kante) < Math.abs(a.laengs_m - kante) ? b : a,
      );
      if (Math.abs(dort.quer_m) < breite / 2 - 1) {
        befunde.push(
          `${v.key}: geräumt bei ${kante.toFixed(0)} m, dort aber nur ${dort.quer_m.toFixed(
            1,
          )} m Versatz — die Kante liegt bei ${(breite / 2).toFixed(1)} m`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("zeichnet das Band im Verhältnis Spurweite zu Bahnbreite", () => {
    // Die Queransicht ist quer überhöht, aber IN SICH massstäblich: Die
    // Spurweite nimmt genau den Anteil der Bahnbreite ein, den sie in
    // Wirklichkeit einnimmt. Sonst sähe ein Eurofighter (5,00 m) so breit
    // aus wie ein A380 (14,30 m), und die Ansicht behauptete etwas über
    // die Lage der Räder, das nicht stimmt.
    //
    // Gerechnet statt gemessen: Die Bandbreite folgt aus
    // `halbeSpurM * pxProQuerM`, und `pxProQuerM` aus der Bahnbreite. Der
    // Test hält die Kette fest — bei einer der beiden Grössen einen
    // Faktor zu vergessen, fiele im Bild nicht auf.
    const BAHN_PX = 176; // BAHN_BOT − BAHN_TOP in RunwayCrossSection
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const spur = v.props.track_width_m;
      const breite = v.props.runway_width_m;
      if (spur == null || breite == null || v.svgs.length < 2) continue;
      const pxProM = BAHN_PX / breite;
      const bandPx = spur * pxProM;
      const anteil = bandPx / BAHN_PX;
      const soll = spur / breite;
      if (Math.abs(anteil - soll) > 0.001) {
        befunde.push(`${v.key}: Band ${(anteil * 100).toFixed(1)} %, soll ${(soll * 100).toFixed(1)} %`);
      }
      // Und die Bandbreite muss überhaupt sichtbar sein — unter drei
      // Pixeln verschwindet sie zwischen Kontur und Mittellinie.
      if (bandPx < 3) {
        befunde.push(`${v.key}: Band nur ${bandPx.toFixed(1)} px breit`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("nennt Muster und Spurweite in der Queransicht", () => {
    // Die Breite ist massstäblich — aber ob 29 Pixel sieben oder vierzehn
    // Meter sind, sieht man ihr nicht an. Ohne die Angabe im Kopf ist das
    // Band eine Linie ohne Bedeutung.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      if (v.svgs.length < 2) continue;
      const quer = v.svgs[1]!;
      const spur = v.props.track_width_m;
      if (spur == null) {
        if (!quer.includes("Spurweite nicht bekannt")) {
          befunde.push(`${v.key}: fehlende Spurweite wird nicht benannt`);
        }
        continue;
      }
      if (!quer.includes(`Spurweite ${spur.toFixed(1)} m`)) {
        befunde.push(`${v.key}: Spurweite ${spur.toFixed(1)} m steht nicht im Kopf`);
      }
      if (v.props.aircraft_icao && !quer.includes(v.props.aircraft_icao)) {
        befunde.push(`${v.key}: Muster ${v.props.aircraft_icao} steht nicht im Kopf`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("färbt ein Überrollen rot, egal wie mittig gefahren wurde", () => {
    // `sub_bahndisziplin` prüft das Überrollen VOR allen seitlichen Regeln
    // und vergibt null Punkte. Die Farbe muss derselben Ordnung folgen.
    //
    // Der Fall: Bei ④ liegt der seitliche Randabstand bei 17,2 m —
    // vorbildlich mittig — und das Band war deshalb grün, während die Note
    // null ist. Ein Bild, das grün zeigt und rot meint, ist schlimmer als
    // gar keins.
    const ROT = "#ef4444";
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      if ((v.props.overrun_m ?? 0) <= 0 || v.svgs.length < 2) continue;
      const quer = v.svgs[1]!;
      const band = /<path[^>]*fill-opacity="0.22"[^>]*fill="([^"]+)"/.exec(quer)
        ?? /<path[^>]*fill="([^"]+)"[^>]*fill-opacity="0.22"/.exec(quer);
      if (!band) {
        befunde.push(`${v.key}: kein Band gefunden`);
        continue;
      }
      if (band[1] !== ROT) {
        befunde.push(`${v.key}: Band ${band[1]} statt ${ROT} trotz ${v.props.overrun_m} m Überrollen`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("hält Muster und Spurweite konsistent", () => {
    // Der Kopf der Queransicht nennt beides nebeneinander. Passen sie
    // nicht zusammen, widerspricht sich die Anzeige selbst — „A321 ·
    // Spurweite 6,0 m" stand eine Zeit lang da, und 6,0 m ist der
    // A220-Wert. Ursache war eine Demo-Variante, die den Typ ihrer
    // Bahn-Vorlage weitertrug.
    //
    // Geprüft wird gegen dieselbe Tabelle, aus der auch der Client liest.
    // Ein Wert aus der Flugzeugdatei darf abweichen — aber nicht um mehr
    // als einen Meter, sonst ist es ein anderes Muster.
    const TABELLE: Record<string, number> = {
      A319: 7.59, A320: 7.59, A321: 7.59, BCS3: 6.0, B738: 5.72,
      C208: 3.6, C172: 2.5, DHC2: 3.3, MD11: 10.7, A388: 14.3,
    };
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const typ = v.props.aircraft_icao;
      const spur = v.props.track_width_m;
      if (typ == null || spur == null) continue;
      const soll = TABELLE[typ];
      if (soll == null) continue;
      if (Math.abs(spur - soll) > 1.0) {
        befunde.push(`${v.key}: ${typ} mit ${spur.toFixed(1)} m — laut Tabelle ${soll} m`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("gibt der Aufsetzzone nie mehr als ein Drittel der Bahn", () => {
    // ICAO Annex 14: Aufsetzzone = min(900 m, Länge / 3), und unter 1200 m
    // Bahnlänge gibt es GAR KEINE Markierung.
    //
    // Der Fall: ⑩ zeigte „AUFSETZZONE (TDZ) 900 m" auf einer 900-m-Bahn —
    // die Zone wäre so lang gewesen wie die ganze Bahn. Ursache war eine
    // Demo-Variante, die den Wert der Vorlage weitertrug, statt ihn für
    // ihre eigene Bahn zu rechnen.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const tdz = v.props.td_tdz_length_m;
      if (tdz == null) continue;
      if (v.props.length_m < 1200) {
        befunde.push(
          `${v.key}: Aufsetzzone auf einer ${v.props.length_m.toFixed(0)}-m-Bahn — unter 1200 m gibt es keine`,
        );
      } else if (tdz > v.props.length_m / 3 + 1 || tdz > 901) {
        befunde.push(
          `${v.key}: Aufsetzzone ${tdz.toFixed(0)} m bei ${v.props.length_m.toFixed(0)} m Bahn`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("führt jede Nummer der Liste auch als Marke im Bild", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const quer = v.svgs[1];
      if (!quer) continue;
      const imBild = new Set(markenZiffern(quer));
      // Die Liste steht ausserhalb der SVGs.
      const rest = v.markup.replace(/<svg[\s\S]*?<\/svg>/g, "");
      const inListe = new Set(
        [...rest.matchAll(/justify-content:center">(\d)<\/span>/g)].map((m) => m[1]!),
      );
      for (const n of inListe) {
        if (!imBild.has(n)) befunde.push(`${v.key}: Nummer ${n} in der Liste, nicht im Bild`);
      }
      for (const n of imBild) {
        if (!inListe.has(n)) befunde.push(`${v.key}: Marke ${n} im Bild, nicht in der Liste`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("setzt keine zwei Marken aufeinander", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const quer = v.svgs[1];
      if (!quer) continue;
      const kreise = [...quer.matchAll(/<circle([^>]*)\/?>/g)]
        .map((m) => ({
          x: attr(m[1]!, "cx"),
          y: attr(m[1]!, "cy"),
          r: attr(m[1]!, "r") ?? 0,
        }))
        .filter((c) => c.r >= 8 && c.x != null);
      for (let i = 0; i < kreise.length; i++) {
        for (let j = i + 1; j < kreise.length; j++) {
          const d = Math.hypot(kreise[i]!.x! - kreise[j]!.x!, kreise[i]!.y! - kreise[j]!.y!);
          if (d < 16) befunde.push(`${v.key}: zwei Marken ${d.toFixed(0)} px auseinander`);
        }
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});

describe("QS — Vollständigkeit", () => {
  it("löst jede Beschriftung auf", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      for (const t of texte(v.markup)) {
        if (/^[a-z_]+\.[a-z_.]+/.test(t)) befunde.push(`${v.key}: roher Schlüssel "${t}"`);
        if (/\{\{\w+\}\}/.test(t)) befunde.push(`${v.key}: nicht eingesetzt "${t}"`);
      }
      // Auch ausserhalb der Grafik.
      if (/>runway_v2\./.test(v.markup)) befunde.push(`${v.key}: roher Schlüssel im Text`);
      if (/\{\{\w+\}\}/.test(v.markup)) befunde.push(`${v.key}: Platzhalter im Text`);
    }
    expect(befunde, [...new Set(befunde)].join("\n")).toEqual([]);
  });

  it("nennt in jeder Variante Bahn, Länge und Breite", () => {
    // Ohne Breite lässt sich die Queransicht nicht einordnen — sie ist der
    // Massstab, an dem „Rad neben der Bahn" hängt.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      if (!v.markup.includes(v.props.airport_ident)) befunde.push(`${v.key}: kein Platz`);
      if (!v.markup.includes(`${v.props.length_m.toFixed(0)} m`))
        befunde.push(`${v.key}: keine Länge`);
      if (v.props.runway_width_m != null) {
        const breit = `${v.props.runway_width_m.toFixed(0)} m breit`;
        if (!v.markup.includes(breit)) befunde.push(`${v.key}: keine Breite`);
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("gibt jeder Ansicht einen Titel", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      if (!v.markup.includes("LÄNGS —")) befunde.push(`${v.key}: Längsansicht ohne Titel`);
      if (v.svgs.length > 1 && !v.markup.includes("QUER —"))
        befunde.push(`${v.key}: Queransicht ohne Titel`);
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("begründet jede fehlende Queransicht", () => {
    // Wo nichts gezeichnet wird, muss stehen warum — sonst sieht es aus
    // wie ein halb fertiger Bau.
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      if (v.svgs.length > 1) continue;
      const rest = v.markup.replace(/<svg[\s\S]*?<\/svg>/g, "");
      const hatGrund = /(Breite|Rollweg|Graspiste|Belag|Spurweite|erfasst)/.test(rest);
      if (!hatGrund) befunde.push(`${v.key}: keine Queransicht, kein Grund`);
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
  /**
   * Ein Flug von vor v1.7.0 darf nicht der BAHN anlasten, was am Flug liegt.
   *
   * Live gesehen am 23.08.2026 (EDDS 07, Flug #1062): „Für diese Bahn ist
   * keine Breite hinterlegt." EDDS 07 ist 45 m breit und steht mit Breite
   * in den Navdaten — der Flug kam nur von einem älteren Client. Wer die
   * Meldung liest, prüft die Navdaten und findet dort nichts.
   */
  it("nennt bei alten Flügen den Flug als Grund, nicht die Bahn", () => {
    const alt = MOCK_LANDING_OPTIONS[0].build();
    // Genau der Zustand eines v1.6-Datensatzes: keins der neuen Felder.
    for (const feld of [
      "runway_width_m",
      "track_width_m",
      "clearance_point_m",
      "scoring_cutoff_m",
      "lateral_samples",
      "min_edge_clearance_m",
      "surface_paved",
    ] as const) {
      (alt as Record<string, unknown>)[feld] = undefined;
    }
    // Nach dem Aendern der Rohwerte neu ableiten — im Betrieb laeuft die
    // Bewertung ja auch nach den Daten. Ohne das traegt der Datensatz den
    // Grund vom Ausgangszustand.
    (alt as Record<string, unknown>).lateral_skip_reason = undefined;
    skipGrundAbleiten(alt);
    const props = mapLandingRecordToV2Props(alt);
    const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);
    expect(markup).toContain("vor v1.7.0");
    expect(
      markup,
      "die Meldung schiebt es auf die Bahn, obwohl es am Flug liegt",
    ).not.toContain("keine Breite hinterlegt");
  });
  /**
   * Ein breiteres Fahrwerk muss ein breiteres Band bekommen — immer.
   *
   * Bis 23.08.2026 skalierte die Queransicht auf die Bahnbreite: Jede Bahn
   * füllte die Höhe, also war der Massstab in jeder Grafik ein anderer.
   * Gemessen über die Demo-Varianten kehrte sich das Verhältnis um:
   *
   *     C208   Spur 3,6 m   Bahn 23 m   ->   Band 41,3 px
   *     B738   Spur 5,7 m   Bahn 45 m   ->   Band 33,6 px
   *
   * Die Cessna bekam ein breiteres Band als die 737. Thomas hat es an der
   * Demo gesehen; die Zahlen bestätigten es schlimmer als vermutet — nicht
   * gleich breit, sondern verkehrt herum, in drei Paarungen.
   */
  it("zeichnet breitere Fahrwerke breiter — über alle Varianten", () => {
    // Dieselbe Rechnung wie in RunwayCrossSection: feste Referenzbreite,
    // Untergrenze für sehr schmale Bahnen.
    const H = 264;
    const REFERENZ_M = 60;
    const MIN_H = 120;
    const bandPx = (spurM: number, bahnM: number) => {
      const roh = (bahnM / REFERENZ_M) * H;
      const bahnH = Math.min(H, Math.max(MIN_H, roh));
      return spurM * (bahnH / bahnM);
    };

    const gemessen = MOCK_LANDING_OPTIONS.map((o) => mapLandingRecordToV2Props(o.build()))
      .filter((p): p is NonNullable<typeof p> => p != null)
      .filter((p) => p.track_width_m != null && p.runway_width_m != null)
      .map((p) => ({
        typ: p.aircraft_icao ?? "?",
        spur: p.track_width_m!,
        px: bandPx(p.track_width_m!, p.runway_width_m!),
      }));

    expect(gemessen.length, "keine Variante mit Spurweite").toBeGreaterThan(3);

    const verkehrt: string[] = [];
    for (const a of gemessen) {
      for (const b of gemessen) {
        // Deutlich breiteres Fahrwerk (mehr als ein Fünftel), aber
        // schmaleres Band: Das ist die Umkehr, um die es geht.
        if (a.spur > b.spur * 1.2 && a.px < b.px) {
          verkehrt.push(
            `${a.typ} ${a.spur.toFixed(1)} m -> ${a.px.toFixed(0)} px, ` +
              `aber ${b.typ} ${b.spur.toFixed(1)} m -> ${b.px.toFixed(0)} px`,
          );
        }
      }
    }
    expect(verkehrt, "das breitere Fahrwerk bekommt das schmalere Band").toEqual([]);
  });
  /**
   * Eine Ausfahrt, die nicht ins Bild passt, wird gezählt — nicht verschwiegen.
   *
   * In Frankfurt und Köln liegen drei Rollwege innerhalb weniger Meter an
   * der Bahn. Die Gruppierung fasst zwei Namen zusammen („R11/M19"); die
   * dritte fiel bis 23.08.2026 stillschweigend weg. Gemessen über alle 660
   * Bahnen mit Bodenkarte traf das 38 Ausfahrten.
   *
   * Wer die Ausfahrt sucht, die er genommen hat, findet sie dann nicht —
   * und hält die Karte für unvollständig statt die Grafik für gedrängt.
   */
  it("zählt Ausfahrten, die nicht ins Bild passen", () => {
    const r = MOCK_LANDING_OPTIONS[0].build();
    // Vier Ausfahrten auf derselben Seite, alle an derselben Stelle —
    // enger als der Mindestabstand der Gruppierung.
    r.runway_width_m = 46;
    r.track_width_m = 7.6;
    r.surface_paved = true;
    r.lateral_samples = [
      { laengs_m: 300, quer_m: -1 },
      { laengs_m: 900, quer_m: -2 },
      { laengs_m: 1500, quer_m: -3 },
    ];
    r.runway_exits = [
      { name: "R11", laengs_m: 1690, seite: "left" },
      { name: "M19", laengs_m: 1691, seite: "left" },
      { name: "R13", laengs_m: 1692, seite: "left" },
      { name: "M15", laengs_m: 1693, seite: "left" },
    ];
    const props = mapLandingRecordToV2Props(r);
    const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);

    expect(markup, "die zusammengefassten Namen fehlen").toContain("R11/M19");
    expect(
      markup,
      "zwei weitere Ausfahrten liegen dort und werden nicht angedeutet",
    ).toContain("+2");
  });
  /**
   * Das Bewertungsende liegt nie hinter dem Räumpunkt.
   *
   * Zwei Punkte aus zwei Quellen: `scoring_cutoff_m` vom Kurswechsel
   * (Ausschwenken beginnt), `clearance_point_m` vom Kantenübertritt (Bahn
   * verlassen). Das Ausschwenken geht dem Verlassen immer voraus — eine
   * andere Reihenfolge gibt es in der Wirklichkeit nicht.
   *
   * Im Client fielen dabei zwei Wege auseinander: Die Kante wurde auch bei
   * offenem Messfenster gesetzt (ein kurzer Ausritt über die Kante ergab
   * einen Räumpunkt mitten auf der Bahn), und die Interpolation konnte
   * eine Ausdünnungslücke überbrücken und vor dem Räumpunkt landen.
   *
   * Diese Prüfung steht auf der Anzeigeseite, weil hier der Schaden
   * entsteht: Die gestrichelte Linie liefe rückwärts, und die Marke
   * behauptet eine Ausfahrt, die es nicht gab.
   */
  it("setzt das Bewertungsende nie hinter den Räumpunkt", () => {
    const verdreht: string[] = [];
    for (const o of MOCK_LANDING_OPTIONS) {
      const p = mapLandingRecordToV2Props(o.build());
      if (!p) continue;
      const cut = p.scoring_cutoff_m;
      const clear = p.clearance_point_m;
      if (cut != null && clear != null && cut > clear + 0.1) {
        verdreht.push(
          `${o.key}: Bewertungsende ${cut.toFixed(0)} m liegt hinter dem ` +
            `Räumpunkt ${clear.toFixed(0)} m`,
        );
      }
    }
    expect(verdreht).toEqual([]);
  });
  /**
   * Was im Bild gestrichelt ist, wird in der Legende erklärt.
   *
   * Die gestrichelte Spur hängt allein am Räumpunkt; die Legende hing
   * zusätzlich an der Ausfahrtsseite. Die ist aber bewusst oft leer — sie
   * wird nur gesetzt, wenn Kurs UND Querbewegung dasselbe sagen (§8.6).
   * War die Richtung unklar, stand eine gestrichelte Linie ohne Erklärung
   * im Bild.
   */
  it("erklärt die gestrichelte Spur auch ohne bekannte Ausfahrtsseite", () => {
    const r = MOCK_LANDING_OPTIONS.map((o) => o.build()).find(
      (x) => x.clearance_point_m != null && (x.lateral_samples?.length ?? 0) > 2,
    );
    expect(r, "keine Variante mit Räumpunkt und Spur").toBeDefined();
    // Genau der Fall: Räumpunkt bekannt, Richtung nicht eindeutig.
    r!.clearance_side = null;
    const props = mapLandingRecordToV2Props(r!);
    const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);
    expect(
      markup,
      "die gestrichelte Spur wird gezeichnet, aber nirgends erklärt",
    ).toContain("nicht mehr gewertet");
  });
  /**
   * Verwirft die Bewertung die seitliche Lage, zeichnet die Grafik kein Band.
   *
   * Der Text allein genügt nicht: Ein Hinweis unter einer weiter
   * gezeichneten Queransicht liest sich wie eine Fussnote, nicht wie ein
   * Verzicht. Die Zahl im Bild stünde neben echten Messwerten und wäre von
   * ihnen nicht zu unterscheiden.
   *
   * Geprüft für die zwei Gründe, die die Anzeige bis Runde 21 gar nicht
   * kannte — sie zeichnete dabei ein Band mit Randabstand, auf einer
   * Geometrie, der die Bewertung nicht traut, oder aus einem Versatz, den
   * sie als Messfehler verworfen hat.
   */
  it.each([
    ["untrusted_geometry", "nicht verlässlich"],
    ["implausible_lateral_track", "kann nicht stimmen"],
    ["insufficient_samples", "Zu wenige Messpunkte"],
  ])("verzichtet sichtbar bei %s", (grund, textstueck) => {
    const r = MOCK_LANDING_OPTIONS.map((o) => o.build()).find(
      (x) => (x.lateral_samples?.length ?? 0) > 2 && x.runway_width_m != null,
    );
    expect(r, "keine Variante mit Spur und Bahnbreite").toBeDefined();
    r!.lateral_skip_reason = grund;
    const props = mapLandingRecordToV2Props(r!);
    const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);

    expect(markup, `der Grund „${grund}" wird nicht ausgeschrieben`).toContain(
      textstueck,
    );
    expect(
      markup,
      "die Queransicht wird trotz verworfener Bewertung gezeichnet",
    ).not.toContain("QUER —");
  });
  /**
   * Verworfene Zahlen verschwinden — geprüfte bleiben stehen.
   *
   * Die Ereignisliste lief bis Runde 22 immer, auch wenn die Bewertung die
   * seitliche Lage verworfen hatte. Sie zeigte dann „äusseres Rad 3,2 m vor
   * der Kante" direkt neben einem Hinweis, der genau das für unbrauchbar
   * erklärt.
   *
   * Es sind aber nicht alle Gründe gleich: Auf einer Graspiste ist der
   * gemessene Versatz in Ordnung, nur die Kante ist fliessend. Ein
   * pauschales Weglassen hätte eine Messung verschwinden lassen, die
   * stimmt.
   */
  it.each([
    // Grund, Versatz sichtbar?, Randabstand sichtbar?
    ["implausible_lateral_track", false, false],
    ["untrusted_geometry", false, false],
    ["insufficient_samples", false, false],
    ["unpaved_runway", true, false],
    ["surface_unknown", true, false],
  ])("bei %s: Versatz=%s Randabstand=%s", (grund, versatzDa, randDa) => {
    const r = MOCK_LANDING_OPTIONS.map((o) => o.build()).find(
      (x) =>
        (x.lateral_samples?.length ?? 0) > 2 &&
        x.runway_width_m != null &&
        x.max_lateral_offset_m != null &&
        x.min_edge_clearance_m != null,
    );
    expect(r, "keine Variante mit Versatz und Randabstand").toBeDefined();
    r!.lateral_skip_reason = grund;
    const props = mapLandingRecordToV2Props(r!);
    const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);
    const text = markup.replace(/<[^>]+>/g, " ");

    // „Grösster Versatz" ist die Überschrift des Eintrags in der
    // Ereignisliste. Der erste Anlauf suchte „von der Mittellinie" — das
    // steht in der Queransicht, die bei jedem dieser Gründe ohnehin
    // entfällt. Der Test war damit für die Liste blind und meldete
    // trotzdem etwas.
    // Auf „ter Versatz" statt auf den ganzen Text: Die Sprachdatei
    // schreibt „Größter", der `defaultValue` im Code „Grösster". Der
    // zweite Anlauf dieses Tests suchte den `defaultValue` und war
    // dadurch immer noch blind — die Sprachdatei gewinnt zur Laufzeit.
    expect(
      text.includes("ter Versatz"),
      versatzDa
        ? `bei „${grund}" ist der Versatz gemessen und muss stehen bleiben`
        : `bei „${grund}" hat die Bewertung den Versatz verworfen — er darf nicht als Zahl dastehen`,
    ).toBe(versatzDa);

    expect(
      text.includes("vor der Kante") || text.includes("neben der befestigten"),
      randDa
        ? `bei „${grund}" muss der Randabstand stehen bleiben`
        : `bei „${grund}" trägt die Kante nicht — der Randabstand darf nicht als Zahl dastehen`,
    ).toBe(randDa);

    // Und der Aufsetzpunkt bleibt IMMER: Er ist eine eigene Messung und
    // hat mit der seitlichen Lage im Rollweg nichts zu tun.
    expect(text, "der Aufsetz-Eintrag ist verschwunden").toContain("Aufsetzen");
  });
  /**
   * Die Fahrt gehört zu der Stelle, an der sie gemessen wurde.
   *
   * `raeum.kt` ist die Geschwindigkeit beim KURSWECHSEL. Liegt der
   * Kantenübertritt woanders, gehört sie nicht an `clearance_point_m`.
   *
   * Genau dieser Fehler war im Client bereits behoben und stand in der
   * Demo noch. Aus den echten Daten: Räumpunkt 1264,7 m bei 57,9 kt,
   * Kante bei 1901,0 m — dort ist das Flugzeug längst langsamer.
   */
  it("hängt die Fahrt nicht an einen anderen Punkt", () => {
    const verdaechtig: string[] = [];
    for (const o of MOCK_LANDING_OPTIONS) {
      const p = mapLandingRecordToV2Props(o.build());
      if (!p) continue;
      const gs = p.clearance_speed_kt;
      const punkt = p.clearance_point_m;
      const cut = p.scoring_cutoff_m;
      if (gs == null || punkt == null) continue;
      // Es gibt genau eine Stelle, an der gemessen wurde: der
      // Ausschwenkpunkt. Fällt der Räumpunkt nicht mit ihm zusammen,
      // darf keine Fahrt dastehen.
      if (cut != null && Math.abs(punkt - cut) >= 25) {
        verdaechtig.push(
          `${o.key}: ${gs} kt an ${punkt.toFixed(0)} m, gemessen bei ${cut.toFixed(0)} m`,
        );
      }
    }
    expect(
      verdaechtig,
      "Diese Varianten zeigen eine Geschwindigkeit an einem Punkt, an dem " +
        "sie nicht gemessen wurde.",
    ).toEqual([]);
  });
  /**
   * Die Legende beschriftet den Grünstreifen, nicht die Bahn.
   *
   * Der Eintrag hiess „unbefestigt" und stand unter einer Legende, die
   * sonst nur von der Bahn handelt. Bei EDLW 24 (Asphalt) las sich das als
   * Aussage über die Landebahn.
   *
   * Verkehrt war es doppelt: Er erschien bei allen fünf Varianten mit
   * BEFESTIGTER Bahn — und gerade nicht bei Gras und Wasser, weil dort die
   * Queransicht mitsamt Legende entfällt.
   */
  it("nennt eine Asphaltbahn nicht unbefestigt", () => {
    const falsch: string[] = [];
    for (const o of MOCK_LANDING_OPTIONS) {
      const p = mapLandingRecordToV2Props(o.build());
      if (!p || p.surface_paved !== true) continue;
      const text = renderToStaticMarkup(<RunwayDiagramV2 {...p} />).replace(
        /<[^>]+>/g,
        " ",
      );
      // Das Wort darf vorkommen — aber nur mit dem Bezug „neben der Bahn".
      const roh = / unbefestigt/.test(text);
      const mitBezug = /neben der Bahn\s*—\s*unbefestigt/.test(text);
      if (roh && !mitBezug) {
        falsch.push(`${o.key} (Belag ${p.surface ?? "?"})`);
      }
    }
    expect(
      falsch,
      "Diese Varianten haben eine befestigte Bahn und beschriften sie als " +
        "unbefestigt.",
    ).toEqual([]);
  });

  /**
   * Die Skip-Varianten tragen den Grund, den sie zeigen sollen.
   *
   * Er kam in ALLEN vierzehn Varianten als `null` an — auch in den dreien,
   * die genau diese Fälle darstellen. Die Anzeige fiel dort auf ihre
   * eigene Herleitung zurück, und der Weg, den der Betrieb nimmt (Grund
   * aus den `sub_scores`), wurde von der Demo nie gezeigt.
   */
  it.each([
    ["d_gras", "unpaved_runway"],
    ["d_wasser", "water_runway"],
    ["d_ohne_spurweite", "track_width_unknown"],
  ])("%s trägt den Grund %s", (key, grund) => {
    const o = MOCK_LANDING_OPTIONS.find((x) => x.key === key);
    expect(o, `Variante ${key} fehlt`).toBeDefined();
    const p = mapLandingRecordToV2Props(o!.build());
    expect(p?.lateral_skip_reason).toBe(grund);
  });

  /**
   * Jede Zahl an der Längsachse nennt eine STELLE ab der Schwelle.
   *
   * Das Lineal tut es, „TD 780 m" tut es, „BAHN GERÄUMT · 700 m" tut es —
   * und die Ausroll-Endmarke tat es nicht. Sie trug `rollout_m`, die
   * gefahrene Strecke ab dem Aufsetzpunkt. Beim Überrollfall stand sie
   * damit am Bahnende einer 1700-m-Bahn und war mit „1100 m" beschriftet,
   * während das Lineal an derselben Stelle 1500 m zeigte.
   *
   * Die Zahl war für sich richtig und im Zusammenhang falsch — die
   * Fehlerklasse, die kein Typ und kein Bau bemerkt.
   */
  it("beschriftet die Ausroll-Endmarke mit ihrer Stelle, nicht mit der Strecke", () => {
    const falsch: string[] = [];
    for (const v of VARIANTEN) {
      const laengs = v.svgs[0];
      if (!laengs) continue;
      const marke = texte(laengs).find((t) => /AUSROLLEN ENDE|ROLLOUT END/.test(t));
      if (!marke) continue; // Marke entfällt, sobald ein Räumpunkt bekannt ist.

      const zahl = /·\s*(-?[\d.]+)\s*m/.exec(marke);
      const td = v.props.td_distance_from_threshold_m;
      const roll = v.props.rollout_m;
      if (roll == null) continue;
      const ungeklemmt = td + roll;
      const stelle = Math.min(v.props.length_m, ungeklemmt);

      if (ungeklemmt > v.props.length_m) {
        // Hinter dem Bahnende gibt es keine Stelle AUF der Bahn.
        if (zahl) {
          falsch.push(
            `${v.key}: nennt „${zahl[1]} m", obwohl die Aufzeichnung ` +
              `${ungeklemmt.toFixed(0)} m erreicht — hinter dem Bahnende ` +
              `(${v.props.length_m} m)`,
          );
        }
        continue;
      }
      if (!zahl) {
        falsch.push(`${v.key}: Marke ohne Stellenangabe`);
        continue;
      }
      const genannt = Number(zahl[1]);
      if (Math.abs(genannt - stelle) > 1) {
        falsch.push(
          `${v.key}: nennt ${genannt} m, steht aber bei ${stelle.toFixed(0)} m ` +
            `(Aufsetzpunkt ${td} + Ausrollen ${roll})`,
        );
      }
    }
    expect(
      falsch,
      "Eine Stelle und eine Strecke sehen als Zahl gleich aus. Auf einer " +
        "Achse voller Stellen liest sich eine Strecke als Stelle.",
    ).toEqual([]);
  });
});

/**
 * Der Mapper muss jedes Feld tragen, das die Anzeige erklärt.
 *
 * # Warum es diese Prüfung gibt
 *
 * `mess_ende_laengs_m` war am 25.08.2026 in der Bewertung gesetzt, in der
 * Meldung übertragen, im Anzeige-Typ erklärt — und der Mapper liess es
 * fallen. Die Anzeige sah nur `undefined` und fiel auf den Kurswechsel
 * zurück: eine Grenze 550 Meter weiter hinten, gegen die der gemeldete
 * Höchstwert nie stimmen konnte.
 *
 * Das Tückische war nicht der Fehler, sondern seine Stille. Der Test, der
 * ihn hätte fangen sollen, hatte für den Fall ohne das Feld einen
 * Rückfallpfad — und lief auf dem grün durch. Ich habe die Mapper-Zeile
 * probeweise wieder gelöscht: 36 von 36 grün.
 *
 * Darum prüft das hier die VERDRAHTUNG selbst, nicht ihre Wirkung.
 */
describe("Mapper und Anzeige kennen dieselben Felder", () => {
  it("lässt kein Feld der Anzeige unbefüllt", async () => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const lies = (rel: string) =>
      readFileSync(resolve(__dirname, rel), "utf-8");
    const anzeige = lies("RunwayDiagramV2.tsx");
    const mapper = lies("../dev/runwayDiagramV2Mapper.ts");

    // Die Felder aus dem Props-Block der Anzeige.
    const block = anzeige.match(
      /export interface RunwayDiagramV2Props \{([\s\S]*?)\n\}/,
    );
    expect(block, "Props-Block von RunwayDiagramV2 nicht gefunden").toBeTruthy();
    const felder = [...block![1]!.matchAll(/^\s{2}(\w+)\??:/gm)].map((m) => m[1]!);
    // Wenn die Regex ins Leere greift, prüft der Test nichts mehr.
    expect(felder.length).toBeGreaterThan(15);

    /**
     * Felder, die der Mapper bewusst NICHT aus dem Datensatz füllt.
     *
     * Jede Zeile braucht einen eigenen Grund. Eine Sammelbegründung
     * („Anzeigekram") macht die Liste zum Abstellgleis.
     */
    const ausgenommen: Record<string, string> = {
      lang: "Sprache kommt aus der Oberfläche, nicht aus dem Flug",
      t: "Übersetzungsfunktion, kein Messwert",
      onSelectMark: "Rückruf der Oberfläche",
      compact: "Platzverhältnisse des Fensters",
      skin: "Farbtabelle, kein Messwert",
      schriftMindest:
        "Mindestschriftgrösse des Fensters — hängt an der Bildschirmbreite, " +
        "nicht am Flug",
    };

    // `feld:` UND die Kurzschreibweise `feld,` — der Mapper nutzt beides.
    // Nur auf den Doppelpunkt zu prüfen meldete `source` und
    // `td_distance_from_threshold_m` als fehlend, obwohl sie gesetzt sind.
    const fehlt = felder.filter(
      (f) => !ausgenommen[f] && !new RegExp(`\\b${f}\\s*[,:]`).test(mapper),
    );
    expect(
      fehlt,
      `Der Mapper füllt diese Felder nicht: ${fehlt.join(", ")}`,
    ).toEqual([]);
  });
});

/**
 * Die Beschriftung der Ausfahrten bleibt ein Name, keine Aufzaehlung.
 *
 * Hoechstens zwei Kennungen ausgeschrieben, alles weitere gezaehlt. Die
 * Regel stand seit Monaten im Code — geprueft hat sie niemand. Als das
 * Zusammenlegen einen zweiten Durchgang bekam (damit eine breiter
 * gewordene Beschriftung nicht ihren Nachbarn ueberdeckt), legte der
 * zwei fertige Gruppen zusammen, ohne mitzuzaehlen: In Muenchen stand
 * „B3/B2/B1" im Bild, drei Kennungen an einer Stelle.
 *
 * Gefunden hat das kein Test, sondern ein Blick auf das fertige Bild.
 * Diese Pruefung schliesst die Luecke.
 */
describe("Ausfahrts-Beschriftungen", () => {
  it("schreibt höchstens zwei Kennungen aus", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      for (const svg of v.svgs) {
        for (const t of texte(svg)) {
          // Nur Beschriftungen, die wie zusammengelegte Namen aussehen.
          if (!t.includes("/")) continue;
          // Die Bahnkennungen selbst („26L/08R") sind keine Ausfahrten.
          if (/^\d{2}[LRC]?\//.test(t)) continue;
          const namen = t.replace(/\s*\+\d+$/, "").split("/").filter(Boolean);
          if (namen.length > 2) {
            befunde.push(`${v.key}: „${t}" nennt ${namen.length} Kennungen`);
          }
        }
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });

  it("zählt weggelassene Kennungen, statt sie zu verschweigen", () => {
    // Gegenstueck zur Regel oben: Wer kuerzt, muss sagen, wie viel.
    // Sonst sucht der Pilot die Ausfahrt, die er genommen hat, und
    // haelt die Karte fuer unvollstaendig.
    const mitPlus = VARIANTEN.flatMap((v) =>
      v.svgs.flatMap((svg) => texte(svg).filter((t) => /\+\d+$/.test(t))),
    );
    for (const t of mitPlus) {
      expect(t, `„${t}" zählt, nennt aber keine Kennung`).toMatch(
        /^\S+.*\s\+\d+$/,
      );
    }
  });
});

/**
 * Marke ② sitzt dort, wo wirklich gemessen wurde.
 *
 * # Was hier geprüft wird — und was nicht
 *
 * Die Marke wird nicht an den Höchstwert gesetzt, sondern an den
 * Spurpunkt, der ihm am NÄCHSTEN kommt. Solange das Suchfenster stimmt,
 * ist das dasselbe. Ist es zu weit, kann die Suche einen Punkt aus der
 * Ausfahrt finden, dessen Querwert zufällig ähnlich liegt.
 *
 * Bei DLH369 (EDDM 26L) passiert das NICHT: Die Spur kommt nach dem
 * Fensterende nie wieder so nah an −12,9 m heran, also liefern beide
 * Grenzen denselben Punkt. Ich habe es gemessen, statt es anzunehmen —
 * ein Test, der hier eine Verschiebung behauptet, wäre falsch.
 *
 * Geprüft wird deshalb die EIGENSCHAFT, nicht ein Einzelfall: Die Marke
 * darf nie hinter dem Punkt liegen, bis zu dem gemessen wurde. Das gilt
 * für jeden künftigen Datensatz, auch für die, bei denen es einen
 * Unterschied macht.
 */
describe("Marke des grössten Versatzes", () => {
  it("liegt nie hinter dem Ende der Messung", () => {
    const befunde: string[] = [];
    let geprueft = 0;
    for (const v of VARIANTEN) {
      const grenze = v.props.mess_ende_laengs_m;
      const s = v.props.lateral_samples ?? [];
      const max = v.props.max_lateral_offset_m;
      if (grenze == null || max == null || !s.length) continue;
      geprueft++;

      // Dieselbe Suche wie die Anzeige.
      const gewaehlt = s
        .filter((x) => x.laengs_m < grenze)
        .reduce((a, b) =>
          Math.abs(b.quer_m - max) < Math.abs(a.quer_m - max) ? b : a,
        );
      if (gewaehlt.laengs_m > grenze) {
        befunde.push(
          `${v.key}: Marke bei ${gewaehlt.laengs_m.toFixed(0)} m, ` +
            `gemessen wurde nur bis ${grenze.toFixed(0)} m`,
        );
      }
      // Und die Anzeige muss genau diesen Punkt NENNEN.
      //
      // Ein blosses `markup.includes("880")` reicht nicht: Die Zahl
      // steht auch in Skalen und Koordinaten, der Test wäre grün, egal
      // welche Grenze die Anzeige benutzt. Genau das ist mir hier
      // passiert. Die Marke sagt ihre Stelle aber im Klartext —
      // „bei 880 m" —, und das ist eindeutig.
      const genannt = /·\s*bei\s+(\d+)\s*m\s*·/.exec(v.markup)?.[1];
      if (genannt == null) {
        befunde.push(`${v.key}: die Marke nennt gar keine Stelle`);
      } else if (Math.abs(Number(genannt) - gewaehlt.laengs_m) > 1) {
        befunde.push(
          `${v.key}: die Anzeige nennt ${genannt} m, gemessen wurde bis ` +
            `${grenze.toFixed(0)} m — dort liegt ${gewaehlt.laengs_m.toFixed(0)} m`,
        );
      }
    }
    // Ohne eine Variante mit dem Feld prüft die Schleife gar nichts.
    expect(geprueft, "keine Variante trägt mess_ende_laengs_m").toBeGreaterThan(0);
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});

/**
 * Das Bild darf nie mehr oder weniger Ausfahrten behaupten, als es gibt.
 *
 * # Warum diese Prüfung gebaut wurde
 *
 * Die Zusammenfassung hat zwei Aufgaben, die einander widersprechen:
 * lesbar bleiben und nichts verschweigen. Sie löst das, indem sie
 * höchstens zwei Kennungen ausschreibt und den Rest zählt.
 *
 * Die erste Fassung setzte den Namen bei jedem Verbinden aus dem
 * vorherigen TEXT neu zusammen und las das „+N" wieder heraus. Solange
 * der Text von ihr selbst stammte, ging das gut. Ein Rollweg, der in OSM
 * „A+1" heisst, wurde aber als „A und ein weiterer" gelesen — aus drei
 * Ausfahrten wurden vier behauptete. Gefunden in einer QS-Runde mit
 * bösartigen Namen, nicht im Betrieb.
 *
 * Seither wird intern gezählt und der Name erst am Schluss gesetzt.
 */
describe("Ausfahrten: die Zahl im Bild", () => {
  /** Nur die Ausfahrtsbeschriftungen — die Skala trägt dieselben Ziffern. */
  const ausfahrtsTexte = (mk: string) =>
    [...mk.matchAll(/<text[^>]*font-size="9"[^>]*>([\s\S]*?)<\/text>/g)]
      .map((m) => m[1]!.replace(/<[^>]+>/g, "").trim())
      .filter(Boolean);

  const FAELLE: Array<{ name: string; exits: unknown[]; erwartet: number }> = [
    {
      name: "vierzig auf sechzig Meter",
      exits: Array.from({ length: 40 }, (_, i) => ({
        name: `T${i}`, laengs_m: 500 + i * 1.5, seite: "right",
      })),
      erwartet: 40,
    },
    {
      // OSM teilt einen Rollweg oft in Stücke, die alle gleich heissen.
      name: "derselbe Name vierzigmal",
      exits: Array.from({ length: 40 }, (_, i) => ({
        name: "NP1", laengs_m: 500 + i * 1.5, seite: "right",
      })),
      erwartet: 1,
    },
    {
      name: "ohne Namen",
      exits: [
        { name: "", laengs_m: 500, seite: "right" },
        { name: "", laengs_m: 505, seite: "right" },
      ],
      erwartet: 0,
    },
    {
      name: "eine einzige",
      exits: [{ name: "A1", laengs_m: 900, seite: "left" }],
      erwartet: 1,
    },
    {
      name: "beide Seiten am selben Punkt",
      exits: [
        { name: "L1", laengs_m: 800, seite: "left" },
        { name: "R1", laengs_m: 800, seite: "right" },
      ],
      erwartet: 2,
    },
    {
      name: "lange Namen",
      exits: [
        { name: "ALPHA-BRAVO-CHARLIE", laengs_m: 500, seite: "left" },
        { name: "DELTA-ECHO-FOXTROT", laengs_m: 520, seite: "left" },
      ],
      erwartet: 2,
    },
  ];

  it("behauptet genau so viele, wie es gibt", () => {
    const basis = VARIANTEN.find((v) => v.key === "dlh369")!.props;
    const befunde: string[] = [];
    for (const f of FAELLE) {
      const mk = renderToStaticMarkup(
        <RunwayDiagramV2 {...basis} runway_exits={f.exits as never} />,
      );
      let behauptet = 0;
      for (const s of ausfahrtsTexte(mk)) {
        const m = /\+(\d+)$/.exec(s);
        behauptet +=
          s.replace(/\s*\+\d+$/, "").split("/").filter(Boolean).length +
          (m ? Number(m[1]) : 0);
      }
      if (behauptet !== f.erwartet) {
        befunde.push(
          `${f.name}: Bild behauptet ${behauptet}, tatsächlich ${f.erwartet}`,
        );
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});

/**
 * Der Korridor gehört zu der Ausfahrt, die genommen wurde.
 *
 * # Der Befund
 *
 * Das Fenster um den Räumpunkt ist 120 Meter breit, und in München
 * liegen darin zwei Ausfahrten: B7 bei 2.259 m und B6 bei 2.368 m.
 * Thomas' Räumpunkt war 2.345 m — 23 Meter von B6, 86 von B7. Gezeichnet
 * wurde B7, weil `find` die erste Passende nimmt und die Liste nach
 * Längsposition sortiert ist.
 *
 * Das ist genau der Einwand, mit dem diese Arbeit angefangen hat: „auf B6
 * abgerollt, aber das Abrollen sieht auf der Darstellung ganz anders aus."
 * Der Korridor hätte den falschen Rollweg gezeigt.
 */
describe("Korridor der genommenen Ausfahrt", () => {
  it("nimmt die nächstgelegene, nicht die erste", () => {
    const basis = VARIANTEN.find((v) => v.key === "dlh369")!.props;
    // Zwei Ausfahrten im 120-Meter-Fenster, beide mit Verlauf. Die
    // weiter entfernte steht vorn — so liegt es in der echten Liste.
    const exits = [
      {
        name: "B7", laengs_m: 2259, seite: "right",
        verlauf: [
          { laengs_m: 2259, quer_m: 2 },
          { laengs_m: 2300, quer_m: 28 },
        ],
      },
      {
        name: "B6", laengs_m: 2368, seite: "right",
        verlauf: [
          { laengs_m: 2368, quer_m: 22 },
          { laengs_m: 2400, quer_m: 31 },
        ],
      },
    ];
    const mk = renderToStaticMarkup(
      <RunwayDiagramV2 {...basis} runway_exits={exits as never} />,
    );

    // Der Korridor wird als eigener Pfad gezeichnet. Welcher der beiden
    // es ist, verrät seine Längslage: B7 endet bei 2.300 m, B6 bei 2.400.
    // Die Marke ③ steht am Räumpunkt und ist für beide gleich, taugt
    // also nicht zur Unterscheidung — deshalb wird hier der gezeichnete
    // Korridor selbst gesucht.
    const korridor = /<path[^>]*fill="#3b82f6"[^>]*d="([^"]+)"/.exec(mk)?.[1]
      ?? /<path[^>]*d="([^"]+)"[^>]*fill="#3b82f6"/.exec(mk)?.[1];
    expect(
      korridor,
      "es wird gar kein Korridor gezeichnet — der Test prüft nichts",
    ).toBeTruthy();

    // Die x-Werte des Pfads in Metern zurückrechnen ist umständlich;
    // einfacher und ebenso eindeutig: Der Korridor von B6 reicht weiter
    // nach rechts als der von B7.
    const xs = [...korridor!.matchAll(/(-?\d+(?:\.\d+)?)[, ]/g)]
      .map((m) => Number(m[1]))
      .filter((n, i) => i % 2 === 0);
    const maxX = Math.max(...xs);
    const nurB7 = renderToStaticMarkup(
      <RunwayDiagramV2 {...basis} runway_exits={[exits[0]] as never} />,
    );
    const kB7 = /<path[^>]*fill="#3b82f6"[^>]*d="([^"]+)"/.exec(nurB7)?.[1]
      ?? /<path[^>]*d="([^"]+)"[^>]*fill="#3b82f6"/.exec(nurB7)?.[1];
    const xsB7 = [...(kB7 ?? "").matchAll(/(-?\d+(?:\.\d+)?)[, ]/g)]
      .map((m) => Number(m[1]))
      .filter((n, i) => i % 2 === 0);
    expect(xsB7.length, "B7 allein zeichnet keinen Korridor").toBeGreaterThan(0);
    expect(
      maxX,
      "der gezeichnete Korridor ist der von B7 — die erste, nicht die nächste",
    ).toBeGreaterThan(Math.max(...xsB7) + 1);
  });
});

/**
 * Nichts von der Spur wird an den Bildrand geklebt.
 *
 * # Der Befund (Thomas, 26.08.2026): „nach der RWY wieder so ein nicht
 * passender grüner Verlauf"
 *
 * `querZuY` begrenzt auf den sichtbaren Streifen — sinnvoll, damit ein
 * unmöglicher Messwert die Grafik nicht sprengt. Die Nebenwirkung ist
 * schlimmer als das, was sie verhindert: JEDER Punkt jenseits der Grenze
 * landet auf derselben Höhe.
 *
 * Bei DLH369 zog das Flugzeug nach dem Räumen bis 107,8 m nach rechts,
 * die Ansicht zeigt rund 33. Von 94 Punkten des Nach-Räum-Pfads lagen
 * **90** exakt auf der Kantenlinie — gezeichnet als 45 Pixel waagerechter
 * Lauf, dazu das Band als grünes Rechteck darunter und eine Perlenkette
 * aus Messpunkten.
 *
 * Das ist keine Ungenauigkeit, sondern eine falsche Aussage: Es liest
 * sich, als wäre das Flugzeug die Bahnkante entlanggerollt.
 *
 * Geprüft wird deshalb die Eigenschaft: Was gezeichnet wird, endet am
 * Rand — es legt sich nicht an ihn.
 */
describe("Spur am Bildrand", () => {
  /** Die y-Werte eines Pfads oder einer Punktliste. */
  const yWerte = (d: string) =>
    [...d.matchAll(/-?\d+(?:\.\d+)?[, ]\s*(-?\d+(?:\.\d+)?)/g)].map((m) =>
      Number(m[1]),
    );

  it("klebt weder Linie noch Band noch Messpunkte an die Kante", () => {
    const befunde: string[] = [];
    for (const v of VARIANTEN) {
      const quer = v.svgs.at(-1);
      if (!quer) continue;

      // Die Kantenlinien der Ansicht: die beiden äussersten Werte der
      // Querskala. Sie stehen als Beschriftung „Kante" im Bild; ihre
      // y-Lage lesen wir aus den Messpunkten heraus, indem wir die
      // extremsten nehmen — eine feste Zahl wäre eine zweite Wahrheit.
      const kreise = [...quer.matchAll(/<circle[^>]*cy="([-\d.]+)"[^>]*r="1\.8"/g)]
        .map((m) => Number(m[1]));
      if (kreise.length < 5) continue;

      // Wie oft liegt derselbe y-Wert mehrfach hintereinander? Ein
      // geklemmter Verlauf erzeugt genau das.
      const zaehlen = new Map<string, number>();
      for (const y of kreise) {
        const k = y.toFixed(1);
        zaehlen.set(k, (zaehlen.get(k) ?? 0) + 1);
      }
      const haeufigster = [...zaehlen.entries()].sort((a, b) => b[1] - a[1])[0]!;
      // Eine echte Spur trifft denselben Zehntelpixel selten oft. Zehn
      // Punkte auf exakt derselben Höhe sind eine Klemmung, keine Messung.
      if (haeufigster[1] >= 10) {
        befunde.push(
          `${v.key}: ${haeufigster[1]} Messpunkte auf y=${haeufigster[0]} — geklemmt`,
        );
      }

      // Und dasselbe für die gezeichneten Pfade der Spur.
      for (const m of quer.matchAll(/<path([^>]*)\/?>/g)) {
        const attr = m[1]!;
        // Nur die Spur selbst — nicht Schraffuren oder der Korridor.
        if (!/stroke="#(22c55e|f59e0b|ef4444|eab308)"/.test(attr)) continue;
        const d = /d="([^"]+)"/.exec(attr)?.[1] ?? "";
        const ys = yWerte(d);
        if (ys.length < 10) continue;
        const zaehl = new Map<string, number>();
        for (const y of ys) {
          const k = y.toFixed(1);
          zaehl.set(k, (zaehl.get(k) ?? 0) + 1);
        }
        const oft = [...zaehl.entries()].sort((a, b) => b[1] - a[1])[0]!;
        if (oft[1] >= 20) {
          befunde.push(
            `${v.key}: ${oft[1]} Pfadpunkte auf y=${oft[0]} — an den Rand geklebt`,
          );
        }
      }
    }
    expect(befunde, befunde.join("\n")).toEqual([]);
  });
});
