// Druckvorschau des Landungs-Berichts — als eigene Seite.
//
// # Warum es diese Datei gibt
//
// Der Bericht war bisher nur über `window.print()` aus der laufenden App
// erreichbar. Damit liess sich nichts nachmessen: Wie gross eine Schrift
// auf dem Papier wirklich wird, hängt an Seitenrand, Spaltenbreite und
// viewBox — und das ergibt sich erst beim Drucken.
//
// Diese Seite rendert denselben Bericht in eine eigene HTML-Datei. Chrome
// druckt sie kopflos in ein PDF, und das PDF lässt sich vermessen:
//
//   node scripts/bericht-bauen.mjs                 # HTML
//   node scripts/bericht-drucken.mjs               # HTML -> PDF -> Messung
//
// Damit ist „ist das lesbar?" keine Schätzung mehr.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { MOCK_LANDING_OPTIONS } from "./mockLandingRecords";
import { LandingReport, type LandingRecord } from "../components/LandingPanel";
import "../App.css";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } },
  lng: "de",
  fallbackLng: "de",
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

/** Welche Landung gezeigt wird — `?variante=d_kante`. */
function varianteAusAdresse(): string {
  return (
    new URLSearchParams(window.location.search).get("variante") ?? "d_kante"
  );
}

function Seite() {
  const key = varianteAusAdresse();
  const opt =
    MOCK_LANDING_OPTIONS.find((o) => o.key === key) ?? MOCK_LANDING_OPTIONS[0];
  return <LandingReport record={opt.build() as unknown as LandingRecord} />;
}

// Die App hängt den Bericht per `createPortal` NEBEN `#root` an
// `document.body`, in ein `<div class="landing-report-print">`. Das
// Druck-CSS blendet `body > #root` komplett aus (sonst erzeugte die lange
// Detailansicht acht leere Seiten VOR dem Bericht). Wer die Vorschau in
// `#root` rendert, druckt deshalb ein leeres Blatt — genau das ist beim
// ersten Versuch passiert, 1253 Byte PDF.
const behaelter = document.createElement("div");
behaelter.className = "landing-report-print";
document.body.appendChild(behaelter);

createRoot(behaelter).render(
  <StrictMode>
    <Seite />
  </StrictMode>,
);
