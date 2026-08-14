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
 * Vollständigkeits-Ausnahmen — derzeit **leer**, und das soll so bleiben.
 *
 * Als dieser Test entstand, standen hier 39 fehlende italienische Texte
 * (Pilotenchat, MSFS-HUD, Integritätsmeldungen, CPDLC). Sie sind übersetzt;
 * die Liste ist der Ort, an dem eine neue Lücke sichtbar würde, bevor sie
 * beim Piloten als roher Schlüssel auf dem Bildschirm landet. Wer hier
 * etwas einträgt, verschiebt Arbeit — besser gleich übersetzen.
 */
const ALTLAST_IT = new Set<string>([]);

/** Keine Namensräume ausgenommen — jeder Text zählt. */
const AUSGENOMMEN: RegExp[] = [];

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
