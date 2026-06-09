import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";

interface XPlaneDatarefSample {
  index: number;
  name: string;
  value: number;
  has_value: boolean;
}

/**
 * X-Plane DataRef Inspector — Settings → Debug companion to the
 * MSFS SimInspector. Unlike the MSFS variant (where pilots add
 * SimVar names manually), the X-Plane catalog is fixed at compile
 * time, so this is a read-only auto-populated table. Pilots check
 * it to verify the UDP feed is live and individual DataRefs are
 * arriving (those with `has_value=false` were rejected by X-Plane,
 * usually because the addon doesn't wire them).
 *
 * Polls `xplane_inspector_list` once per second. Cheap (a Vec<f32>
 * snapshot from a Mutex) so the cadence has no perf impact.
 */
export function XPlaneInspector() {
  const [items, setItems] = useState<XPlaneDatarefSample[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const data = await invoke<XPlaneDatarefSample[]>("xplane_inspector_list");
        if (!cancelled) {
          setItems(data);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    }
    void poll();
    const t = setInterval(poll, 1000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, []);

  const live = items.filter((i) => i.has_value).length;
  const missing = items.length - live;

  return (
    <div className="inspector">
      <header className="inspector__header">
        <h4>X-Plane DataRefs</h4>
        <span className="inspector__hint">
          {live} live · {missing} missing of {items.length}
        </span>
      </header>
      {error && <p className="inspector__error">{error}</p>}
      <table className="inspector__table">
        <thead>
          <tr>
            <th>#</th>
            <th>DataRef</th>
            <th style={{ textAlign: "right" }}>Value</th>
          </tr>
        </thead>
        <tbody>
          {items.map((it) => (
            <tr key={it.index} className={it.has_value ? "" : "inspector__row--missing"}>
              <td>{it.index}</td>
              <td>
                <code>{it.name}</code>
              </td>
              <td style={{ textAlign: "right" }}>
                {it.has_value ? formatValue(it.value) : "—"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function formatValue(v: number): string {
  if (Number.isInteger(v) || Math.abs(v) > 1000) {
    return v.toFixed(0);
  }
  return v.toFixed(3);
}
