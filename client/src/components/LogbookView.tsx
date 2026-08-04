// Pilot-Logbuch — live über die GsgLogbook-API (Tauri-Commands
// logbook_pireps / logbook_stats / logbook_pirep), nichts lokal gespeichert.
// Liste (Stats + Tabelle, keine Filter) → Klick → Detail mit geflogenem Track,
// 3-Linien-Höhenprofil (MSL/AGL/Gelände) und Fluglogbuch.
import { useEffect, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
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
/** v0.19.x FIX: was a hand-rolled German-only month-abbreviation array
 *  (JAN..DEZ), shown regardless of locale. `toLocaleDateString` gives the
 *  same "12 AUG 2026"-style layout while actually respecting `locale`. */
const fmtDate = (iso: string | undefined, locale: string) => {
  if (!iso) return "—";
  const d = new Date(iso);
  const day = pad(d.getDate());
  const month = d.toLocaleDateString(locale, { month: "short" }).toUpperCase().replace(/\.$/, "");
  return `${day} ${month} ${d.getFullYear()}`;
};
/** Der Pilot braucht keinen Stacktrace, aber wir dürfen die Ursache auch
 *  nicht verschlucken — Klartext vorn, technisches Detail dahinter. */
const errText = (e: unknown, t: TFunction): string => {
  const raw = String((e as { message?: string })?.message ?? e ?? "").trim();
  if (/network|fetch|connect|timeout|dns/i.test(raw)) {
    return t("logbook_view.error_network", { detail: raw });
  }
  if (/401|403|unauth|forbidden|api.?key/i.test(raw)) {
    return t("logbook_view.error_unauthorized", { detail: raw });
  }
  if (/404|not found/i.test(raw)) {
    return t("logbook_view.error_not_found", { detail: raw });
  }
  return raw || t("logbook_view.error_unknown");
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
// Field feedback (2026-08-03): a transient phpVMS hiccup showed a bare
// error with zero recovery — one failed request, no retry, no manual
// affordance. Retries only what LOOKS transient (same network/timeout
// pattern `errText` above already detects) — retrying an auth or 404
// error endlessly wouldn't help, just delays showing the real cause.
export const RETRY_DELAYS_MS = [800, 2000]; // 3 attempts total: immediate + 2 retries
export async function invokeWithRetry<T>(fn: () => Promise<T>): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fn();
    } catch (e) {
      const msg = String((e as { message?: string })?.message ?? e ?? "");
      if (attempt >= RETRY_DELAYS_MS.length || !/network|fetch|connect|timeout|dns/i.test(msg)) {
        throw e;
      }
      await new Promise((r) => setTimeout(r, RETRY_DELAYS_MS[attempt]));
    }
  }
}

const statusSlug = (s?: string) => (s === "accepted" || s === "pending" || s === "rejected" ? s : "pending");
/** v0.19.x FIX: the badge text used to be the raw English status slug
 *  itself ("accepted"/"pending"/"rejected"), shown verbatim regardless
 *  of locale — the CSS class keeps the English slug (styling hook), only
 *  the visible label is now localized. */
const badge = (s: string | undefined, t: TFunction) =>
  `<span class="aa-lb-badge aa-lb-b-${statusSlug(s)}">${esc(t(`logbook_view.status_${statusSlug(s)}`))}</span>`;
const esc = (s: unknown) => String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);

export function LogbookView() {
  const { t, i18n } = useTranslation();
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
  // Manual "Erneut versuchen" — bumped on click, included in the fetch
  // effect's deps below so a click re-runs it even when `page` didn't
  // change (all automatic retries in invokeWithRetry already exhausted).
  const [retryTick, setRetryTick] = useState(0);
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
    invokeWithRetry(() =>
      invoke<{ items?: Item[]; total?: number }>("logbook_pireps", { limit: PAGE, offset: page * PAGE }),
    )
      .then((r) => { if (!cancelled) { setItems(r.items ?? []); setTotal(r.total ?? 0); } })
      .catch((e) => { if (!cancelled) setError(errText(e, t)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, retryTick]);

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
      setError(errText(e, t));
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
          <button type="button" className="aa-lb-btn" onClick={() => setDetail(null)}>{t("logbook_view.detail_back")}</button>
          <span className="aa-lb-cs">{detail.callsign}</span>
          <span className="aa-lb-route">{detail.dep_icao}<span className="aa-lb-arr">→</span>{detail.arr_icao}</span>
          <span className="aa-lb-muted">{detail.aircraft_icao} · {detail.aircraft_reg}</span>
          <span dangerouslySetInnerHTML={{ __html: badge(detail.status, t) }} />
          <div className="aa-lb-det-stats">
            <div><div className="aa-lb-k">{t("logbook_view.table_duration")}</div><div className="aa-lb-v">{dur(detail.duration_min)}</div></div>
            <div><div className="aa-lb-k">{t("logbook_view.table_distance")}</div><div className="aa-lb-v">{detail.distance_nm} nm</div></div>
            <div><div className="aa-lb-k">{t("logbook_view.table_landing")}</div><div className="aa-lb-v">{detail.landing_rate_fpm} fpm</div></div>
          </div>
        </div>
        <div className="aa-lb-det-row">
          <div className="aa-lb-map" ref={mapElRef} />
          <div className="aa-lb-panel aa-lb-logpanel">
            <h3>{t("logbook_view.detail_log_title")}</h3>
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
          <h3>{t("logbook_view.detail_profile_title")}</h3>
          <FlightProfile route={route} />
        </div>
      </section>
    );
  }

  return (
    <section className="aa-lb">
      <div className="aa-lb-head"><div className="aa-lb-title">{t("logbook_view.section_title")}</div><div className="aa-lb-sub">{t("logbook_view.section_subtitle")}</div></div>
      <div className="aa-lb-stats">
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_flights")}</div><div className="aa-lb-bigv">{stats?.total_flights ?? "—"}</div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_hours")}</div><div className="aa-lb-bigv">{stats?.hours_flown != null ? Math.round(stats.hours_flown) : "—"}<small> {t("logbook_view.unit_hours")}</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_distance")}</div><div className="aa-lb-bigv">{fmtNm(stats?.distance_nm)}<small> nm</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_avg_landing")}</div><div className="aa-lb-bigv">{stats?.avg_landing_fpm ?? "—"}<small> fpm</small></div></div>
        {/* Die API lieferte beides schon immer, angezeigt wurde es nie. */}
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_this_month")}</div><div className="aa-lb-bigv">{stats?.flights_this_month ?? "—"}<small> {t("logbook_view.unit_flights")}</small></div></div>
        <div className="aa-lb-stat"><div className="aa-lb-k">{t("logbook_view.stat_this_year")}</div><div className="aa-lb-bigv">{stats?.hours_this_year != null ? Math.round(stats.hours_this_year) : "—"}<small> {t("logbook_view.unit_hours")}</small></div></div>
        <div className="aa-lb-stat aa-lb-rankcard">{stats?.rank_image && <img src={stats.rank_image} alt="" />}<div><div className="aa-lb-k">{t("logbook_view.stat_rank")}</div><div className="aa-lb-rankv">{stats?.rank ?? "—"}</div></div></div>
      </div>
      <div className="aa-lb-card">
        {error && (
          <div className="aa-lb-error">
            {t("logbook_view.error_prefix", { error })}
            {" "}
            <button type="button" className="aa-lb-btn" onClick={() => setRetryTick((n) => n + 1)}>
              {t("logbook_view.retry")}
            </button>
          </div>
        )}
        {/* Der Balken macht das Warten sichtbar, bevor der Blick überhaupt bei
            der Zeile ankommt — er sitzt an der Oberkante der Karte. */}
        {(loading || openingId) && <div className="aa-lb-progress" role="presentation" />}
        <table className={openingId ? "is-busy" : undefined} aria-busy={openingId ? true : undefined}>
          <thead>
            <tr>
              <th>{t("logbook_view.table_date")}</th>
              <th>{t("logbook_view.table_route")}</th>
              <th>{t("logbook_view.table_type")}</th>
              <th className="num">{t("logbook_view.table_duration")}</th>
              <th className="num">{t("logbook_view.table_distance")}</th>
              <th className="num">{t("logbook_view.table_landing")}</th>
              <th>{t("logbook_view.table_status")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {items.map((f) => (
              <tr
                key={f.id}
                className={openingId === f.id ? "is-opening" : undefined}
                onClick={() => openDetail(f.id)}
              >
                <td className="aa-lb-muted">{fmtDate(f.date, i18n.language)}</td>
                <td><span className="aa-lb-rt">{f.dep_icao}<span className="aa-lb-arr">→</span>{f.arr_icao}</span><div className="aa-lb-cs2">{f.callsign}</div></td>
                <td>{f.aircraft_icao} <span className="aa-lb-muted">· {f.aircraft_reg}</span></td>
                <td className="num">{dur(f.duration_min)}</td>
                <td className="num">{f.distance_nm} nm</td>
                <td className="num">{f.landing_rate_fpm} fpm</td>
                <td><span dangerouslySetInnerHTML={{ __html: badge(f.status, t) }} /></td>
                {/* Der Pfeil wird zum Kreisel — an genau der Stelle, auf die
                    man geklickt hat, statt irgendwo sonst auf der Seite. */}
                <td className="aa-lb-chev">{openingId === f.id ? <span className="aa-lb-spin" aria-label={t("logbook_view.opening_aria")} /> : "›"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="aa-lb-pager">
          <span>
            {loading
              ? t("logbook_view.loading")
              : t("logbook_view.pager_range", {
                  from: total ? page * PAGE + 1 : 0,
                  to: Math.min((page + 1) * PAGE, total),
                  total,
                })}
          </span>
          <span className="aa-lb-pagebtns">
            <button type="button" disabled={page === 0} onClick={() => setPage((p) => Math.max(0, p - 1))}>{t("logbook_view.pager_back")}</button>
            <span>{t("logbook_view.pager_page", { page: page + 1, pages: Math.max(1, Math.ceil(total / PAGE)) })}</span>
            <button type="button" disabled={(page + 1) * PAGE >= total} onClick={() => setPage((p) => p + 1)}>{t("logbook_view.pager_next")}</button>
          </span>
        </div>
      </div>
    </section>
  );
}
