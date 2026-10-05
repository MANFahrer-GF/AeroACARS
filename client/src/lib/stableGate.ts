// Score-Version 19 (05.10.2026): Stable Gate aus der Prüfliste des
// Datensatzes (`sub_scores[stability].gate`, gerechnet in
// landing_scoring::anflug_urteil). Die Oberfläche rechnet das Urteil nicht
// mehr selbst nach — vorher stand bei GSG1709 „Stabiler Anflug" neben
// „teilweise stabil", ohne dass irgendwo stand, warum.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN): Client und
// Webapp zeigen Urteil, Werte und Gründe aus GENAU dieser Datei. Texte nur
// über `t()` — die Sprachschlüssel übernimmt der Abgleich mit.

/** Übersetzer, wie `useTranslation().t` ihn liefert. */
export type Uebersetzer = (k: string, o?: Record<string, unknown>) => string;

export interface GatePunkt {
  /** `gleitpfad` | `fahrt` | `querneigung` | `ruck` | `sinken` | `konfiguration` */
  key: string;
  stufe: "gut" | "mittel" | "schlecht";
  wert?: number | null;
  gut_unter?: number | null;
}

export type GateUrteil = "stable" | "partial" | "unstable";

/** Das Urteil, wie der Client es eingefroren hat: die Marke der
 *  Stabilitätsachse (`warning` bzw. bei übersprungener Achse `reason`).
 *  Keine eigene Zählregel — die steht nur in `anflug_urteil::urteil_aus`
 *  (QS 05.10.2026: zwei TS-Kopien hätten bei einer Schwellenänderung in
 *  Rust still abweichen können). `null` ohne Prüfliste (Altbestand). */
export function gateUrteil(
  gate: GatePunkt[] | null | undefined,
  marke: string | null | undefined,
): GateUrteil | null {
  if (!gate || gate.length === 0) return null;
  if (marke === "anflug_unstable") return "unstable";
  if (marke === "anflug_partial") return "partial";
  return "stable";
}

/** Marke der Stabilitätsachse eines Datensatzes (warning, sonst reason). */
export function gateMarke(
  subs: Array<{ key: string; warning?: string | null; reason?: string | null }> | null | undefined,
): string | null {
  const s = subs?.find((x) => x.key === "stability");
  return s?.warning ?? s?.reason ?? null;
}

/** Nachkommastellen je Prüfung für die Anzeige. */
export const GATE_STELLEN: Record<string, number> = {
  gleitpfad: 2,
  fahrt: 1,
  querneigung: 1,
  ruck: 0,
};

function zahl(v: number, stellen: number, lang: string): string {
  return v.toLocaleString(lang, {
    minimumFractionDigits: stellen,
    maximumFractionDigits: stellen,
  });
}

/** Begründungen für alles, was nicht „gut" war — in der Reihenfolge des
 *  Gates (Gleitpfad zuerst). Leer bei STABLE. */
export function gateGruende(
  t: Uebersetzer,
  gate: GatePunkt[] | null | undefined,
  lang: string,
): string[] {
  if (!gate) return [];
  return gate
    .filter((p) => p.stufe !== "gut")
    .map((p) => {
      const stellen = GATE_STELLEN[p.key] ?? 1;
      return t(`landing.gate.grund.${p.key}`, {
        wert: p.wert != null ? zahl(p.wert, stellen, lang) : "",
        grenze: p.gut_unter != null ? zahl(p.gut_unter, 0, lang) : "",
      });
    });
}

/** Beschriftung einer Prüfung (Kachel). */
export function gateLabel(t: Uebersetzer, key: string): string {
  return t(`landing.gate.label.${key}`);
}

/** Einheit einer Prüfung mit Zahl; `undefined` für ja/nein-Prüfungen. */
export function gateEinheit(t: Uebersetzer, p: GatePunkt): string | undefined {
  return p.wert != null ? t(`landing.gate.einheit.${p.key}`) : undefined;
}

/** Wert einer Prüfung für die Kachel: Zahl mit den Stellen der Prüfung
 *  oder das Wort (ok / zu stark, komplett / nicht komplett). */
export function gateWert(t: Uebersetzer, p: GatePunkt, lang: string): string {
  if (p.wert != null) return zahl(p.wert, GATE_STELLEN[p.key] ?? 1, lang);
  return t(`landing.gate.wert.${p.key}_${p.stufe === "gut" ? "ok" : "nein"}`);
}

/** Überschrift der Gründe-Liste. */
export function gateGrundTitel(t: Uebersetzer): string {
  return t("landing.gate.grund_titel");
}

/** Die Prüfliste der Stabilitätsachse eines Datensatzes (neu ab v19). */
export function gateAus(
  subs: Array<{ key: string; gate?: GatePunkt[] | null }> | null | undefined,
): GatePunkt[] | null {
  const g = subs?.find((s) => s.key === "stability")?.gate;
  return g && g.length > 0 ? g : null;
}
