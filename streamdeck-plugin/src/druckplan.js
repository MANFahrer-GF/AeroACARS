// Was ein Druck auf eine Kachel tut — ohne SDK, damit die Tests es prüfen können.

/** Layout je Gerätetyp (Elgato DeviceType). Unbekannte Geräte: keins. */
export function layoutFuer(typ) {
  return { 0: "15", 2: "xl", 7: "plus", 9: "plus" }[typ] ?? null;
}

/**
 * Übersetzt den Eintrag der Szene in kurz/halten/lang, abhängig vom
 * Zustand bei Druckbeginn. Rein, damit die Tests es prüfen können.
 */
export function druckPlan(eintrag, kabine) {
  if (!eintrag) return {};
  switch (eintrag.bedienung) {
    case "zurueck": return { kurz: "zurueck" };
    case "cdm": return { kurz: "cdm" };
    case "ans_minus": return { kurz: "ans_leiser" };
    case "ans_plus": return { kurz: "ans_lauter" };
    case "mus_minus": return { kurz: "mus_leiser" };
    case "mus_plus": return { kurz: "mus_lauter" };
    // Kurzer Druck schaltet um (Thomas 24.10.: nicht nur per langem Druck).
    // Stumm verwirft laufende und wartende Ansagen.
    case "ans_stumm": return { kurz: "ans_umschalten" };
    case "mus_stumm": return { kurz: "mus_stumm", halten: "boarding" };
    default: return { kurz: eintrag.kurz, halten: eintrag.halten === "boarding" ? "boarding" : undefined, lang: eintrag.lang };
  }
}

