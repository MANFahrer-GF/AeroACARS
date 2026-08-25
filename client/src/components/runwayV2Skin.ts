// Runway Diagram v2 — Skin definition.
//
// Eine "Skin" ist die komplette visuelle Konfiguration der V2-Component:
// Farben, Geometrie, Tone-Schwellen, Display-Flags. Die Pilot-Client- und
// Webapp-Komponenten lesen NICHT mehr Hardcoded-Werte sondern beziehen
// alles aus dem aktuell geladenen Skin.
//
// v0.19.x FIX: das `labels`-Feld (Label-Strings) wurde entfernt — es war
// hier definiert + befüllt + gemerged, aber die Component las es nie
// (sie war längst auf i18next umgestiegen, siehe `t("runway_v2.*")`).
// Ein VA-Admin, der über den VPS-Skin ein Label geändert hätte, hätte
// KEINE Wirkung gesehen — toter Contract. Labels bleiben jetzt allein
// bei i18next (3 Sprachen), statt einem zweiten, nicht-funktionierenden
// System mit nur einer Sprache daneben herzuschleppen.
//
// Lade-Strategie:
//   1. Beim App-Start versucht der Pilot-Client einen frischen Skin von
//      https://live.kant.ovh/api/v2-skin zu holen
//   2. Erfolgreicher Fetch → in AppData\com.aeroacars.app\v2-skin.json
//      cachen + verwenden
//   3. Fehlgeschlagener Fetch (offline) → letzten Cache lesen
//   4. Kein Cache (Erstinstallation offline) → DEFAULT_SKIN aus diesem
//      File verwenden
//
// Damit:
//   - Funktioniert offline (DEFAULT_SKIN als Fallback)
//   - Live-Änderungen für alle Piloten (= VPS-Skin tauschen, Piloten
//     pulken beim nächsten App-Start)
//   - Pilot-Client-Design bleibt nativ (kein iframe), Skin steuert nur
//     Tokens/Labels/Schwellen/Flags

export interface V2Skin {
  /** Schema-Version. Erhöhen wenn breaking changes an den Feldern. */
  version: number;

  // ─── Visual Tokens (Farben + strokeWidth + dashArray) ─────────────
  tokens: {
    tarmac: string;
    tarmacBorder: string;
    threshold: string;
    centerline: string;
    centerlineDashArray: string;
    tdzFill: string;
    tdzStroke: string;
    aimMarker: string;
    rollout: string;
    rolloutGlow: string;
    exitDot: string;
    ddsZone: string;
    ddsBorder: string;
    tdPerfect: string;
    tdAcceptable: string;
    tdWarn: string;
    tdSevere: string;

    /**
     * Der genutzte Rollweg in der Queransicht — Fläche und Rand.
     *
     * Blau, weil er der Untergrund ist, auf dem die grüne Spur läuft:
     * Zwei Grüntöne wären nicht auseinanderzuhalten, und die Frage lautet
     * ja gerade „lief die Spur IM Rollweg".
     */
    rollweg: string;
    rollwegRand: string;
  };

  // ─── SVG-Geometrie ─────────────────────────────────────────────────
  geometry: {
    svgWidth: number;
    svgHeight: number;
    rwyPaddingX: number;
    rwyPaddingY: number;
  };

  // ─── Tone-Schwellen (= Grenzwerte für grün/gelb/rot) ───────────────
  thresholds: {
    peak_g_warn: number;             // 1.5
    peak_g_bad: number;              // 1.7
    crosswind_warn: number;          // 15
    crosswind_bad: number;           // 25
    tailwind_bad: number;            // 3 (negative HW)
    pitch_bad_below: number;         // 0 (Pitch < 0 = Tail-Strike-Risk)
    bank_warn_above: number;         // 5
    bahn_auslastung_warn_above: number;   // 85
    centerline_warn_above: number;        // 5
    centerline_bad_above: number;         // 15
    hinter_schwelle_warn_above: number;   // 1000
  };

  // ─── Display-Flags (= ein/ausblenden ohne Code-Änderung) ───────────
  display: {
    show_aim_marker: boolean;
    show_aufsetzzone_box: boolean;
    show_brakepoint: boolean;
    show_opposite_runway: boolean;
    show_bahn_length: boolean;
    show_flugzeug_bar: boolean;
    show_lr_offset_arrow: boolean;
  };
}

// ─── Default-Skin (bundled, used als Offline-Fallback) ──────────────

export const DEFAULT_SKIN: V2Skin = {
  version: 1,
  tokens: {
    tarmac: "#1a2030",
    tarmacBorder: "rgba(255,255,255,0.18)",
    threshold: "rgba(255,255,255,0.85)",
    centerline: "rgba(255,255,255,0.5)",
    centerlineDashArray: "14,10",
    tdzFill: "rgba(253,224,138,0.18)",
    tdzStroke: "rgba(253,224,138,0.55)",
    aimMarker: "#fbbf24",
    rollout: "#22d3ee",
    rolloutGlow: "rgba(34,211,238,0.18)",
    exitDot: "#f59e0b",
    ddsZone: "rgba(124,45,18,0.45)",
    ddsBorder: "rgba(220,38,38,0.65)",
    tdPerfect: "#22c55e",
    tdAcceptable: "#22d3ee",
    tdWarn: "#fbbf24",
    tdSevere: "#ef4444",
    rollweg: "#3b82f6",
    rollwegRand: "#7dd3fc",
  },
  geometry: {
    svgWidth: 1200,
    svgHeight: 400,
    rwyPaddingX: 70,
    rwyPaddingY: 95,
  },
  thresholds: {
    peak_g_warn: 1.5,
    peak_g_bad: 1.7,
    crosswind_warn: 15,
    crosswind_bad: 25,
    tailwind_bad: 3,
    pitch_bad_below: 0,
    bank_warn_above: 5,
    bahn_auslastung_warn_above: 85,
    centerline_warn_above: 5,
    centerline_bad_above: 15,
    hinter_schwelle_warn_above: 1000,
  },
  display: {
    show_aim_marker: true,
    show_aufsetzzone_box: true,
    show_brakepoint: true,
    show_opposite_runway: true,
    show_bahn_length: true,
    show_flugzeug_bar: true,
    show_lr_offset_arrow: true,
  },
};

/**
 * Tiefes Mergen einer fremden (möglicherweise unvollständigen) Skin
 * mit den Defaults. Fehlende Felder werden mit Defaults ergänzt, damit
 * eine neu-deployed Pilot-Client-Version mit altem VPS-Skin nicht
 * crasht.
 */
export function mergeWithDefaults(partial: Partial<V2Skin> | null | undefined): V2Skin {
  if (!partial) return DEFAULT_SKIN;
  return {
    version: partial.version ?? DEFAULT_SKIN.version,
    tokens: { ...DEFAULT_SKIN.tokens, ...(partial.tokens ?? {}) },
    geometry: { ...DEFAULT_SKIN.geometry, ...(partial.geometry ?? {}) },
    thresholds: { ...DEFAULT_SKIN.thresholds, ...(partial.thresholds ?? {}) },
    display: { ...DEFAULT_SKIN.display, ...(partial.display ?? {}) },
  };
}
