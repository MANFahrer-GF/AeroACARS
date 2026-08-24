// Lesbarkeit als Prüfung, nicht als Geschmacksfrage.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.6 — „harte Anforderung, kein
// Feinschliff", und ausdrücklich: „Gehört in den Snapshot-Test."
//
// # Warum das automatisch geprüft wird
//
// Jeder der drei Fehler, gegen die hier geprüft wird, ist beim Bau
// tatsächlich aufgetreten — und keiner davon fiel beim Ansehen einer
// einzelnen Variante auf:
//
//   * `RECHTS` stand rechtsbündig an der Skalenachse und begann damit bei
//     x = −11, also ausserhalb des Zeichenbereichs.
//   * Auf einer 46-m-Bahn lagen die Skalenwerte `20` und `23 m` elf Pixel
//     auseinander und überdeckten sich.
//   * Der Marker „Bremspunkt 40 kt" brachte eine dreistufige Ausweichlogik
//     für seine eigenen Beschriftungen mit und belegte den Platz oberhalb
//     der Bahn, den die Aufsetzzonen-Klammer braucht.
//
// # Die Lehre vom 23.08.
//
// Eine Prüfung, die nur Text gegen Text testet, meldet „0 Kollisionen",
// während eine Beschriftung quer über der Fahrspur liegt. Text gegen Grafik
// ist der Fall, der in der Praxis auftritt — deshalb prüft diese Datei
// beides.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { mapLandingRecordToV2Props } from "../dev/runwayDiagramV2Mapper";
import { RunwayDiagramV2 } from "./RunwayDiagramV2";

interface Kasten {
  x: number;
  y: number;
  b: number;
  h: number;
  text: string;
}

/**
 * Textkästen aus dem Markup schätzen.
 *
 * Die Breite kommt aus Zeichenzahl mal Schriftgrösse — jsdom rechnet kein
 * Layout, also gibt es keine echte Textmetrik.
 *
 * Der Faktor war 0,55 und damit zu knapp: „23 m" bei neun Punkt ergab
 * geschätzte 19,8 statt gemessener 22 Pixel, und ein Text, der zwei Pixel
 * links aus dem Bild lief, kam durch. 0,62 liegt über der mittleren
 * Zeichenbreite serifenloser Schriften — die Prüfung ist damit strenger als
 * die Wirklichkeit, und das ist die richtige Richtung: Ein knapp
 * bestandener Fall soll auffallen, nicht durchrutschen.
 */
function textKaesten(svg: string): Kasten[] {
  const out: Kasten[] = [];
  const re = /<text([^>]*)>([\s\S]*?)<\/text>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg))) {
    const attr = m[1]!;
    const text = m[2]!.replace(/<[^>]+>/g, "").trim();
    if (!text) continue;
    const zahl = (name: string, vorgabe: number) => {
      const g = new RegExp(`${name}="([-\\d.]+)"`).exec(attr);
      return g ? Number(g[1]) : vorgabe;
    };
    const fs = zahl("font-size", 10);
    const anker = /text-anchor="(\w+)"/.exec(attr)?.[1] ?? "start";
    const b = text.length * fs * 0.62;
    let x = zahl("x", 0);
    if (anker === "middle") x -= b / 2;
    else if (anker === "end") x -= b;
    out.push({ x, y: zahl("y", 0) - fs * 0.8, b, h: fs * 1.15, text });
  }
  return out;
}

/** Kreise mit nennenswertem Radius — Markierungen, keine Messpunkte. */
function kreise(svg: string): Array<{ x: number; y: number; r: number }> {
  const out: Array<{ x: number; y: number; r: number }> = [];
  const re = /<circle([^>]*)\/?>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg))) {
    const attr = m[1]!;
    const z = (n: string) => {
      const g = new RegExp(`${n}="([-\\d.]+)"`).exec(attr);
      return g ? Number(g[1]) : null;
    };
    const [cx, cy, r] = [z("cx"), z("cy"), z("r")];
    if (cx != null && cy != null && r != null && r >= 4) out.push({ x: cx, y: cy, r });
  }
  return out;
}

/** Stützpunkte aller Linienzüge — Text darf keinen davon treffen. */
function grafikPunkte(svg: string): Array<{ x: number; y: number }> {
  const out: Array<{ x: number; y: number }> = [];
  const re = /<(?:polyline|polygon|path)[^>]*\bd="([^"]+)"/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(svg))) {
    const paare = m[1]!.matchAll(/([-\d.]+)[, ]([-\d.]+)/g);
    for (const q of paare) out.push({ x: Number(q[1]), y: Number(q[2]) });
  }
  return out;
}

function ueberlappt(a: Kasten, b: Kasten): boolean {
  return !(
    a.x + a.b <= b.x ||
    b.x + b.b <= a.x ||
    a.y + a.h <= b.y ||
    b.y + b.h <= a.y
  );
}

/**
 * Sitzt der Text in einer Markierung?
 *
 * Die Ziffer einer Marke liegt bewusst im Kreis — das ist keine Kollision,
 * sondern der Zweck. Ohne diese Ausnahme meldet die Prüfung vierzehn
 * Befunde, die alle richtig gezeichnet sind, und wird dadurch wertlos.
 */
function inMarkierung(k: Kasten, kr: Array<{ x: number; y: number; r: number }>): boolean {
  const mx = k.x + k.b / 2;
  const my = k.y + k.h / 2;
  return kr.some((c) => Math.hypot(c.x - mx, c.y - my) <= c.r + 1);
}

function svgs(markup: string): string[] {
  return markup.match(/<svg[\s\S]*?<\/svg>/g) ?? [];
}

describe("§8.6 Lesbarkeit — über alle Demo-Varianten", () => {
  const varianten = MOCK_LANDING_OPTIONS.map((o) => {
    const props = mapLandingRecordToV2Props(o.build());
    return { key: o.key, markup: renderToStaticMarkup(<RunwayDiagramV2 {...props! } />) };
  });

  it("lässt keinen Text einen anderen überdecken", () => {
    const befunde: string[] = [];
    for (const v of varianten) {
      for (const svg of svgs(v.markup)) {
        const ks = textKaesten(svg);
        for (let i = 0; i < ks.length; i++) {
          for (let j = i + 1; j < ks.length; j++) {
            if (ueberlappt(ks[i]!, ks[j]!)) {
              befunde.push(`${v.key}: "${ks[i]!.text}" × "${ks[j]!.text}"`);
            }
          }
        }
      }
    }
    expect(befunde, befunde.slice(0, 10).join("\n")).toEqual([]);
  });

  it("lässt keinen Text über einem Linienzug liegen", () => {
    // Der Fall, den eine reine Text-gegen-Text-Prüfung nicht sieht.
    const befunde: string[] = [];
    for (const v of varianten) {
      for (const svg of svgs(v.markup)) {
        const kr = kreise(svg);
        const punkte = grafikPunkte(svg);
        for (const k of textKaesten(svg)) {
          if (inMarkierung(k, kr)) continue;
          const treffer = punkte.find(
            (q) => q.x >= k.x && q.x <= k.x + k.b && q.y >= k.y && q.y <= k.y + k.h,
          );
          if (treffer) {
            befunde.push(
              `${v.key}: "${k.text}" liegt auf (${treffer.x.toFixed(0)}, ${treffer.y.toFixed(0)})`,
            );
          }
        }
      }
    }
    expect(befunde, befunde.slice(0, 10).join("\n")).toEqual([]);
  });

  it("hält jeden Text im Zeichenbereich", () => {
    const befunde: string[] = [];
    for (const v of varianten) {
      for (const svg of svgs(v.markup)) {
        const vb = /viewBox="([-\d.]+) ([-\d.]+) ([\d.]+) ([\d.]+)"/.exec(svg);
        if (!vb) continue;
        const [x0, y0, w, h] = vb.slice(1).map(Number) as [number, number, number, number];
        for (const k of textKaesten(svg)) {
          if (k.x < x0 - 1 || k.x + k.b > x0 + w + 1 || k.y < y0 - 1 || k.y + k.h > y0 + h + 1) {
            befunde.push(`${v.key}: "${k.text}" bei x=${k.x.toFixed(0)}…${(k.x + k.b).toFixed(0)}`);
          }
        }
      }
    }
    expect(befunde, befunde.slice(0, 10).join("\n")).toEqual([]);
  });

  it("setzt keine Beschriftung auf die Bahnfläche", () => {
    // §8.6.3: „Keine Beschriftung auf der Bahnfläche, ausser den
    // Bahnkennungen. Alles andere gehört darüber oder darunter — und die
    // Prüfung muss das eigens testen."
    //
    // Der Fall, den diese Prüfung fängt: „LANDUNG VERBOTEN" stand mittig in
    // der Zone vor der Schwelle. Bei EDDH 23 ist diese Zone 51 Pixel breit,
    // der Text 97 — er ragte über seine eigene Zone hinaus und lag auf der
    // roten Schraffur, rot auf rot. Die Text-gegen-Linie-Prüfung sah ihn
    // nicht: Eine gefüllte Fläche hat keine Stützpunkte.
    const befunde: string[] = [];
    for (const v of varianten) {
      for (const svg of svgs(v.markup)) {
        // Die grösste gefüllte Fläche ist die Bahn.
        const rects = [...svg.matchAll(/<rect([^>]*)\/?>/g)]
          .map((m) => {
            const z = (n: string) => {
              const g = new RegExp(`${n}="([-\\d.]+)"`).exec(m[1]!);
              return g ? Number(g[1]) : null;
            };
            return { x: z("x"), y: z("y"), b: z("width"), h: z("height") };
          })
          .filter((r) => r.x != null && r.b != null && r.b > 200 && r.h! > 30);
        if (!rects.length) continue;
        const bahn = rects.reduce((a, b) => (a.b! * a.h! > b.b! * b.h! ? a : b));
        for (const k of textKaesten(svg)) {
          // Bahnkennungen sind erlaubt: kurze Zeichenfolgen wie „23" oder
          // „05L". Alles Längere gehört nicht auf die Fläche.
          if (/^\d{1,2}[LRC]?$/.test(k.text)) continue;
          const mx = k.x + k.b / 2;
          const my = k.y + k.h / 2;
          if (
            mx > bahn.x! &&
            mx < bahn.x! + bahn.b! &&
            my > bahn.y! &&
            my < bahn.y! + bahn.h!
          ) {
            befunde.push(`${v.key}: "${k.text}" liegt in der Bahnfläche`);
          }
        }
      }
    }
    expect(befunde, befunde.slice(0, 10).join("\n")).toEqual([]);
  });

  it("zeigt den Bremspunkt in keiner Variante", () => {
    // Der Vertrag streicht ihn ersatzlos (runway-diagram-v2.contract.md,
    // Abschnitt v1.7.0). Hier über ALLE Varianten geprüft, nicht nur an
    // einer: Er hing an einem Skin-Schalter, und Skin-Schalter kommen vom
    // VPS.
    for (const v of varianten) {
      expect(v.markup, `${v.key} zeigt noch den Bremspunkt`).not.toContain("40 kt");
    }
  });
});
