// Demo des Pilotenchats im echten Anwendungsrahmen (.shell → .nav + main.app),
// IPC durch chatFakeIpc.ts ersetzt. ?n=200 setzt die Zahl der Nachrichten.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import "../App.css";
import { ChatView } from "../components/ChatView";
import { PaketNeu } from "../components/PaketNeu";
import { sichtbereichHalten } from "../lib/sichtbereich";

void i18n.use(initReactI18next).init({
  lng: "de",
  resources: { de: { common: deCommon } },
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});
document.documentElement.dataset.theme = "dark";
// Wie im LAN-Browser (main.tsx); ?roh zeigt den Stand ohne Abhilfe.
if (!new URLSearchParams(location.search).has("roh")) sichtbereichHalten();

// ?mess blendet die Maße ein, an denen sich der iPad-Fehler zeigt
// (Dokument verschoben, sichtbarer Ausschnitt kleiner als das Fenster).
if (new URLSearchParams(location.search).has("mess")) {
  const tafel = document.createElement("pre");
  tafel.style.cssText = "position:fixed;right:8px;top:8px;z-index:99;margin:0;padding:6px 8px;background:#ff0;color:#000;font:12px monospace";
  document.body.append(tafel);
  setInterval(() => {
    const app = document.querySelector(".app");
    const vv = window.visualViewport;
    tafel.textContent = [
      `scrollY ${Math.round(window.scrollY)}  innerH ${window.innerHeight}`,
      `vv.h ${Math.round(vv?.height ?? 0)}  vv.top ${Math.round(vv?.offsetTop ?? 0)}`,
      `doc ${document.documentElement.scrollHeight}  app.top ${app?.scrollTop}/${app?.scrollHeight}-${app?.clientHeight}`,
    ].join("\n");
  }, 250);
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div className="shell" data-nav="wide">
      <aside className="nav" />
      <main className="app">
        <PaketNeu imFlug={false} />
        <ChatView eigenePilotId="GSG0001" phase="CRUISE" onGelesen={() => undefined} />
      </main>
    </div>
  </StrictMode>,
);
