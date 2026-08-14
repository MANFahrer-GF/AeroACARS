// Sprach-Parität der Oberflächen-Texte.
//
// **Warum es diesen Test gibt (QS-Befund v1.6.2).** Beim Einbau der achten
// Score-Achse fiel auf: es gab keinerlei Netz, das fehlende Übersetzungen
// bemerkt. Dass damals alle 16 neuen Schlüssel in allen drei Sprachen
// landeten, war Handarbeit. Fehlt beim nächsten Mal ein Eintrag, zeigt
// i18next dem Piloten wörtlich den Schlüssel — z. B. `landing.rat.off_centerline`
// mitten in der Kachel — und es fällt erst im Feld auf.
//
// Der `chat.*`-Zweig ist vorbestehend unvollständig (die italienische
// Übersetzung des Pilotenchats steht noch aus). Er ist bewusst ausgenommen,
// damit dieser Test ab sofort scharf ist, statt dauerhaft rot zu stehen.

import { describe, expect, it } from "vitest";
import de from "./de/common.json";
import en from "./en/common.json";
import italienisch from "./it/common.json";

/**
 * Altlast: italienische Übersetzungen, die schon vor diesem Test fehlten.
 *
 * Bewusst als KONKRETE Liste statt als Namensraum-Muster — so schrumpft sie,
 * wenn jemand übersetzt, und ein neuer fehlender Schlüssel im selben Zweig
 * fällt trotzdem sofort auf. Wer hier etwas austrägt, hat übersetzt.
 */
const ALTLAST_IT = new Set([
  "tabs.chat",
  "msfs_hud.section_title",
  "msfs_hud.intro",
  "msfs_hud.toggle_label",
  "msfs_hud.toggle_hint",
  "msfs_hud.restart_hint",
  "integrity.flag_type.POSITION_DELTA_EXCESSIVE",
  "integrity.flag_type.ALTITUDE_DELTA_EXCESSIVE",
  "integrity.flag_type.FUEL_RATE_IMPOSSIBLE",
  "integrity.flag_type.FUEL_INCREASE_IN_FLIGHT",
  "integrity.flag_type.WEIGHT_DELTA_EXCESSIVE",
  "integrity.flag_type.GROUND_ELEVATION_MISMATCH",
  "integrity.flag_type.AIR_TO_GROUND_TELEPORT",
  "integrity.flag_type.SIM_STATE_RESET_SIGNATURE",
  "integrity.flag_type.TELEMETRY_GAP_SHORT",
  "integrity.flag_type.TELEMETRY_GAP_LONG",
  "integrity.flag_type.UNKNOWN",
  "integrity.phase_name.BOARDING",
  "integrity.phase_name.PUSHBACK",
  "integrity.phase_name.TAXI_OUT",
  "integrity.phase_name.TAXI_IN",
  "integrity.phase_name.TAKEOFF_ROLL",
  "integrity.phase_name.TAKEOFF",
  "integrity.phase_name.CLIMB",
  "integrity.phase_name.CRUISE",
  "integrity.phase_name.DESCENT",
  "integrity.phase_name.APPROACH",
  "integrity.phase_name.FINAL",
  "integrity.phase_name.LANDING",
  "integrity.phase_name.BLOCKS_ON",
  "integrity.phase_name.ARRIVED",
  "integrity.flag_description_readable",
  "integrity.flag_count_readable_one",
  "integrity.flag_count_readable_other",
  "cpdlc.callsign_from_plan",
  "cpdlc.callsign_override_active",
  "cpdlc.callsign_locked_logon",
  "cpdlc.callsign_reconnecting",
  "cpdlc.field_callsign_readonly",
]);

/** Der Pilotenchat ist in beiden Fremdsprachen noch nicht übersetzt. */
const AUSGENOMMEN = [/^chat\./];

function schluessel(obj: unknown, praefix = ""): string[] {
  if (obj === null || typeof obj !== "object" || Array.isArray(obj)) {
    return [praefix];
  }
  return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
    schluessel(v, praefix ? `${praefix}.${k}` : k),
  );
}

const gefiltert = (obj: unknown) =>
  schluessel(obj).filter((k) => !AUSGENOMMEN.some((r) => r.test(k)));

describe("Sprach-Parität der Locale-Dateien", () => {
  const referenz = new Set(gefiltert(de));

  for (const [name, daten] of [
    ["Englisch", en],
    ["Italienisch", italienisch],
  ] as const) {
    it(`${name} kennt jeden deutschen Schlüssel`, () => {
      const vorhanden = new Set(gefiltert(daten));
      const fehlend = [...referenz].filter(
        (k) => !vorhanden.has(k) && !(name === "Italienisch" && ALTLAST_IT.has(k)),
      );
      expect(fehlend, `fehlende Übersetzungen (${name}): ${fehlend.join(", ")}`).toEqual([]);
    });
  }

  it("die achte Score-Achse ist vollständig übersetzt", () => {
    // Punktprobe auf genau die Schlüssel, deren Fehlen der Pilot als
    // rohen Key auf dem Bildschirm sähe.
    const pflicht = [
      "landing.sub.alignment",
      "landing.info.alignment",
      "landing.rat.aligned_on_centerline",
      "landing.rat.off_centerline",
      "landing.rat.off_runway_surface",
      "landing.rat.crooked_touchdown",
      "landing.tip.aligned_on_centerline",
      "landing.tip.off_centerline",
      "landing.tip.off_runway_surface",
      "landing.tip.crooked_touchdown",
      "landing.skipped_reason.alignment_off_airport",
      "landing.skipped_reason.alignment_untrusted_geometry",
      "landing.skipped_reason.missing_centerline_offset",
      "landing.skipped_reason.missing_runway_width",
      "landing.skipped_reason.missing_runway_course",
      "landing.skipped_reason.missing_heading",
      "landing.skipped_reason.implausible_runway_geometry",
      "landing.skipped_reason.not_applicable_for_category",
    ];
    for (const [name, daten] of [
      ["Deutsch", de],
      ["Englisch", en],
      ["Italienisch", italienisch],
    ] as const) {
      const vorhanden = new Set(schluessel(daten));
      const fehlend = pflicht.filter((k) => !vorhanden.has(k));
      expect(fehlend, `${name}: ${fehlend.join(", ")}`).toEqual([]);
    }
  });
});
