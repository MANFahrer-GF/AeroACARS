// Pilot-Logbuch — live über die StratosLogbook-API (Tauri-Commands
// logbook_pireps / logbook_stats / logbook_pirep), nichts lokal gespeichert.
// Liste (Stats + Tabelle, keine Filter) → Klick → Detail mit geflogenem Track,
// 3-Linien-Höhenprofil (MSL/AGL/Gelände) und Fluglogbuch.
import { useEffect, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "@tauri-apps/api/core";

const BASEMAP_DARK = "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json";
const BASEMAP_LIGHT = "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json";
const PAGE = 25;

interface Stats {
  total_flights?: number; hours_flown?: number; distance_nm?: number;
  avg_landing_fpm?: number; rank?: string; rank_image?: string;
}
interface Item {
  id: string; date?: string; dep_icao?: string; arr_icao?: string; callsign?: string;
  aircraft_icao?: string; aircraft_reg?: string; status?: string;
  duration_min?: number; distance_nm?: number; landing_rate_fpm?: number;
}
interface RoutePt { lat: number; lon: number; alt_ft?: number; agl_ft?: number }
interface Detail extends Item {
  route?: RoutePt[];
  log?: { t: number; level?: string; message: string }[];
}

const pad = (n: number) => String(n).padStart(2, "0");
const dur = (m?: number) => (m == null ? "—" : m >= 60 ? `${Math.floor(m / 60)}h ${pad(m % 60)}m` : `0h ${pad(m)}m`);
const elapsed = (ms: number) => { const t = Math.round(ms / 60000); return `${pad(Math.floor(t / 60))}:${pad(t % 60)}`; };
const fmtDate = (iso?: string) => {
  if (!iso) return "—";
  const d = new Date(iso);
  const M = ["JAN", "FEB", "MRZ", "APR", "MAI", "JUN", "JUL", "AUG", "SEP", "OKT", "NOV", "DEZ"];
  return `${pad(d.getDate())} ${M[d.getMonth()]} ${d.getFullYear()}`;
};
const statusSlug = (s?: string) => (s === "accepted" || s === "pending" || s === "rejected" ? s : "pending");
const badge = (s?: string) => `<span class="aa-lb-badge aa-lb-b-${statusSlug(s)}">${statusSlug(s)}</span>`;
const esc = (s: unknown) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);

export function LogbookView() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(0);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const mapElRef = useRef<HTMLDivElement | null>(null);

  // Stats einmal laden
  useEffect(() => {
    invoke<Stats>("logbook_stats").then(setStats).catch(() => {});
  }, []);

  // Seite laden
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<{ items?: Item[]; total?: number }>("logbook_pireps", { limit: PAGE, offset: page * PAGE })
      .then((r) => { if (!cancelled) { setItems(r.items ?? []); setTotal(r.total ?? 0); } })
      .catch((e) => { if (!cancelled) setError(String(e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [page]);

  async function openDetail(id: string) {
    setLoading(true);
    try {
      const d = await invoke<Detail>("logbook_pirep", { id });
      setDetail(d);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  // Detail-Karte + Profil zeichnen
  useEffect(() => {
    if (!detail || !mapElRef.current) return;
    const route = (detail.route ?? []).filter((p) => typeof p.lat === "number" && typeof p.lon === "number");
    const dark = document.documentElement.dataset.theme === "dark";
    const map = new maplibregl.Map({
      container: mapElRef.current,
      style: dark ? BASEMAP_DARK : BASEMAP_LIGHT,
      center: route.length ? [route[Math.floor(route.length / 2)].lon, route[Math.floor(route.length / 2)].lat] : [6, 48],
      zoom: 5,
      attributionControl: { compact: true },
    });
    mapRef.current = map;
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#0a84ff";
    map.on("load", () => {
      if (route.length >= 2) {
        const coords = route.map((p) => [p.lon, p.lat] as [number, number]);
        map.addSource("trk", { type: "geojson", data: { type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: coords } } });
        map.addLayer({ id: "trk", type: "line", source: "trk", layout: { "line-cap": "round", "line-join": "round" }, paint: { "line-color": accent, "line-width": 3 } });
        const pin = (c: [number, number], col: string) => { const el = document.createElement("div"); el.style.cssText = `width:12px;height:12px;border-radius:50%;background:${col};border:2px solid #fff;box-shadow:0 0 3px rgba(0,0,0,.5)`; new maplibregl.Marker({ element: el }).setLngLat(c).addTo(map); };
        pin(coords[0], "#30d158");
        pin(coords[coords.length - 1], "#ff453a");
        const b = coords.reduce((acc, c) => acc.extend(c), new maplibregl.LngLatBounds(coords[0], coords[0]));
        map.fitBounds(b, { padding: 50, duration: 0 });
      }
    });
    return () => { map.remove(); mapRef.current = null; };
  }, [detail]);

  if (detail) {
    const route = detail.route ?? [];
    return (
      <section className="aa-lb">
        <div className="aa-lb-det-head">
          <button type="button" className="aa-lb-btn" onClick={() => setDetail(null)}>← Logbuch</button>
          <span className="aa-lb-cs">{detail.callsign}</span>
          <span className="aa-lb-route">{detail.dep_icao}<span className="aa-lb-arr">→</span>{detail.arr_icao}</span>
          <span className="aa-lb-muted">{detail.aircraft_icao} · {detail.aircraft_reg}</span>
          <span dangerouslySetInnerHTML={{ __html: badge(detail.status) }} />
          <div className="aa-lb-det-stats">
            <div><div className="aa-lb-k">Dauer</div><div className="aa-lb-v">{dur(detail.duration_min)}</div></div>
            <div><div className="aa-lb-k">Distanz</div><div className="aa-lb-v">{detail.distance_nm} nm</div></div>
            <div><div className="aa-lb-k">Landung</div><div className="aa-lb-v">{detail.landing_rate_fpm} fpm</div></div>
          </div>
        </div>
        <div className="aa-lb-det-row">
          <div className="aa-lb-map" ref={mapElRef} />
          <div className="aa-lb-panel aa-lb-logpanel">
            <h3>Fluglogbuch</h3>
            <div className="aa-lb-log" dangerouslySetInnerHTML={{
              __html: (detail.log ?? []).map((l) => {
                const phase = l.message.startsWith("Phase:");
                return `<div class="aa-lb-logrow ${phase ? "phase" : ""}"><span class="aa-lb-t">${elapsed(l.t)}</span><span class="aa-lb-m">${phase ? '<span class="aa-lb-dot"></span>' : ""}${esc(l.message)}</span></div>`;
              }).join(""),
            }} />
          </div>
        </div>
        <div className="aa-lb-panel">
          <h3>Höhenprofil <span className="aa-lb-leg"><i style={{ background: "var(--accent)" }} />MSL <i style={{ background: "var(--success)" }} />AGL <i style={{ background: "var(--text-muted)" }} />Gelände</span></h3>
          <div className="aa-lb-vprofile" dangerouslySetInnerHTML={{ __html: profileSvg(route) }} />
        </div>
      </section>
    );
  }

  return (
    <section className="aa-lb">
      <div className="aa-lb-head"><div className="aa-lb-title">Logbuch</div><div className="aa-lb-sub">deine geflogenen Flüge — live aus phpVMS</div></div>
      <div className="aa-lb-stats">
        <div className="aa-lb-stat"><div className="aa-lb-k">Flüge</div><div className="aa-lb-bigv">{stats?.total_flights ?? "—"}</div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">Stunden</div><div className="aa-lb-bigv">{stats?.hours_flown != null ? Math.round(stats.hours_flown) : "—"}<small> h</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">Distanz</div><div className="aa-lb-bigv">{stats?.distance_nm != null ? (stats.distance_nm / 1000).toFixed(0) + "k" : "—"}<small> nm</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">Ø Landung</div><div className="aa-lb-bigv">{stats?.avg_landing_fpm ?? "—"}<small> fpm</small></div></div>
        <div className="aa-lb-stat aa-lb-rankcard">{stats?.rank_image && <img src={stats.rank_image} alt="" />}<div><div className="aa-lb-k">Rang</div><div className="aa-lb-rankv">{stats?.rank ?? "—"}</div></div></div>
      </div>
      <div className="aa-lb-card">
        {error && <div className="aa-lb-error">Logbuch konnte nicht geladen werden: {error}</div>}
        <table>
          <thead><tr><th>Datum</th><th>Route</th><th>Muster</th><th className="num">Dauer</th><th className="num">Distanz</th><th className="num">Landung</th><th>Status</th><th></th></tr></thead>
          <tbody>
            {items.map((f) => (
              <tr key={f.id} onClick={() => openDetail(f.id)}>
                <td className="aa-lb-muted">{fmtDate(f.date)}</td>
                <td><span className="aa-lb-rt">{f.dep_icao}<span className="aa-lb-arr">→</span>{f.arr_icao}</span><div className="aa-lb-cs2">{f.callsign}</div></td>
                <td>{f.aircraft_icao} <span className="aa-lb-muted">· {f.aircraft_reg}</span></td>
                <td className="num">{dur(f.duration_min)}</td>
                <td className="num">{f.distance_nm} nm</td>
                <td className="num">{f.landing_rate_fpm} fpm</td>
                <td><span dangerouslySetInnerHTML={{ __html: badge(f.status) }} /></td>
                <td className="aa-lb-chev">›</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="aa-lb-pager">
          <span>{loading ? "lädt …" : `${total ? page * PAGE + 1 : 0}–${Math.min((page + 1) * PAGE, total)} von ${total}`}</span>
          <span className="aa-lb-pagebtns">
            <button type="button" disabled={page === 0} onClick={() => setPage((p) => Math.max(0, p - 1))}>‹ Zurück</button>
            <span>Seite {page + 1} / {Math.max(1, Math.ceil(total / PAGE))}</span>
            <button type="button" disabled={(page + 1) * PAGE >= total} onClick={() => setPage((p) => p + 1)}>Weiter ›</button>
          </span>
        </div>
      </div>
    </section>
  );
}

/** 3-Linien-Höhenprofil (MSL/AGL/Gelände) als SVG. */
function profileSvg(route: RoutePt[]): string {
  const pts = route.filter((p) => typeof p.alt_ft === "number");
  if (pts.length < 2) return '<div class="aa-lb-muted" style="padding:8px">Keine Höhendaten.</div>';
  const W = 1000, H = 210;
  const msl = pts.map((p) => p.alt_ft ?? 0);
  const agl = pts.map((p) => p.agl_ft ?? 0);
  const terr = pts.map((p) => Math.max(0, (p.alt_ft ?? 0) - (p.agl_ft ?? 0)));
  const maxAlt = Math.max(10000, Math.ceil(Math.max(...msl) / 10000) * 10000);
  const x = (i: number) => (i / (pts.length - 1)) * W;
  const y = (a: number) => H - (a / maxAlt) * H;
  const poly = (arr: number[]) => arr.map((a, i) => `${x(i).toFixed(1)},${y(a).toFixed(1)}`).join(" ");
  let grid = "", labels = "";
  for (let a = 0; a <= maxAlt; a += 10000) {
    const gy = y(a);
    grid += `<line x1="0" y1="${gy.toFixed(1)}" x2="${W}" y2="${gy.toFixed(1)}" stroke="var(--border)" stroke-width="1" opacity="0.55"/>`;
    labels += `<div class="aa-lb-ylab" style="top:${((gy / H) * 100).toFixed(1)}%">${a === 0 ? "0" : a / 1000 + "k"}</div>`;
  }
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none">${grid}` +
    `<polyline points="0,${H} ${poly(msl)} ${W},${H}" fill="color-mix(in srgb, var(--accent) 14%, transparent)" stroke="none"/>` +
    `<polyline points="0,${H} ${poly(terr)} ${W},${H}" fill="color-mix(in srgb, var(--text-muted) 30%, transparent)" stroke="none"/>` +
    `<polyline points="${poly(terr)}" fill="none" stroke="var(--text-muted)" stroke-width="1.4" vector-effect="non-scaling-stroke"/>` +
    `<polyline points="${poly(agl)}" fill="none" stroke="var(--success)" stroke-width="1.6" vector-effect="non-scaling-stroke"/>` +
    `<polyline points="${poly(msl)}" fill="none" stroke="var(--accent)" stroke-width="2" vector-effect="non-scaling-stroke"/>` +
    `</svg>${labels}`;
}
