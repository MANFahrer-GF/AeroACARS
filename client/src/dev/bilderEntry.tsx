// Vorschau des Bilder-Tabs (vite.bilder.config.mjs): Seitenleiste und Tab mit gestellten
// `invoke`-Antworten. Parameter: ?fall=... &sprache=de|en|it &theme=dark|light
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import de from "../../../bundle/texte/de.json";
import en from "../../../bundle/texte/en.json";
import it from "../../../bundle/texte/it.json";
import "../App.css";
import { Sidebar } from "../components/Sidebar";
import { BilderTab } from "../components/bilder/BilderTab";
import { BilderHintergrund } from "../components/bilder/BilderHintergrund";
import FlugkarteFenster from "../flugkarte/FlugkarteFenster";

const q = new URLSearchParams(window.location.search);
const sprache = q.get("sprache") ?? "de";
document.documentElement.dataset.theme = q.get("theme") ?? "dark";
void i18n.use(initReactI18next).init({
  lng: sprache,
  resources: { de: { common: de }, en: { common: en }, it: { common: it } },
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

const fall = q.get("fall") ?? "normal";
const imFlug = !["ohneflug", "leer", "leerein"].includes(fall);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div className="shell" data-nav={q.get("nav") === "icon" ? "icon" : "wide"} style={{ height: "100vh" }}>
      <Sidebar
        tab="bilder" setTab={() => undefined} collapsed={q.get("nav") === "icon"} onToggleCollapsed={() => undefined}
        cpdlcEnabled={false} cpdlcPendingCount={0} onCpdlcOpen={() => undefined} chatAn={true} chatUngelesen={0} onChatOpen={() => undefined}
        kabineAn={true} bilderAn={true} unreadNews={0} hasActiveFlight={imFlug} phpvmsConnected={true} simConnected={true} simConnecting={false} simLabel="X-Plane 12"
      />
      <main className="app">
        <BilderHintergrund />
        <BilderTab
          pilotId="17"
          pilotName="Thomas K."
          aktiverFlug={imFlug ? { pirep_id: "P412", callsign: "GSG412", dpt: "EDDF", arr: "LSZH" } : null}
          simKind={q.get("sim") === "msfs" ? "msfs2024" : "xplane12"}
          onKarteOeffnen={(f) => { (window as unknown as { __karteGeoeffnet: unknown }).__karteGeoeffnet = f; }}
        />
      </main>
      {q.get("flugkarte") === "1" && <FlugkarteFenster pirepId="P412" onClose={() => undefined} />}
    </div>
  </StrictMode>,
);
