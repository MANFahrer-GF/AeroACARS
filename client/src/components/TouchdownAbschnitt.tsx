// Touchdown: die Werte im Aufsetz-Moment und der Windkompass.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 zeigten Client („Touchdown") und Webapp („Touchdown & Wind")
// verschiedene Werte: GS, G im Aufsetz-Frame und der rechtweisende Kurs
// fehlten im Client, Hopser in der Webapp; die Webapp zeigte die Querlage
// ohne Vorzeichen. Jetzt ein Code, dieselben Werte.

import { useTranslation } from "react-i18next";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { fensterWerteGueltig, fmtNumber, fmtSigned, scoreG } from "../lib/landungsFormat";
import { scoreBasisVs } from "./SinkrateForensik";
import "./touchdownAbschnitt.css";
import { InfoBadge } from "./InfoBadge";

// v0.12.8-dev: Wind-Visualisierung — animiertes Stromlinien-Feld. Der Wind
// "weht" sichtbar über die Karte: Richtung = echte Anströmrichtung relativ
// zur Landerichtung (Bahn waagerecht, Nase rechts), Tempo + Dichte der
// Streaks = Windstärke, Farbe = Kritikalität (Seitenwind-/Rückenwind-Limit).
// Im Vordergrund die Kennzahlen als Hero-Zahl + Chips.
export function WindCompass({
  headwindKt,
  crosswindKt,
  runwayIdent,
}: {
  headwindKt: number | null;
  crosswindKt: number | null;
  runwayIdent?: string | null;
}) {
  const { t } = useTranslation();
  if (headwindKt == null && crosswindKt == null) return null;
  const hw = headwindKt ?? 0;
  const xw = crosswindKt ?? 0;

  const totalKt = Math.sqrt(hw * hw + xw * xw);
  const isTailwind = hw < -0.5;
  const xwAbs = Math.abs(xw);
  const twAbs = Math.abs(hw);
  const xwFromRight = xw >= 0;
  const calm = totalKt < 1.5;

  // Kritikalität nach Seitenwind-/Rückenwind-Limit — färbt Streaks + Zahl.
  const critColor =
    xwAbs >= 25 || (isTailwind && twAbs >= 10)
      ? "#f87171"
      : xwAbs >= 15 || (isTailwind && twAbs >= 5)
        ? "#fbbf24"
        : "#38bdf8";

  // Anströmrichtung in Screen-Koordinaten. Wind WEHT von der Quelle weg →
  // Travel-Vektor (−hw, −xw). +x = mit der Landerichtung, +y = nach unten.
  const travelDeg = calm ? 0 : (Math.atan2(-xw, -hw) * 180) / Math.PI;

  // Mehr Wind → mehr & schnellere Streaks.
  const streakCount = calm ? 0 : Math.round(Math.min(34, 8 + totalKt * 0.95));
  const durationMs = Math.round(
    Math.min(2400, Math.max(620, 2600 - totalKt * 62)),
  );

  // Deterministisches Pseudo-Feld — kein Flackern bei Re-Render.
  const streaks = Array.from({ length: streakCount }, (_, i) => {
    const rand = ((i * 9301 + 49297) % 233280) / 233280;
    return {
      y: -110 + ((i * 81) % 440),
      len: 26 + rand * 40,
      delay: -(i / Math.max(1, streakCount)) * durationMs,
      thickness: 1.4 + rand * 1.7,
      opacity: 0.3 + rand * 0.42,
    };
  });

  const headLabel = isTailwind
    ? t("landing.wind_tailwind")
    : t("landing.wind_headwind");
  const sideLabel = xwFromRight
    ? t("landing.wind_side_right")
    : t("landing.wind_side_left");

  return (
    <div className="windflow">
      <svg
        className="windflow__field"
        viewBox="0 0 360 200"
        preserveAspectRatio="xMidYMid slice"
        aria-hidden="true"
      >
        <g transform={`rotate(${travelDeg.toFixed(1)} 180 100)`}>
          {streaks.map((s, i) => (
            <line
              key={i}
              className="windflow__streak"
              x1={-140}
              y1={s.y}
              x2={-140 + s.len}
              y2={s.y}
              stroke={critColor}
              strokeWidth={s.thickness}
              strokeLinecap="round"
              opacity={s.opacity}
              style={{
                animationDuration: `${durationMs}ms`,
                animationDelay: `${s.delay}ms`,
              }}
            />
          ))}
        </g>
      </svg>
      <div className="windflow__overlay">
        <div className="windflow__top">
          <span className="windflow__cap">{t("landing.wind")}{" "}<InfoBadge explanation={t("landing.erklaer.td.wind")} /></span>
        </div>
        {calm ? (
          <div className="windflow__hero">
            <span className="windflow__hero-label">
              {t("landing.wind_calm")}
            </span>
          </div>
        ) : (
          <div className="windflow__hero">
            <span
              className="windflow__hero-num"
              style={{ color: critColor }}
            >
              {xwAbs.toFixed(0)}
            </span>
            <div className="windflow__hero-meta">
              <span className="windflow__hero-unit">kt</span>
              <span className="windflow__hero-sub">
                {t("landing.wind_crosswind")} · {sideLabel}
              </span>
            </div>
          </div>
        )}
        {/* Landebahn — waagerecht, UNTER der Hero-Zahl. Bleibt fix, damit
            der Winkel der Streaks die Anströmrichtung relativ zur Bahn
            zeigt. */}
        {runwayIdent && (
          <div className="windflow__runway">
            <span className="windflow__rwy-keys" />
            <span className="windflow__rwy-line" />
            <span className="windflow__rwy-id">{runwayIdent}</span>
          </div>
        )}
        {!calm && (
          <div className="windflow__chips">
            <span className="windflow__chip">
              {headLabel} {twAbs.toFixed(0)} kt
            </span>
            <span className="windflow__chip">
              {t("landing.wind_total")} {totalKt.toFixed(0)} kt
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

export function TouchdownAbschnitt({ record }: { record: LandingRecord }) {
  const { t } = useTranslation();
  return (
    <section className="landing-section">
      <h3>{t("landing.touchdown")}</h3>
      <div className="landing-grid landing-grid--td">
        <dl className="landing-keyvals">
          {/* v0.7.11: Touchdown-Card auf die wichtigen Werte reduziert.
              Alle smoothed-VS-Werte (250/500/1000/1500 ms) + vs_at_edge
              + landing_peak_vs_fpm + Peak-G post-TD wurden hier
              entfernt — die gehoeren in die Sinkrate-Forensik-Sektion
              weiter unten (v0.7.8). Pilot sieht hier nur EINE Sinkrate
              (= Score-Basis nach v0.7.11 = vs_at_edge_fpm) + die
              Aufprall-Werte. Kein Werte-Dschungel mehr. */}
          <div>
            <dt>{t("landing.landing_rate")}{" "}<InfoBadge explanation={t("landing.erklaer.td.sinkrate")} /></dt>
            {/* v0.7.17 (B-015): Edge-Wert bevorzugen — Touchdown-Card
                zeigte bisher `landing_rate_fpm` (Streamer-Tick), was
                meist 30-50 fpm vom echten Aufsetz-Moment abwich. */}
            <dd>{fmtNumber(scoreBasisVs(record), 0, "fpm")}</dd>
          </div>
          <div>
            <dt>{t("landing.g_force")}{" "}<InfoBadge explanation={t("landing.erklaer.td.g")} /></dt>
            {/* v0.20.0: scoreG() statt Roh-G am Touchdown-Frame — sonst
                zeigt diese Kachel eine andere Zahl als der G-Balken
                daneben, der auf dem EMA-Wert bewertet. */}
            <dd>{fmtNumber(scoreG(record), 2, "G")}</dd>
          </div>
          {/* v0.12.8-dev: Der ROH-PEAK G bleibt in der G-Force-Forensik.
              Score-Version 19: dafür steht hier der G-Wert genau im
              Aufsetz-Frame (Webapp „Touchdown G") — eine andere Größe
              als der bewertete EMA-Wert darüber. */}
          <div>
            <dt>{t("landing.g_force_frame")}{" "}<InfoBadge explanation={t("landing.erklaer.td.g_frame")} /></dt>
            <dd>{fmtNumber(record.landing_g_force, 2, "G")}</dd>
          </div>
          <div>
            <dt>{t("landing.pitch")}{" "}<InfoBadge explanation={t("landing.erklaer.td.pitch")} /></dt>
            <dd>{fmtSigned(record.landing_pitch_deg, 1, "°")}</dd>
          </div>
          <div>
            <dt>{t("landing.bank")}{" "}<InfoBadge explanation={t("landing.erklaer.td.bank")} /></dt>
            <dd>{fmtSigned(record.landing_bank_deg, 1, "°")}</dd>
          </div>
          <div>
            <dt>{t("landing.speed")}{" "}<InfoBadge explanation={t("landing.erklaer.td.ias")} /></dt>
            <dd>{fmtNumber(record.landing_speed_kt, 0, "kt")}</dd>
          </div>
          <div>
            <dt>{t("landing.groundspeed")}{" "}<InfoBadge explanation={t("landing.erklaer.td.gs")} /></dt>
            <dd>{fmtNumber(record.landing_groundspeed_kt, 0, "kt")}</dd>
          </div>
          <div>
            <dt>{t("landing.sideslip")}{" "}<InfoBadge explanation={t("landing.erklaer.td.sideslip")} /></dt>
            <dd>{fmtSigned(record.touchdown_sideslip_deg, 1, "°")}</dd>
          </div>
          <div>
            <dt>{t("landing.bounces")}{" "}<InfoBadge explanation={t("landing.erklaer.td.hopser")} /></dt>
            <dd>
              {fensterWerteGueltig(record)
                ? record.bounce_count
                : t("landing.nicht_bewertbar.kein_wert")}
            </dd>
          </div>
          <div>
            <dt>{t("landing.heading")}{" "}<InfoBadge explanation={t("landing.erklaer.td.hdg_mag")} /></dt>
            <dd>{fmtNumber(record.landing_heading_deg, 0, "°")}</dd>
          </div>
          <div>
            <dt>{t("landing.heading_true")}{" "}<InfoBadge explanation={t("landing.erklaer.td.hdg_true")} /></dt>
            <dd>{fmtNumber(record.landing_heading_true_deg, 0, "°")}</dd>
          </div>
        </dl>
        <WindCompass
          headwindKt={record.headwind_kt}
          crosswindKt={record.crosswind_kt}
          runwayIdent={record.runway_match?.runway_ident ?? null}
        />
      </div>
    </section>
  );
}
