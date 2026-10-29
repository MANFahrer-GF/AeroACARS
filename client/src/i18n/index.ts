import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";

import enCommon from "../../../bundle/texte/en.json";
import deCommon from "../../../bundle/texte/de.json";
import itCommon from "../../../bundle/texte/it.json";

export const SUPPORTED_LANGUAGES = ["en", "de", "it"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const LANGUAGE_LABELS: Record<SupportedLanguage, string> = {
  en: "English",
  de: "Deutsch",
  it: "Italiano",
};

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { common: enCommon },
      de: { common: deCommon },
      it: { common: itCommon },
    },
    fallbackLng: "en",
    supportedLngs: SUPPORTED_LANGUAGES,
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "aeroacars.lang",
      caches: ["localStorage"],
    },
  });

// v0.5.37: Sprache nach Auto-Detection EXPLIZIT in localStorage schreiben.
// Bug-Report: User sah nach jedem App-Update wieder Englisch obwohl
// Browser-Locale Deutsch war. Ursache: i18next-browser-languagedetector
// `caches: ["localStorage"]` schreibt nur bei i18n.changeLanguage(),
// nicht bei reiner Auto-Detection. Beim ersten Run bleibt also localStorage
// leer → nach Update fängt die Detection wieder von vorn an. Wir schreiben
// hier einmalig den detected Wert ins localStorage damit er dauerhaft bleibt.
const STORAGE_KEY = "aeroacars.lang";
if (typeof localStorage !== "undefined" && !localStorage.getItem(STORAGE_KEY)) {
  const current = i18n.language?.split("-")[0] as SupportedLanguage | undefined;
  if (current && (SUPPORTED_LANGUAGES as readonly string[]).includes(current)) {
    try { localStorage.setItem(STORAGE_KEY, current); } catch { /* noop */ }
  }
}

/** Laufzeit-Paket (P-3.56): Texte des geltenden Stands ueber die eingebauten
 *  legen. Fehlt der Befehl (Fernbedienung, Test), bleibt der eingebaute Stand. */
export async function texteAusPaketLaden(): Promise<void> {
  const { invoke } = await import("../lib/ipc");
  for (const sprache of SUPPORTED_LANGUAGES) {
    try {
      const text = await invoke<string>("paket_datei", { pfad: `texte/${sprache}.json` });
      const daten = JSON.parse(text) as unknown;
      if (daten && typeof daten === "object") {
        i18n.addResourceBundle(sprache, "common", daten, true, true);
      }
    } catch {
      /* eingebauter Stand bleibt */
    }
  }
}

/** Setzt die Sprache explizit und schreibt in localStorage. */
export function setLanguage(lang: SupportedLanguage): void {
  void i18n.changeLanguage(lang);
  try { localStorage.setItem(STORAGE_KEY, lang); } catch { /* noop */ }
}

export default i18n;
