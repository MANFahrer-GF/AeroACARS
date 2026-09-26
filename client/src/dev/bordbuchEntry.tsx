// Vorschau des Bordbuchs mit Beispieldaten — die echten Komponenten, im
// Browser gebündelt (wie demoEntry.tsx). Zum Anschauen vor der Abnahme:
// Checkliste, Flugprofil, Routine, Hinweis, laufender Flug.
//   npx vite --port 1430  →  /src/dev/bordbuch.html  (?hell für das helle Thema)

import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import itCommon from "../locales/it/common.json";
import "../App.css";
import "../components/bordbuch/bordbuch.css";
import { BordbuchCheckliste } from "../components/bordbuch/BordbuchCheckliste";
import { BordbuchProfil } from "../components/bordbuch/BordbuchProfil";
import { BordbuchRoutineAnzeige } from "../components/bordbuch/BordbuchRoutine";
import { bilanz, type Eintrag, type Punkt, type Regel, type Schalter } from "../lib/bordbuch";

const sprache = new URLSearchParams(location.search).get("lang") ?? "de";
void i18n.use(initReactI18next).init({
  resources: { de: { common: deCommon }, en: { common: enCommon }, it: { common: itCommon } },
  lng: sprache,
  fallbackLng: "de",
  defaultNS: "common",
  interpolation: { escapeValue: false },
});
if (new URLSearchParams(location.search).has("hell")) document.documentElement.dataset.theme = "light";

const T0 = Date.parse("2026-09-26T09:30:00Z");
const um = (min: number) => new Date(T0 + min * 60_000).toISOString();
const pk = (regel: Regel, schalter: Schalter, abschnitt: Punkt["abschnitt"], status: Punkt["status"], min: number, hoehe: number, extra: Partial<Punkt> = {}): Punkt => ({
  regel, schalter, abschnitt, art: "pflicht", status, auto_status: status, zeit: um(min), hoehe_ft: hoehe,
  stellung: null, grund: null, markiert_at: null, ...extra,
});

const profil: Array<[number, number]> = [];
for (let m = 0; m <= 140; m += 2) {
  const h = m < 12 ? 364 : m < 35 ? 364 + (m - 12) * 1550 : m < 105 ? 36000 : m < 132 ? 36000 - (m - 105) * 1320 : 30;
  profil.push([Math.round((T0 + m * 60_000) / 1000), Math.round(h)]);
}

const flug: Eintrag = {
  schema: 1, pirep_id: "X", erstellt_at: um(0), updated_at: um(150), client_version: "1.9.0",
  flug: { callsign: "DLH1156", dep: "EDDF", arr: "LEPA", muster: "A20N", titel: "Fenix A320" },
  klasse: "airliner", klasse_quelle: "profil", regelwerk: "ifr", nacht_start: false, nacht_landung: false, zeitquelle: "sim",
  eingeschaltet: ["beacon", "strobes", "nav", "transponder", "landelicht", "rolltempo", "parkbremse", "apu", "autobrake", "spoiler", "anschnallzeichen", "klappen"],
  aus_grund: null,
  punkte: [
    pk("beacon_anlassen", "beacon", "vor_dem_rollen", "erledigt", 11, 364),
    pk("nav_lichter", "nav", "vor_dem_rollen", "erledigt", 13, 364),
    pk("parkbremse_geloest", "parkbremse", "vor_dem_rollen", "erledigt", 13, 364),
    pk("rolltempo_abflug", "rolltempo", "rollen", "erledigt", 21, 364, { stellung: "max 24 kt" }),
    pk("strobes_start", "strobes", "start", "erledigt", 21, 364, { stellung: "ON" }),
    pk("landelicht_start", "landelicht", "start", "erledigt", 21, 364),
    pk("transponder_start", "transponder", "start", "erledigt", 21, 364, { stellung: "TA-RA" }),
    pk("tcas_start", "transponder", "start", "erledigt", 21, 364, { stellung: "TA-RA" }),
    pk("klappen_start", "klappen", "start", "erledigt", 21, 364, { stellung: "1" }),
    pk("anschnall_start", "anschnallzeichen", "start", "erledigt", 21, 364, { stellung: "ON" }),
    pk("apu_reiseflug", "apu", "reiseflug", "erledigt", 37, 36000, { art: "bestaetigung" }),
    pk("landelicht_anflug", "landelicht", "anflug", "diesmal_ohne", 124, 9800),
    pk("autobrake_landung", "autobrake", "anflug", "erledigt", 136, 1100, { art: "bestaetigung", stellung: "LO" }),
    pk("spoiler_landung", "spoiler", "anflug", "nicht_messbar", 137, 30, { grund: "wert_fehlt" }),
    pk("anschnall_landung", "anschnallzeichen", "anflug", "nach_atc", 136, 1100),
    pk("rolltempo_ankunft", "rolltempo", "nach_der_landung", "erledigt", 142, 30, { stellung: "max 19 kt" }),
  ],
  rollen_max_abflug_kt: 24, rollen_max_ankunft_kt: 19, rolltempo_grenze_kt: 30, profil,
};

const routine: Eintrag[] = Array.from({ length: 12 }, (_, i) => {
  const e = structuredClone(flug);
  e.pirep_id = `R${i}`;
  e.erstellt_at = new Date(T0 - (12 - i) * 86_400_000).toISOString();
  for (const p of e.punkte) {
    if (p.regel === "landelicht_anflug") p.status = i % 3 === 0 ? "diesmal_ohne" : "erledigt";
    if (p.regel === "strobes_start") p.status = i < 4 ? "diesmal_ohne" : "erledigt";
    if (p.regel === "rolltempo_abflug") p.status = i === 8 ? "diesmal_ohne" : "erledigt";
    if (p.regel === "spoiler_landung") p.status = "nicht_messbar";
  }
  return e;
});

function Seite() {
  const [e, setE] = useState(flug);
  const b = bilanz(e.punkte, e.eingeschaltet);
  const markieren = async (regel: Regel, nachAtc: boolean) => {
    setE((alt) => ({ ...alt, punkte: alt.punkte.map((p) => (p.regel === regel ? { ...p, status: nachAtc ? "nach_atc" : "diesmal_ohne" } : p)) }));
  };
  return (
    <div className="app" style={{ background: "var(--surface)", color: "var(--text)", minHeight: "100vh" }}>
      <section className="bb-seite">
        <header className="bb-kopf">
          <div>
            <h2>DLH1156 <span className="bb-route">EDDF–LEPA</span></h2>
            <div className="bb-unter">26.09.26 · A20N · Airliner · IFR</div>
          </div>
          <div className="bb-bilanz">
            <span className="bb-bilanz-zahl">{b.ok}</span>
            <span className="bb-bilanz-text">{i18n.t("bordbuch.bilanz", b)}<span className="bb-bilanz-satz">{i18n.t(b.ok === b.von ? "bordbuch.satz_alles" : "bordbuch.satz_teil")}</span></span>
          </div>
        </header>
        <h3 style={{ margin: 0 }}>A · Checkliste</h3>
        <BordbuchCheckliste punkte={e.punkte} eingeschaltet={e.eingeschaltet} flugzeug="Fenix A320" onMarkieren={markieren} />
        <h3 style={{ margin: 0 }}>B · Flugprofil</h3>
        <BordbuchProfil punkte={e.punkte} eingeschaltet={e.eingeschaltet} profil={e.profil} dep="EDDF" arr="LEPA" />
        <h3 style={{ margin: 0 }}>C · Routine (im Logbuch)</h3>
        <BordbuchRoutineAnzeige eintraege={routine} eingeschaltet={["beacon", "strobes", "nav", "transponder", "landelicht", "rolltempo", "spoiler"]} />
        <h3 style={{ margin: 0 }}>Hinweis im Cockpit</h3>
        <div className="bb-hinweis-leiste"><span className="bb-hinweis-frage"><span className="bb-hinweis-marke">{i18n.t("bordbuch.titel")}</span>{i18n.t("bordbuch.frage.strobes_start")}</span></div>
      </section>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Seite />
  </StrictMode>,
);
