/**
 * Zeitdiagramm fuer den Telemetrie-Monitor — duenne Huelle um uPlot.
 *
 * Ein Diagramm zeigt einen oder mehrere Zahlkanaele ueber dem gewaehlten
 * Zeitfenster. Hilfslinien (VAPP, VLS …) koennen fest oder aus einem
 * anderen Kanal kommen. Farben kommen aus den CSS-Variablen, damit helles
 * und dunkles Design stimmen.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

// uPlot erst beim ersten Diagramm laden: Es liest schon beim Import
// `matchMedia` (fehlt in der Testumgebung) und gehoert nicht in den
// Startcode der App.
let uPlotLaden: Promise<typeof uPlot> | null = null;
function ladeUPlot(): Promise<typeof uPlot> {
  uPlotLaden ??= import("uplot").then((m) => m.default);
  return uPlotLaden;
}
import type { Frame } from "./typen";

export interface Reihe {
  id: string;
  /** Beschriftung in der Legende. */
  name: string;
  /** CSS-Variable oder Farbwert. */
  farbe: string;
  gestrichelt?: boolean;
  /** Fuellen unter der Linie. */
  flaeche?: boolean;
}

export interface Hilfslinie {
  /** Fester Wert oder Kanal-ID, dessen letzter Wert gilt. */
  wert: number | string;
  farbe: string;
}

export interface Band {
  von: number;
  bis: number;
  farbe: string;
}

interface Props {
  frames: Frame[];
  index: Map<string, number>;
  reihen: Reihe[];
  fensterMs: number;
  /** Feste Achsgrenzen; ohne passt sich die Achse an. */
  min?: number;
  max?: number;
  hilfslinien?: Hilfslinie[];
  baender?: Band[];
  hoehe?: number;
  /** Stellen an der Achse. */
  stellen?: number;
  version: number;
  marker?: { t: number }[];
}

function farbeAus(v: string, el: Element): string {
  if (!v.startsWith("--")) return v;
  return getComputedStyle(el).getPropertyValue(v).trim() || "#888";
}

export function ZeitDiagramm({
  frames,
  index,
  reihen,
  fensterMs,
  min,
  max,
  hilfslinien = [],
  baender = [],
  hoehe = 130,
  stellen = 0,
  version,
  marker = [],
}: Props) {
  const box = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const [UPlot, setUPlot] = useState<typeof uPlot | null>(null);
  useEffect(() => {
    let aus = false;
    void ladeUPlot().then((k) => {
      if (!aus) setUPlot(() => k);
    });
    return () => {
      aus = true;
    };
  }, []);
  // Die Zeichen-Hooks lesen immer den neuesten Stand ueber Refs, damit
  // uPlot nicht bei jeder Aenderung neu gebaut werden muss.
  const extras = useRef({ hilfslinien, baender, marker, frames, index, fensterMs });
  extras.current = { hilfslinien, baender, marker, frames, index, fensterMs };

  // Das Thema gehoert dazu: uPlot liest die Farben beim Aufbau.
  const thema = typeof document !== "undefined" ? document.documentElement.dataset.theme ?? "" : "";
  const schluessel =
    reihen.map((r) => `${r.id}:${r.farbe}`).join("|") + `|${min}|${max}|${hoehe}|${stellen}|${thema}`;

  const daten = useMemo<uPlot.AlignedData>(() => {
    const letzter = frames.length ? frames[frames.length - 1].t : Date.now();
    const ab = letzter - fensterMs;
    let start = 0;
    while (start < frames.length && frames[start].t < ab) start++;
    const teil = frames.slice(start);
    const xs = teil.map((f) => f.t / 1000);
    const ys = reihen.map((r) => {
      const i = index.get(r.id);
      return teil.map((f) => {
        if (i === undefined) return null;
        const v = f.z[i];
        return typeof v === "number" ? v : null;
      });
    });
    return [xs, ...ys] as uPlot.AlignedData;
    // version erzwingt die Neuberechnung, frames ist ein veraenderliches Ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, fensterMs, schluessel, index]);

  useEffect(() => {
    const el = box.current;
    if (!el || !UPlot) return;
    const breite = el.clientWidth || 300;
    const stil = getComputedStyle(el);
    const achse = stil.getPropertyValue("--text-dim").trim() || "#888";
    const gitter = stil.getPropertyValue("--line").trim() || "#ccc";
    const schrift = `11px ${stil.getPropertyValue("--font-mono").trim() || "monospace"}`;

    const opts: uPlot.Options = {
      width: breite,
      height: hoehe,
      legend: { show: false },
      cursor: { drag: { x: false, y: false }, points: { size: 6 } },
      scales: {
        // Immer das ganze Zeitfenster zeigen, rechts = jetzt — auch wenn
        // der Verlauf noch kuerzer ist.
        x: {
          time: true,
          range: (_u, _min, dmax) => [dmax - extras.current.fensterMs / 1000, dmax],
        },
        y: min !== undefined && max !== undefined ? { range: [min, max] } : { auto: true },
      },
      axes: [
        {
          stroke: achse,
          font: schrift,
          grid: { stroke: gitter, width: 1 },
          ticks: { stroke: gitter, width: 1 },
          space: 70,
          values: (_u, vals) =>
            vals.map((v) => {
              const d = new Date(v * 1000);
              return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}:${String(d.getUTCSeconds()).padStart(2, "0")}`;
            }),
        },
        {
          stroke: achse,
          font: schrift,
          grid: { stroke: gitter, width: 1 },
          ticks: { stroke: gitter, width: 1 },
          size: 52,
          values: (_u, vals) => vals.map((v) => v.toFixed(stellen)),
        },
      ],
      series: [
        {},
        ...reihen.map((r) => {
          const farbe = farbeAus(r.farbe, el);
          return {
            label: r.name,
            stroke: farbe,
            width: 1.6,
            dash: r.gestrichelt ? [5, 4] : undefined,
            fill: r.flaeche ? farbe + "22" : undefined,
            spanGaps: false,
            points: { show: false },
          } satisfies uPlot.Series;
        }),
      ],
      hooks: {
        drawClear: [
          (u) => {
            const ctx = u.ctx;
            const { left, top, width, height } = u.bbox;
            ctx.save();
            for (const b of extras.current.baender) {
              const y1 = u.valToPos(Math.min(b.bis, u.scales.y.max ?? b.bis), "y", true);
              const y2 = u.valToPos(Math.max(b.von, u.scales.y.min ?? b.von), "y", true);
              if (!Number.isFinite(y1) || !Number.isFinite(y2)) continue;
              ctx.fillStyle = farbeAus(b.farbe, el);
              ctx.globalAlpha = 0.14;
              ctx.fillRect(left, Math.min(y1, y2), width, Math.abs(y2 - y1));
            }
            ctx.restore();
            void top;
            void height;
          },
        ],
        draw: [
          (u) => {
            const ctx = u.ctx;
            const { left, top, width, height } = u.bbox;
            const { hilfslinien: hl, marker: mk, frames: fr, index: ix } = extras.current;
            const letzter = fr.length ? fr[fr.length - 1] : null;
            ctx.save();
            ctx.lineWidth = 1.2 * devicePixelRatio;
            ctx.setLineDash([6 * devicePixelRatio, 4 * devicePixelRatio]);
            for (const h of hl) {
              let w: number | null = null;
              if (typeof h.wert === "number") w = h.wert;
              else if (letzter) {
                const i = ix.get(h.wert);
                const v = i === undefined ? null : letzter.z[i];
                w = typeof v === "number" ? v : null;
              }
              if (w === null) continue;
              const y = u.valToPos(w, "y", true);
              if (y < top || y > top + height) continue;
              ctx.strokeStyle = farbeAus(h.farbe, el);
              ctx.beginPath();
              ctx.moveTo(left, y);
              ctx.lineTo(left + width, y);
              ctx.stroke();
            }
            ctx.strokeStyle = farbeAus("--sem-warn", el);
            ctx.setLineDash([2 * devicePixelRatio, 3 * devicePixelRatio]);
            for (const m of mk) {
              const x = u.valToPos(m.t / 1000, "x", true);
              if (x < left || x > left + width) continue;
              ctx.beginPath();
              ctx.moveTo(x, top);
              ctx.lineTo(x, top + height);
              ctx.stroke();
            }
            ctx.restore();
          },
        ],
      },
    };
    const u = new UPlot(opts, daten, el);
    plot.current = u;
    const beobachter = new ResizeObserver(() => {
      const b = el.clientWidth;
      if (b > 0 && Math.abs(b - u.width) > 1) u.setSize({ width: b, height: hoehe });
    });
    beobachter.observe(el);
    return () => {
      beobachter.disconnect();
      u.destroy();
      plot.current = null;
    };
    // Neu bauen nur, wenn sich Reihen, Achse oder Hoehe aendern.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [schluessel, UPlot]);

  useEffect(() => {
    plot.current?.setData(daten, true);
  }, [daten]);

  return <div ref={box} className="tele-diagramm" />;
}
