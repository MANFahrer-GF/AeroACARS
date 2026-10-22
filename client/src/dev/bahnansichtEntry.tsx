// VORSCHAU (nur Entwicklung, nicht eingecheckt): Bahnansicht 2.1.8 an echten
// Landungen. `?fall=1936&theme=dark&breite=390&druck=1`
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createPortal } from "react-dom";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { RunwayDiagramV2 } from "../components/RunwayDiagramV2";
import { mapTouchdownPayloadToV2Props } from "./bahnansicht/payloadMapper";
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

const FAELLE = import.meta.glob("./bahnansicht/fixtures/*.json", { eager: true, import: "default" }) as Record<string, Record<string, unknown>>;
// Typ wie die Webapp ihn aus flight_sessions ergänzt (live gelesen).
const TYP: Record<string, string> = { "1936": "A320" };
const q = new URLSearchParams(window.location.search);
const fall = q.get("fall") ?? "1936";
applyTheme(q.get("theme") === "light" ? "light" : "dark");
// App.css zentriert den Body senkrecht — hier oben anfangen.
document.body.style.display = "block";
document.body.style.placeItems = "initial";
const pl = FAELLE[`./bahnansicht/fixtures/${fall}.json`];
const roh = pl ? mapTouchdownPayloadToV2Props(pl, Array.isArray(pl.sub_scores) ? (pl.sub_scores as never) : null) : null;
const props = roh ? { ...roh, aircraft_icao: roh.aircraft_icao ?? TYP[fall] ?? null } : null;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div className="landing-panel" style={{ maxWidth: q.get("breite") ? Number(q.get("breite")) : 1100, margin: q.get("breite") ? 0 : "0 auto", padding: q.get("breite") ? 8 : 12, boxSizing: "border-box" }}>
      <div style={{ fontFamily: "monospace", fontSize: 13, opacity: 0.8, marginBottom: 8 }}>
        #{fall} · {String(pl?.flight_number ?? "")} · {String(pl?.runway_match_icao ?? "")} {String(pl?.runway_match_ident ?? "")} · Bahnansicht 2.1.8
      </div>
      {props && q.has("druck") ? (
        createPortal(
          <div className="landing-report landing-report-print">
            <div className="report-bahn-quer">
              <div className="report-chart-card">
                <RunwayDiagramV2 {...props} schriftMindest={11} />
              </div>
            </div>
          </div>,
          document.body,
        )
      ) : props ? (
        <RunwayDiagramV2 {...props} />
      ) : (
        <div>kein Mapping</div>
      )}
    </div>
  </StrictMode>,
);
