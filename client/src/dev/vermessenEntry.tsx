// Vorschau „Flugzeug vermessen" mit nachgespieltem Simulator.
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { FlugzeugVermessen } from "../components/vermessen/FlugzeugVermessen";
import { applyTheme } from "../theme";
import "../App.css";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } }, lng: "de", fallbackLng: "de",
  ns: ["common"], defaultNS: "common", interpolation: { escapeValue: false },
});
applyTheme(new URLSearchParams(window.location.search).has("hell") ? "light" : "dark");
createRoot(document.getElementById("root")!).render(
  <div style={{ maxWidth: 820, margin: "0 auto", padding: 20 }}>
    <FlugzeugVermessen />
  </div>,
);
