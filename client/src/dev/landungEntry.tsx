// Vorschau der Landungs-Detailansicht mit Bordbuch-Abschnitt (27.09.2026,
// Thomas: „erstmal eine Vorschau sehen"). Echte Komponente, nur die IPC
// des Bordbuchs ist ersetzt (vite.landung.config.mjs).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { MOCK_LANDING_OPTIONS } from "./mockLandingRecords";
import { LandingDetail, type LandingRecord } from "../components/LandingPanel";
import { applyTheme } from "../theme";
import "../App.css";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } },
  lng: "de",
  fallbackLng: "de",
  ns: ["common"],
  defaultNS: "common",
  interpolation: { escapeValue: false },
});
applyTheme("dark");

const opt = MOCK_LANDING_OPTIONS.find((o) => o.key === "d_kante") ?? MOCK_LANDING_OPTIONS[0]!;
const record = { ...(opt.build() as unknown as LandingRecord), pirep_id: "BEISPIEL" };

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div className="landing-panel" style={{ maxWidth: 1100, margin: "0 auto", padding: 16 }}>
      <LandingDetail record={record} allRecords={[record]} onBack={() => undefined} isPreview={false} />
    </div>
  </StrictMode>,
);
