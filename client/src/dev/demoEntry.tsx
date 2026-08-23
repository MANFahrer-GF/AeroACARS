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

function Demo() {
  return (
    <>
      <h1>Bahndisziplin — die Varianten aus §11</h1>
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
        const props = mapLandingRecordToV2Props(opt.build());
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
