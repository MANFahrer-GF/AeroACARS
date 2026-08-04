// Nachflug-Profil: gestapelte Bahnen über einer gemeinsamen Zeitachse.
//
// WARUM NICHT EIN DIAGRAMM: Die Vorgängerversion zeichnete MSL, AGL und
// Gelände in EINE 40.000-ft-Skala. Auf einem Flug über flachem Land liegen
// MSL und AGL dann wenige hundert Fuß auseinander — bei dieser Skala unter
// einem Pixel. Die drei Kurven fielen zusammen, eine verdeckte die andere,
// und die Ansicht zeigte faktisch nichts. Mehrere Messgrößen mit
// verschiedenen Wertebereichen in einen Plot zu zwingen ist der klassische
// Fehler (Doppelachse); die Lösung sind getrennte Bahnen mit gemeinsamer
// X-Achse, damit man senkrecht vergleichen kann, ohne dass eine Skala die
// andere erdrückt.
//
// Die Höhenbahn zeichnet Gelände als gefüllte Silhouette UND MSL als Linie.
// Der senkrechte Abstand zwischen beiden IST die Höhe über Grund — das
// braucht keine dritte Kurve, sondern liest sich unmittelbar. Die eigene
// AGL-Bahn darunter existiert trotzdem, weil im Anflug die absoluten Fuß
// über Grund zählen und die auf einer Reiseflug-Skala unsichtbar wären.
//
// Farben sind gegen das Farbfehlsichtigkeits-Prüfskript des dataviz-Skills
// geprüft, getrennt für hell und dunkel. Die einzigen Paare, die je im
// selben Diagramm stehen, sind Blau↔Orange (Steigen/Sinken, ΔE 27–31) und
// Blau↔Grau (Höhe/Geschwindigkeit, ΔE 15–18).

import { useState } from "react";
import { useTranslation } from "react-i18next";

export interface ProfilePt {
  t?: number;
  alt_ft?: number;
  agl_ft?: number | null;
  gnd_ft?: number | null;
  spd_kt?: number;
  ias_kt?: number | null;
  vs_fpm?: number | null;
  fuel?: number | null;
}

type Series = { label: string; vals: (number | null)[]; color: string; dash?: boolean; width?: number };
type Band = {
  key: string;
  title: string;
  unit: string;
  series: Series[];
  /** Geländesilhouette (nur Höhenbahn). */
  fill?: (number | null)[];
  /** Nulllinie betonen und Skala symmetrisch — für Steig-/Sinkrate. */
  diverging?: boolean;
  height: number;
};

const W = 1000;
/** Achsenbeschriftung braucht Luft; ohne die klebt "40k" am oberen Rand. */
const PAD_T = 10;
const PAD_B = 14;

/** Hübsche Schrittweite, die nicht in 7er-Sprüngen beschriftet. */
function niceStep(span: number, targetLines = 4): number {
  const raw = span / targetLines;
  const mag = Math.pow(10, Math.floor(Math.log10(Math.max(raw, 1))));
  for (const m of [1, 2, 2.5, 5, 10]) {
    if (raw <= mag * m) return mag * m;
  }
  return mag * 10;
}

const fmtTick = (v: number, unit: string) => {
  const a = Math.abs(v);
  if (unit === "ft" && a >= 1000) return `${v / 1000}k`;
  if (unit === "kg" && a >= 1000) return `${(v / 1000).toFixed(a >= 10000 ? 0 : 1)}t`;
  return String(Math.round(v));
};

const hhmm = (ms: number) => {
  const m = Math.round(ms / 60000);
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
};

/**
 * v0.19.x FIX: the crosshair used to pick `hoverIdx` by treating the
 * mouse's X-percentage as a linear fraction of the point ARRAY (`round(
 * hoverPercent * (n-1))`) — but the crosshair line itself is drawn at
 * `x(i) = (t(i)/tMax) * W`, i.e. proportional to TIMESTAMP, not index.
 * Real telemetry samples aren't evenly time-spaced (cruise ticks are
 * sparser than approach ticks), so wherever sampling density varies the
 * two disagreed: the vertical line the pilot sees and the readout values
 * shown could belong to visibly different points on the timeline. This
 * inverts the SAME time-proportional mapping `x()` uses, so the crosshair
 * and the values it reports always refer to the same point.
 */
export function findHoverIndex(
  pts: ProfilePt[],
  hasTime: boolean,
  tMax: number,
  hoverPercent: number,
): number {
  const n = pts.length;
  if (n === 0) return 0;
  const pct = Math.max(0, Math.min(100, hoverPercent));
  if (!hasTime) {
    return Math.round((pct / 100) * (n - 1));
  }
  const targetT = (pct / 100) * tMax;
  // Binary search for the first point with t >= targetT — pts are
  // chronologically ordered telemetry, so t(i) is monotonic.
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((pts[mid].t ?? 0) < targetT) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0) {
    const before = pts[lo - 1].t ?? 0;
    const at = pts[lo].t ?? 0;
    if (Math.abs(before - targetT) <= Math.abs(at - targetT)) return lo - 1;
  }
  return lo;
}

export function FlightProfile({ route }: { route: ProfilePt[] }) {
  const { t } = useTranslation();
  const [hover, setHover] = useState<number | null>(null);

  const pts = route.filter((p) => typeof p.alt_ft === "number");
  if (pts.length < 2) {
    return <div className="aa-lb-muted" style={{ padding: 8 }}>{t("flight_profile.no_altitude_data")}</div>;
  }

  // Gemeinsame Zeitachse. Fehlt `t`, wird gleichmäßig über den Index verteilt —
  // dann stimmt die Form, nur die Abstände sind nicht zeitgetreu.
  const hasTime = pts.some((p) => typeof p.t === "number");
  const tMax = hasTime ? Math.max(...pts.map((p) => p.t ?? 0)) : pts.length - 1;
  const tOf = (p: ProfilePt, i: number) => (hasTime ? (p.t ?? 0) : i);

  const col = (light: string, dark: string) =>
    document.documentElement.dataset.theme === "dark" ? dark : light;
  const BLUE = col("#0a6fd1", "#0a84ff");
  const GREY = col("#6e6e73", "#98989d");
  const GREEN = col("#1a7f3c", "#22a04a");
  const ORANGE = col("#b45309", "#d97706");

  const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : null);
  const has = (arr: (number | null)[]) => arr.some((v) => v !== null);

  const msl = pts.map((p) => num(p.alt_ft));
  const gnd = pts.map((p) => num(p.gnd_ft));
  const agl = pts.map((p) => num(p.agl_ft));
  const ias = pts.map((p) => num(p.ias_kt));
  const gs = pts.map((p) => num(p.spd_kt));
  const vs = pts.map((p) => num(p.vs_fpm));
  const fuel = pts.map((p) => num(p.fuel));

  // Nur Bahnen zeigen, für die Daten vorliegen. Eine leere Achse mit dem
  // Titel "Treibstoff" behauptet, es gäbe nichts zu tanken.
  const bands: Band[] = [
    {
      key: "alt", title: t("flight_profile.title_alt"), unit: "ft", height: 150,
      series: [{ label: "MSL", vals: msl, color: BLUE, width: 2 }],
      fill: has(gnd) ? gnd : undefined,
    },
    has(agl) && {
      key: "agl", title: t("flight_profile.title_agl"), unit: "ft", height: 100,
      series: [{ label: "AGL", vals: agl, color: GREEN, width: 2 }],
    },
    (has(ias) || has(gs)) && {
      key: "spd", title: t("flight_profile.title_speed"), unit: "kt", height: 100,
      series: [
        ...(has(ias) ? [{ label: "IAS", vals: ias, color: BLUE, width: 2 }] : []),
        ...(has(gs) ? [{ label: "GS", vals: gs, color: GREY, width: 1.5, dash: true }] : []),
      ],
    },
    has(vs) && {
      key: "vs", title: t("flight_profile.title_vs"), unit: "fpm", height: 100,
      series: [{ label: "V/S", vals: vs, color: BLUE, width: 1.6 }],
      diverging: true,
    },
    has(fuel) && {
      key: "fuel", title: t("flight_profile.title_fuel"), unit: "kg", height: 90,
      series: [{ label: "Fuel", vals: fuel, color: ORANGE, width: 2 }],
    },
  ].filter(Boolean) as Band[];

  const x = (i: number) => (tMax === 0 ? 0 : (tOf(pts[i], i) / tMax) * W);

  const hoverIdx = hover === null ? null : findHoverIndex(pts, hasTime, tMax, hover);

  return (
    <div
      className="aa-fp"
      onMouseLeave={() => setHover(null)}
      onMouseMove={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        setHover(((e.clientX - r.left) / r.width) * 100);
      }}
    >
      {bands.map((b) => {
        const all = b.series.flatMap((s) => s.vals).filter((v): v is number => v !== null);
        const fillVals = (b.fill ?? []).filter((v): v is number => v !== null);
        let lo = Math.min(...all, ...fillVals, b.diverging ? 0 : Infinity);
        let hi = Math.max(...all, ...fillVals, b.diverging ? 0 : -Infinity);
        if (b.diverging) { const m = Math.max(Math.abs(lo), Math.abs(hi), 1); lo = -m; hi = m; }
        if (!b.diverging) { lo = Math.min(lo, 0); }
        if (hi === lo) hi = lo + 1;
        const step = niceStep(hi - lo);
        const gLo = Math.floor(lo / step) * step;
        const gHi = Math.ceil(hi / step) * step;
        const H = b.height;
        const y = (v: number) => PAD_T + (1 - (v - gLo) / (gHi - gLo)) * (H - PAD_T - PAD_B);

        const path = (vals: (number | null)[]) => {
          const runs: string[] = [];
          let cur: string[] = [];
          vals.forEach((v, i) => {
            if (v === null) { if (cur.length > 1) runs.push(cur.join(" ")); cur = []; return; }
            cur.push(`${x(i).toFixed(1)},${y(v).toFixed(1)}`);
          });
          if (cur.length > 1) runs.push(cur.join(" "));
          return runs;
        };

        // v0.19.x FIX: a missing terrain sample must NOT draw as 0 ft MSL —
        // that would be sea level under an aircraft over the Alps (see the
        // matching contract comment in LogbookView.tsx). One polygon per
        // gap-free run, closed at ITS OWN baseline endpoints, mirrors how
        // `path()` already breaks the MSL/AGL/speed lines into separate
        // segments at null gaps instead of interpolating through them.
        const fillRuns = (vals: (number | null)[]) => {
          const runs: string[] = [];
          let cur: string[] = [];
          let startIdx = -1;
          let endIdx = -1;
          const flush = () => {
            if (cur.length > 1) {
              runs.push(
                `${x(startIdx).toFixed(1)},${H - PAD_B} ${cur.join(" ")} ${x(endIdx).toFixed(1)},${H - PAD_B}`,
              );
            }
            cur = [];
            startIdx = -1;
          };
          vals.forEach((v, i) => {
            if (v === null) { flush(); return; }
            if (startIdx === -1) startIdx = i;
            endIdx = i;
            cur.push(`${x(i).toFixed(1)},${y(v).toFixed(1)}`);
          });
          flush();
          return runs;
        };

        const ticks: number[] = [];
        for (let v = gLo; v <= gHi + 1e-6; v += step) ticks.push(v);

        return (
          <section className="aa-fp-band" key={b.key}>
            <header>
              <span className="aa-fp-title">{b.title}</span>
              <span className="aa-fp-unit">{b.unit}</span>
              {b.series.length > 1 && (
                <span className="aa-fp-legend">
                  {b.series.map((s) => (
                    <span key={s.label}>
                      <i style={{ background: s.color, opacity: s.dash ? 0.6 : 1 }} />{s.label}
                    </span>
                  ))}
                </span>
              )}
              {hoverIdx !== null && (
                <span className="aa-fp-read">
                  {b.series.map((s) => {
                    const v = s.vals[hoverIdx];
                    return (
                      <span key={s.label}>
                        {b.series.length > 1 && <em>{s.label} </em>}
                        {v === null ? "—" : Math.round(v).toLocaleString("de-DE")}
                      </span>
                    );
                  })}
                </span>
              )}
            </header>
            <div className="aa-fp-plot" style={{ height: H }}>
              <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
                {ticks.map((v) => (
                  <line key={v} x1="0" y1={y(v)} x2={W} y2={y(v)}
                    stroke="var(--border)" strokeWidth="1"
                    opacity={b.diverging && v === 0 ? 0.9 : 0.45} />
                ))}
                {b.fill && fillRuns(b.fill).map((pts, k) => (
                  <polygon key={`fill${k}`} points={pts} fill={GREY} opacity="0.38" />
                ))}
                {b.series.map((s) =>
                  path(s.vals).map((d, k) => (
                    <polyline key={`${s.label}${k}`} points={d} fill="none"
                      stroke={s.color} strokeWidth={s.width ?? 2}
                      strokeDasharray={s.dash ? "5 4" : undefined}
                      vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
                  )),
                )}
                {hoverIdx !== null && (
                  <line x1={x(hoverIdx)} y1={PAD_T} x2={x(hoverIdx)} y2={H - PAD_B}
                    stroke="var(--text-muted)" strokeWidth="1" opacity="0.8" />
                )}
              </svg>
              <div className="aa-fp-ylabs">
                {ticks.map((v) => (
                  <span key={v} style={{ top: `${((y(v) / H) * 100).toFixed(1)}%` }}>
                    {fmtTick(v, b.unit)}
                  </span>
                ))}
              </div>
            </div>
          </section>
        );
      })}
      <div className="aa-fp-xaxis">
        <span>00:00</span>
        {hoverIdx !== null && hasTime && (
          <span className="aa-fp-xnow" style={{ left: `${(x(hoverIdx) / W) * 100}%` }}>
            {hhmm(tOf(pts[hoverIdx], hoverIdx))}
          </span>
        )}
        <span>{hasTime ? hhmm(tMax) : `${pts.length} Punkte`}</span>
      </div>
    </div>
  );
}
