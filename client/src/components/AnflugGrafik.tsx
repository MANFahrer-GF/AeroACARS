// Anfluggrafik (Sinkrate über den Anflug mit Soll-Band aus dem Gleitpfad)
// und 50-Hz-Nahaufnahme um das Aufsetzen.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 lebten beide Grafiken in LandingPanel.tsx; die Webapp hatte eine
// eigene Portierung (_ApproachChart.tsx) mit anderen Beschriftungen und
// Legenden. Jetzt zeigen Client und Webapp dieselbe Grafik aus diesem Code.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDruck } from "../lib/druck";
import {
  gleitwinkelFaktor,
  sollband,
  sollbandPfad,
  sollbandStuecke,
  SOLLBAND_TOLERANZ_FPM,
} from "../lib/anflugSollband";
import type { ApproachSample, LandingProfilePoint } from "../lib/landungsDatensatz";
import type { GleitpfadPunkt } from "./AnflugForensikInfo";
import "./anflugGrafik.css";
import { InfoBadge } from "./InfoBadge";

// v0.12.8: Touchdown-Nahaufnahme — 50-Hz-Window, exakt wie auf dem VPS.
// Zeitbasierte X-Achse (−4 s … +3 s), Touchdown als senkrechte Linie,
// Auto-Zoom-Y, Gridlines, Zonen vor-Flare/Flare/nach-TD, Hover-Tooltip.
export function VsCurveChart({ profile }: { profile: LandingProfilePoint[] }) {
  const { t } = useTranslation();
  const [hover, setHover] = useState<number | null>(null);

  // Auf das vereinbarte Fenster −4 s … +3 s beschneiden.
  // v0.12.8: Fenster −4 s … +10 s nach TD (zeigt, was der 50-Hz-Buffer
  // hergibt — typischerweise ~8 s post-TD).
  const data = profile.filter((p) => p.t_ms >= -4000 && p.t_ms <= 10000);
  if (data.length < 5) {
    return (
      <div className="landing-chart landing-chart--empty">
        {t("landing.no_profile")}
      </div>
    );
  }

  const w = 1120;
  const h = 320;
  const pad = { top: 20, right: 20, bottom: 52, left: 64 };
  const innerW = w - pad.left - pad.right;
  const innerH = h - pad.top - pad.bottom;

  const vss = data.map((p) => p.vs_fpm);
  let lo = Math.min(...vss);
  let hi = Math.max(...vss);
  const padv = Math.max(60, (hi - lo) * 0.12);
  lo = Math.floor((lo - padv) / 100) * 100;
  hi = Math.ceil((hi + padv) / 100) * 100;
  if (hi < 0) hi = 0;
  const range = Math.max(1, hi - lo);

  const t0 = data[0]!.t_ms;
  const t1 = data[data.length - 1]!.t_ms;
  const tRange = Math.max(1, t1 - t0);
  const x = (tMs: number) => pad.left + ((tMs - t0) / tRange) * innerW;
  const y = (vs: number) => pad.top + innerH - ((vs - lo) / range) * innerH;

  const path = data
    .map((p, i) => `${i === 0 ? "M" : "L"} ${x(p.t_ms).toFixed(1)} ${y(p.vs_fpm).toFixed(1)}`)
    .join(" ");

  const step = range > 1400 ? 400 : 200;
  const gridVals: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) gridVals.push(v);

  const tdX = x(0);
  const preEnd = x(-3000);

  return (
    <svg
      className="landing-chart"
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={t("landing.vs_curve")}
      onMouseMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const sx = (e.clientX - rect.left) * (w / rect.width);
        const tMs = t0 + ((sx - pad.left) / innerW) * tRange;
        let best = 0;
        let bd = Infinity;
        data.forEach((p, i) => {
          const d = Math.abs(p.t_ms - tMs);
          if (d < bd) { bd = d; best = i; }
        });
        setHover(best);
      }}
      onMouseLeave={() => setHover(null)}
    >
      <rect x={pad.left} y={pad.top} width={innerW} height={innerH}
            fill="rgba(255,255,255,0.02)" stroke="rgba(255,255,255,0.15)" className="ag-rahmen" />

      {/* Zonen: vor Flare / Flare / nach TD */}
      {[
        { x0: pad.left, x1: Math.max(pad.left, Math.min(preEnd, pad.left + innerW)), fill: "rgba(56,189,248,0.10)" },
        { x0: Math.max(pad.left, preEnd), x1: Math.min(pad.left + innerW, tdX), fill: "rgba(234,179,8,0.16)" },
        { x0: Math.max(pad.left, tdX), x1: pad.left + innerW, fill: "rgba(248,113,113,0.13)" },
      ].map((z, i) =>
        z.x1 > z.x0 ? (
          <rect key={i} x={z.x0} y={pad.top} width={z.x1 - z.x0} height={innerH} fill={z.fill} />
        ) : null,
      )}

      {/* Gridlines */}
      {gridVals.map((v) => {
        const gy = y(v);
        const zero = v === 0;
        return (
          <g key={v}>
            <line x1={pad.left} y1={gy} x2={pad.left + innerW} y2={gy}
                  stroke={zero ? "#475569" : "rgba(255,255,255,0.07)"} className={zero ? undefined : "ag-gitter"}
                  strokeWidth={zero ? 1.6 : 1} />
            <text x={pad.left - 8} y={gy + 4} textAnchor="end" fontSize="12"
                  fill={zero ? "#94a3b8" : "#64748b"} className="ag-achse">{v}</text>
          </g>
        );
      })}

      {/* Touchdown-Linie */}
      <line x1={tdX} y1={pad.top} x2={tdX} y2={pad.top + innerH}
            stroke="#f87171" strokeWidth="1.4" strokeDasharray="4 3" />
      <text x={tdX} y={pad.top - 6} textAnchor="middle" fontSize="11" fill="#f87171">
        {t("landing.touchdown")}
      </text>

      <path d={path} fill="none" stroke="#38bdf8" strokeWidth="2" />

      <text x={pad.left} y={h - 28} fontSize="12" fill="#94a3b8" className="ag-achse">
        {(t0 / 1000).toFixed(1)} s
      </text>
      <text x={pad.left + innerW} y={h - 28} textAnchor="end" fontSize="12" fill="#94a3b8" className="ag-achse">
        +{(t1 / 1000).toFixed(1)} s
      </text>
      <text x={16} y={pad.top + innerH / 2} fontSize="11" fill="#64748b" className="ag-achse" textAnchor="middle"
            transform={`rotate(-90 16 ${pad.top + innerH / 2})`}>
        {t("landing.vs_chart.axis")}
      </text>

      {hover != null && data[hover] && (() => {
        const p = data[hover]!;
        const hx = x(p.t_ms);
        const hy = y(p.vs_fpm);
        const tRel = p.t_ms / 1000;
        const tLabel = tRel <= 0
          ? t("landing.vs_chart.before_td", { s: Math.abs(tRel).toFixed(1) })
          : t("landing.vs_chart.after_td", { s: tRel.toFixed(1) });
        const boxW = 188;
        const boxX = Math.min(Math.max(hx + 12, pad.left), pad.left + innerW - boxW);
        const boxY = Math.max(hy - 46, pad.top + 2);
        return (
          <g pointerEvents="none">
            <line x1={hx} y1={pad.top} x2={hx} y2={pad.top + innerH}
                  stroke="#38bdf8" strokeWidth="1" strokeDasharray="3 3" />
            <circle cx={hx} cy={hy} r="4" fill="#38bdf8" stroke="#0e1420" strokeWidth="1.5" />
            <rect x={boxX} y={boxY} width={boxW} height={40} rx="5"
                  fill="#1e293b" stroke="#334155" />
            <text x={boxX + 9} y={boxY + 17} fontSize="12.5" fill="#38bdf8" fontWeight="700">
              {Math.round(p.vs_fpm)} fpm
              <tspan fill="#cbd5e1" fontWeight="400">{`  ·  ${tLabel}`}</tspan>
            </text>
            <text x={boxX + 9} y={boxY + 32} fontSize="11" fill="#94a3b8" className="ag-achse">
              AGL {Number.isFinite(p.agl_ft) ? Math.round(p.agl_ft) : "—"} ft  ·  {p.on_ground ? t("landing.on_ground") : t("landing.airborne")}
            </text>
          </g>
        );
      })()}
    </svg>
  );
}

// ---- Approach stability time-series chart ------------------------------

// v0.12.7: Anflug-V/S-Profil — Redesign. Auto-Zoom-Y (Kurve füllt die
// Fläche statt im festen −1500…+100-Band zu verschwinden), Gridlines +
// 0-Linie, Soll-Band −600…−900, gestrichelte Stabilitätsgrenze −1000,
// Hover-Tooltip. Spec: Pilot-Befund Michel/GSG.
// v0.13.15: Fractional sample-index for the touchdown line (t_ms = 0).
// Approach samples carry the (up to ~0.5 s late) streamer-tick timestamp,
// so the first sample with t_ms >= 0 frequently sat ~0.5 s AFTER the real
// touchdown — the old `findIndex(t_ms >= 0)` placed the red line there,
// right of the curve's end. We instead interpolate between the last pre-TD
// and the first post-TD sample by time, returning a fractional index that
// the (index-linear) x() maps to the exact t = 0 position. Falls back to
// the last sample when no post-TD sample exists. Exported for unit tests.
export function approachTdLineIndex(
  samples: { t_ms?: number | null }[],
): number {
  if (samples.length === 0) return 0;
  const firstPos = samples.findIndex((s) => s.t_ms != null && s.t_ms >= 0);
  if (firstPos < 0) return samples.length - 1; // kein Post-TD-Sample
  if (firstPos === 0) return 0;
  const tPrev = samples[firstPos - 1]!.t_ms ?? 0;
  const tCur = samples[firstPos]!.t_ms ?? 0;
  const span = tCur - tPrev;
  const frac = span > 0 ? Math.min(1, Math.max(0, (0 - tPrev) / span)) : 0;
  return firstPos - 1 + frac;
}

/** Dot-Streifen: Achse mindestens ±3 (am PFD ist bei 2 Dots Vollausschlag),
 *  bei größeren Abweichungen bis ±6 mitwachsend — nahe der Schwelle werden
 *  Dots sehr empfindlich (THY39: +5 bei 100 ft), und eine flach am Rand
 *  klebende Linie sähe aus wie ein Messwert. Erst jenseits von 6 wird
 *  abgeschnitten. */
const DOT_ACHSE_MIN = 3;
const DOT_ACHSE_MAX = 6;
/** Farbe der Dot-Kurve: auf dunklem Bildschirm und hellem Papier lesbar. */
const DOT_FARBE = "#8b5cf6";

/** Dots zu einer Zeit (ms relativ zum Aufsetzen), linear zwischen den
 *  Punkten der Kurve; `null` außerhalb der Kurve. */
export function dotsZurZeit(verlauf: GleitpfadPunkt[], tMs: number): number | null {
  const ts = tMs / 1000;
  for (let i = 1; i < verlauf.length; i++) {
    const a = verlauf[i - 1]!;
    const b = verlauf[i]!;
    if (ts >= a.t && ts <= b.t) {
      if (b.t === a.t) return b.d;
      return a.d + ((b.d - a.d) * (ts - a.t)) / (b.t - a.t);
    }
  }
  return null;
}

/** Bruch-Index der Anflugspur zu einer Zeit — die Spur ist im Index
 *  gezeichnet, die Kurve in Sekunden. `null` außerhalb der Spur. */
function indexZurZeit(samples: ApproachSample[], tMs: number): number | null {
  for (let i = 1; i < samples.length; i++) {
    const a = samples[i - 1]!.t_ms;
    const b = samples[i]!.t_ms;
    if (a == null || b == null) return null;
    if (tMs >= a && tMs <= b) return b === a ? i : i - 1 + (tMs - a) / (b - a);
  }
  return null;
}

/** Die Kurvenpunkte, die der Dot-Streifen zeichnen kann — in der Zeitspanne
 *  der Spur, und nur, wenn die Spur Zeiten trägt. Eine Quelle für Streifen,
 *  (i) und den Hinweis „fehlt" (QS 06.10.2026: ohne `t_ms` in der Spur fehlte
 *  der Streifen, das (i) dazu stand trotzdem da). */
export function dotStreifenPunkte(
  samples: ApproachSample[],
  verlauf: GleitpfadPunkt[] | null | undefined,
): { p: GleitpfadPunkt; i: number }[] {
  return (verlauf ?? [])
    .map((p) => ({ p, i: indexZurZeit(samples, p.t * 1000) }))
    .filter((q): q is { p: GleitpfadPunkt; i: number } => q.i != null);
}

export function ApproachChart({
  samples,
  glideslopeAngleDeg,
  gleitpfadVerlauf,
}: {
  samples: ApproachSample[];
  /** v0.15.18: echter Gleitpfad-Winkel (Navdaten). Skaliert Soll-Band +
   *  Stabilitätsgrenze 1:1 wie das Backend. null/3° → unverändert. */
  glideslopeAngleDeg?: number | null;
  /** 05.10.2026: Gleitpfad-Abweichung je Probe (Client-Messung). Mit ihr
   *  bekommt die Grafik den Dot-Streifen auf derselben Zeitachse. */
  gleitpfadVerlauf?: GleitpfadPunkt[] | null;
}) {
  const { t, i18n } = useTranslation();
  // Dots in der Sprache der App („+2,22"), wie alle anderen Werte.
  const dotZahl = (v: number) =>
    v.toLocaleString(i18n.language, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const druck = useDruck();
  const [hover, setHover] = useState<number | null>(null);
  if (samples.length < 3) return null;

  // v0.15.18: Soll-Band + Stabilitätsgrenze mit dem ECHTEN Gleitwinkel
  // skalieren — identisch zum Backend (compute_approach_stability_v2:
  // gs_factor = tan(g)/tan(3°), Plausibilitäts-Clamp 2–7,5°, sonst ×1,0).
  // Bei 3°/unbekannt bleibt alles bei −600…−900 / −1000 (bit-identisch zu
  // vorher). Steilanflüge (ENTC 4°, EGLC 5,5°) bekommen die korrekte, tiefere
  // Linie — sonst „stürzt" die V/S-Spur scheinbar unter eine falsche −1000-
  // Marke, obwohl das Backend (korrekt skaliert) den Anflug gar nicht flaggt.
  const gsFactor = gleitwinkelFaktor(glideslopeAngleDeg);
  // Soll-Band aus der echten Geschwindigkeit über Grund — dieselbe Rechnung
  // wie die Bewertung (siehe lib/anflugSollband.ts). Aufzeichnungen ohne
  // gs_kt (vor v1.7.35) behalten den alten Richtwert, klar als solcher
  // beschriftet, statt ein Soll zu erfinden.
  const sollPunkte = sollband(samples, glideslopeAngleDeg);
  const hatSollband = sollPunkte.length > 0;
  const bandHi = -600 * gsFactor;
  const bandLo = -900 * gsFactor;
  const limitFpm = -1000 * gsFactor;
  const isScaledGp = gsFactor !== 1;
  const fmtFpm = (v: number) => `−${Math.abs(Math.round(v / 10) * 10)}`;

  const w = 1120;
  const hOben = 320;
  const pad = { top: 20, right: 20, bottom: 52, left: 64 };
  const innerW = w - pad.left - pad.right;
  const innerH = hOben - pad.top - pad.bottom;

  // Dot-Streifen (05.10.2026): unter der Sinkrate, gleiche Zeitachse. Nur
  // die Kurvenpunkte, die in der Zeitspanne der Spur liegen.
  const dotPunkte = dotStreifenPunkte(samples, gleitpfadVerlauf);
  const mitStreifen = dotPunkte.length >= 2;
  const streifen = { top: hOben + 26, h: 200 };
  const h = mitStreifen ? streifen.top + streifen.h + 34 : hOben;
  const DOT_ACHSE = Math.min(
    DOT_ACHSE_MAX,
    Math.max(DOT_ACHSE_MIN, Math.ceil(Math.max(0, ...dotPunkte.map((q) => Math.abs(q.p.d))))),
  );
  const dotTicks = Array.from({ length: 2 * DOT_ACHSE + 1 }, (_, k) => DOT_ACHSE - k);
  const yDot = (d: number) =>
    streifen.top +
    streifen.h / 2 -
    (Math.max(-DOT_ACHSE, Math.min(DOT_ACHSE, d)) / DOT_ACHSE) * (streifen.h / 2);
  // Markiert wird der größte Wert im Stable Gate (1000–200 ft) unter den
  // gezeichneten Punkten. Der Verlauf ist auf einen Punkt je Sekunde
  // ausgedünnt — die Beschriftung sagt deshalb „im Verlauf"; die
  // Gleitpfad-Forensik rechnet über alle Proben (QS Runde 13). Unter 200 ft
  // werden Dots nahe der Schwelle sehr empfindlich (THY39: +5 bei 100 ft).
  const imGate = dotPunkte.filter((q) => q.p.h >= 200 && q.p.h <= 1000);
  const groessteAbw =
    imGate.length > 0
      ? imGate.reduce((m, q) => (Math.abs(q.p.d) > Math.abs(m.p.d) ? q : m))
      : null;

  // Auto-Zoom-Y auf den echten Wertebereich (+12 % Polster), auf 100er
  // gerundet. 0-Linie bleibt immer sichtbar.
  const vss = samples.map((s) => s.vs_fpm);
  let lo = Math.min(...vss);
  let hi = Math.max(...vss);
  const padv = Math.max(60, (hi - lo) * 0.12);
  lo = Math.floor((lo - padv) / 100) * 100;
  hi = Math.ceil((hi + padv) / 100) * 100;
  if (hi < 0) hi = 0;
  // v0.12.8: Achse IMMER mindestens 0 … −1100 — sonst klebt bei einem
  // ruhigen Anflug das Soll-Band unten und die Stabilitätslinie fällt raus.
  // v0.15.18: mit dem Gleitwinkel mit-skaliert, damit die (ggf. tiefere)
  // Grenze + Band auch bei Steilanflügen im Bild bleiben.
  lo = Math.min(lo, Math.floor((limitFpm - 100) / 100) * 100);
  // Das Soll-Band hängt von der Geschwindigkeit ab und kann bei schnellen
  // Anflügen tiefer liegen als Kurve und Grenze — dann mit ins Bild nehmen,
  // statt es unbemerkt am Achsenrand abzuschneiden.
  if (sollPunkte.length > 0) {
    const tiefste = Math.min(...sollPunkte.map((p) => p.unten));
    lo = Math.min(lo, Math.floor((tiefste - 60) / 100) * 100);
  }
  const range = Math.max(1, hi - lo);

  const xStep = innerW / Math.max(1, samples.length - 1);
  const x = (i: number) => pad.left + i * xStep;
  const y = (vs: number) => pad.top + innerH - ((vs - lo) / range) * innerH;
  const clampY = (v: number) => Math.min(Math.max(y(v), pad.top), pad.top + innerH);

  const path = samples
    .map((s, i) => `${i === 0 ? "M" : "L"} ${x(i).toFixed(1)} ${y(s.vs_fpm).toFixed(1)}`)
    .join(" ");

  const zoneOf = (s: ApproachSample): "vorlauf" | "gate" | "flare" =>
    s.is_flare ? "flare" : s.is_scored_gate ? "gate" : "vorlauf";
  const hasZones = samples.some((s) => s.is_scored_gate != null);
  const zones: { start: number; end: number; kind: "vorlauf" | "gate" | "flare" }[] = [];
  if (hasZones) {
    let i = 0;
    while (i < samples.length) {
      const kind = zoneOf(samples[i]!);
      let j = i;
      while (j + 1 < samples.length && zoneOf(samples[j + 1]!) === kind) j++;
      zones.push({ start: i, end: j, kind });
      i = j + 1;
    }
  }
  // Grenzen des Stable Gate auf der Zeitachse — im Streifen als Linien statt
  // Fläche: die Flächenfarbe mischte sich mit den Ampelbändern.
  const gateZone = zones.find((z) => z.kind === "gate");
  const zoneFill = (k: string) =>
    k === "gate" ? "rgba(56,189,248,0.10)"
    : k === "flare" ? "rgba(234,179,8,0.16)"
    : "rgba(120,120,120,0.10)";

  const step = range > 1400 ? 400 : 200;
  const gridVals: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) gridVals.push(v);

  // v0.13.15 (Pilot-Befund ViolonC 2026-05-31): TD-Linie EXAKT bei
  // t_ms = 0 setzen. x() ist linear im Sample-Index, also liefert der
  // (ggf. gebrochene) Index aus approachTdLineIndex die korrekte X-Position.
  const tdX = x(approachTdLineIndex(samples));
  const tdNearRight = tdX > pad.left + innerW - 70;

  const bandTop = clampY(bandHi);
  const bandBottom = clampY(bandLo);
  const limitVisible = limitFpm > lo && limitFpm < hi;

  return (
    <svg
      className="landing-chart"
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={t("landing.approach_chart")}
      // Pointer statt Maus: Antippen auf dem iPhone zeigt den Wert genauso.
      // Nach dem Antippen bleibt er stehen; nur die Maus nimmt ihn beim
      // Verlassen wieder weg.
      onPointerMove={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const sx = (e.clientX - rect.left) * (w / rect.width);
        let k = Math.round((sx - pad.left) / xStep);
        k = Math.max(0, Math.min(samples.length - 1, k));
        setHover(k);
      }}
      onPointerDown={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        const sx = (e.clientX - rect.left) * (w / rect.width);
        let k = Math.round((sx - pad.left) / xStep);
        k = Math.max(0, Math.min(samples.length - 1, k));
        setHover(k);
      }}
      onPointerLeave={(e) => {
        if (e.pointerType === "mouse") setHover(null);
      }}
    >
      <rect x={pad.left} y={pad.top} width={innerW} height={innerH}
            fill="rgba(255,255,255,0.02)" stroke="rgba(255,255,255,0.15)" className="ag-rahmen" />

      {zones.map((z, idx) => {
        const x0 = z.start > 0 ? (x(z.start - 1) + x(z.start)) / 2 : x(z.start) - 2;
        const x1 = z.end < samples.length - 1 ? (x(z.end) + x(z.end + 1)) / 2 : x(z.end) + 2;
        return <rect key={idx} x={x0} y={pad.top} width={Math.max(0, x1 - x0)}
                     height={innerH} fill={zoneFill(z.kind)} />;
      })}

      {hatSollband
        ? sollbandStuecke(sollPunkte).map((stueck, idx) => (
            <path key={idx} d={sollbandPfad(stueck, x, clampY)}
                  fill="rgba(34,197,94,0.16)" />
          ))
        : bandBottom > bandTop && (
            <rect x={pad.left} y={bandTop} width={innerW} height={bandBottom - bandTop}
                  fill="rgba(34,197,94,0.16)" />
          )}

      {gridVals.map((v) => {
        const gy = y(v);
        const zero = v === 0;
        return (
          <g key={v}>
            <line x1={pad.left} y1={gy} x2={pad.left + innerW} y2={gy}
                  stroke={zero ? "#475569" : "rgba(255,255,255,0.07)"} className={zero ? undefined : "ag-gitter"}
                  strokeWidth={zero ? 1.6 : 1} />
            <text x={pad.left - 8} y={gy + 4} textAnchor="end" fontSize="12"
                  fill={zero ? "#94a3b8" : "#64748b"} className="ag-achse">{v}</text>
          </g>
        );
      })}

      {limitVisible && (
        <g>
          <line x1={pad.left} y1={y(limitFpm)} x2={pad.left + innerW} y2={y(limitFpm)}
                stroke="#f87171" strokeWidth="1.2" strokeDasharray="6 4" opacity="0.7" />
          <text x={pad.left + innerW - 6} y={y(limitFpm) - 5} textAnchor="end"
                fontSize="10" fill="#f87171" opacity="0.85">
            {isScaledGp
              ? t("landing.vs_chart.limit_custom", {
                  fpm: fmtFpm(limitFpm),
                  angle: String(glideslopeAngleDeg),
                })
              : t("landing.vs_chart.limit", { fpm: fmtFpm(limitFpm) })}
          </text>
        </g>
      )}

      <line x1={tdX} y1={pad.top} x2={tdX} y2={pad.top + innerH}
            stroke="#f87171" strokeWidth="1.4" strokeDasharray="4 3" />
      <text x={tdNearRight ? tdX - 6 : tdX} y={pad.top - 6}
            textAnchor={tdNearRight ? "end" : "middle"} fontSize="11" fill="#f87171">
        {t("landing.touchdown")}
      </text>

      <path d={path} fill="none" stroke="#38bdf8" strokeWidth="2" />

      <text x={pad.left} y={hOben - 28} fontSize="12" fill="#94a3b8" className="ag-achse">
        {t("landing.approach_start")}
      </text>
      <text x={pad.left + innerW} y={hOben - 28} textAnchor="end" fontSize="12" fill="#94a3b8" className="ag-achse">
        {t("landing.touchdown")}
      </text>
      <text x={16} y={pad.top + innerH / 2} fontSize="11" fill="#64748b" className="ag-achse" textAnchor="middle"
            transform={`rotate(-90 16 ${pad.top + innerH / 2})`}>
        {t("landing.vs_chart.axis")}
      </text>

      {hasZones && (
        <g fontSize="11" fill="currentColor">
          <rect x={pad.left} y={hOben - 14} width={9} height={9} fill="rgba(120,120,120,0.4)" />
          <text x={pad.left + 13} y={hOben - 6}>{t("landing.chart_zone.vorlauf")}</text>
          <rect x={pad.left + 78} y={hOben - 14} width={9} height={9} fill="rgba(56,189,248,0.4)" />
          <text x={pad.left + 91} y={hOben - 6}>{t("landing.chart_zone.gate")}</text>
          <rect x={pad.left + 160} y={hOben - 14} width={9} height={9} fill="rgba(234,179,8,0.4)" />
          <text x={pad.left + 173} y={hOben - 6}>{t("landing.chart_zone.flare")}</text>
          <rect x={pad.left + 230} y={hOben - 14} width={9} height={9} fill="rgba(34,197,94,0.4)" />
          <text x={pad.left + 243} y={hOben - 6}>
            {hatSollband
              ? t("landing.vs_chart.band_gs", {
                  angle: String(isScaledGp ? glideslopeAngleDeg : 3),
                  tol: SOLLBAND_TOLERANZ_FPM,
                })
              : t("landing.vs_chart.band_legacy", {
                  hi: fmtFpm(bandHi),
                  lo: fmtFpm(bandLo),
                })}
          </text>
        </g>
      )}

      {mitStreifen && (
        <g>
          <text x={pad.left} y={streifen.top - 8} fontSize="13" fontWeight="600" fill="#94a3b8" className="ag-achse">
            {t("landing.vs_chart.gleitpfad_titel")}
          </text>
          {/* Bänder wie die Stufen des Gleitpfads im Anflug-Urteil: gut
              unter 1 Dot, schlecht ab 2 Dots. */}
          {(
            [
              [2, DOT_ACHSE, "rgba(248,113,113,0.13)"],
              [1, 2, "rgba(234,179,8,0.13)"],
              [-1, 1, "rgba(34,197,94,0.12)"],
              [-2, -1, "rgba(234,179,8,0.13)"],
              [-DOT_ACHSE, -2, "rgba(248,113,113,0.13)"],
            ] as const
          ).map(([u, o, f]) => (
            <rect key={u} x={pad.left} y={yDot(o)} width={innerW}
                  height={yDot(u) - yDot(o)} fill={f} />
          ))}
          <rect x={pad.left} y={streifen.top} width={innerW} height={streifen.h}
                fill="none" stroke="rgba(148,163,184,0.45)" />
          {gateZone && (() => {
            const x0 = gateZone.start > 0 ? (x(gateZone.start - 1) + x(gateZone.start)) / 2 : x(gateZone.start);
            const x1 = gateZone.end < samples.length - 1 ? (x(gateZone.end) + x(gateZone.end + 1)) / 2 : x(gateZone.end);
            return (
              <g>
                {[x0, x1].map((gx) => (
                  <line key={gx} x1={gx} y1={streifen.top} x2={gx} y2={streifen.top + streifen.h}
                        stroke="#38bdf8" strokeWidth="1.2" strokeDasharray="6 4" />
                ))}
                <text x={(x0 + x1) / 2} y={streifen.top + 14} textAnchor="middle" fontSize="12"
                      fill="#38bdf8">
                  {t("landing.chart_zone.gate")}
                </text>
              </g>
            );
          })()}
          {dotTicks.map((d) => (
            <g key={d}>
              <line x1={pad.left} y1={yDot(d)} x2={pad.left + innerW} y2={yDot(d)}
                    stroke={d === 0 ? "#64748b" : "rgba(148,163,184,0.18)"} className={d === 0 ? undefined : "ag-gitter"}
                    strokeDasharray={d === 0 ? "5 4" : undefined} />
              <text x={pad.left - 8} y={yDot(d) + 4} textAnchor="end" fontSize="12"
                    fill={d === 0 ? "#94a3b8" : "#64748b"} className="ag-achse">
                {d > 0 ? `+${d}` : d === 0 ? "0" : `−${Math.abs(d)}`}
              </text>
            </g>
          ))}
          <line x1={tdX} y1={streifen.top} x2={tdX} y2={streifen.top + streifen.h}
                stroke="#f87171" strokeWidth="1.4" strokeDasharray="4 3" />
          <path
            d={dotPunkte
              .map((q, k) => `${k === 0 ? "M" : "L"} ${x(q.i).toFixed(1)} ${yDot(q.p.d).toFixed(1)}`)
              .join(" ")}
            fill="none" stroke={DOT_FARBE} strokeWidth="3" />
          {groessteAbw && (
            <g>
              <circle cx={x(groessteAbw.i)} cy={yDot(groessteAbw.p.d)} r="5" fill={DOT_FARBE} />
              <text x={x(groessteAbw.i) + (x(groessteAbw.i) > pad.left + innerW - 300 ? -8 : 8)}
                    textAnchor={x(groessteAbw.i) > pad.left + innerW - 300 ? "end" : "start"}
                    y={yDot(groessteAbw.p.d) + (groessteAbw.p.d > 0 ? 16 : -8)}
                    fontSize="13" fontWeight="600" fill={DOT_FARBE}>
                {t("landing.vs_chart.gleitpfad_max", {
                  d: `${groessteAbw.p.d > 0 ? "+" : "−"}${dotZahl(Math.abs(groessteAbw.p.d))}`,
                  h: Math.round(groessteAbw.p.h),
                })}
              </text>
            </g>
          )}
          <text x={16} y={streifen.top + streifen.h / 2} fontSize="11" fill="#64748b" className="ag-achse"
                textAnchor="middle"
                transform={`rotate(-90 16 ${streifen.top + streifen.h / 2})`}>
            {t("landing.vs_chart.gleitpfad_achse")}
          </text>
          <text x={pad.left} y={streifen.top + streifen.h + 18} fontSize="11" fill="#94a3b8" className="ag-achse">
            {/* „Rechts beim Darüberfahren …" ist Bedienung — im Druck nur die
                Lesart (QS 06.10.2026). */}
            {druck ? t("landing.vs_chart.gleitpfad_lesart_druck") : t("landing.vs_chart.gleitpfad_lesart")}
          </text>
        </g>
      )}

      {hover != null && samples[hover] && (() => {
        const s = samples[hover]!;
        const hx = x(hover);
        const hy = y(s.vs_fpm);
        const tRel = (s.t_ms ?? 0) / 1000;
        const tLabel = tRel <= 0
          ? t("landing.vs_chart.before_td", { s: Math.abs(tRel).toFixed(1) })
          : t("landing.vs_chart.after_td", { s: tRel.toFixed(1) });
        const zoneLabel = s.is_flare
          ? t("landing.chart_zone.flare")
          : s.is_scored_gate
            ? t("landing.chart_zone.gate")
            : t("landing.chart_zone.vorlauf");
        const sollBeiHover = sollPunkte.find((p) => p.index === hover) ?? null;
        const dotsBeiHover =
          mitStreifen && s.t_ms != null ? dotsZurZeit(gleitpfadVerlauf!, s.t_ms) : null;
        const boxW = sollBeiHover != null ? 268 : 188;
        const boxX = Math.min(Math.max(hx + 12, pad.left), pad.left + innerW - boxW);
        const boxY = Math.max(hy - 46, pad.top + 2);
        return (
          <g pointerEvents="none">
            <line x1={hx} y1={pad.top} x2={hx}
                  y2={mitStreifen ? streifen.top + streifen.h : pad.top + innerH}
                  stroke="#38bdf8" strokeWidth="1" strokeDasharray="3 3" />
            {dotsBeiHover != null && (
              <g>
                <circle cx={hx} cy={yDot(dotsBeiHover)} r="5" fill={DOT_FARBE}
                        stroke="#0e1420" strokeWidth="1.5" />
                {/* Skala wie am PFD, rechts im Streifen: zwei Punkte je Seite.
                    Die Raute zeigt, wo der PFAD liegt — Flugzeug zu hoch,
                    Raute unter der Mitte. */}
                <rect x={pad.left + innerW - 46} y={streifen.top + 6} width={40}
                      height={streifen.h - 12} rx="4" fill="#0f172a" stroke="#334155" />
                {[-2, -1, 1, 2].map((d) => (
                  <circle key={d} cx={pad.left + innerW - 26} cy={yDot(d)} r="3.5"
                          fill="none" stroke="#94a3b8" />
                ))}
                <line x1={pad.left + innerW - 38} y1={yDot(0)} x2={pad.left + innerW - 14}
                      y2={yDot(0)} stroke="#e2e8f0" strokeWidth="1.5" />
                <path
                  d={`M ${pad.left + innerW - 26} ${yDot(-dotsBeiHover) - 7} l 7 7 l -7 7 l -7 -7 z`}
                  fill="#e879f9" />
              </g>
            )}
            <circle cx={hx} cy={hy} r="4" fill="#38bdf8" stroke="#0e1420" strokeWidth="1.5" />
            <rect x={boxX} y={boxY} width={boxW} height={dotsBeiHover != null ? 55 : 40} rx="5"
                  fill="#1e293b" stroke="#334155" />
            <text x={boxX + 9} y={boxY + 17} fontSize="12.5" fill="#38bdf8" fontWeight="700">
              {Math.round(s.vs_fpm)} fpm
              <tspan fill="#cbd5e1" fontWeight="400">{`  ·  ${tLabel}`}</tspan>
            </text>
            <text x={boxX + 9} y={boxY + 32} fontSize="11" fill="#94a3b8" className="ag-achse">
              {s.agl_ft != null ? `AGL ${Math.round(s.agl_ft)} ft  ·  ` : ""}{zoneLabel}
              {sollBeiHover != null
                ? `  ·  ${t("landing.vs_chart.target", { fpm: Math.round(sollBeiHover.soll) })}`
                : ""}
            </text>
            {dotsBeiHover != null && (
              <text x={boxX + 9} y={boxY + 47} fontSize="11" fill="#c4b5fd">
                {t(
                  dotsBeiHover >= 0
                    ? "landing.vs_chart.gleitpfad_ueber"
                    : "landing.vs_chart.gleitpfad_unter",
                  { d: dotZahl(Math.abs(dotsBeiHover)) },
                )}
              </text>
            )}
          </g>
        );
      })()}
    </svg>
  );
}

/** Der Abschnitt mit beiden Grafiken, wie Client und Webapp ihn zeigen.
 *  Jede Grafik erscheint, sobald ihre Messpunkte reichen (Anflug ab 3,
 *  Nahaufnahme ab 5) — unabhängig voneinander. */
export function AnflugGrafikAbschnitt({
  samples,
  profile,
  glideslopeAngleDeg,
  gleitpfadVerlauf,
}: {
  samples: ApproachSample[] | null | undefined;
  profile: LandingProfilePoint[] | null | undefined;
  glideslopeAngleDeg?: number | null;
  /** 05.10.2026: Gleitpfad je Probe (`anflug_gleitpfad.verlauf`). */
  gleitpfadVerlauf?: GleitpfadPunkt[] | null;
}) {
  const { t } = useTranslation();
  const anflug = samples != null && samples.length >= 3;
  const streifenDa = anflug && dotStreifenPunkte(samples!, gleitpfadVerlauf).length >= 2;
  const nah = profile != null && profile.length >= 5;
  if (!anflug && !nah) return null;
  return (
    <section className="landing-section">
      {anflug && (
        <>
          <h3>
            {t("landing.approach_stability")}
            <InfoBadge explanation={t("landing.erklaer.grafik.anflug")} />
          </h3>
          <div className="landing-stability-chart">
            <ApproachChart
              samples={samples}
              glideslopeAngleDeg={glideslopeAngleDeg}
              gleitpfadVerlauf={gleitpfadVerlauf}
            />
          </div>
          {streifenDa && (
            // Nur der (i)-Knopf zum Streifen — auf Papier überflüssig.
            <p className="landing-chart__hinweis nur-bildschirm">
              {t("landing.vs_chart.gleitpfad_titel")}{" "}
              <InfoBadge explanation={t("landing.erklaer.grafik.gleitpfad")} />
            </p>
          )}
          {!(gleitpfadVerlauf && gleitpfadVerlauf.length >= 2) && (
            <p className="landing-chart__hinweis">{t("landing.vs_chart.gleitpfad_fehlt")}</p>
          )}
          {/* Runde 13: Verlauf da, aber ohne gemeinsame Zeit mit der
              Anflugspur — sonst stand gar nichts da. */}
          {gleitpfadVerlauf && gleitpfadVerlauf.length >= 2 && !streifenDa && (
            <p className="landing-chart__hinweis">{t("landing.vs_chart.gleitpfad_ohne_zeitbezug")}</p>
          )}
        </>
      )}
      {nah && (
        <>
          <h3 style={anflug ? { marginTop: 18 } : undefined}>
            {t("landing.vs_curve_section")}
            <InfoBadge explanation={t("landing.erklaer.grafik.nah")} />
          </h3>
          <div className="landing-stability-chart">
            <VsCurveChart profile={profile} />
          </div>
        </>
      )}
    </section>
  );
}
