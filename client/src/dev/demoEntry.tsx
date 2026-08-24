// Einstiegspunkt der bedienbaren Demo.
//
// # Warum es diese Datei gibt
//
// Die Demo entstand bisher über `renderToStaticMarkup` — reines HTML ohne
// eine Zeile JavaScript. Für „sieht die Grafik richtig aus?" reicht das,
// und dafür war sie gedacht. Nur ist sie danach zur Abnahmestufe geworden,
// und die Grafik hat inzwischen Bedienung: Zoom mit Strg + Mausrad,
// Zoomknöpfe, Ziehen zum Verschieben, das Glossar.
//
// In der statischen Fassung tut davon **nichts** etwas. Es sieht nicht
// kaputt aus — es sieht aus, als wäre die Bedienung nicht gebaut. Genau so
// kam es beim Abnehmen an: „drücken passiert nix".
//
// Diese Datei wird mit Vite gebündelt (`npm run demo`), also mit React im
// Browser. Was hier läuft, läuft im Client genauso.
//
// # Warum alle Varianten untereinander
//
// Die Abnahme vergleicht sie: Ein Fehler, der in einer Variante unsichtbar
// ist, fällt in der nächsten auf. Ein Auswahlfeld würde genau das
// verhindern.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { MOCK_LANDING_OPTIONS } from "./mockLandingRecords";
import { mapLandingRecordToV2Props } from "./runwayDiagramV2Mapper";
import { RunwayDiagramV2 } from "../components/RunwayDiagramV2";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } },
  lng: "de",
  fallbackLng: "de",
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

/**
 * Druck-Untergrenze über `?mindest=17` — die Fassung, die im PDF landet.
 *
 * Im Bericht skaliert das SVG auf die A4-Spalte, und jede Schrift darin
 * schrumpft mit (gemessen 3,6–4,4 pt). Der Bericht setzt deshalb einen
 * Schriftmaßstab. Ob die grössere Schrift sich überlappt, kann kein
 * jsdom-Test beantworten — dafür braucht es einen echten SVG-Motor.
 * Hier ist er.
 */
function mindestAusAdresse(): number {
  const p = new URLSearchParams(window.location.search).get("mindest");
  const n = p == null ? NaN : Number(p);
  return Number.isFinite(n) && n > 0 ? n : 0;
}

function Demo() {
  const mindest = mindestAusAdresse();
  return (
    <>
      <h1>Bahndisziplin — die Varianten aus §11</h1>
      {mindest > 0 && (
        <p className="hinweis">
          <strong>Druck-Untergrenze {mindest} Einheiten</strong> — so erscheint die Grafik im
          PDF-Bericht. <a href="?">zurück auf Bildschirmgröße</a>
        </p>
      )}
      <p className="lead">
        Gerendert aus denselben Bausteinen, die im Client laufen — und seit
        dem Abgleich auch in der Webapp. Die Spuren sind echte
        Aufzeichnungen aus dem Korpus, keine gezeichneten Linien.
      </p>
      <p className="hinweis">
        <strong>Zoom:</strong> Strg (oder ⌘) + Mausrad über der Grafik, oder
        die Knöpfe <code>+</code> / <code>−</code>. Im gezoomten Zustand
        lässt sich mit gedrückter Maustaste verschieben. Ohne Strg scrollt
        das Rad die Seite — sonst bliebe man über der Grafik hängen.
      </p>
      {MOCK_LANDING_OPTIONS.map((opt) => {
        const roh = mapLandingRecordToV2Props(opt.build());
        const props = roh ? { ...roh, schriftMindest: mindest } : roh;
        return (
          <section key={opt.key} className="variante">
            <h2>
              {opt.label}
              <span className="schluessel">{opt.key}</span>
            </h2>
            <p className="hint">{opt.hint}</p>
            {props ? (
              <RunwayDiagramV2 {...props} />
            ) : (
              <p className="fehlt">
                Kein Bahn-Treffer — die Grafik entfällt sichtbar, statt eine
                leere Bahn zu zeichnen.
              </p>
            )}
          </section>
        );
      })}
    </>
  );
}

const wurzel = document.getElementById("root");
if (!wurzel) throw new Error("kein #root — index.html der Demo prüfen");
createRoot(wurzel).render(
  <StrictMode>
    <Demo />
  </StrictMode>,
);
