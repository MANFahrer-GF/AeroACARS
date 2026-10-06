// Score-Version 19 (QS 05.10.2026): Was über einer Landung als Urteil steht
// — Deckel-Grund der Gesamtnote und die Hinweis-Marken —, an EINER Stelle.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Vorher
// hatten Client-Bericht, Webapp-Kopf, Touchdown-Liste und PIREP-Karte je
// eigene Marken mit eigenen Grenzen (600 fpm, 1,7 g, 1,8 g, σ 200, 15 m …)
// und die Webapp die Deckel-Sätze als feste Kopie. Hier wird NICHTS
// gerechnet: gelesen wird nur, was der Client beim Bewerten eingefroren hat
// (Bänder der Teilnoten, Deckel-Grund, Marke der Stabilitätsachse, Hopser).
// Texte nur über `t()` — die Sprachschlüssel übernimmt der Abgleich mit.

/** Übersetzer, wie `useTranslation().t` ihn liefert. */
export type Uebersetzer = (k: string, o?: Record<string, unknown>) => string;

/** Deckel-Gründe mit eigenem Satz unter `landing.deckel.*` bzw.
 *  `landing.deckel_kurz.*`. Die Teil-Gründe (`teil_mittel_<achse>`,
 *  `teil_schlecht_<achse>`) setzt `deckelText` zusammen. */
export const DECKEL_MIT_TEXT: ReadonlySet<string> = new Set([
  "harte_landung",
  "ueberlast",
  "anflug_partial",
  "anflug_unstable",
  "anflug_partial_gesamt",
  "anflug_unstable_gesamt",
  "anflug_nicht_gemessen",
  // Score-Version 19: gefährliche Ereignisse (höchstens 40).
  "vor_der_schwelle",
  "overrun",
  "neben_der_bahn",
  "mehrfach_hopser",
]);

/** Score-Version 21: Abzug statt Deckel. Gründe mit eigenem Satz unter
 *  `landing.abzug.*` bzw. `landing.abzug_kurz.*`; im Datensatz mit Endung
 *  `_abzug`, mehrere mit `+` verbunden (Landung zuerst, dann Anflug). */
export const ABZUG_MIT_TEXT: ReadonlySet<string> = new Set([
  "ueberlast",
  "harte_landung",
  "vor_der_schwelle",
  "overrun",
  "neben_der_bahn",
  "mehrfach_hopser",
  "anflug_partial",
  "anflug_unstable",
  "anflug_nicht_gemessen",
]);

/** Die Gründe eines Deckel-/Abzug-Felds ohne Endung `_abzug` — für Marken,
 *  die wissen wollen, OB z. B. „vor der Schwelle" vorlag, egal ob als
 *  Deckel (Version 19–20) oder als Abzug (ab 21). */
export function deckelGruende(feld: string | null | undefined): string[] {
  if (!feld) return [];
  return feld.split("+").map((g) => g.replace(/_abzug$/, ""));
}

function teilName(t: Uebersetzer, achse: string | undefined): string {
  // Die Bahn-Achse heißt im v2-Pfad „Bahndisziplin" (label_key).
  const a = achse === "rollout" ? "runway_discipline" : achse;
  return a ? t(`landing.sub.${a}`) : t("landing.deckel.ein_teil");
}

// Schlüssel ausgeschrieben (nicht `${familie}.…`): Das Spiegelskript
// erkennt zusammengesetzte Schlüssel nur am festen Vorspann.
function abzugText(t: Uebersetzer, grund: string, kurz: boolean): string | null {
  if (ABZUG_MIT_TEXT.has(grund)) {
    return kurz ? t(`landing.abzug_kurz.${grund}`) : t(`landing.abzug.${grund}`);
  }
  const m = /^teil_(mittel|schlecht)(?:_([a-z_]+))?$/.exec(grund);
  if (!m) return null;
  const teil = teilName(t, m[2]);
  return kurz
    ? t(`landing.abzug_kurz.teil_${m[1]}`, { teil })
    : t(`landing.abzug.teil_${m[1]}`, { teil });
}

/** Text zum Deckel- bzw. Abzug-Grund der Gesamtnote, lang oder kurz
 *  (schmale Spalte). Ab Score-Version 19 nennen `teil_mittel_<achse>` /
 *  `teil_schlecht_<achse>` den Teil. `null` für unbekannte Gründe. */
export function deckelText(t: Uebersetzer, grund: string, kurz = false): string | null {
  if (grund.endsWith("_abzug")) {
    const teile = grund.split("+").map((g) => abzugText(t, g.replace(/_abzug$/, ""), kurz));
    if (teile.some((x) => x == null)) return null;
    return teile.join(kurz ? " · " : " ");
  }
  if (DECKEL_MIT_TEXT.has(grund)) {
    return kurz ? t(`landing.deckel_kurz.${grund}`) : t(`landing.deckel.${grund}`);
  }
  const m = /^teil_(mittel|schlecht)(?:_([a-z_]+))?$/.exec(grund);
  if (!m) return null;
  const teil = teilName(t, m[2]);
  return kurz
    ? t(`landing.deckel_kurz.teil_${m[1]}`, { teil })
    : t(`landing.deckel.teil_${m[1]}`, { teil });
}

/** Ein Hinweis über der Landung. */
export interface LandungsMarke {
  label: string;
  tone: "warn" | "err";
}

/** Eingefrorene Teilnote, so weit die Marken sie lesen. */
export interface MarkenTeil {
  key: string;
  points?: number | null;
  score?: number | null;
  band?: string | null;
  skipped?: boolean | null;
}

/** Die Hinweis-Marken einer Landung ab Score-Version 19.
 *
 *  - HART: Teilnote Sinkrate oder G-Kraft im Band „schlecht" oder G-Deckel
 *    (ab 1,75 g); SCHWER: Überlast-Deckel (ab 2,6 g).
 *  - Hopser: gezählte Hopser (ab zwei rot — Gefahr-Deckel), sonst ein
 *    score-freier leichter Hopser.
 *  - Gefahr: vor der Schwelle, Overrun, neben der Bahn — aus dem Deckel.
 *  - Mittellinie: Teilnote Ausrichtung unter 75 (außer neben der Bahn).
 *  - Anflug: Urteil des Stable Gate (siehe stableGate.ts `gateUrteil`). */
export function landungsMarkenV19(
  e: {
    subs: readonly MarkenTeil[];
    deckel: string | null | undefined;
    urteil: "stable" | "partial" | "unstable" | null;
    bounceCount: number | null | undefined;
    forensicBounceCount?: number | null;
    bounceMaxAglFt?: number | null;
  },
  t: Uebersetzer,
): LandungsMarke[] {
  const marken: LandungsMarke[] = [];
  const achse = (k: string) => e.subs.find((s) => s.key === k && !s.skipped);
  const gruende = deckelGruende(e.deckel);
  const hat = (g: string) => gruende.includes(g);
  if (hat("ueberlast")) {
    marken.push({ label: t("landing.flag.severe"), tone: "err" });
  } else if (
    hat("harte_landung") ||
    achse("landing_rate")?.band === "bad" ||
    achse("g_force")?.band === "bad"
  ) {
    marken.push({ label: t("landing.flag.hard"), tone: "err" });
  }
  const n = e.bounceCount ?? 0;
  if (n > 0) {
    marken.push({ label: `${t("landing.flag.bounce")} × ${n}`, tone: n >= 2 ? "err" : "warn" });
  } else if ((e.forensicBounceCount ?? 0) > 0) {
    marken.push({
      label:
        e.bounceMaxAglFt != null
          ? t("landing.flag.bounce_light_with_height", { ft: Math.round(e.bounceMaxAglFt) })
          : t("landing.flag.bounce_light"),
      tone: "warn",
    });
  }
  if (hat("vor_der_schwelle")) marken.push({ label: t("landing.flag.vor_der_schwelle"), tone: "err" });
  if (hat("overrun")) marken.push({ label: t("landing.flag.overrun"), tone: "err" });
  if (hat("neben_der_bahn")) marken.push({ label: t("landing.flag.neben_der_bahn"), tone: "err" });
  const ausrichtung = achse("alignment");
  if (
    ausrichtung != null &&
    (ausrichtung.points ?? ausrichtung.score ?? 100) < 75 &&
    !hat("neben_der_bahn")
  ) {
    marken.push({ label: t("landing.flag.off_centerline"), tone: "warn" });
  }
  if (e.urteil === "unstable") marken.push({ label: t("landing.flag.unstable_approach"), tone: "err" });
  else if (e.urteil === "partial") marken.push({ label: t("landing.flag.partly_stable_approach"), tone: "warn" });
  return marken;
}
