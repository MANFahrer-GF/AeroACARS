// v0.7.8 Phase 1: Vitest-Setup
// Spec: docs/spec/v0.7.8-landing-rate-explainability.md §8.0
//
// `@testing-library/jest-dom` ergaenzt die `expect`-Matchers
// (toBeInTheDocument, toHaveClass, toHaveTextContent etc.).

import "@testing-library/jest-dom";

// ─── i18next, einmal für alle Tests ──────────────────────────────────
//
// Ohne initialisiertes i18next liefert jedes `t("runway_v2.…")` ohne
// Vorgabewert den **rohen Schlüssel** zurück. Das ist nicht nur unschön,
// es verfälscht jede Prüfung, die mit Textlängen rechnet: Aus „6,6 m links"
// wird „6.6 m runway_v2.centerline_left" — viermal so breit, und die
// Lesbarkeitsprüfung meldet einen Überlauf, den es im Produkt nicht gibt.
//
// Bisher initialisierte jede Testdatei i18next selbst. Das funktionierte,
// solange nur eine es tat; die zweite fand eine bereits laufende Instanz vor
// und übersprang ihren Aufruf — mit der Sprache der ersten. Hier steht es
// einmal, vor allen Tests, mit Deutsch als Sprache.
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
// ⚠ Italienisch gehoert hierher, auch wenn kein Test es heute braucht.
// Fehlt eine Sprache, faellt i18next lautlos auf `fallbackLng` zurueck:
// Ein Test, der italienischen Text prueft, bekaeme deutschen — und
// waere gruen. Genau darauf bin ich am 27.08.2026 hereingefallen, als
// die Vorschau des Pflicht-Riegels den italienischen Block auf Deutsch
// zeigte und ich den Fehler zuerst in der Komponente suchte.
import itCommon from "../locales/it/common.json";

if (!i18next.isInitialized) {
  void i18next.use(initReactI18next).init({
    resources: {
      de: { common: deCommon },
      en: { common: enCommon },
      it: { common: itCommon },
    },
    lng: "de",
    fallbackLng: "de",
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
}
