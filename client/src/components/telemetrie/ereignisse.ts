/**
 * Ereignisse fuer die Leiste im Telemetrie-Monitor: aus zwei aufeinander
 * folgenden Frames abgeleitet (Fahrwerk faehrt aus, AP geht aus, Aufsetzen …).
 *
 * Reine Funktion, damit sie ohne Oberflaeche testbar ist. Die Texte kommen
 * als i18n-Schluessel mit Werten zurueck; uebersetzt wird in der Anzeige.
 */

import type { Frame } from "./typen";

export interface Ereignis {
  t: number;
  /** i18n-Schluessel unter `telemetrie.ereignis.` */
  art: string;
  werte?: Record<string, string | number>;
  /** Hervorheben (Aufsetzen, Abheben). */
  wichtig?: boolean;
}

type Index = Map<string, number>;

function z(f: Frame, idx: Index, id: string): number | null {
  const i = idx.get(id);
  if (i === undefined) return null;
  const v = f.z[i];
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

/** Schaltet ein Schalter-Kanal zwischen den Frames um? */
function umschalten(a: Frame, b: Frame, idx: Index, id: string): "an" | "aus" | null {
  const va = z(a, idx, id);
  const vb = z(b, idx, id);
  if (va === null || vb === null) return null;
  if (va < 0.5 && vb >= 0.5) return "an";
  if (va >= 0.5 && vb < 0.5) return "aus";
  return null;
}

const SCHALTER: Array<[string, string]> = [
  ["ap", "ap"],
  ["athr", "athr"],
  ["spoiler_armed", "spoiler_armed"],
  ["parkbremse", "parkbremse"],
  ["umkehrschub", "umkehrschub"],
  ["licht_lande", "licht_lande"],
  ["master_warning", "master_warning"],
  ["master_caution", "master_caution"],
  ["ueberziehwarnung", "ueberziehwarnung"],
  ["pause", "pause"],
];

export function ereignisseAus(a: Frame | null, b: Frame, idx: Index): Ereignis[] {
  if (!a) return [];
  const aus: Ereignis[] = [];
  const t = b.t;

  // Bodenkontakt: Abheben / Aufsetzen mit V/S und g des Moments.
  const boden = umschalten(a, b, idx, "am_boden");
  if (boden === "an") {
    const vs = z(a, idx, "vs") ?? z(b, idx, "vs");
    const g = z(b, idx, "g");
    aus.push({
      t,
      art: "aufsetzen",
      wichtig: true,
      werte: {
        vs: vs === null ? "–" : Math.round(vs),
        g: g === null ? "–" : g.toFixed(2),
      },
    });
  } else if (boden === "aus") {
    const ias = z(b, idx, "ias");
    aus.push({ t, art: "abheben", wichtig: true, werte: { ias: ias === null ? "–" : Math.round(ias) } });
  }

  // Fahrwerk: ganz aus- bzw. eingefahren.
  const fa = z(a, idx, "fahrwerk");
  const fb = z(b, idx, "fahrwerk");
  if (fa !== null && fb !== null) {
    if (fa < 99.5 && fb >= 99.5) aus.push({ t, art: "fahrwerk_unten" });
    if (fa > 0.5 && fb <= 0.5) aus.push({ t, art: "fahrwerk_oben" });
  }

  // Klappenstufe.
  const ka = z(a, idx, "klappen_stufe");
  const kb = z(b, idx, "klappen_stufe");
  if (ka !== null && kb !== null && ka !== kb) {
    aus.push({ t, art: "klappen", werte: { stufe: Math.round(kb) } });
  }

  for (const [id, art] of SCHALTER) {
    const u = umschalten(a, b, idx, id);
    if (u) aus.push({ t, art: `${art}_${u}` });
  }
  return aus;
}
