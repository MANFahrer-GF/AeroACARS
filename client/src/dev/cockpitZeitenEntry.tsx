// Vorschau „Zeiten gegen Flugplan" im Cockpit (v2.0.20): die ECHTEN
// Komponenten FahrplanBand und InfoStrip mit Beispieldaten in allen Lagen.
// `?hell` für das helle Thema. Aufruf: npx vite --config vite.demo.config.mjs
// und /cockpitZeiten.html.
import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { FahrplanBand } from "../components/FahrplanBand";
import { InfoStrip } from "../components/InfoStrip";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import type { FlugZeiten } from "../lib/flugzeiten";
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
applyTheme(new URLSearchParams(location.search).get("hell") != null ? "light" : "dark");

// Alle Zeiten relativ zu „jetzt", damit die Vorschau zu jeder Uhrzeit stimmt.
const JETZT = Math.floor(Date.now() / 1000);
const m = (min: number) => JETZT + min * 60;
const PLAN: FlugZeiten = {
  plan_off_block: m(-95),
  plan_abheben: m(-80),
  plan_aufsetzen: m(70),
  plan_on_block: m(78),
  ofp_erstellt: m(-170),
};
const snapshot = {
  fuel_total_kg: 14820,
  total_weight_kg: 196400,
  zfw_kg: 181580,
  empty_weight_kg: 142300,
  mach: 0.82,
  true_airspeed_kt: 472,
  wind_direction_deg: 285,
  wind_speed_kt: 38,
  qnh_hpa: 1013,
  outside_air_temp_c: -52,
  total_air_temp_c: -21,
} as unknown as SimSnapshot;

function flug(flugzeiten: FlugZeiten | null, landung_erwartet?: number, extra?: object) {
  return {
    distance_nm: 612.4,
    position_count: 1841,
    touch_and_go_count: 0,
    go_around_count: 0,
    flugzeiten,
    landung_erwartet,
    ...extra,
  } as unknown as ActiveFlightInfo;
}

function Lage({ titel, info }: { titel: string; info: ActiveFlightInfo }) {
  return (
    <section style={{ marginBottom: 26 }}>
      <h2 style={{ font: "600 14px system-ui", color: "var(--text)", margin: "0 0 8px" }}>{titel}</h2>
      <FahrplanBand info={info} />
      <div className="grid4" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
        <InfoStrip info={info} snapshot={snapshot} elapsedMinutes={88} />
      </div>
    </section>
  );
}

function Seite(): ReactNode {
  return (
    <div style={{ maxWidth: 1240, margin: "0 auto", padding: "18px 18px 30px", background: "var(--bg)" }}>
      <Lage
        titel="Reiseflug, 22 min hinter Plan"
        info={flug({ ...PLAN, off_block: m(-79), abheben: m(-56) }, m(92))}
      />
      <Lage
        titel="Reiseflug, pünktlich (bis 15 min ohne Farbe)"
        info={flug({ ...PLAN, off_block: m(-88), abheben: m(-69) }, m(73))}
      />
      <Lage
        titel="Am Gate, Off-Block in 12 min"
        info={flug({ ...PLAN, plan_off_block: m(12), plan_abheben: m(27), plan_aufsetzen: m(177), plan_on_block: m(185) })}
      />
      <Lage
        titel="Am Gate, Off-Block 24 min überfällig"
        info={flug({ ...PLAN, plan_off_block: m(-24), plan_abheben: m(-9), plan_aufsetzen: m(141), plan_on_block: m(149) })}
      />
      <Lage titel="Ohne SimBrief-Plan, mit Durchstart" info={flug(null, undefined, { go_around_count: 1 })} />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Seite />
  </StrictMode>,
);
