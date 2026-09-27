// Vorschau des Wiederaufnahme-Dialogs nach einem Sim-Absturz (SIA 375,
// 27.09.2026). `?sim=1` = Simulator verbunden, `&weg=1` = Position passt nicht.
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { ResumeFlightBanner } from "../components/ResumeFlightBanner";
import type { ActiveFlightInfo } from "../types";
import { applyTheme } from "../theme";
import "../App.css";

void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon } }, lng: "de", fallbackLng: "de",
  ns: ["common"], defaultNS: "common", interpolation: { escapeValue: false },
});
applyTheme("dark");
const q = new URLSearchParams(window.location.search);
const flug = {
  pirep_id: "p", airline_icao: "SIA", flight_number: "375", callsign: "",
  dpt_airport: "WSSS", arr_airport: "EDDM", was_just_resumed: true,
  resume_position_suspect: q.has("weg"),
  last_known_lat: 21.2929, last_known_lon: 86.2577, last_known_alt_ft: 36012,
  last_known_heading_deg: 302, last_known_gs_kt: 488,
  last_known_at: new Date(Date.now() - 4 * 60_000).toISOString(),
  last_known_fuel_kg: 90723, last_known_zfw_kg: 224886, last_known_total_weight_kg: 315608,
  last_known_aircraft_icao: "B77W",
} as unknown as ActiveFlightInfo;
createRoot(document.getElementById("root")!).render(
  <div style={{ maxWidth: 1100, margin: "0 auto", padding: 16 }}>
    <ResumeFlightBanner activeFlight={flug} onAdopted={() => {}} onCancelled={() => {}} />
  </div>,
);
