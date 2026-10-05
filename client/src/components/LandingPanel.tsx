import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import i18n from "../i18n";
import { invoke } from "../lib/ipc";
import {
  ladeNamenGedeckelt,
  leeresGedaechtnis,
  type NachladeGedaechtnis,
} from "../lib/namenNachladen";
import { Sentry } from "../lib/sentry";
import { useConfirm } from "./ConfirmDialog";
import { BordbuchBericht, BordbuchLandungsAbschnitt, useBordbuchEintrag } from "./bordbuch/BordbuchLandung";
import type { Eintrag as BordbuchEintrag } from "../lib/bordbuch";
import { hauptzahlText as spritHauptzahl, kg as spritKg, minuten as spritMinuten, reserveAbstand as spritReserveAbstand } from "../lib/sprit";
import { SinkrateForensik, scoreBasisVs, istBewertbar } from "./SinkrateForensik";
import { GForceForensik } from "./GForceForensik";
import { RunwayDiagramV2 } from "./RunwayDiagramV2";
import { ApproachStabilityCard } from "./ApproachStabilityCard";
import { AnflugForensikInfo } from "./AnflugForensikInfo";
import { mapLandingRecordToV2Props } from "../dev/runwayDiagramV2Mapper";
import { rolloutLdaMeters } from "../lib/runwayGeometry";
import { displayCallsign } from "../lib/callsign";
// v0.5.47 — Score-Modul ist jetzt zentral, identisch zu webapp/src/
// components/landingScoring.ts. Dieselben Schwellen, Bands, Coach-Tipps
// für Pilot-App und Live-Monitor.
import {
  computeSubScores as libComputeSubScores,
  type SubScore as LibSubScore,
  type LandingCategory,
  T_VS_SMOOTH_FPM,
  T_VS_FIRM_FPM,
  T_VS_HARD_FPM,
  T_VS_SEVERE_FPM,
} from "../lib/landingScoring";
// Anfluggrafik und 50-Hz-Nahaufnahme: components/AnflugGrafik.tsx
// (gespiegelt in die Webapp).
import { AnflugGrafikAbschnitt, ApproachChart, VsCurveChart } from "./AnflugGrafik";
// Zahlformate, scoreG, Fenster-Gültigkeit: lib/landungsFormat.ts (gespiegelt).
import { fensterWerteGueltig, fmtNumber, fmtSigned, scoreG } from "../lib/landungsFormat";
import { TouchdownAbschnitt } from "./TouchdownAbschnitt";
// Kopf, Banner, Hinweise, Teilnoten: components/LandungsBewertung.tsx (gespiegelt).
import {
  AccidentBanner,
  OffAirportBanner,
  QuickFlags,
  BewertungsAbschnitt,
  LandungsKopf,
  NichtBewertbarKasten,
  BahnUeberschrift,
  achsenLabel,
  coachTipKey,
  fmtDateTime,
  gradeColor,
  rateCategoryWord,
  recordCategory,
  subScoresAusDatensatz,
  type SubScore,
} from "./LandungsBewertung";
export { rateCategoryWord, recordCategory };
export type { SubScore };
import { FlareAbschnitt } from "./FlareAbschnitt";
import { LandingQualitaet } from "./LandingQualitaet";
import { LadeblattAbschnitt } from "./LadeblattAbschnitt";
import { MetarAbschnitt } from "./MetarAbschnitt";
import { RohdatenAbschnitt } from "./RohdatenAbschnitt";
export { scoreG };
import { PruefstatusKasten, PruefstatusMarke, usePirepPruefstatus, type PirepPruefstatus } from "./PirepPruefstatus";
import { gateAus, gateGruende, gateMarke, gateUrteil } from "../lib/stableGate";
import { deckelText } from "../lib/landungsUrteil";
// Datensatz-Typen: lib/landungsDatensatz.ts (gespiegelt in die Webapp).
import type {
  ApproachSample,
  GateWindow,
  LandingProfilePoint,
  LandingRecord,
  LandingRunwayMatch,
  SubScoreEntry,
} from "../lib/landungsDatensatz";
export type {
  ApproachSample,
  GateWindow,
  LandingProfilePoint,
  LandingRecord,
  LandingRunwayMatch,
  SubScoreEntry,
};

// ---- Types (mirror storage::LandingRecord on the Rust side) -------------

/// v0.7.1 SubScoreEntry — voll ausgebautes Wire-Format aus der
/// landing-scoring Crate (Spec §5.4 P1.5-A). Spiegel des Rust-Typs.
/// UI rendert direkt aus diesen Felder, kein Recompute.


// ---- Score breakdown ---------------------------------------------------
//
// We split the overall touchdown score into 6 sub-categories so the pilot
// can see *which* aspect of the landing pulled the grade down. Each is a
// 0-100 score with a short rationale. Thresholds are calibrated against
// FOQA-style guidelines and the existing primary score table.

// v0.5.47 — Sub-Score-Berechnung delegiert an die zentrale Lib.
// Webapp und Client nutzen jetzt dieselben Schwellen, Bands und
// Coach-Tipps. SubScore (lokal) ist strukturidentisch zu LibSubScore.

/// v0.7.1 Phase 3 (Spec §3.5 Legacy-Schutz):
///   - ux_version >= 1 → gespeicherte sub_scores aus dem Record nutzen
///     (kein Recompute), damit Werte konsistent zum PIREP-Payload sind.
///     SKIPPED Sub-Scores BLEIBEN drin und werden als "nicht bewertet"
///     mit grauem Band gerendert (P1.2-Fix).
///   - ux_version < 1 → Legacy-Pfad mit libComputeSubScores (nur fuer
///     pre-v0.7.1-PIREPs als Backward-Compat)
///   - bei v0.7.1+ ohne sub_scores (sollte nie passieren) → Legacy-Pfad
function getSubScores(r: LandingRecord): SubScore[] {
  const ux = r.ux_version ?? 0;
  if (ux >= 1 && r.sub_scores && r.sub_scores.length > 0) {
    // Phase 3 (P1.2-Fix): skipped sind sichtbar als "nicht bewertet"
    // v0.10.0 (#runway-utilization-score): extra-Lines + warning werden
    // 1:1 vom Rust-Crate durchgereicht (SSoT — kein Recompute in TS).
    // Seit 05.10.2026 in LandungsBewertung.tsx (gespiegelt, die Webapp
    // nutzt dieselbe Abbildung).
    return subScoresAusDatensatz(r);
  }
  // Legacy-Pfad fuer pre-v0.7.1-PIREPs (forward-compat)
  // v0.20.0: ueber scoreBasisVs() statt handkopierter Kaskade — dieselbe
  // Regel dreimal ausgeschrieben ist genau die Drift, aus der der
  // PIA3452-Split entstanden ist (Log -233 vs Karte -206).
  // Ohne Messung keine Teilwerte: Sie stammen alle aus demselben Fenster
  // (Codex-Abnahme 12.09.2026 — `getSubScores` reichte sie ungeprüft durch).
  if (!istBewertbar(r)) return [];
  const peakVs = scoreBasisVs(r);
  const subs: LibSubScore[] = libComputeSubScores({
    vs_fpm: peakVs,
    peak_g_load: r.landing_peak_g_force,
    // v0.12.3 (LE8): EMA-Scored-G → sub_g_force scort diesen Wert,
    // sonst Fallback auf den rohen peak_g_load.
    scored_g_load: r.landing_scored_g_force,
    bounce_count: r.bounce_count,
    approach_vs_stddev_fpm: r.approach_vs_stddev_fpm,
    approach_bank_stddev_deg: r.approach_bank_stddev_deg,
    rollout_distance_m: r.rollout_distance_m,
    fuel_efficiency_pct: r.fuel_efficiency_pct,
  });
  return subs as SubScore[];
}

// Alias fuer Backward-Compat mit bestehenden Aufruf-Stellen
function computeSubScores(r: LandingRecord): SubScore[] {
  return getSubScores(r);
}

// ─── v0.12.0 (#runway-utilization-refinement) — TS-gerenderte Extra-Zeilen ──
//
// Spec docs/spec/v0.12.0-runway-utilization-refinement.md LE5: ab
// `score_algorithm_version >= 3` lässt das Rust-Crate das `extra`-Feld
// LEER. Die drei Bahn-Auslastungs-Extra-Zeilen (Aufsetzpunkt, Ausroll-
// strecke, Bahn) baut stattdessen der TS-Renderer aus den ohnehin
// vorhandenen Record-Feldern + i18n — damit sie sprach-fähig sind statt
// hardcoded-Deutsch. Alt-v2-Records (`< 3`) behalten ihre gespeicherten
// `extra`-Strings (Legacy).


/** Wie viele Landungen die Übersichts-Kurve zeigt. */
const KURVE_LAENGE = 12;

/** Stehen in dieser Liste Landungen aus verschiedenen Bewertungs-Ständen?
 *
 *  Ab Version 7 kommt die Sinkrate aus dem Höhenverlauf statt vom
 *  Instrument des Simulators. Das verschiebt sowohl die angezeigte Zahl
 *  als auch die Punkte — in MSFS deutlich. Ein Durchschnitt über beide
 *  Stände ist deshalb kein Trend, und der weichste Flug bleibt auf
 *  absehbare Zeit ein alter, zu weich gemessener. */
export function gemischteBewertungsstaende(records: LandingRecord[]): boolean {
  // Nur die Landungen, die auch in der Kurve stehen. `landing_list` gibt
  // ALLE Datensätze zurück — an die volle Liste gebunden bliebe der Hinweis
  // dauerhaft stehen, solange irgendwo noch ein alter Flug liegt. Der
  // Sprung ist aber genau dort sichtbar, wo alt und neu nebeneinander
  // gezeichnet werden; nach zwölf neuen Landungen erklärt sich nichts mehr
  // und der Hinweis verschwindet von selbst.
  const sichtbar = records.slice(0, KURVE_LAENGE);
  // Fehlt die Versionsnummer, stammt der Datensatz aus einer Zeit, in der
  // es sie noch gar nicht gab — also erst recht von vor der Umstellung.
  // Ihn zu überspringen ließe den Hinweis ausgerechnet bei den ältesten
  // Beständen aus (QS-Befund v1.6.3).
  const vorHoehenkurve = sichtbar.some(
    (r) => r.score_algorithm_version == null || r.score_algorithm_version < 7,
  );
  const abHoehenkurve = sichtbar.some(
    (r) => r.score_algorithm_version != null && r.score_algorithm_version >= 7,
  );
  return vorHoehenkurve && abHoehenkurve;
}


// v0.20.0: `rolloutLdaMeters` lebt jetzt in `lib/runwayGeometry.ts` — der
// RunwayDiagramV2-Mapper braucht dieselbe Formel, und ein Import von hier
// waere ein zirkulaerer Laufzeit-Import (LandingPanel importiert den Mapper).
// Re-Export, damit bestehende Aufrufer und Tests unveraendert bleiben; der
// Import oben (Zeile 1-30) macht sie zusaetzlich lokal verfuegbar.
export { rolloutLdaMeters };

// ---- Helpers ------------------------------------------------------------

// ---- Landungen-Übersicht (7a) — formatting helpers -----------------------
// Separate from fmtDateTime() (LandungsBewertung.tsx): that one is locale/local-time and is
// used by the (untouched) detail report. The overview table/chart/footer
// need UTC, no-seconds, and a couple of overview-only date shapes.

/** "31.07.2026", UTC. */
function fmtDateUtc(iso: string): string {
  try {
    const d = new Date(iso);
    const dd = String(d.getUTCDate()).padStart(2, "0");
    const mm = String(d.getUTCMonth() + 1).padStart(2, "0");
    return `${dd}.${mm}.${d.getUTCFullYear()}`;
  } catch {
    return iso;
  }
}

/** "20:46z", UTC, no seconds. */
function fmtTimeUtcZ(iso: string): string {
  try {
    const d = new Date(iso);
    const hh = String(d.getUTCHours()).padStart(2, "0");
    const mm = String(d.getUTCMinutes()).padStart(2, "0");
    return `${hh}:${mm}z`;
  } catch {
    return "—";
  }
}

/** "12.06.", UTC, no year — used by the footer's softest/hardest stats. */
function fmtDateShortUtc(iso: string): string {
  try {
    const d = new Date(iso);
    const dd = String(d.getUTCDate()).padStart(2, "0");
    const mm = String(d.getUTCMonth() + 1).padStart(2, "0");
    return `${dd}.${mm}.`;
  } catch {
    return iso;
  }
}

/** "29. JUN", UTC — the sink-rate chart's axis-adjacent date labels.
 *  Hardcoded 3-letter abbreviations (not Intl) so the shape stays fixed
 *  regardless of how a given locale's "short" month style happens to be
 *  spelled — the mock wants exactly 3 uppercase letters. */
const CHART_MONTH_ABBR: Record<string, string[]> = {
  de: ["JAN", "FEB", "MÄR", "APR", "MAI", "JUN", "JUL", "AUG", "SEP", "OKT", "NOV", "DEZ"],
  en: ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"],
  it: ["GEN", "FEB", "MAR", "APR", "MAG", "GIU", "LUG", "AGO", "SET", "OTT", "NOV", "DIC"],
};
function fmtChartDateUtc(iso: string, lang: string): string {
  try {
    const d = new Date(iso);
    const months = CHART_MONTH_ABBR[lang] ?? CHART_MONTH_ABBR.en;
    return `${d.getUTCDate()}. ${months[d.getUTCMonth()]}`;
  } catch {
    return iso;
  }
}

/** Spec §6: every number on this screen is de-DE-formatted regardless of
 *  the app's UI language (comma decimal, dot thousands). */
function fmtDeDe(v: number, digits = 0): string {
  return v.toLocaleString("de-DE", { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

// ---- VS Curve chart: components/AnflugGrafik.tsx ------------------------

// ---- v0.7.6 P1-3: Runway-Geometry-Trust: BahnUeberschrift in LandungsBewertung.tsx


// ---- Wind compass -------------------------------------------------------


// ---- Approach stability time-series chart: components/AnflugGrafik.tsx --

// ---- (i) info badge: components/InfoBadge.tsx --------------------------

// ---- Score breakdown card grid -----------------------------------------

// ---- Coach tip — focuses on the worst sub-score ------------------------

// ---- Off-airport banner (v0.7.18 B-012) -------------------------------
//
// Zeigt wenn der echte Touchdown nicht beim geplanten Destination-Airport
// war — Diversion, Off-airport-Crash (GAF-152-Fall), oder Crash mit
// Nearest-Airport in Reichweite.
//
// Drei sichtbare Fälle:
//   1. runway_match-Resolution, aber icao != arr_airport → Divert
//   2. nearest_25nm-Resolution → Off-airport, Nearest gefunden
//   3. planned_fallback mit Distanz > 5 nmi → Off-airport, kein Nearest
//
// Match-Resolution mit icao == arr_airport rendert keinen Banner (= normaler Flug).


// ---- Quick-Flag chips (v0.5.47) ---------------------------------------
//
// Auf-einen-Blick-Auffälligkeiten direkt unter dem Headline-Block.
// Spiegelt die Chips aus webapp/src/components/LandingAnalysis.tsx
// (B:124-133) — Pilot sieht im Client und im Live-Monitor exakt dieselben
// Flags. Nur die wirklichen Auffälligkeiten anzeigen — keine "OK"-Chips.

// ---- Trend sparkline (last N landings) ---------------------------------

// Landungen-Übersicht (7a) §3 — sink rate is the lead value now (was score).
// v1.3.5 live-consistency fix: the handoff doc's own axis (0/350/600/800,
// 3 colors) matched a 3-band scale invented for this screen only. Swapped
// for the report's real 5-band scale — axis/reflines/bar heights now derive
// from T_VS_SEVERE_FPM (1000) instead of a hardcoded 800.
const CHART_MAX_FPM = T_VS_SEVERE_FPM;

function LandingRateChart({ records }: { records: LandingRecord[] }) {
  const { t, i18n } = useTranslation();
  if (records.length < 2) return null;
  // Newest-first list; chart wants oldest→newest left→right.
  // Nicht bewertbare Landungen erscheinen in keiner Statistik: Ein
  // Mittelwert über "nicht gemessen" ist keine Zahl (Untersuchung
  // 12.09.2026). Sie verschwinden damit aus dem Verlauf — richtig, denn
  // eine Aussage über ihre Härte gibt es nicht.
  const bewertbare = records.filter(istBewertbar);
  // ⚠ Die Prüfung auf "genug Einträge" muss NACH dem Filtern stehen.
  // Codex-Abnahme 12.09.2026: Bei zwei nicht bewertbaren Einträgen blieb
  // die Liste leer, und `newest.touchdown_at` lief auf `undefined`.
  if (bewertbare.length < 2) return null;
  const latest = bewertbare.slice(0, KURVE_LAENGE).reverse();
  const rates = latest.map((r) => Math.abs(scoreBasisVs(r) ?? 0));
  const oldest = latest[0];
  const newest = latest[latest.length - 1];
  // Same figure as the header's "Ø SINKRATE" (average over ALL loaded
  // records, not just these 12 bars) — a second, differently-scoped
  // average confused more than it helped when both were on screen at once.
  const avgRateSigned =
    bewertbare.reduce((s, r) => s + (scoreBasisVs(r) ?? 0), 0) / (bewertbare.length || 1);
  const avgRateAbs = Math.abs(avgRateSigned);
  const legendCats: LandingCategory[] = ["smooth", "acceptable", "firm", "hard", "severe"];

  return (
    <div className="landing-ov-chart">
      <div className="landing-ov-chart__head">
        <span className="landing-ov-chart__title">{t("landing.ov_chart_title")}</span>
        <div className="landing-ov-chart__legend">
          {legendCats.map((cat) => (
            <span key={cat} className="landing-ov-chart__legend-item">
              <i className={`landing-ov-chart__swatch landing-ov-chart__swatch--${cat}`} />
              {rateCategoryWord(cat, t)}
            </span>
          ))}
        </div>
      </div>

      <div
        className="landing-ov-chart__body"
        role="img"
        aria-label={t("landing.ov_chart_aria", {
          count: latest.length,
          min: Math.round(Math.min(...rates)),
          max: Math.round(Math.max(...rates)),
          last: Math.round(rates[rates.length - 1]),
        })}
      >
        <div className="landing-ov-chart__axis">
          <span style={{ bottom: "100%" }}>{T_VS_SEVERE_FPM}</span>
          <span style={{ bottom: `${(T_VS_HARD_FPM / CHART_MAX_FPM) * 100}%` }}>{T_VS_HARD_FPM}</span>
          <span style={{ bottom: `${(T_VS_FIRM_FPM / CHART_MAX_FPM) * 100}%` }}>{T_VS_FIRM_FPM}</span>
          <span style={{ bottom: `${(T_VS_SMOOTH_FPM / CHART_MAX_FPM) * 100}%` }}>{T_VS_SMOOTH_FPM}</span>
          <span style={{ bottom: "0" }}>0</span>
        </div>
        {/* No tinted BAND reference lines anymore: bar COLOR is now the
            record's overall score_label category (§ recordCategory), which
            isn't a pure function of THIS bar's own fpm height, so a line at
            a fixed fpm value wouldn't mark an actual colour boundary. A
            plain average line is still useful on its own terms though —
            added back per pilot request. */}
        <div className="landing-ov-chart__plot">
          <div
            className="landing-ov-chart__avgline"
            style={{ bottom: `${Math.min(100, (avgRateAbs / CHART_MAX_FPM) * 100)}%` }}
          >
            <span className="landing-ov-chart__avgline-label">
              {t("landing.ov_chart_avg_label", { rate: fmtDeDe(Math.round(avgRateSigned)) })}
            </span>
          </div>
          {latest.map((r, i) => {
            const rate = rates[i];
            const cat = recordCategory(r);
            const isLatest = i === latest.length - 1;
            return (
              <div
                key={r.pirep_id}
                className={`landing-ov-chart__bar landing-ov-chart__bar--${cat}${
                  isLatest ? " landing-ov-chart__bar--latest" : ""
                }`}
                style={{ height: `${Math.min(100, (rate / CHART_MAX_FPM) * 100)}%` }}
              />
            );
          })}
        </div>
      </div>

      <div className="landing-ov-chart__foot">
        <span>{fmtChartDateUtc(oldest.touchdown_at, i18n.language)}</span>
        <span>
          {t("landing.ov_chart_last", {
            date: fmtChartDateUtc(newest.touchdown_at, i18n.language),
            rate: fmtDeDe(Math.round(scoreBasisVs(newest) ?? 0)),
          })}
        </span>
      </div>
    </div>
  );
}

// ---- Detail view --------------------------------------------------------

// ─── v0.12.8-dev: PDF-Export der Landungs-Analyse ───────────────────────
//
// Der Pilot exportiert die Landungs-Analyse als mehrseitigen A4-Report.
// Mechanik:
//   1. Button "PDF exportieren" in `.landing-detail__top` setzt
//      `printing = true`.
//   2. `<LandingReport>` wird via `createPortal` in einen an `document.body`
//      gehängten Container gerendert. Auf dem Bildschirm ist der Container
//      `display:none` — sichtbar wird er NUR im `@media print`.
//   3. Ein Effect ruft `window.print()` (nach dem ersten Render-Frame) und
//      registriert einen One-Shot `afterprint`-Listener, der `printing`
//      zurücksetzt — der Report verschwindet wieder aus dem DOM.
// Der Report nutzt ALLE lokalen Helper (gradeColor, fmtNumber, computeSub-
// Scores, ApproachChart, VsCurveChart, RunwayDiagramV2 …), darum lebt er
// hier in-file statt in einer eigenen Datei.

/**
 * Mindest-Schriftgrösse der Bahn-Grafik im Bericht, in SVG-Einheiten.
 *
 * # Das Problem
 *
 * Im Bericht skaliert das SVG auf die Spaltenbreite (`width: 100%`), und
 * jede Schrift darin schrumpft mit:
 *
 *   A4 hoch, `@page margin: 14mm 15mm`  →  180 mm Spalte
 *   minus 2 × 5 mm Polster der Karte    →  170 mm Zeichenbreite
 *   viewBox der Grafik                  →  1200 Einheiten
 *   also  1 Einheit = 170/1200 mm       =  0,4016 pt
 *
 * Gemessen am 24.08.2026 landeten die Beschriftungen damit bei 3,6 bis
 * 4,4 pt. Lesbar ist Druck etwa ab 6 pt.
 *
 * # Warum 11 und nicht mehr
 *
 * Für 6,8 pt bräuchte es 17 Einheiten. Bei 17 zerfällt das Layout: In der
 * Demo (alle vierzehn Varianten, echter SVG-Motor) waren es 31 Befunde —
 * Beschriftungen, die sich überlappen oder aus dem Bild laufen. Das
 * Layout ist auf die Schriftgrössen von damals abgestimmt, und die
 * waagerechten Abstände lassen sich nicht durch Anheben lösen, sondern
 * nur durch ein anderes Layout.
 *
 * Abgetastet ergab sich: 17 → 31 Befunde, 13 → 15, 12 → 3, **11 → 0**.
 * Elf ist damit der grösste Wert, der nachweislich sauber bleibt: 4,4 pt
 * statt 3,6 pt.
 *
 * # Was das bedeutet — und was nicht
 *
 * Die Beschriftungen IM BILD bleiben auf Papier klein. Verloren geht
 * dadurch nichts: Jeder gemessene Wert steht als normaler HTML-Text
 * neben der Grafik (Ereignisliste, Kennzahlen-Zeile, Legende) und druckt
 * in gewohnter Grösse. Das Bild zeigt die Lage, die Liste die Zahlen.
 *
 * Wirklich lesbar würde das Bild erst auf einer Querformat-Seite
 * (269 mm statt 170 mm → 1,58×, mit dieser Untergrenze rund 7 pt). Das
 * ist eine Layout-Entscheidung und steht offen.
 *
 * `LandingReport.test.tsx` rechnet die Herleitung nach, statt sie zu
 * glauben.
 */
const BERICHT_SCHRIFT_MINDEST = 11;

/**
 * App-Version für die Report-Fußzeile — aus dem Paket, nicht von Hand.
 *
 * Hier stand eine getippte Konstante: `"0.12.8"`. Die App war bei 1.7.0,
 * und jeder ausgedruckte Bericht behauptete seit fünf Versionen einen
 * falschen Stand. Genau dafür ist ein Bericht da — jemand legt ihn zur
 * Seite und sieht später nach, womit er erzeugt wurde.
 *
 * `__APP_VERSION__` setzt Vite aus `package.json` (siehe
 * `vite.config.ts`). Im Testlauf ohne Vite-Define fehlt es, deshalb der
 * Rückfall.
 */
const REPORT_APP_VERSION =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "0.0.0";

/** v0.12.8-dev: Fußzeile — EINMAL am Ende des fließenden Dokuments
 *  (vorher pro Seite). Generierungs-Datum + App-Version. */
function ReportFooter() {
  const { t } = useTranslation();
  const generated = t("landing.report.generated", {
    date: new Date().toLocaleDateString(i18n.language || undefined),
    version: REPORT_APP_VERSION,
  });
  return (
    <div className="report-footer">
      <span>{generated}</span>
    </div>
  );
}

/** Branding-Kopfzeile (Wortmarke + Untertitel + Akzent-Linie). */
function ReportHeader() {
  const { t } = useTranslation();
  return (
    <div className="report-head">
      <div className="report-head__brand">
        <span className="report-head__mark">{t("landing.report.title")}</span>
        <span className="report-head__sub">
          {t("landing.report.subtitle")}
        </span>
      </div>
      <div className="report-head__rule" />
    </div>
  );
}

/** Section-Überschrift mit kurzem Akzent-Strich. v0.12.8-dev: jede
 *  Section bekommt eine eigene Akzentfarbe (`accent`) — wird als CSS-
 *  Variable `--report-accent` gesetzt und färbt Titel, Strich und die
 *  Kachel-Akzente. Bringt Farbe in den sonst sehr weißen Report. */
function ReportSection({
  title,
  accent = "#2563eb",
  children,
}: {
  title: string;
  accent?: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className="report-section"
      style={{ ["--report-accent" as string]: accent } as React.CSSProperties}
    >
      <h3 className="report-section__title">
        {title}
        <span className="report-section__rule" />
      </h3>
      {children}
    </section>
  );
}

/** Dunkle Karte um ein Chart — die Chart-Komponenten zeichnen helle
 *  Strokes auf transparentem Grund und wären auf weißem Papier
 *  unsichtbar. Die explizite Breite sorgt für korrektes SVG-Sizing
 *  im Print. */
function ReportChartCard({
  caption,
  children,
}: {
  caption: string;
  children: React.ReactNode;
}) {
  return (
    <div className="report-chart-card">
      <div className="report-chart-card__caption">{caption}</div>
      <div className="report-chart-card__panel">{children}</div>
    </div>
  );
}

/** Eine Touchdown-Kennwert-Kachel: Label + großer Wert. */
function ReportTile({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="report-tile">
      <div className="report-tile__label">{label}</div>
      <div className="report-tile__value">{value}</div>
      {/* v1.7.35: Detailzeile fuer den Druck — dort hilft kein Tooltip. */}
      {detail && <div className="report-tile__detail">{detail}</div>}
    </div>
  );
}

/**
 * v0.12.8-dev: Der A4-Landungs-Report. Hell, luftig, EIN Akzent
 * (#2563eb). KEIN fixes A4-Box-Paradigma mehr — der Report ist EIN
 * fließendes Dokument; der Browser paginiert selbst (kein Leerseiten-
 * Bug). Ausgewählte Section-Gruppen starten via `.report-break-before`
 * auf einer frischen Seite. Nur sichtbar im `@media print`.
 */
/** Exportiert für die Prüfung — der Bericht ist sonst nur über
 *  `window.print()` erreichbar, und das lässt sich nicht lesen. */
export function LandingReport({
  record,
  bordbuch,
}: {
  record: LandingRecord;
  /** Bordbuch des Flugs (27.09.2026) — vorab geladen, der Druck wartet nicht. */
  bordbuch?: BordbuchEintrag | null;
}) {
  const { t, i18n } = useTranslation();

  const callsign = record.airline_icao
    ? displayCallsign(record.airline_icao, record.flight_number)
    : record.flight_number;

  const subs = useMemo(() => computeSubScores(record), [record]);
  const scored = subs.filter((s) => !s.skipped);
  const worst = scored.length
    ? [...scored].sort((a, b) => a.points - b.points)[0]
    : null;

  // Charts nur rendern, wenn genug Datenpunkte da sind (gleiche
  // Schwellen wie LandingDetail).
  const hasApproach = record.approach_samples.length >= 3;
  const hasCloseup = record.touchdown_profile.length >= 5;
  const v2Props =
    record.runway_match && (record.runway_geometry_trusted ?? true)
      ? mapLandingRecordToV2Props(record)
      : null;
  // Die Bahn-Grafik zählt hier NICHT mehr mit: Sie hat seit dem
  // Querformat-Umbau eine eigene Seite und steht nicht mehr in diesem
  // Abschnitt. Blieb sie in der Bedingung, rendert „Profile & Verläufe"
  // als leere Überschrift mit grauem Kasten darunter — im gedruckten PDF
  // gesehen, nicht vermutet.
  const hasCharts = hasApproach || hasCloseup;

  // Score-Label: bei Confirmed Accident → "ABSTURZ ERKANNT".
  // QS 2026-08-04: liest bewusst ueber `recordCategory`/`rateCategoryWord`
  // statt direkt `record.score_label` — eine Anzeigequelle fuer Uebersicht
  // UND Bericht, damit beide nicht wieder auseinanderlaufen koennen. Die
  // Frische des Feldes stellt inzwischen `landing_list` (Rust) sicher.
  const kategorie = recordCategory(record);
  const heroLabel =
    record.accident === true
      ? t("landing.report.accident_label")
      : kategorie == null
      ? t("landing.nicht_bewertbar.kurz")
      : rateCategoryWord(kategorie, t);

  // Sub-Score-Balken: Farbe nach Punkten (grün / amber / rot).
  const barColor = (pts: number) =>
    // Bänder der Bewertung (band_from_points): ab 75 gut, ab 45 mittel.
    pts >= 75 ? "#22c55e" : pts >= 45 ? "#f59e0b" : "#ef4444";

  const rm = record.runway_match;

  // v0.12.8-dev: Anflug-Stabilität — sichtbar wenn mindestens eines der
  // 7 Stability-v2-Felder vorhanden ist (alte PIREPs ohne sie zeigen die
  // "no_data"-Hint). Bools zählen nur als vorhanden wenn != null.
  // Score-Version 19: Prüfliste des Stable Gate (falls vorhanden).
  const reportGate = gateAus(record.sub_scores);
  const hasStability =
    record.approach_vs_jerk_fpm != null ||
    record.approach_bank_stddev_deg != null ||
    record.approach_ias_stddev_kt != null ||
    record.approach_vs_deviation_fpm != null ||
    record.approach_max_vs_deviation_below_500_fpm != null ||
    record.approach_stable_config != null ||
    record.approach_excessive_sink != null;

  // v0.12.8-dev: Sprit & Gewicht — sichtbar wenn irgendein Fuel/Weight-
  // Feld da ist (gleiche Bedingung wie die Loadsheet-Section im
  // on-screen LandingDetail).
  const hasFuel =
    record.planned_burn_kg != null ||
    record.actual_trip_burn_kg != null ||
    record.block_fuel_kg != null ||
    record.takeoff_fuel_kg != null ||
    record.landing_fuel_kg != null ||
    record.takeoff_weight_kg != null ||
    record.landing_weight_kg != null ||
    record.fuel_efficiency_pct != null;

  return (
    // v0.12.8-dev: EIN fließendes Dokument — kein <ReportPage>-A4-Box-
    // Stapel mehr. EIN <ReportHeader> oben, dann fließende Sections,
    // EINE <ReportFooter> ganz unten. Saubere Seitenanfänge entstehen
    // durch `.report-break-before` auf den Section-Gruppen.
    <div className="landing-report report-page">
      <ReportHeader />

      {/* ── Identität & Score ──────────────────────────────────────── */}
      <div className="report-identity">
        <div className="report-identity__callsign">{callsign}</div>
        <div className="report-identity__route">
          <span>{record.dpt_airport}</span>
          <span className="report-identity__arrow">→</span>
          <span>{record.arr_airport}</span>
        </div>
        <div className="report-identity__meta">
          <div>
            <span className="report-identity__k">
              {t("landing.report.aircraft")}
            </span>
            <span className="report-identity__v">
              {record.aircraft_title ?? "—"}
              {record.aircraft_registration
                ? ` · ${record.aircraft_registration}`
                : ""}
              {record.aircraft_icao ? ` · ${record.aircraft_icao}` : ""}
            </span>
          </div>
          <div>
            <span className="report-identity__k">
              {t("landing.report.simulator")}
            </span>
            <span className="report-identity__v">
              {record.sim_kind ?? "—"}
            </span>
          </div>
          <div>
            <span className="report-identity__k">
              {t("landing.report.touchdown_time")}
            </span>
            <span className="report-identity__v">
              {fmtDateTime(record.touchdown_at, i18n.language)}
            </span>
          </div>
        </div>
      </div>

      <div
        className="report-hero"
        style={{
          borderLeft: `7px solid ${gradeColor(record.grade_letter)}`,
        }}
      >
        <div
          className="report-hero__badge"
          style={{ background: gradeColor(record.grade_letter) }}
        >
          {record.grade_letter ?? "—"}
        </div>
        <div className="report-hero__text">
          <div className="report-hero__score">
            {record.score_numeric != null ? (
              <>
                {record.score_numeric}
                <span className="report-hero__of">
                  {" "}
                  {t("landing.report.hero_of")}
                </span>
              </>
            ) : (
              <span className="report-hero__of">
                {t("landing.nicht_bewertbar.kein_wert")}
              </span>
            )}
          </div>
          <div className="report-hero__label">{heroLabel}</div>
          {record.score_deckel && deckelText(t, record.score_deckel) ? (
            <div className="report-hero__deckel">
              {deckelText(t, record.score_deckel)}
            </div>
          ) : null}
        </div>
      </div>

      <ReportSection title={t("landing.report.breakdown")}>
        <div className="report-bars">
          {subs.map((s) => (
            <div key={s.key} className="report-bar">
              <div className="report-bar__head">
                <span className="report-bar__label">
                  {achsenLabel(t, s)}
                </span>
                <span className="report-bar__pts">
                  {s.skipped
                    ? t("landing.skipped_label")
                    : `${s.points} PTS`}
                </span>
              </div>
              <div className="report-bar__track">
                {!s.skipped && (
                  <div
                    className="report-bar__fill"
                    style={{
                      width: `${Math.max(0, Math.min(100, s.points))}%`,
                      background: barColor(s.points),
                    }}
                  />
                )}
              </div>
            </div>
          ))}
        </div>
      </ReportSection>

      {worst && (
        <ReportSection title={t("landing.report.coach")}>
          <div className="report-coach">
            <div className="report-coach__focus">
              {achsenLabel(t, worst)}
            </div>
            <p className="report-coach__body">
              {t(coachTipKey(worst.rationale))}
            </p>
          </div>
        </ReportSection>
      )}

      {/* ── Touchdown-Kennwerte (frische Seite) ────────────────────── */}
      <div className="report-break-before">
        <ReportSection title={t("landing.report.touchdown_section")}>
          <div className="report-tiles">
            <ReportTile
              label={t("landing.landing_rate")}
              value={fmtNumber(scoreBasisVs(record), 0, "fpm")}
            />
            <ReportTile
              label={t("landing.g_force")}
              /* v0.20.0: scoreG() — dieselbe Zahl, auf der der G-Balken
                 bewertet. `landing_g_force` ist der Roh-Wert am Touchdown-
                 Frame und wich sichtbar vom bewerteten EMA-Wert ab. */
              value={fmtNumber(scoreG(record), 2, "G")}
            />
            <ReportTile
              label={t("landing.pitch")}
              value={fmtSigned(record.landing_pitch_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.bank")}
              value={fmtSigned(record.landing_bank_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.speed")}
              value={fmtNumber(record.landing_speed_kt, 0, "kt")}
            />
            <ReportTile
              label={t("landing.sideslip")}
              value={fmtSigned(record.touchdown_sideslip_deg, 1, "°")}
            />
            <ReportTile
              label={t("landing.bounces")}
              value={
                fensterWerteGueltig(record)
                  ? String(record.bounce_count)
                  : t("landing.nicht_bewertbar.kein_wert")
              }
            />
            <ReportTile
              label={t("landing.heading")}
              value={fmtNumber(record.landing_heading_deg, 0, "°")}
            />
          </div>
        </ReportSection>

        {/* v0.12.8-dev: NEUE Section — Anflug-Stabilität. Felder +
            Labels gespiegelt von ApproachStabilityCard für Konsistenz. */}
        <ReportSection
          title={t("landing.report.stability_section")}
          accent="#7c3aed"
        >
          {hasStability ? (
            <>
            <div className="report-tiles">
              {reportGate && (
                <ReportTile
                  label={t("landing.approach_stability_card.title")}
                  value={t(`landing.approach_stability_card.pill_${gateUrteil(reportGate, gateMarke(record.sub_scores)) ?? "stable"}`)}
                />
              )}
              {reportGate?.find((p) => p.key === "gleitpfad")?.wert != null && (
                <ReportTile
                  label={t("landing.gate.label.gleitpfad")}
                  value={`${reportGate
                    .find((p) => p.key === "gleitpfad")!
                    .wert!.toLocaleString(i18n.language, {
                      minimumFractionDigits: 2,
                      maximumFractionDigits: 2,
                    })} ${t("landing.gate.einheit.gleitpfad")}`}
                />
              )}
              {record.approach_vs_jerk_fpm != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.vs_jerk.label",
                  )}
                  value={fmtNumber(record.approach_vs_jerk_fpm, 0, "fpm")}
                />
              )}
              {record.approach_bank_stddev_deg != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.bank_sigma.label",
                  )}
                  value={fmtNumber(record.approach_bank_stddev_deg, 1, "°")}
                />
              )}
              {record.approach_ias_stddev_kt != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.ias_sigma.label",
                  )}
                  value={fmtNumber(record.approach_ias_stddev_kt, 1, "kt")}
                />
              )}
              {!reportGate && record.approach_vs_deviation_fpm != null && (
                <ReportTile
                  label={(() => {
                    const gs = record.runway_match?.glideslope_angle_deg;
                    return gs != null &&
                      gs >= 2 &&
                      gs <= 7.5 &&
                      Math.abs(gs - 3) > 0.05
                      ? t(
                          "landing.approach_stability_card.tiles.vs_vs_ils.label_custom",
                          { angle: String(gs) },
                        )
                      : t("landing.approach_stability_card.tiles.vs_vs_ils.label");
                  })()}
                  value={fmtNumber(record.approach_vs_deviation_fpm, 0, "fpm")}
                />
              )}
              {!reportGate && record.approach_max_vs_deviation_below_500_fpm != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.max_vs_dev.label",
                  )}
                  value={fmtNumber(
                    record.approach_max_vs_deviation_below_500_fpm,
                    0,
                    "fpm",
                  )}
                />
              )}
              {record.approach_stable_config != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.landing_config.label",
                  )}
                  value={
                    record.approach_stable_config
                      ? t(
                          "landing.approach_stability_card.tiles.landing_config.value_ok",
                        )
                      : t(
                          "landing.approach_stability_card.tiles.landing_config.value_partial",
                        )
                  }
                />
              )}
              {record.approach_excessive_sink != null && (
                <ReportTile
                  label={t(
                    "landing.approach_stability_card.tiles.sink_rate.label",
                  )}
                  value={
                    record.approach_excessive_sink
                      ? t(
                          "landing.approach_stability_card.tiles.sink_rate.value_excessive",
                        )
                      : t(
                          "landing.approach_stability_card.tiles.sink_rate.value_ok",
                        )
                  }
                />
              )}
            </div>
            {reportGate && gateGruende(t, reportGate, i18n.language).length > 0 && (
              <ul className="report-gate-gruende">
                {gateGruende(t, reportGate, i18n.language).map((g) => (
                  <li key={g}>{g}</li>
                ))}
              </ul>
            )}
          </>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>
      </div>

      {/* ── Wind & Bahn (frische Seite) ────────────────────────────── */}
      <div className="report-break-before">
        <ReportSection
          title={t("landing.report.wind_section")}
          accent="#0891b2"
        >
          {(() => {
            const hw = record.headwind_kt;
            const xw = record.crosswind_kt;
            if (hw == null && xw == null) {
              return (
                <div className="report-empty">
                  {t("landing.report.no_data")}
                </div>
              );
            }
            const hwV = hw ?? 0;
            const xwV = xw ?? 0;
            const total = Math.sqrt(hwV * hwV + xwV * xwV);
            const hwLabel =
              hwV < -0.5
                ? t("landing.wind_tailwind")
                : t("landing.wind_headwind");
            const xwSide =
              xwV >= 0
                ? t("landing.wind_side_right")
                : t("landing.wind_side_left");
            return (
              <div className="report-tiles">
                <ReportTile
                  label={hwLabel}
                  value={fmtNumber(Math.abs(hwV), 0, "kt")}
                />
                <ReportTile
                  label={`${t("landing.wind_crosswind")} · ${xwSide}`}
                  value={fmtNumber(Math.abs(xwV), 0, "kt")}
                />
                <ReportTile
                  label={t("landing.wind_total")}
                  value={fmtNumber(total, 0, "kt")}
                />
              </div>
            );
          })()}
        </ReportSection>

        <ReportSection
          title={t("landing.report.runway_section")}
          accent="#0d9488"
        >
          {rm ? (
            <div className="report-tiles">
              <ReportTile
                label={t("landing.runway")}
                value={`${rm.airport_ident} ${rm.runway_ident}`}
              />
              <ReportTile
                label={t("landing.report.rwy_length")}
                value={fmtNumber(rm.length_ft * 0.3048, 0, "m")}
              />
              {/* Die landbare Länge — aber nur, wenn sie abweicht.

                  Auf derselben Seite standen zwei verschiedene
                  Bahnlängen: hier 3250 m (die bauliche), in der Grafik
                  darunter 2952 m (nach der versetzten Schwelle). Beide
                  stimmen, keine sagte welche sie ist, und der Leser
                  hatte keine Möglichkeit, sie zu vereinbaren.

                  Bei einer Bahn ohne versetzte Schwelle sind sie gleich
                  — dann wäre eine zweite Kachel nur Rauschen. */}
              {(() => {
                const lda = rolloutLdaMeters(rm);
                const voll = rm.length_ft * 0.3048;
                if (lda == null || Math.abs(voll - lda) < 1) return null;
                return (
                  <ReportTile
                    label={t("landing.report.rwy_lda")}
                    value={fmtNumber(lda, 0, "m")}
                  />
                );
              })()}
              <ReportTile
                label={t("landing.report.rwy_surface")}
                value={rm.surface || "—"}
              />
              {record.td_distance_from_threshold_m != null && (
                <ReportTile
                  label={t("landing.report.td_distance")}
                  value={fmtNumber(
                    record.td_distance_from_threshold_m,
                    0,
                    "m",
                  )}
                />
              )}
              {record.td_in_tdz != null && (
                <ReportTile
                  label={t("landing.report.in_tdz")}
                  value={
                    record.td_in_tdz
                      ? t("landing.report.yes")
                      : t("landing.report.no")
                  }
                />
              )}
              {record.aim_delta_m != null && (
                <ReportTile
                  label={t("landing.report.aim_delta")}
                  value={fmtSigned(record.aim_delta_m, 0, "m")}
                />
              )}
              {Number.isFinite(rm.centerline_distance_m) && (
                <ReportTile
                  label={t("landing.report.centerline")}
                  value={fmtNumber(
                    Math.abs(rm.centerline_distance_m),
                    1,
                    "m",
                  )}
                />
              )}
              {record.rollout_distance_m != null && (
                <ReportTile
                  label={t("landing.report.rollout_dist")}
                  value={fmtNumber(record.rollout_distance_m, 0, "m")}
                />
              )}
            </div>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>

        {/* v0.12.8-dev: NEUE Section — Sprit & Gewicht (Plan vs. Ist).
            Felder + Labels gespiegelt von der Loadsheet-Section im
            on-screen LandingDetail. */}
        <ReportSection
          title={t("landing.report.fuel_section")}
          accent="#d97706"
        >
          {hasFuel ? (
            <div className="report-tiles">
              {/* v1.7.35: die fertige Sprit-Auswertung zuerst — nur gerendert. */}
              {record.sprit?.bis_sinkflug && (
                <ReportTile
                  label={t("landing.sprit.report_bis_sinkflug")}
                  value={spritHauptzahl(record.sprit.bis_sinkflug, "bis_sinkflug")}
                />
              )}
              {record.sprit?.anflug && (
                <ReportTile
                  label={t("landing.sprit.report_anflug")}
                  value={spritHauptzahl(record.sprit.anflug, "anflug")}
                />
              )}
              {record.sprit?.zeit_unter_schwelle_min != null && record.sprit.schwelle_ft != null && (
                <ReportTile
                  label={t("landing.sprit.report_zeit")}
                  value={t("landing.sprit.zeit_wert", { min: spritMinuten(record.sprit.zeit_unter_schwelle_min), ft: spritKg(record.sprit.schwelle_ft) })}
                />
              )}
              {/* v1.7.36: der Rollsprit — erst damit ist der Bericht von
                  Triebwerk an bis Triebwerk aus vollständig. */}
              {record.sprit?.rollen_vor_start && (
                <ReportTile
                  label={t("landing.sprit.rollen_vor_start")}
                  value={`${spritKg(record.sprit.rollen_vor_start.ist_kg)} kg`}
                  detail={`Plan ${spritKg(record.sprit.rollen_vor_start.plan_kg)} kg`}
                />
              )}
              {record.sprit?.rollen_nach_landung_kg != null && (
                <ReportTile
                  label={t("landing.sprit.report_rollen_nach")}
                  value={`${spritKg(record.sprit.rollen_nach_landung_kg)} kg`}
                />
              )}
              {record.sprit && (
                <ReportTile
                  label={t("landing.sprit.report_reserve")}
                  value={
                    // v1.7.37: Abstand in kg wie in der Sektion; die Quote nur
                    // noch fuer aeltere Datensaetze ohne das Feld.
                    spritReserveAbstand(record.sprit) != null
                      ? `${record.sprit.reserve.status === "intakt" ? "✓" : "⚠"} ${
                          spritReserveAbstand(record.sprit)! > 0 ? "+" : spritReserveAbstand(record.sprit)! < 0 ? "−" : ""
                        }${spritKg(Math.abs(spritReserveAbstand(record.sprit)!))} kg`
                      : record.sprit.reserve.status === "intakt"
                      ? `✓ ${Math.round(record.sprit.reserve.quote_pct)} %`
                      : record.sprit.reserve.status === "unterschritten"
                        ? `⚠ ${Math.round(record.sprit.reserve.quote_pct)} %`
                        : t("landing.sprit.badge_np")
                  }
                />
              )}
              {record.sprit?.extra_getankt_kg != null && record.sprit.extra_genutzt_kg != null && (
                <ReportTile
                  label={t("landing.sprit.report_extra")}
                  value={record.sprit.extra_getankt_kg <= 0 ? t("landing.sprit.extra_keins") : `${spritKg(record.sprit.extra_getankt_kg)} kg`}
                  detail={
                    record.sprit.extra_getankt_kg <= 0
                      ? undefined
                      : t("landing.sprit.extra_detail", {
                          genutzt: spritKg(record.sprit.extra_genutzt_kg),
                          ungenutzt: spritKg(record.sprit.extra_ungenutzt_kg),
                        })
                  }
                />
              )}
              {(record.planned_burn_kg != null ||
                record.actual_trip_burn_kg != null) && (
                <ReportTile
                  label={`${t("landing.trip_burn")} · ${t(
                    "landing.actual_burn",
                  )}`}
                  value={fmtNumber(record.actual_trip_burn_kg, 0, "kg")}
                />
              )}
              {record.planned_burn_kg != null && (
                <ReportTile
                  label={`${t("landing.trip_burn")} · ${t(
                    "landing.plan_burn",
                  )}`}
                  value={fmtNumber(record.planned_burn_kg, 0, "kg")}
                />
              )}
              {/* v1.7.35: Diese beiden stammen aus der abgeschafften Achse und
                  rechnen ueber den GANZEN Trip — neben den Phasen oben waere das
                  eine dritte, anders gerechnete Prozentzahl. Sie bleiben nur fuer
                  Datensaetze ohne Auswertung (vor v1.7.35). */}
              {!record.sprit && record.fuel_efficiency_kg_diff != null && (
                <ReportTile
                  label={t("landing.report.fuel_diff")}
                  value={fmtSigned(record.fuel_efficiency_kg_diff, 0, "kg")}
                />
              )}
              {!record.sprit && record.fuel_efficiency_pct != null && (
                <ReportTile
                  label={t("landing.report.fuel_efficiency")}
                  value={fmtSigned(record.fuel_efficiency_pct, 1, "%")}
                />
              )}
              {record.block_fuel_kg != null && (
                <ReportTile
                  label={t("landing.block_fuel")}
                  value={fmtNumber(record.block_fuel_kg, 0, "kg")}
                />
              )}
              {record.landing_fuel_kg != null && (
                <ReportTile
                  label={t("landing.landing_fuel")}
                  value={fmtNumber(record.landing_fuel_kg, 0, "kg")}
                />
              )}
              {record.takeoff_weight_kg != null && (
                <ReportTile
                  label={t("landing.tow")}
                  value={fmtNumber(record.takeoff_weight_kg, 0, "kg")}
                />
              )}
              {record.landing_weight_kg != null && (
                <ReportTile
                  label={t("landing.ldw")}
                  value={fmtNumber(record.landing_weight_kg, 0, "kg")}
                />
              )}
            </div>
          ) : (
            <div className="report-empty">
              {t("landing.report.no_data")}
            </div>
          )}
        </ReportSection>
      </div>

      {/* ── Bordbuch (27.09.2026) ─────────────────────────────────────
          Kein Seitenumbruch davor: die Checkliste ist kurz und soll nicht
          allein auf einem Blatt stehen. */}
      {bordbuch && (
        <ReportSection title={t("bordbuch.titel")}>
          <BordbuchBericht eintrag={bordbuch} />
        </ReportSection>
      )}

      {/* ── Profile & Verläufe (frische Seite) ─────────────────────── */}
      {hasCharts && (
        <div className="report-break-before">
          <ReportSection title={t("landing.report.charts_section")}>
            <div className="report-charts">
              {hasApproach && (
                <ReportChartCard
                  caption={t("landing.report.approach_chart")}
                >
                  <ApproachChart
                    samples={record.approach_samples}
                    glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
                  />
                </ReportChartCard>
              )}
              {hasCloseup && (
                <ReportChartCard caption={t("landing.report.vs_curve")}>
                  <VsCurveChart profile={record.touchdown_profile} />
                </ReportChartCard>
              )}
            </div>
          </ReportSection>
        </div>
      )}

      {/* EINE Fußzeile — am Ende des Fließtextes, VOR dem Anhang.

          Hinter der Querformat-Grafik bekam sie eine eigene, sonst leere
          Seite: vierzig Zeichen auf einem Blatt. Was nach einem benannten
          `@page` kommt, braucht wieder eine eigene Seite. Die Grafik ist
          jetzt ein abgesetzter Anhang, die Fußzeile schliesst den
          Fließtext ab. */}
      <ReportFooter />

      {/* Die Bahn-Grafik bekommt eine eigene QUERFORMAT-Seite.

          Gemessen im gedruckten PDF (`scripts/bericht-messen.py`): In der
          hochkant gesetzten Textspalte landeten ihre Beschriftungen bei
          **4,3 pt** — als einzige Stelle im ganzen Bericht unter der
          Lesbarkeitsschwelle; alles andere lag bei 7,1 pt und darüber.

          Der Grund ist Geometrie, kein Fehler: Das SVG skaliert auf die
          Spaltenbreite, und die Schrift darin skaliert mit. Hochkant sind
          das 170 mm, quer 273 mm — Faktor 1,6.

          Die Schrift anzuheben hat nicht gereicht: Für 6,8 pt bräuchte es
          17 SVG-Einheiten, und dabei zerfällt das Layout an 31 Stellen
          (siehe `BERICHT_SCHRIFT_MINDEST`). Die Seite zu drehen ändert die
          Geometrie statt das Layout. */}
      {v2Props && (
        // Nur die Karte, kein zusaetzlicher Abschnitt: Beide trugen
        // denselben Titel, und der Abschnittstitel landete allein auf
        // einer sonst leeren Seite, weil der Inhalt darunter nicht mehr
        // draufpasste. Gemessen im gedruckten PDF, nicht vermutet.
        <div className="report-bahn-quer">
          <ReportChartCard caption={t("landing.report.runway_diagram")}>
            <RunwayDiagramV2
              {...v2Props}
              schriftMindest={BERICHT_SCHRIFT_MINDEST}
            />
          </ReportChartCard>
        </div>
      )}

    </div>
  );
}

export function LandingDetail({
  record,
  allRecords,
  onBack,
  onDelete,
  isPreview,
  pruefstatus,
}: {
  /** Stand beim Live-Server (Integritäts-Gate), falls bekannt. */
  pruefstatus?: PirepPruefstatus;
  record: LandingRecord;
  /** Full history — used to compute personal-best comparisons. */
  allRecords: LandingRecord[];
  onBack: () => void;
  onDelete?: () => void;
  isPreview: boolean;
}) {
  const { t } = useTranslation();

  const callsign = record.airline_icao
    ? displayCallsign(record.airline_icao, record.flight_number)
    : record.flight_number;

  const subs = useMemo(() => computeSubScores(record), [record]);
  // Bordbuch dieses Flugs — im Tab und im PDF (in der Live-Vorschau gibt es
  // noch keinen fertigen Eintrag).
  const bordbuch = useBordbuchEintrag(isPreview ? null : record.pirep_id);

  // Personal-best comparison — best (closest to zero) landing rate
  // across ALL filed PIREPs. None when this is the only record yet.
  //
  // v0.20.0: ueber scoreBasisVs() statt landing_rate_fpm. Das Rohfeld ist der
  // Streamer-Tick; gescort und angezeigt wird der Edge-Wert. Auf dem Rohfeld
  // zu ranken kuert bei Alt-Datensaetzen die falsche "beste Landung" und
  // stellt eine Zahl in die Kopfzeile, die neben der Kachel nicht aufgeht.
  const personalBest = useMemo(() => {
    const others = allRecords.filter((r) => r.pirep_id !== record.pirep_id);
    // Nur gemessene Landungen können eine Bestleistung sein.
    const messbar = others.filter(istBewertbar);
    if (messbar.length === 0) return null;
    return messbar.reduce(
      (best, r) =>
        Math.abs(scoreBasisVs(r) ?? 0) < Math.abs(scoreBasisVs(best) ?? 0) ? r : best,
      messbar[0],
    );
  }, [allRecords, record.pirep_id]);

  // Eine nicht gemessene Landung kann keine Bestleistung schlagen.
  const isNewBest =
    personalBest != null &&
    istBewertbar(record) &&
    Math.abs(scoreBasisVs(record) ?? 0) < Math.abs(scoreBasisVs(personalBest) ?? 0);

  // v0.12.8-dev: PDF-Export-State. Sobald `printing` true wird, rendert
  // der Effect den <LandingReport> ins DOM, ruft `window.print()` und
  // setzt nach `afterprint` wieder zurück.
  //
  // Feld-Bug (2026-08-05, macOS): der Button tat nach dem ersten Klick
  // GAR NICHTS mehr. Ursache: `afterprint` ist in WKWebView (macOS-
  // Tauri-Build) nicht zuverlässig — feuert das Event einmal nicht,
  // bleibt `printing` fuer immer `true`. Der Button-Klick ruft dann nur
  // noch `setPrinting(true)` auf einen bereits-`true`-Wert — React sieht
  // keine State-Aenderung, der Effect laeuft nie wieder, `window.print()`
  // wird nie wieder aufgerufen. Zwei Absicherungen:
  //   1. `window.print()` selbst in try/catch — wirft es (oder existiert
  //      gar nicht), zeigen wir sofort einen sichtbaren Fehler statt
  //      still nichts zu tun.
  //   2. Ein Fallback-Timeout setzt `printing` so oder so zurueck, falls
  //      `afterprint` nie kommt — der Button darf nie dauerhaft blockiert
  //      bleiben. 20 s laesst genug Zeit fuer einen echten, vom Piloten
  //      bedienten Speichern-Dialog (das Druck-Snapshot wird von
  //      WKWebView/WebView2 SOFORT bei `window.print()` erfasst, nicht
  //      laufend aus dem Live-DOM neu gelesen — den Portal danach zu
  //      entfernen stoert einen bereits offenen echten Dialog nicht).
  //      Trade-off: feuert der Timeout, OBWOHL der Pilot nur langsam war,
  //      zeigen wir faelschlich die Fehlermeldung — im Zweifel besser als
  //      gar kein Feedback bei einem echten Fehlschlag.
  const [printing, setPrinting] = useState(false);
  const [printFailed, setPrintFailed] = useState(false);
  // Der Druck wartet, bis das Bordbuch dieser Landung geladen ist (über die
  // LAN-Oberfläche dauert das), höchstens 3 s — sonst fehlte es im PDF.
  const [bordbuchWarteEnde, setBordbuchWarteEnde] = useState(false);
  useEffect(() => {
    if (!printing) {
      setBordbuchWarteEnde(false);
      return;
    }
    if (bordbuch.bereit) return;
    const t = window.setTimeout(() => setBordbuchWarteEnde(true), 3_000);
    return () => window.clearTimeout(t);
  }, [printing, bordbuch.bereit]);
  const druckLos = printing && (bordbuch.bereit || bordbuchWarteEnde);
  useEffect(() => {
    if (!druckLos) return;
    let settled = false;
    const settle = (failed: boolean) => {
      if (settled) return;
      settled = true;
      if (failed) setPrintFailed(true);
      setPrinting(false);
    };
    const onAfterPrint = () => settle(false);
    // Einen Frame warten, damit der Report (inkl. Charts) im DOM ist,
    // bevor der Druckdialog aufgeht.
    const raf = requestAnimationFrame(() => {
      window.addEventListener("afterprint", onAfterPrint, { once: true });
      try {
        if (typeof window.print !== "function") {
          throw new Error("window.print is not available");
        }
        window.print();
      } catch (err) {
        Sentry.captureException(err, {
          tags: { feature: "landing-pdf-export" },
        });
        settle(true);
      }
    });
    const timeout = window.setTimeout(() => settle(true), 20_000);
    return () => {
      cancelAnimationFrame(raf);
      window.clearTimeout(timeout);
      window.removeEventListener("afterprint", onAfterPrint);
    };
  }, [druckLos]);

  return (
    <div className="landing-detail">
      <div className="landing-detail__top">
        <button type="button" className="landing-back" onClick={onBack}>
          ← {t("landing.back_to_list")}
        </button>
        <button
          type="button"
          className="landing-export"
          onClick={() => {
            setPrintFailed(false);
            setPrinting(true);
          }}
          title={t("landing.report.button")}
        >
          🖨 {t("landing.report.button")}
        </button>
        {!isPreview && onDelete && (
          <button
            type="button"
            className="landing-delete"
            onClick={onDelete}
            title={t("landing.delete")}
          >
            🗑 {t("landing.delete")}
          </button>
        )}
      </div>

      {printFailed && (
        <div role="alert" className="landing-export-error">
          {t("landing.report.export_failed")}
        </div>
      )}

      {/* v0.12.8-dev: Druck-Report — per Portal an document.body gehängt,
          auf dem Bildschirm display:none, nur sichtbar im @media print. */}
      {printing &&
        createPortal(
          <div className="landing-report-print">
            <LandingReport record={record} bordbuch={bordbuch.eintrag} />
          </div>,
          document.body,
        )}

      {/* ── Landung erkannt, aber nicht gemessen ─────────────────────────
          Untersuchung 12.09.2026 (CFG 2090): Eine Landung erhielt 97 Punkte
          und die Note A+, obwohl im Aufsetzmoment 0,92 s lang keine Probe
          ankam. Seitdem gibt es dafür keine Note mehr — und diese Zeile
          sagt dem Piloten, warum. Sie steht ÜBER der Kopfzeile, damit
          niemand erst nach der fehlenden Zahl sucht. */}
      {/* Prüfstatus beim Live-Server (Befund DLH 880, 15.09.2026): Hängt
          der Flug im Integritäts-Gate, soll der Pilot das hier sehen —
          und ebenso, wenn er inzwischen freigegeben wurde. */}
      {!isPreview && <PruefstatusKasten status={pruefstatus} />}

      {/* Kasten „nicht bewertbar", Kopf, Banner, Hinweise und Teilnoten:
          gespiegelt (LandungsBewertung.tsx) — dieselbe Darstellung in der
          Webapp. */}
      <NichtBewertbarKasten record={record} />
      <LandungsKopf
        record={record}
        callsign={callsign}
        isPreview={isPreview}
        isNewBest={isNewBest}
        personalBest={personalBest}
      />
      {/* v0.7.19 GAF-707 Accident-Detection: rot/gelber Banner als
          Primary-Klassifikation OBERHALB von Off-Airport + Score-
          Breakdown. GAF 707 darf hier nicht als normale Hard-Landing
          erscheinen. Spec §AeroACARS Client Tab "Landung".
          v0.7.18 (B-012): Off-airport-Banner wenn der Touchdown nicht
          beim geplanten Destination-Airport war. Quelle ist die
          backend-resolution (runway_match / nearest_25nm / planned_fallback).
          v0.5.47 — Quick-Flag-Chips direkt unter dem Headline-Block.
          Pilot sieht auf einen Blick was die Auffälligkeiten sind.
          Webapp hat das schon; jetzt auch im Client für visuelle Parität. */}
      <AccidentBanner record={record} />
      <OffAirportBanner record={record} />
      <QuickFlags record={record} />
      <BewertungsAbschnitt record={record} subs={subs} />

      {/* Runway */}
      {record.runway_match && (() => {
        // v0.7.6 P1-3: Runway-Geometry-Trust check.
        // - trusted ?? true → alte v0.7.5-PIREPs werden wie trusted
        //   behandelt (Backward-Compat).
        // - Bei untrusted: Centerline-Offset, Past-Threshold, runway_used_pct
        //   und das RunwayDiagram ausblenden. Rollout bleibt sichtbar
        //   (kommt aus GPS-Track).
        // - "no_runway_match" zeigt KEINEN Alarm-Pill (Privatplatz normal).
        const geometryTrusted = record.runway_geometry_trusted ?? true;
        return (
          <section className="landing-section">
            {/* Titel, Erklärung, Warnung bei unsicherer Geometrie:
                gespiegelt (LandungsBewertung.tsx, BahnUeberschrift). */}
            <BahnUeberschrift record={record} />
            {(() => {
              // v0.8.2: alte RunwayDiagram → RunwayDiagramV2.
              //
              // v0.8.3.1 (Hotfix): die legacy <dl>-Liste die hier vorher
              // ZUSAETZLICH rendert wurde entfernt — V2 hat alle Felder
              // (bahn/laenge/hinter-schwelle/mittellinie/rollout/bahn-
              // auslastung/navdata/tdz/aim/tch/dds) als eigene Pills.
              // Vorher liefen beide parallel und zeigten WIDERSPRECHENDE
              // Werte: Bahn-Auslastung 52% (V2: (td_dist+rollout)/length)
              // vs 38% (legacy: rollout/length). Reported von Thomas
              // 2026-05-18 mit Fenix-A320 EVRA-Landung.
              //
              // Bei untrusted geometry NICHTS rendern — die trust-Warn-
              // Box oberhalb erklaert dem Piloten warum (vorher zeigten
              // einige legacy-Felder auch bei untrusted weiter, was
              // inkonsistent zur V2-Logik war).
              //
              // Bei v2Props=null trotz trusted geometry → das ist ein
              // Mapping-Bug, kein UI-Fallback. Tritt nicht auf weil
              // mapLandingRecordToV2Props bei trusted records komplett
              // ist (alle Pflichtfelder kommen aus record.runway_match,
              // das bei trusted=true garantiert vollstaendig ist).
              if (!geometryTrusted) return null;
              const v2Props = mapLandingRecordToV2Props(record);
              return v2Props ? <RunwayDiagramV2 {...v2Props} /> : null;
            })()}
          </section>
        );
      })()}

      {/* Touchdown-Werte + Windkompass: gespiegelt (TouchdownAbschnitt.tsx). */}
      <TouchdownAbschnitt record={record} />

      {/* METAR am Zielflughafen: gespiegelt (MetarAbschnitt.tsx). */}
      <MetarAbschnitt metar={record.arr_metar} />

      {/* Approach stability — v0.11.0-dev: 7-Kacheln-Card analog zur
          aeroacars-live-Webapp (V/S-Jerk, Bank σ, IAS σ, Sink Rate,
          Landing-Config, V/S vs. 3°-ILS, Max V/S-Dev <500ft) plus
          STABLE-GATE-Pill und Coaching. Der alte schmale Stability-
          Indicator (nur σ-V/S und σ-Bank) ist abgelöst — alle Werte
          kommen direkt aus dem Backend (compute_approach_stability_v2),
          die Card rendert nur. Der Approach-Chart darunter bleibt. */}
      <ApproachStabilityCard
        vsJerkFpm={record.approach_vs_jerk_fpm}
        bankStddevDeg={record.approach_bank_stddev_deg}
        iasStddevKt={record.approach_ias_stddev_kt}
        excessiveSink={record.approach_excessive_sink}
        stableConfig={record.approach_stable_config}
        vsDeviationFpm={record.approach_vs_deviation_fpm}
        maxVsDeviationBelow500Fpm={
          record.approach_max_vs_deviation_below_500_fpm
        }
        usedHat={record.approach_used_hat}
        sampleCount={
          record.gate_window?.sample_count ?? record.approach_samples.length
        }
        simKind={record.sim_kind}
        glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
        gate={gateAus(record.sub_scores)}
        marke={gateMarke(record.sub_scores)}
        runwayChangedLate={record.approach_runway_changed_late}
        stableAtDa={record.approach_stable_at_da}
        stallWarningCount={record.approach_stall_warning_count}
      />
      {/* Aufsetz-Qualität (keine Note): gespiegelt (LandingQualitaet.tsx). */}
      <LandingQualitaet record={record} />
      {/* Lernpaket AP4/AP5: Gleitpfad + Anflugruhe als Info-Zeilen,
          ohne Note und ohne Farbband. */}
      <AnflugForensikInfo
        gleitpfad={record.anflug_gleitpfad}
        ruhe={record.anflug_ruhe}
      />
      {/* Anfluggrafik + 50-Hz-Nahaufnahme: gespiegelt (AnflugGrafik.tsx). */}
      <AnflugGrafikAbschnitt
        samples={record.approach_samples}
        profile={record.touchdown_profile}
        glideslopeAngleDeg={record.runway_match?.glideslope_angle_deg}
      />

      {/* v0.7.8: Sinkrate-Forensik — erklaert dem Piloten warum die
          Landerate so ist wie sie ist. Spec docs/spec/v0.7.8-landing-rate-
          explainability.md. Rendert nur wenn 50-Hz-Forensik-Felder
          vorhanden sind (hasForensics()), sonst kompakter Legacy-Hinweis. */}
      <SinkrateForensik record={record} />

      {/* v0.7.17 (B-009): G-Force-Forensik — analog zur Sinkrate-Forensik.
          Erklaert warum AeroACARS bei butterweichen Landungen manchmal hohe
          G-Werte misst (Sim-Strut-Compression statt echtem Pilot-Impact)
          und der Master-Score trotzdem als „Smooth" klassifiziert wird. */}
      {/* Die G-Forensik stammt komplett aus dem Aufsetzfenster. Reichte es
          nicht, entfällt die Sektion — auch bei gültiger MSFS-Sinkrate
          (Prüfbefund 13.09.2026: sonst volle G-Kacheln für ungemessene Daten). */}
      {fensterWerteGueltig(record) && <GForceForensik record={record} />}

      {/* Abfangbogen (keine Note): gespiegelt (FlareAbschnitt.tsx). */}
      <FlareAbschnitt record={record} />


      {/* Bordbuch dieses Flugs — was der Pilot an SOPs abgehakt hat. */}
      <BordbuchLandungsAbschnitt eintrag={bordbuch.eintrag} onMarkieren={bordbuch.markieren} />

      {/* Treibstoff + Gewicht (Sprit-Auswertung, Soll/Ist): gespiegelt
          (LadeblattAbschnitt.tsx). */}
      <LadeblattAbschnitt record={record} />

      {/* Rohdaten zur Nachprüfung: gespiegelt (RohdatenAbschnitt.tsx). */}
      <RohdatenAbschnitt record={record} />
    </div>
  );
}

// ---- Stats summary across all landings ----------------------------------

// ---- Main panel ---------------------------------------------------------

/** Landungen-Übersicht (7a) §2/§5 aggregate — one pass over `records` feeds
 *  both the header stats and the footer's softest/hardest/bounce values, so
 *  header and footer can never disagree about which landing is "the" worst. */
function useOverviewStats(records: LandingRecord[]) {
  return useMemo(() => {
    if (records.length === 0) return null;
    const total = records.length;
    // Siehe oben: nicht gemessene Landungen tragen zu keiner Kennzahl bei.
    const messbar = records.filter(istBewertbar);
    // Codex, zweite Abnahme 13.09.2026: Bei ausschliesslich ungemessenen
    // Landungen stand hier "Ø Sinkrate 0 fpm", "Ø Score 0,0" und eine
    // ungemessene Landung als weichste und härteste. Ohne Messung gibt es
    // diese Kennzahlen nicht — `null`, und die Anzeige zeigt einen Strich.
    const avgRate: number | null = messbar.length
      ? messbar.reduce((s, r) => s + (scoreBasisVs(r) ?? 0), 0) / messbar.length
      : null;
    // Auch die Durchschnittsnote zählt nur bewertete Landungen — sonst
    // zöge eine nicht gemessene Landung den Schnitt (Codex-Abnahme
    // 12.09.2026: gesperrt mit 97 und gültig mit 80 ergaben 88,5).
    const bewertete = records.filter((r) => r.score_numeric != null);
    const avgScore: number | null = bewertete.length
      ? bewertete.reduce((s, r) => s + (r.score_numeric ?? 0), 0) / bewertete.length
      : null;
    const byCategory: Record<LandingCategory, number> = {
      smooth: 0,
      acceptable: 0,
      firm: 0,
      hard: 0,
      severe: 0,
    };
    let softest: LandingRecord | null = messbar[0] ?? null;
    let hardest: LandingRecord | null = messbar[0] ?? null;
    for (const r of records) {
      // Nicht bewertete Landungen zählen in keinen Kategorie-Topf — sonst
      // stünden sie unter "fest" (Prüfbefund 13.09.2026).
      const kat = recordCategory(r);
      if (kat != null) byCategory[kat]++;
      if (!istBewertbar(r)) continue;
      if (softest == null || Math.abs(scoreBasisVs(r) ?? 0) < Math.abs(scoreBasisVs(softest) ?? 0)) softest = r;
      if (hardest == null || Math.abs(scoreBasisVs(r) ?? 0) > Math.abs(scoreBasisVs(hardest) ?? 0)) hardest = r;
    }
    // Hopser nur aus gemessenen Landungen: Bei ungemessenen ist die Zahl
    // nicht belastbar (ein Hopser zwischen zwei fehlenden Proben fehlt).
    const totalBounces = records
      .filter(fensterWerteGueltig)
      .reduce((s, r) => s + r.bounce_count, 0);
    return { total, avgRate, avgScore, byCategory, softest, hardest, totalBounces };
  }, [records]);
}

const OVERVIEW_PAGE_SIZE = 15;

export function LandingPanel() {
  const { t } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [records, setRecords] = useState<LandingRecord[]>([]);
  const [preview, setPreview] = useState<LandingRecord | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [sortAsc, setSortAsc] = useState(false);
  const [visibleCount, setVisibleCount] = useState(OVERVIEW_PAGE_SIZE);
  const [airportNames, setAirportNames] = useState<Record<string, string>>({});
  const nachladeGedaechtnisRef = useRef<NachladeGedaechtnis>(leeresGedaechtnis());
  const stats = useOverviewStats(records);
  // Prüfstatus beim Live-Server für die jüngsten Flüge (Befund DLH 880).
  const pruefIds = useMemo(
    () =>
      [...records]
        .sort((a, b) => Date.parse(b.touchdown_at) - Date.parse(a.touchdown_at))
        .slice(0, 50)
        .map((r) => r.pirep_id),
    [records],
  );
  const pruefstatus = usePirepPruefstatus(pruefIds);

  // QS 2026-08-04: `refresh` läuft alle 5 s. Dauert ein Abruf einmal
  // länger als der Takt (träger VPS/phpVMS), überholen sich zwei
  // Anfragen — und die ÄLTERE Antwort überschrieb dann klaglos die
  // neuere, weil hier weder ein Abbruch-Merker noch eine laufende Nummer
  // geprüft wurde. Die laufende Nummer stellt sicher, dass nur die
  // jeweils jüngste Anfrage den Zustand setzen darf. Steigt zusätzlich
  // beim Aushängen, damit eine noch offene Anfrage nach dem Verlassen
  // des Tabs nichts mehr schreibt.
  const refreshSeqRef = useRef(0);

  async function refresh() {
    const seq = ++refreshSeqRef.current;
    setLoading(true);
    try {
      const [list, current] = await Promise.all([
        invoke<LandingRecord[]>("landing_list"),
        invoke<LandingRecord | null>("landing_get_current"),
      ]);
      if (seq !== refreshSeqRef.current) return;
      setRecords(list);
      setPreview(current ?? null);
    } catch (e) {
      console.warn("landing_list failed", e);
    } finally {
      if (seq === refreshSeqRef.current) setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
    // Refresh the preview every 5 s while we're on this tab so the
    // pilot sees their landing scores updating live during rollout.
    const t = setInterval(refresh, 5000);
    return () => {
      clearInterval(t);
      refreshSeqRef.current++;
    };
  }, []);

  // Landungen-Übersicht (7a) §4/§6 "Ortsnamen aus der Flughafentabelle" —
  // same cache-by-ICAO pattern as BidsList's dpt-airport lookup, just for
  // both ends of the route. Falls back to the bare ICAO if a lookup fails.
  // QS-Runde 7: der Effekt hing an `records`. Das Array wird alle 5 s frisch
  // geladen und hat damit jedes Mal eine neue Identitaet — der Effekt lief also
  // alle 5 s neu und brach die noch laufende Warteschlange ab. Er haengt jetzt
  // an der MENGE der Flughaefen: die aendert sich nur, wenn wirklich eine neue
  // Landung dazukommt. Der `useMemo`-Schluessel ist eine Zeichenkette, damit der
  // Vergleich ueber den Inhalt geht und nicht ueber die Objektidentitaet.
  const icaoSchluessel = useMemo(() => {
    const menge = new Set<string>();
    for (const r of records) {
      if (r.dpt_airport) menge.add(r.dpt_airport);
      if (r.arr_airport) menge.add(r.arr_airport);
    }
    return [...menge].sort().join(",");
  }, [records]);

  useEffect(() => {
    const icaos = icaoSchluessel ? icaoSchluessel.split(",") : [];
    // Nebenläufigkeit gedeckelt (Feldbefund 18.08.2026, BTI22): diese Schleife
    // startete FÜR JEDEN Flughafen sofort einen Abruf — bei einem Piloten mit
    // langer Historie waren das 89 gleichzeitig in zwei Sekunden. Jeder braucht
    // eine eigene Verbindung; ein parallel laufender Abruf (die Bid-Liste)
    // bekam keine mehr und lief in den Verbindungs-Timeout. Der Pilot sah einen
    // Netzwerkfehler mitten im Reiseflug.
    //
    // Vier gleichzeitig ist die übliche Browser-Hausnummer pro Gegenstelle. Die
    // Gesamtdauer leidet kaum: die Antworten sind winzig, und der Rust-Teil
    // puffert sie prozessweit — beim zweiten Öffnen des Reiters geht gar keine
    // Anfrage mehr raus.
    // Die Buchhaltung steckt in `namenNachladen.ts` — Deckel, Abbruch-Freigabe
    // und Versuchsgrenze hängen so eng zusammen, dass sie zusammen prüfbar sein
    // müssen. Im Bauch dieser Komponente waren sie es nicht, und genau dort ist
    // der schwerste Fehler dieser Runde entstanden (QS 18.08.2026).
    return ladeNamenGedeckelt(
      icaos,
      nachladeGedaechtnisRef.current,
      async (icao) => {
        const info = await invoke<{ name: string | null }>("airport_get", { icao });
        return info?.name ?? null;
      },
      (icao, name) => setAirportNames((prev) => ({ ...prev, [icao]: name })),
    );
  }, [icaoSchluessel]);

  async function handleDelete(id: string) {
    if (
      !(await confirm({
        message: t("landing.confirm_delete"),
        destructive: true,
      }))
    )
      return;
    try {
      await invoke("landing_delete", { pirepId: id });
      setSelectedId(null);
      await refresh();
    } catch (e) {
      console.warn("landing_delete failed", e);
    }
  }

  // Landungen-Übersicht (7a) §4 — default sort: Datum absteigend (bereits die
  // Backend-Reihenfolge, aber explizit sortiert statt sich darauf zu
  // verlassen, damit der Umschalt-Knopf beide Richtungen korrekt bedient.
  // MUSS vor dem frühen `return` des Detail-Zweigs unten stehen — ein Hook
  // nach einem bedingten `return` verletzt die Rules of Hooks (React #300,
  // "Bericht öffnen" crashte deswegen live: der Hook wurde beim ersten
  // Render mit selectedId=null gezählt, beim zweiten mit selectedId gesetzt
  // übersprungen → Hook-Anzahl änderte sich zwischen den Renders).
  const sortedRecords = useMemo(() => {
    const arr = [...records];
    arr.sort((a, b) => {
      const ta = Date.parse(a.touchdown_at);
      const tb = Date.parse(b.touchdown_at);
      return sortAsc ? ta - tb : tb - ta;
    });
    return arr;
  }, [records, sortAsc]);
  const visibleRecords = sortedRecords.slice(0, visibleCount);

  // Detail view
  if (selectedId) {
    const rec = records.find((r) => r.pirep_id === selectedId);
    if (rec) {
      return (
        <section className="landing-panel">
          {confirmDialog}
          <LandingDetail
            record={rec}
            allRecords={records}
            onBack={() => setSelectedId(null)}
            onDelete={() => handleDelete(rec.pirep_id)}
            isPreview={false}
            pruefstatus={pruefstatus[rec.pirep_id]}
          />
        </section>
      );
    }
  }

  // Preview-only state (active flight has touched down but record not yet filed)
  // v1.3.5 (#Landungen-7a): root is `.landing-overview`, NOT `.landing-panel`
  // — that class is shared with the (untouched) detail view above and its
  // padded/max-width/shrink-wrapped CSS would break this screen's full-height
  // header/chart/table/footer frame.
  return (
    <section className="landing-overview">
      {confirmDialog}
      {preview && (
        <div className="landing-preview-card">
          <h3>{t("landing.live_preview")}</h3>
          <LandingDetail
            record={preview}
            allRecords={records}
            onBack={() => setPreview(null)}
            isPreview={true}
          />
        </div>
      )}

      <header className="landing-ov-header">
          <div className="landing-ov-header__title">
            <h2>{t("landing.ov_title")}</h2>
            <span className="landing-ov-header__sub">
              {t("landing.ov_subtitle", { count: records.length })}
            </span>
          </div>
          <div className="landing-ov-stats">
            <div className="landing-ov-stat">
              <span className="landing-ov-stat__label">{t("landing.ov_avg_rate")}</span>
              <span className="landing-ov-stat__value">
                {stats?.avgRate != null ? (
                  <>
                    {fmtDeDe(Math.round(stats.avgRate))}
                    <span className="landing-ov-stat__unit"> fpm</span>
                  </>
                ) : (
                  "—"
                )}
              </span>
            </div>
            <div className="landing-ov-stat">
              <span className="landing-ov-stat__label">{t("landing.ov_avg_score")}</span>
              <span className="landing-ov-stat__value">
                {stats?.avgScore != null ? fmtDeDe(stats.avgScore, 1) : "—"}
              </span>
            </div>
            <div className="landing-ov-stat">
              <span className="landing-ov-stat__label">{t("landing.ov_distribution")}</span>
              <span className="landing-ov-stat__value">
                {stats ? (
                  <>
                    {(["smooth", "acceptable", "firm", "hard", "severe"] as LandingCategory[]).map(
                      (cat, i) => (
                        <span key={cat}>
                          {i > 0 && <span className="landing-ov-dim"> · </span>}
                          <span className={`landing-ov-cat--${cat}`}>{fmtDeDe(stats.byCategory[cat])}</span>
                        </span>
                      ),
                    )}
                  </>
                ) : (
                  "—"
                )}
              </span>
            </div>
          </div>
        </header>

        {/* v1.6.3: Hinweis, wenn Werte aus verschiedenen Bewertungs-Ständen
            nebeneinanderstehen. Die Sinkraten-Umstellung verschiebt sowohl
            die Zahl als auch die Punkte; ohne Hinweis liest sich der Sprung
            in Durchschnitt, Kurve und Bestwert wie ein Fehler. Die
            Release-Notes kündigen ihn an — die Oberfläche schwieg dazu. */}
        {gemischteBewertungsstaende(records) && (
          <div className="landing-ov-mixed-note">
            {t("landing.ov_mixed_versions")}
          </div>
        )}

        <LandingRateChart records={records} />

        <div className="landing-ov-tablewrap">
          <table className="landing-ov-table">
            <colgroup>
              <col style={{ width: 128 }} />
              <col />
              <col style={{ width: 168 }} />
              <col style={{ width: 150 }} />
              <col style={{ width: 108 }} />
              <col style={{ width: 84 }} />
              <col style={{ width: 26 }} />
            </colgroup>
            <thead>
              <tr>
                <th scope="col" aria-sort={sortAsc ? "ascending" : "descending"}>
                  <button
                    type="button"
                    className="landing-ov-table__sortbtn"
                    onClick={() => setSortAsc((v) => !v)}
                    aria-label={
                      sortAsc ? t("landing.ov_sort_desc_action") : t("landing.ov_sort_asc_action")
                    }
                  >
                    {t("landing.col_when")}
                    <span aria-hidden="true">{sortAsc ? "↑" : "↓"}</span>
                  </button>
                </th>
                <th scope="col">{t("landing.col_route")}</th>
                <th scope="col">{t("landing.col_aircraft")}</th>
                <th scope="col" className="landing-ov-table__num">
                  {t("landing.col_rate")}
                </th>
                <th scope="col" className="landing-ov-table__num">
                  {t("landing.col_score")}
                </th>
                <th scope="col" className="landing-ov-table__num">
                  {t("landing.col_grade")}
                </th>
                <th scope="col" aria-hidden="true" />
              </tr>
            </thead>
            <tbody>
              {loading &&
                records.length === 0 &&
                Array.from({ length: 5 }).map((_, i) => (
                  <tr key={`skeleton-${i}`} className="landing-ov-skeleton-row" aria-hidden="true">
                    <td colSpan={7}>
                      <div className="landing-ov-skeleton-bar" />
                    </td>
                  </tr>
                ))}
              {!loading && records.length === 0 && (
                <tr className="landing-ov-empty-row">
                  <td colSpan={7}>{t("landing.no_landings")}</td>
                </tr>
              )}
              {visibleRecords.map((r) => {
                // `null` heisst: nicht gemessen. Die Zeile bleibt in der
                // Liste — der Flug fand ja statt —, nur die Sinkrate fehlt.
                const gemessen = scoreBasisVs(r);
                const rate = gemessen ?? 0;
                const cat = recordCategory(r) ?? "unbewertet";
                const pattern = r.aircraft_icao || r.aircraft_title || "—";
                const reg = r.aircraft_registration || "—";
                const dep = r.dpt_airport;
                const arr = r.arr_airport;
                const callsign = displayCallsign(r.airline_icao, r.flight_number);
                return (
                  <tr
                    key={r.pirep_id}
                    className="landing-ov-row"
                    tabIndex={0}
                    onClick={() => setSelectedId(r.pirep_id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setSelectedId(r.pirep_id);
                      }
                    }}
                    aria-label={t("landing.ov_row_aria", {
                      date: fmtDateUtc(r.touchdown_at),
                      dep,
                      arr,
                      rate: fmtDeDe(Math.round(rate)),
                    })}
                  >
                    <td>
                      <span className="landing-ov-date__d">{fmtDateUtc(r.touchdown_at)}</span>
                      <span className="landing-ov-date__t">{fmtTimeUtcZ(r.touchdown_at)}</span>
                    </td>
                    <td>
                      <span className="landing-ov-route__l1">
                        {dep} → {arr}
                        <PruefstatusMarke status={pruefstatus[r.pirep_id]} />
                      </span>
                      <span className="landing-ov-route__l2">
                        {callsign} · {airportNames[dep] ?? dep} → {airportNames[arr] ?? arr}
                      </span>
                    </td>
                    <td className="landing-ov-aircraft">
                      {pattern} · {reg}
                    </td>
                    <td className="landing-ov-table__num">
                      <span className="landing-ov-rate-cell">
                        <span className="landing-ov-rate-bar">
                          <span
                            className={`landing-ov-rate-bar__fill landing-ov-rate-bar__fill--${cat}`}
                            style={{ width: `${Math.min(100, (Math.abs(rate) / CHART_MAX_FPM) * 100)}%` }}
                          />
                        </span>
                        <span className={`landing-ov-rate-value landing-ov-rate-value--${cat}`}>
                          {gemessen != null
                            ? fmtDeDe(Math.round(gemessen))
                            : t("landing.nicht_bewertbar.kein_wert")}
                        </span>
                      </span>
                    </td>
                    <td className="landing-ov-table__num landing-ov-score">
                      {r.score_numeric != null ? (
                        <>
                          {fmtDeDe(r.score_numeric)}
                          <span className="landing-ov-score__suffix">/100</span>
                        </>
                      ) : (
                        <span className="landing-ov-score__suffix">
                          {t("landing.nicht_bewertbar.kein_wert")}
                        </span>
                      )}
                    </td>
                    {/* Just the letter, colour-coded by the same category as
                        the sinkrate cell — no word: after seeing SANFT/FEST/
                        HART-style category words next to the letter live,
                        the pilot found two labels for one grade harder to
                        parse than the letter alone, and the letter is
                        already real per-row text (not a colour-only cue),
                        so a11y's "never colour alone" concern still holds. */}
                    <td className={`landing-ov-table__num landing-ov-grade landing-ov-grade--${cat}`}>
                      {r.grade_letter}
                    </td>
                    <td className="landing-ov-chevron" aria-hidden="true">
                      ›
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>

        <footer className="landing-ov-footer">
          <div className="landing-ov-footer__left">
            <span>
              {t("landing.ov_footer_count", {
                shown: Math.min(visibleCount, sortedRecords.length),
                total: records.length,
              })}
            </span>
            {visibleCount < sortedRecords.length && (
              <button
                type="button"
                className="landing-ov-loadmore"
                onClick={() => setVisibleCount((v) => v + OVERVIEW_PAGE_SIZE)}
              >
                {t("landing.ov_load_more")}
              </button>
            )}
          </div>
          {stats && (
            <div className="landing-ov-footer__right">
              <span>
                {t("landing.ov_footer_bounces_label")}{" "}
                <strong>{fmtDeDe(stats.totalBounces)}</strong>
              </span>
              {stats.softest && (
                <span>
                  {t("landing.ov_footer_softest_label")}{" "}
                  <strong>{fmtDeDe(Math.round(scoreBasisVs(stats.softest) ?? 0))} fpm</strong> ·{" "}
                  {fmtDateShortUtc(stats.softest.touchdown_at)}{" "}
                  {stats.softest.touchdown_airport ?? stats.softest.arr_airport}
                </span>
              )}
              {stats.hardest && (
                <span>
                  {t("landing.ov_footer_hardest_label")}{" "}
                  <strong>{fmtDeDe(Math.round(scoreBasisVs(stats.hardest) ?? 0))} fpm</strong> ·{" "}
                  {fmtDateShortUtc(stats.hardest.touchdown_at)}{" "}
                  {stats.hardest.touchdown_airport ?? stats.hardest.arr_airport}
                </span>
              )}
            </div>
          )}
        </footer>
    </section>
  );
}
