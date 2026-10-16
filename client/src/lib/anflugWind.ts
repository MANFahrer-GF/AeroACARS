// Score-Version 24 (16.10.2026, JBU1500 MDPC→MUVR): Wind im Anflug als
// Kontext zum Stable Gate — Rust `landing_scoring::anflug_wind::AnflugWind`,
// beim Aufsetzen EINMAL gerechnet und eingefroren (`approach_wind`).
// Diese Datei rechnet nichts nach, sie formuliert nur.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Texte nur
// über `t()` — die Sprachschlüssel übernimmt der Abgleich mit.

import type { Uebersetzer } from "./stableGate";

export interface SinkLaufInfo {
  dauer_s: number;
  /** Höhe über der Schwelle am Beginn / Ende des Laufs (ft). */
  von_ft: number;
  bis_ft: number;
  /** Stärkste Sinkrate im Lauf (fpm, negativ). */
  spitze_fpm: number;
}

export interface AnflugWind {
  wind_proben: number;
  gegenwind_mittel_kt?: number | null;
  gegenwind_min_kt?: number | null;
  gegenwind_max_kt?: number | null;
  /** Mit Vorzeichen: positiv = von rechts. */
  seitenwind_mittel_kt?: number | null;
  seitenwind_max_kt?: number | null;
  schwankung_kt?: number | null;
  gs_mittel_kt?: number | null;
  /** Soll-Sinkrate des Gleitpfads bei dieser Grundgeschwindigkeit (negativ). */
  soll_sinkrate_fpm?: number | null;
  /** Angewandte Sinkgrenze, gemittelt über das Gate (negativ). */
  sinkgrenze_fpm?: number | null;
  sinklauf?: SinkLaufInfo | null;
}

/** Eine Kachel im Windstreifen: großer Wert, Beschriftung, Zusatz. */
export interface WindKachel {
  key: "gegenwind" | "seitenwind" | "schwankung" | "soll";
  wert: string;
  label: string;
  zusatz?: string;
}

const kt = (v: number) => `${Math.round(Math.abs(v))} kt`;
const fpm = (v: number) => `${Math.round(Math.abs(v) / 10) * 10} fpm`;

/** Die Kacheln des Windstreifens. Fehlt ein Wert, fehlt seine Kachel —
 *  nie eine erfundene Null. */
export function windKacheln(t: Uebersetzer, w: AnflugWind): WindKachel[] {
  const k: WindKachel[] = [];
  const h = w.gegenwind_mittel_kt;
  if (h != null) {
    const spanne =
      w.gegenwind_min_kt != null && w.gegenwind_max_kt != null
        ? t("landing.anflug_wind.spanne", {
            von: Math.round(w.gegenwind_min_kt),
            bis: Math.round(w.gegenwind_max_kt),
          })
        : undefined;
    k.push({
      key: "gegenwind",
      wert: kt(h),
      label: h >= 0 ? t("landing.anflug_wind.gegenwind") : t("landing.anflug_wind.rueckenwind"),
      zusatz: spanne,
    });
  }
  if (w.seitenwind_max_kt != null) {
    const m = w.seitenwind_mittel_kt;
    k.push({
      key: "seitenwind",
      wert: kt(w.seitenwind_max_kt),
      label: t("landing.anflug_wind.seitenwind_max"),
      zusatz:
        m == null || Math.abs(m) < 1
          ? undefined
          : m > 0
            ? t("landing.anflug_wind.meist_von_rechts")
            : t("landing.anflug_wind.meist_von_links"),
    });
  }
  if (w.schwankung_kt != null) {
    k.push({
      key: "schwankung",
      wert: `± ${Math.round(w.schwankung_kt)} kt`,
      label: t("landing.anflug_wind.schwankung"),
    });
  }
  if (w.soll_sinkrate_fpm != null && w.gs_mittel_kt != null) {
    k.push({
      key: "soll",
      wert: fpm(w.soll_sinkrate_fpm),
      label: t("landing.anflug_wind.soll", { gs: Math.round(w.gs_mittel_kt) }),
      zusatz:
        w.sinkgrenze_fpm != null
          ? t("landing.anflug_wind.grenze", { fpm: fpm(w.sinkgrenze_fpm) })
          : undefined,
    });
  }
  return k;
}

/** Satz zum längsten Lauf unter der Sinkgrenze. Die Karte zeigt ihn nur
 *  als Grund der Prüfung „Sinken“, wenn die nicht „gut“ war — die 10-s-
 *  Schwelle steht allein in Rust (`SINK_TOLERANZ_MS`), nicht hier. */
export function sinklaufSatz(
  t: Uebersetzer,
  w: AnflugWind | null | undefined,
  lang: string,
): string | null {
  const l = w?.sinklauf;
  if (!l || w?.sinkgrenze_fpm == null) return null;
  return t("landing.anflug_wind.sinklauf", {
    s: l.dauer_s.toLocaleString(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    grenze: fpm(w.sinkgrenze_fpm),
    von: Math.round(l.von_ft),
    bis: Math.round(l.bis_ft),
    spitze: fpm(l.spitze_fpm),
  });
}
