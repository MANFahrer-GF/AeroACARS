// Pilot-Logbuch — live über die GsgLogbook-API (Tauri-Commands
// logbook_pireps / logbook_stats / logbook_pirep), nichts lokal gespeichert.
// Liste (Stats + Tabelle, keine Filter) → Klick → Detail mit geflogenem Track,
// 3-Linien-Höhenprofil (MSL/AGL/Gelände) und Fluglogbuch.
import { useEffect, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "../lib/ipc";
import { FlightProfile } from "./FlightProfile";

const BASEMAP_DARK = "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json";
const BASEMAP_LIGHT = "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json";
const PAGE = 25;

interface Stats {
  total_flights?: number; hours_flown?: number; distance_nm?: number;
  avg_landing_fpm?: number; rank?: string; rank_image?: string;
  flights_this_month?: number; hours_this_year?: number;
}
interface Item {
  id: string; date?: string; dep_icao?: string; arr_icao?: string; callsign?: string;
  aircraft_icao?: string; aircraft_reg?: string; status?: string;
  duration_min?: number; distance_nm?: number; landing_rate_fpm?: number;
}
interface RoutePt {
  lat: number; lon: number;
  /** Zeit seit Flugbeginn in ms. */
  t?: number;
  alt_ft?: number;
  /** Hoehe ueber Grund. Fehlte in der alten Stratos-API komplett. */
  agl_ft?: number;
  /** Gelaendehoehe, serverseitig als MSL-AGL gerechnet. `null` wenn eine
   *  der beiden Hoehen fehlt — dann darf NICHT 0 gezeichnet werden, das
   *  waere Meereshoehe unter einem Flugzeug ueber den Alpen. */
  gnd_ft?: number | null;
  ias_kt?: number | null;
  vs_fpm?: number | null;
  fuel?: number | null;
  ff?: number | null;
}
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
/** Der Pilot braucht keinen Stacktrace, aber wir dürfen die Ursache auch
 *  nicht verschlucken — Klartext vorn, technisches Detail dahinter. */
const errText = (e: unknown): string => {
  const raw = String((e as { message?: string })?.message ?? e ?? "").trim();
  if (/network|fetch|connect|timeout|dns/i.test(raw)) {
    return `Keine Verbindung zu phpVMS. (${raw})`;
  }
  if (/401|403|unauth|forbidden|api.?key/i.test(raw)) {
    return `Anmeldung abgelehnt — prüfe deinen API-Schlüssel in den Einstellungen. (${raw})`;
  }
  if (/404|not found/i.test(raw)) {
    return `Das Logbuch-Modul antwortet nicht — läuft GsgLogbook auf dem Server? (${raw})`;
  }
  return raw || "Unbekannter Fehler.";
};

/** Distanz gross genug fuer eine Kachel, aber ohne zu luegen.
 *  Vorher stand hier `(n/1000).toFixed(0)+"k"` — das machte aus den
 *  300 nm eines frischen Piloten ein glattes "0k". */
export const fmtNm = (n?: number) => {
  if (n == null) return "—";
  if (n >= 10000) return `${Math.round(n / 1000)}k`;
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(Math.round(n));
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
  // Getrennt vom Listen-`loading`, und die ID statt eines Ja/Nein:
  // Ein Detailabruf dauert je nach Fluglänge 1,5–3 s (Langstrecke: 10.000
  // Streckenpunkte). Bisher zeigte nur der Blätterbalken ganz unten "lädt …"
  // — beim Klick auf eine Zeile passierte oben nichts, also klickt man
  // nochmal. Mit der ID kann genau die angeklickte Zeile antworten.
  const [openingId, setOpeningId] = useState<string | null>(null);
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
      .catch((e) => { if (!cancelled) setError(errText(e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [page]);

  async function openDetail(id: string) {
    // Zweitklick verwerfen statt eine zweite Abfrage loszuschicken. Ohne das
    // holt ein ungeduldiger Doppelklick denselben Flug zweimal — beide
    // Abrufe laufen, der langsamere gewinnt.
    if (openingId) return;
    setOpeningId(id);
    setError(null);
    try {
      const d = await invoke<Detail>("logbook_pirep", { id });
      setDetail(d);
    } catch (e) {
      setError(errText(e));
    } finally {
      setOpeningId(null);
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
                // v0.19-Stage-E: "phase" (ohne Praefix) kollidierte mit der
                // GLOBALEN .phase-Klasse (App.tsx Lade-/Session-Check-Screen)
                // — Grenzflaeche/Hintergrund/Padding/flex-column von dort
                // ueberlagerten die Grid-Zeile hier, sichtbar als kaputte
                // Kaesten im Fluglogbuch. Eigener, praefixierter Modifier.
                return `<div class="aa-lb-logrow ${phase ? "aa-lb-logrow--phase" : ""}"><span class="aa-lb-t">${elapsed(l.t)}</span><span class="aa-lb-m">${phase ? '<span class="aa-lb-dot"></span>' : ""}${esc(l.message)}</span></div>`;
              }).join(""),
            }} />
          </div>
        </div>
        <div className="aa-lb-panel">
          <h3>Flugprofil</h3>
          <FlightProfile route={route} />
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
        <div className="aa-lb-stat"><div className="aa-lb-k">Distanz</div><div className="aa-lb-bigv">{fmtNm(stats?.distance_nm)}<small> nm</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">Ø Landung</div><div className="aa-lb-bigv">{stats?.avg_landing_fpm ?? "—"}<small> fpm</small></div></div>
        {/* Die API lieferte beides schon immer, angezeigt wurde es nie. */}
        <div className="aa-lb-stat"><div className="aa-lb-k">Diesen Monat</div><div className="aa-lb-bigv">{stats?.flights_this_month ?? "—"}<small> Flüge</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">Dieses Jahr</div><div className="aa-lb-bigv">{stats?.hours_this_year != null ? Math.round(stats.hours_this_year) : "—"}<small> h</small></div></div>
        <div className="aa-lb-stat aa-lb-rankcard">{stats?.rank_image && <img src={stats.rank_image} alt="" />}<div><div className="aa-lb-k">Rang</div><div className="aa-lb-rankv">{stats?.rank ?? "—"}</div></div></div>
      </div>
      <div className="aa-lb-card">
        {error && <div className="aa-lb-error">Logbuch konnte nicht geladen werden: {error}</div>}
        {/* Der Balken macht das Warten sichtbar, bevor der Blick überhaupt bei
            der Zeile ankommt — er sitzt an der Oberkante der Karte. */}
        {(loading || openingId) && <div className="aa-lb-progress" role="presentation" />}
        <table className={openingId ? "is-busy" : undefined} aria-busy={openingId ? true : undefined}>
          <thead><tr><th>Datum</th><th>Route</th><th>Muster</th><th className="num">Dauer</th><th className="num">Distanz</th><th className="num">Landung</th><th>Status</th><th></th></tr></thead>
          <tbody>
            {items.map((f) => (
              <tr
                key={f.id}
                className={openingId === f.id ? "is-opening" : undefined}
                onClick={() => openDetail(f.id)}
              >
                <td className="aa-lb-muted">{fmtDate(f.date)}</td>
                <td><span className="aa-lb-rt">{f.dep_icao}<span className="aa-lb-arr">→</span>{f.arr_icao}</span><div className="aa-lb-cs2">{f.callsign}</div></td>
                <td>{f.aircraft_icao} <span className="aa-lb-muted">· {f.aircraft_reg}</span></td>
                <td className="num">{dur(f.duration_min)}</td>
                <td className="num">{f.distance_nm} nm</td>
                <td className="num">{f.landing_rate_fpm} fpm</td>
                <td><span dangerouslySetInnerHTML={{ __html: badge(f.status) }} /></td>
                {/* Der Pfeil wird zum Kreisel — an genau der Stelle, auf die
                    man geklickt hat, statt irgendwo sonst auf der Seite. */}
                <td className="aa-lb-chev">{openingId === f.id ? <span className="aa-lb-spin" aria-label="lädt" /> : "›"}</td>
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
