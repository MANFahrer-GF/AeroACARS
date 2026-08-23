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
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
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
      const raeum = v.props.scoring_cutoff_m ?? v.props.clearance_point_m;
      if (max == null || raeum == null || s.length < 2) continue;
      const gewertet = s.filter((x) => x.laengs_m < raeum);
      if (!gewertet.length) continue;
      const echterMax = gewertet.reduce((a, b) =>
        Math.abs(b.quer_m) > Math.abs(a.quer_m) ? b : a,
      ).quer_m;
      if (Math.abs(Math.abs(max) - Math.abs(echterMax)) > 0.5) {
        befunde.push(
          `${v.key}: gemeldet ${max.toFixed(1)} m, im gewerteten Teil ${echterMax.toFixed(1)} m`,
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
});
