// Demo der Ansage-Seite und des Einstellungsabschnitts mit IPC-Attrappe
// (vite.kabine.config.mjs ersetzt ../lib/ipc durch kabineFakeIpc.ts).
import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import "../App.css";
import { KabinenSeite } from "../components/kabine/KabinenSeite";
import { KabinenSettingsPanel } from "../components/KabinenSettingsPanel";
import { wiedergabeStarten } from "../lib/kabinenansagen";
import { senden } from "./kabineFakeIpc";

void i18n.use(initReactI18next).init({
  lng: "de",
  resources: { de: { common: deCommon } },
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});
wiedergabeStarten();

function Demo() {
  const [dunkel, setDunkel] = useState(true);
  document.documentElement.dataset.theme = dunkel ? "dark" : "light";
  const ansage = () =>
    senden("kabine-ansage", {
      slot: "kapitaen_sinkflug",
      rolle: "kapitaen",
      zeit: Date.now() / 1000,
      vorschau: false,
      ergebnis: null,
      teile: [
        { sprache: "de-DE", text: "Liebe Gäste, hier spricht noch einmal Ihr Kapitän. Wir beginnen in wenigen Minuten mit dem Sinkflug nach Toulouse. Dort ist es zwanzig Uhr fünfunddreißig Ortszeit, leichter Regen bei zwölf Grad.", datei: "lang-de.wav", dauer_s: 6 },
        { sprache: "en-GB", text: "Ladies and gentlemen, this is your captain again. We will begin our descent into Toulouse shortly. Local time is eight thirty-five in the evening, light rain and twelve degrees.", datei: "lang-en.wav", dauer_s: 6 },
      ],
    });
  return (
    <div style={{ background: "var(--bg)", minHeight: "100vh", color: "var(--text)" }}>
      <div style={{ display: "flex", gap: 8, padding: "12px 24px" }}>
        <button type="button" className="button button--primary" onClick={ansage}>Ansage abspielen</button>
        <button type="button" className="button button--ghost" onClick={() => setDunkel(!dunkel)}>{dunkel ? "Hell" : "Dunkel"}</button>
      </div>
      <KabinenSeite onOpenSettings={() => undefined} />
      <div style={{ maxWidth: 920, margin: "0 auto", padding: "0 24px 40px" }} className="settings">
        <KabinenSettingsPanel />
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Demo />
  </StrictMode>,
);
