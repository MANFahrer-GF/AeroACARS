// METAR am Zielflughafen: Rohtext und Auswertung.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 nur in der Webapp (LandingAnalysis.tsx, decodeMetar) — der
// Client hatte den METAR nicht in seiner Aufzeichnung. Auswertung
// unverändert übernommen, Texte jetzt über t().

import { useTranslation } from "react-i18next";
import "./metarAbschnitt.css";
import { InfoBadge } from "./InfoBadge";

type Uebersetzer = (k: string, o?: Record<string, unknown>) => string;

const WETTER = ["DZ", "RA", "SN", "SG", "BR", "FG", "FU", "HZ", "TS", "SH", "FZ", "VC"] as const;

function wetterText(t: Uebersetzer, code: string): string {
  // Intensität vorweg, dann Zweiergruppen (z. B. „-SHRA" → leicht Schauer
  // Regen) — jede Gruppe für sich übersetzt und mit Leerzeichen getrennt.
  // Kein Weitersuchen im übersetzten Text (Endlosschleife, Abnahme 06.10.2026).
  let rest = code;
  const teile: string[] = [];
  if (rest.startsWith("+")) { teile.push(t("landing.metar.stark")); rest = rest.slice(1); }
  else if (rest.startsWith("-")) { teile.push(t("landing.metar.leicht")); rest = rest.slice(1); }
  for (let i = 0; i < rest.length; i += 2) {
    const paar = rest.slice(i, i + 2);
    teile.push((WETTER as readonly string[]).includes(paar) ? t(`landing.metar.wx.${paar}`) : paar);
  }
  return teile.join(" ");
}

/** Auswertung eines METAR (Wind, Sicht, Wetter, Wolken, Temperatur/Taupunkt, QNH). */
export function metarAuswertung(t: Uebersetzer, raw: string): { k: string; v: string; id?: string }[] {
  const out: { k: string; v: string; id?: string }[] = [];
  // Wind: 09020KT, 09015G25KT oder VRB03KT
  const wind = raw.match(/(VRB|\d{3})(\d{2,3})(?:G(\d{2,3}))?KT/);
  if (wind) {
    const dir = wind[1] === "VRB" ? t("landing.metar.variabel") : `${wind[1]}°`;
    const gust = wind[3] ? ` (${t("landing.metar.boeen", { kt: wind[3] })})` : "";
    out.push({ id: "wind", k: t("landing.metar.wind"), v: `${dir} / ${wind[2]} kt${gust}` });
  }
  // Sicht: 3700 oder 9999
  const vis = raw.match(/\s(\d{4})\s/);
  // US-METAR: Sicht in Meilen („10SM", „1 1/2SM", „1/4SM", „P6SM").
  const visSm = raw.match(/\s(P?\d+(?: \d\/\d)?|\d\/\d)SM\s/);
  if (vis) {
    const m = parseInt(vis[1]!, 10);
    out.push({
      id: "sicht",
      k: t("landing.metar.sicht"),
      v: m >= 9999 ? "≥ 10 km" : m >= 1000 ? `${(m / 1000).toFixed(1)} km` : `${m} m`,
    });
  } else if (visSm) {
    const roh = visSm[1]!;
    out.push({ id: "sicht", k: t("landing.metar.sicht"), v: roh.startsWith("P") ? `> ${roh.slice(1)} SM` : `${roh} SM` });
  }
  const wx = raw.match(/\s([+-]?(?:VC)?(?:DZ|RA|SN|SG|IC|PL|GR|GS|UP|BR|FG|FU|VA|DU|SA|HZ|PY|TS|SH|FZ){1,3})\s/);
  if (wx) out.push({ id: "wetter", k: t("landing.metar.wetter"), v: wetterText(t, wx[1]!) });
  // Wolken: FEW015, SCT030, BKN030, OVC017 (mehrere)
  const wolken: string[] = [];
  raw.replace(/(FEW|SCT|BKN|OVC)(\d{3})/g, (_, deckung: string, h: string) => {
    wolken.push(`${t(`landing.metar.wolke.${deckung}`)} ${parseInt(h, 10) * 100} ft`);
    return "";
  });
  if (wolken.length) out.push({ id: "wolken", k: t("landing.metar.wolken"), v: wolken.join(", ") });
  // Temperatur/Taupunkt: 06/03 oder M03/M05
  const td = raw.match(/\s(M?\d{2})\/(M?\d{2})\s/);
  if (td) {
    const temp = td[1]!.startsWith("M") ? -parseInt(td[1]!.slice(1), 10) : parseInt(td[1]!, 10);
    const tau = td[2]!.startsWith("M") ? -parseInt(td[2]!.slice(1), 10) : parseInt(td[2]!, 10);
    out.push({ id: "temp_tau", k: t("landing.metar.temp_tau"), v: `${temp}°C / ${tau}°C` });
  }
  // QNH: Q1004 (hPa) oder A2992 (inHg)
  const q = raw.match(/\bQ(\d{4})\b/);
  const a = raw.match(/\bA(\d{4})\b/);
  if (q) {
    const hpa = parseInt(q[1]!, 10);
    out.push({ id: "qnh", k: "QNH", v: `${hpa} hPa / ${(hpa * 0.0295299830714).toFixed(2)} inHg` });
  } else if (a) {
    const inHg = parseInt(a[1]!, 10) / 100;
    const hpa = Math.round(inHg / 0.0295299830714);
    out.push({ id: "qnh", k: "QNH", v: `${hpa} hPa / ${inHg.toFixed(2)} inHg` });
  }
  return out;
}

export function MetarAbschnitt({ metar }: { metar: string | null | undefined }) {
  const { t } = useTranslation();
  if (!metar) return null;
  return (
    <section className="landing-section">
      <h3>{t("landing.metar.title")}{" "}<InfoBadge explanation={t("landing.erklaer.metar.titel")} /></h3>
      <pre className="metar-roh">{metar}</pre>
      <div className="metar-auswertung">
        {metarAuswertung(t, metar).map((d, i) => (
          <div key={i} className="metar-eintrag">
            <span className="metar-eintrag__k">
              {d.k}
              {d.id && <InfoBadge explanation={t(`landing.erklaer.metar.${d.id}`)} />}
            </span>
            <span className="metar-eintrag__v">{d.v}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
