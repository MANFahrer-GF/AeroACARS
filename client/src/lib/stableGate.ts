// Score-Version 19 (05.10.2026): Stable Gate aus der Prüfliste des
// Datensatzes (`sub_scores[stability].gate`, gerechnet in
// landing_scoring::anflug_urteil). Die Oberfläche rechnet das Urteil nicht
// mehr selbst nach — vorher stand bei GSG1709 „Stabiler Anflug" neben
// „teilweise stabil", ohne dass irgendwo stand, warum.

export interface GatePunkt {
  /** `gleitpfad` | `fahrt` | `querneigung` | `ruck` | `sinken` | `konfiguration` */
  key: string;
  stufe: "gut" | "mittel" | "schlecht";
  wert?: number | null;
  gut_unter?: number | null;
}

export type GateUrteil = "stable" | "partial" | "unstable";

/** Wie `anflug_urteil::urteil_aus`: alles gut → stable; zwei schlecht oder
 *  drei außerhalb von „gut" → unstable; sonst partial. */
export function gateUrteil(gate: GatePunkt[] | null | undefined): GateUrteil | null {
  if (!gate || gate.length === 0) return null;
  const schlecht = gate.filter((p) => p.stufe === "schlecht").length;
  const mittel = gate.filter((p) => p.stufe === "mittel").length;
  if (schlecht === 0 && mittel === 0) return "stable";
  if (schlecht >= 2 || schlecht + mittel >= 3) return "unstable";
  return "partial";
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
  t: (k: string, o?: Record<string, unknown>) => string,
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

/** Die Prüfliste der Stabilitätsachse eines Datensatzes (neu ab v19). */
export function gateAus(
  subs: Array<{ key: string; gate?: GatePunkt[] | null }> | null | undefined,
): GatePunkt[] | null {
  const g = subs?.find((s) => s.key === "stability")?.gate;
  return g && g.length > 0 ? g : null;
}
