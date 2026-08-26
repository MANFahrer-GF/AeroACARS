// Erzeugt Vorschauseiten des Pflicht-Riegels zum Anschauen.
//
// Kein Nachbau: gerendert wird die echte Komponente mit den echten
// Texten aus den Sprachdateien; das Stylesheet ist App.css von der
// Platte. Was fehlt, ist nur die App drumherum.
//
// Je Theme eine eigene Datei, weil die Farbwerte an `:root[data-theme]`
// haengen — zwei Themes auf einer Seite gaebe es nur ueber ein Umbiegen
// der Regeln, und dann zeigte die Vorschau etwas anderes als die App.
//
// Laeuft nur mit VORSCHAU=1, damit der normale Testlauf nichts schreibt.
import { describe, it, beforeAll } from "vitest";
import { render } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { UpdateGate } from "./UpdateGate";
import type { UseUpdateCheckerResult } from "../hooks/useUpdateChecker";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import itCommon from "../locales/it/common.json";

const AN = process.env.VORSCHAU === "1";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      fallbackLng: "de",
      resources: {
        de: { common: deCommon },
        en: { common: enCommon },
        it: { common: itCommon },
      },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

function basis(u: Partial<UseUpdateCheckerResult> = {}): UseUpdateCheckerResult {
  return {
    update: { version: "1.7.7", body: "" } as UseUpdateCheckerResult["update"],
    stage: "fresh",
    installing: false,
    progress: null,
    snoozeBanner: () => {},
    bannerSnoozed: false,
    installAndRelaunch: async () => {},
    pflichtUpdate: true,
    installationGescheitert: false,
    ...u,
  };
}

const FORTSCHRITT: Record<string, string> = {
  de: "Download: 24,8 / 61,3 MB",
  en: "Download: 24.8 / 61.3 MB",
  it: "Download: 24,8 / 61,3 MB",
};

describe.runIf(AN)("Vorschau", () => {
  it("schreibt beide Themes, drei Sprachen, drei Zustaende", async () => {
    const css = readFileSync(resolve(__dirname, "../App.css"), "utf-8");
    const sprachen = ["de", "en", "it"] as const;

    for (const theme of ["light", "dark"] as const) {
      const zeilen: string[] = [];
      for (const sprache of sprachen) {
        await i18next.changeLanguage(sprache);
        // Ein stiller Sprachrueckfall macht die Vorschau zur Luege: Sie
        // zeigt dann deutschen Text unter italienischer Beschriftung.
        // Deshalb hier gepruerft, nicht angenommen.
        const probe = i18next.t("update.gate_install");
        if (sprache !== "de" && probe === deCommon.update.gate_install) {
          throw new Error(
            `Sprache ${sprache} faellt auf Deutsch zurueck — fehlt sie in den Test-Ressourcen?`,
          );
        }
        const zustaende = [
          { k: "Start", props: basis() },
          {
            k: "Installation laeuft",
            props: basis({ installing: true, progress: FORTSCHRITT[sprache] }),
          },
          {
            k: "gescheitert",
            props: basis({
              installationGescheitert: true,
              progress:
                "Fehler: failed to write to target: permission denied",
            }),
          },
        ];
        const zellen = zustaende.map((z) => {
          const { container, unmount } = render(
            <UpdateGate checker={z.props} activePhase={null} />,
          );
          const html = container.innerHTML;
          unmount();
          return `<figure class="z"><figcaption>${sprache.toUpperCase()} · ${z.k}</figcaption><div class="rahmen">${html}</div></figure>`;
        });
        zeilen.push(`<div class="reihe">${zellen.join("")}</div>`);
      }
      await i18next.changeLanguage("de");

      // ⚠ KEINE Backticks in diesem Text — er steht in einem
      // Template-Literal und wuerde es beenden. Genau daran ist der
      // erste Anlauf gescheitert.
      const html = `<!doctype html><html data-theme="${theme}"><meta charset="utf-8">
<title>AeroACARS Pflicht-Riegel — ${theme}</title>
<style>
${css}
body { margin:0; background:var(--bg); font-family:var(--font-sans); padding:18px; }
.reihe { display:block; }
.z { margin:0 0 18px; }
/* ⚠ Direkter Nachfahre. Ohne das trifft die Regel auch die
   Kartenueberschrift (die ist ebenfalls ein h2) und gewinnt ueber sie —
   die Vorschau zeigte dann Kapitaelchen in Grau, die es in der App
   nicht gibt. */
.z > figcaption { color:var(--text-muted); font:600 11px/1.4 var(--font-sans);
  letter-spacing:.12em; text-transform:uppercase; margin:0 0 10px; }
/* Der Riegel ist position:fixed — fuer die Vorschau in einen Rahmen
   sperren, damit alle Zustaende nebeneinander sichtbar bleiben. */
/* Echte Kartenbreite (520 px) — sonst bricht der Text anders um als
   beim Piloten, und die Vorschau zeigt ein Problem, das es nicht gibt. */
.rahmen { position:relative; border-radius:10px; overflow:hidden;
  border:1px solid var(--border); background:var(--bg); padding:20px 0; }
.rahmen .update-gate { position:static; padding:0; background:rgba(8,12,16,0.42); }
.rahmen .update-gate__card { max-height:none; }
</style>
${zeilen.join("\n")}
</html>`;
      writeFileSync(`/tmp/riegel-${theme}.html`, html, "utf-8");
    }
    console.log("geschrieben: /tmp/riegel-light.html, /tmp/riegel-dark.html");
  });
});
