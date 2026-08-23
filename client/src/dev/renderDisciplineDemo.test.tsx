// Erzeugt eine ansehbare Vorschau aller Bahndisziplin-Varianten.
//
// Kein Test im engeren Sinn, sondern ein **Werkzeug**: Es rendert die
// Pflichtvarianten aus Spec §11 in eine HTML-Datei, die man ohne laufenden
// Simulator und ohne Tauri-Build im Browser ansehen kann.
//
//   cd client && DEMO_OUT=/tmp/bahndisziplin.html npx vitest run \
//     src/dev/renderDisciplineDemo.test.tsx
//
// Warum als vitest-Lauf und nicht als eigenes Skript: Die Komponenten
// brauchen React, jsdom und i18next. Das steht hier bereits eingerichtet —
// ein zweites Setup daneben wäre wieder eine Stelle, die driftet.
//
// Ohne `DEMO_OUT` prüft der Lauf nur, dass jede Variante überhaupt rendert.
// Das ist die eigentliche Zusicherung: Eine Demo-Variante, die abstürzt,
// fällt sonst erst am Abnahmetag auf.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { writeFileSync } from "node:fs";
import { MOCK_LANDING_OPTIONS } from "./mockLandingRecords";
import { mapLandingRecordToV2Props } from "./runwayDiagramV2Mapper";
import { RunwayDiagramV2 } from "../components/RunwayDiagramV2";
// Die ECHTEN Sprachdateien, nicht nur die `defaultValue`-Rückfälle: Ohne
// initialisiertes i18next rendert jedes `t("runway_v2.…")` ohne Vorgabewert
// den rohen Schlüssel — die erste Fassung dieser Demo war voll davon
// („runway_v2.flugzeug_label" statt „Flugzeug"), und weil die Vorgabewerte
// meiner eigenen Bausteine griffen, sah es nach einem halb fertigen Bau aus
// statt nach einem fehlenden Aufruf.
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } },
  lng: "de",
  fallbackLng: "de",
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

describe("Bahndisziplin-Demo", () => {
  it("rendert jede Variante ohne Absturz", () => {
    const bloecke: string[] = [];

    for (const opt of MOCK_LANDING_OPTIONS) {
      const record = opt.build();
      const props = mapLandingRecordToV2Props(record);
      expect(props, `${opt.key}: kein Mapping`).not.toBeNull();

      // Die GANZE Grafik, nicht nur der Disziplin-Block: Laengs- und
      // Queransicht muessen untereinander stehen, sonst laesst sich nicht
      // pruefen, ob sie fluchten -- und genau daran ist der Aim-Marker im
      // ersten Entwurf um 209 m danebengegangen, ohne dass es auffiel.
      // `useV2Skin` faellt ohne Provider auf DEFAULT_SKIN zurueck, deshalb
      // laeuft das hier ohne Tauri und ohne VPS.
      const markup = renderToStaticMarkup(<RunwayDiagramV2 {...props!} />);
      expect(markup.length, `${opt.key}: leer gerendert`).toBeGreaterThan(50);

      bloecke.push(
        `<section>
           <h2>${escape(opt.label)}</h2>
           <p class="hint">${escape(opt.hint)}</p>
           <div class="panel">${markup}</div>
         </section>`,
      );
    }

    const ziel = process.env.DEMO_OUT;
    if (ziel) {
      writeFileSync(ziel, seite(bloecke.join("\n")), "utf-8");
    }
  });
});

function escape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function seite(inhalt: string): string {
  return `<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<title>Bahndisziplin — Varianten</title>
<style>
  :root { color-scheme: dark; }
  body {
    margin: 0; padding: 28px 20px 60px;
    background: #0b1220; color: #e2e8f0;
    font: 15px/1.6 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  h1 { font-size: 1.4rem; margin: 0 0 6px; letter-spacing: -0.01em; }
  .lead { color: #94a3b8; margin: 0 0 32px; max-width: 62ch; }
  section {
    max-width: 1240px; margin: 0 auto 26px; padding: 16px 18px 18px;
    background: #111a2e; border: 1px solid #1e293b; border-radius: 8px;
  }
  h2 { font-size: 1rem; margin: 0 0 4px; font-weight: 600; }
  .hint { color: #94a3b8; font-size: 0.84rem; margin: 0 0 14px; max-width: 78ch; }
  .panel { overflow-x: auto; }
  svg { max-width: 100%; height: auto; }
</style></head><body>
<h1>Bahndisziplin — die Varianten aus §11</h1>
<p class="lead">Gerendert aus denselben Komponenten, die im Client laufen.
Die Nummern in der Grafik verweisen auf die Liste darunter; im Bild steht
kein Text, damit nichts überlappen kann.</p>
${inhalt}
</body></html>`;
}
