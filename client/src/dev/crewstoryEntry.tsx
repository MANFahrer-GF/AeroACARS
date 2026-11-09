// Vorschau des Crew-Story-Fensters (vite.crewstory.config.mjs): echte Buchungsliste mit
// gestellten `invoke`-Antworten. Parameter: ?fall=... &sprache=de|en|it &theme=dark|light &laufend=1
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import de from "../../../bundle/texte/de.json";
import en from "../../../bundle/texte/en.json";
import it from "../../../bundle/texte/it.json";
import "../App.css";
import { BidsList } from "../components/BidsList";

const q = new URLSearchParams(window.location.search);
document.documentElement.dataset.theme = q.get("theme") ?? "dark";
void i18n.use(initReactI18next).init({
  lng: q.get("sprache") ?? "de",
  resources: { de: { common: de }, en: { common: en }, it: { common: it } },
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

const laufend = q.has("laufend");
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <main style={{ padding: 16 }}>
      <BidsList baseUrl="https://german-sky-group.eu" simState="connected" simSnapshot={null} hasActiveFlight={laufend} activeBidId={laufend ? 11 : null} laufendenFlugZeigen={laufend} />
    </main>
  </StrictMode>,
);
