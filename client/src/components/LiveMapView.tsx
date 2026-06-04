// v0.13.x — In-App Live-Map (Stratos-orientiert, AeroACARS-Identität).
//
// Ansichten:
//   • "own" — eigener aktiver Flug: geplante Route (gestrichelt + Wegpunkt-Dots
//     + TOC/TOD aus dem SimBrief-Navlog), geflogener Track (solide, app-weit ab
//     Flugstart akkumuliert), Flugzeug-Marker (heading-gedreht), Dep/Arr-Pins,
//     Stats-Leiste, Log-Panel, Follow-Zoom.
//   • "va"  — VA-Übersicht: alle aktiven Piloten (Proxy auf phpVMS /api/acars).
//
// DEMO-Modus (dev-only): synthetischer Flug, der durch die Phasen läuft — zum
// Ansehen des Looks OHNE echten Flug/Sim (funktioniert auch im reinen
// `npm run dev`-Browser, da er keine Tauri-Commands braucht).
//
// Theme-aware: dunkle (dark-matter) bzw. helle (positron) CARTO-Basemap, die mit
// dem App-Theme (data-theme) umschaltet; Overlay-Farben aus CSS-Vars.
// Dev/Beta-only (Tab hinter import.meta.env.DEV). Rein Anzeige — keine Wertung.

import { useEffect, useMemo, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "@tauri-apps/api/core";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import { ActivityLogPanel } from "./ActivityLogPanel";
import { getTrack } from "../lib/trackStore";

const BASEMAP_DARK = "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json";
const BASEMAP_LIGHT = "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json";

interface RouteFix {
  ident: string;
  lat: number;
  lon: number;
  kind: string;
}
interface VaFlight {
  ident?: string;
  flight_number?: string;
  aircraft?: { icao?: string; registration?: string } | null;
  dpt_airport_id?: string;
  arr_airport_id?: string;
  position?: { lat?: number; lon?: number; heading?: number } | null;
}
type View = "own" | "va";
interface Aircraft {
  lon: number;
  lat: number;
  hdg: number;
}

function readTheme(): "dark" | "light" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

// ---- Demo-Flug (EDDH → LEMD), inkl. TOC/TOD ----
const DEMO_FIXES: RouteFix[] = [
  { ident: "EDDH", lat: 53.63, lon: 9.99, kind: "apt" },
  { ident: "TOC", lat: 52.0, lon: 8.4, kind: "wpt" },
  { ident: "OSN", lat: 50.4, lon: 6.9, kind: "vor" },
  { ident: "DIK", lat: 48.6, lon: 4.9, kind: "vor" },
  { ident: "NTS", lat: 46.5, lon: 1.6, kind: "vor" },
  { ident: "PPN", lat: 43.6, lon: -1.4, kind: "vor" },
  { ident: "TOD", lat: 41.7, lon: -2.6, kind: "wpt" },
  { ident: "LEMD", lat: 40.49, lon: -3.57, kind: "apt" },
];

/** Densify a fix polyline into N evenly-spaced points (planar approx). */
function densify(fixes: RouteFix[], n: number): [number, number][] {
  const pts = fixes.map((f) => [f.lon, f.lat] as [number, number]);
  const segLen: number[] = [];
  let total = 0;
  for (let i = 1; i < pts.length; i++) {
    const dx = (pts[i][0] - pts[i - 1][0]) * Math.cos((pts[i][1] * Math.PI) / 180);
    const dy = pts[i][1] - pts[i - 1][1];
    const d = Math.hypot(dx, dy);
    segLen.push(d);
    total += d;
  }
  const out: [number, number][] = [];
  for (let k = 0; k <= n; k++) {
    const target = (k / n) * total;
    let acc = 0;
    let i = 0;
    while (i < segLen.length && acc + segLen[i] < target) {
      acc += segLen[i];
      i++;
    }
    if (i >= segLen.length) {
      out.push(pts[pts.length - 1]);
      continue;
    }
    const t = segLen[i] > 0 ? (target - acc) / segLen[i] : 0;
    out.push([
      pts[i][0] + (pts[i + 1][0] - pts[i][0]) * t,
      pts[i][1] + (pts[i + 1][1] - pts[i][1]) * t,
    ]);
  }
  return out;
}
function bearing(a: [number, number], b: [number, number]): number {
  const dLon = ((b[0] - a[0]) * Math.PI) / 180;
  const y = Math.sin(dLon) * Math.cos((b[1] * Math.PI) / 180);
  const x =
    Math.cos((a[1] * Math.PI) / 180) * Math.sin((b[1] * Math.PI) / 180) -
    Math.sin((a[1] * Math.PI) / 180) * Math.cos((b[1] * Math.PI) / 180) * Math.cos(dLon);
  return ((Math.atan2(y, x) * 180) / Math.PI + 360) % 360;
}
const DEMO_LINE = densify(DEMO_FIXES, 240);

const SRC_ROUTE = "aa-planned-route";
const SRC_WPTS = "aa-planned-wpts";
const SRC_TRACK = "aa-flown-track";
const LYR_ROUTE = "aa-planned-route-line";
const LYR_WPTS = "aa-planned-wpts-circles";
const LYR_TRACK = "aa-flown-track-line";

interface Props {
  activeFlight: ActiveFlightInfo | null;
  simSnapshot: SimSnapshot | null;
}

export function LiveMapView({ activeFlight, simSnapshot }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const acMarkerRef = useRef<maplibregl.Marker | null>(null);
  const pinMarkersRef = useRef<maplibregl.Marker[]>([]);
  const vaMarkersRef = useRef<maplibregl.Marker[]>([]);
  const dataRef = useRef<{
    fixes: RouteFix[];
    track: [number, number][];
    dep?: [number, number];
    arr?: [number, number];
  }>({ fixes: [], track: [] });

  const [mapReady, setMapReady] = useState(false);
  const [view, setView] = useState<View>("own");
  const [follow, setFollow] = useState(true);
  const [theme, setTheme] = useState<"dark" | "light">(readTheme());
  const [routeFixes, setRouteFixes] = useState<RouteFix[]>([]);
  const [depArr, setDepArr] = useState<{ dep?: [number, number]; arr?: [number, number] }>({});
  const [vaFlights, setVaFlights] = useState<VaFlight[]>([]);
  const [demo, setDemo] = useState(false);
  const [demoPlaying, setDemoPlaying] = useState(true);
  const [demoT, setDemoT] = useState(0); // 0..1 entlang der Demo-Route

  const isDev = import.meta.env.DEV;
  const pirepId = activeFlight?.pirep_id ?? null;

  // ---- Demo-Animation ----
  useEffect(() => {
    if (!demo || !demoPlaying) return;
    const id = setInterval(() => {
      setDemoT((t) => (t >= 1 ? 0 : Math.min(1, t + 0.0025)));
    }, 80);
    return () => clearInterval(id);
  }, [demo, demoPlaying]);

  // ---- effektive Daten: Demo ODER echt ----
  const demoIdx = Math.min(DEMO_LINE.length - 1, Math.floor(demoT * (DEMO_LINE.length - 1)));
  const effFixes = demo ? DEMO_FIXES : routeFixes;
  const effTrack: [number, number][] = demo
    ? DEMO_LINE.slice(0, Math.max(2, demoIdx + 1))
    : getTrack(pirepId);
  const effDep = demo ? ([DEMO_FIXES[0].lon, DEMO_FIXES[0].lat] as [number, number]) : depArr.dep;
  const effArr = demo
    ? ([DEMO_FIXES[DEMO_FIXES.length - 1].lon, DEMO_FIXES[DEMO_FIXES.length - 1].lat] as [number, number])
    : depArr.arr;
  const effAircraft: Aircraft | null = demo
    ? {
        lon: DEMO_LINE[demoIdx][0],
        lat: DEMO_LINE[demoIdx][1],
        hdg: bearing(DEMO_LINE[Math.max(0, demoIdx - 1)], DEMO_LINE[Math.min(DEMO_LINE.length - 1, demoIdx + 1)]),
      }
    : simSnapshot && typeof simSnapshot.lat === "number"
      ? {
          lon: simSnapshot.lon,
          lat: simSnapshot.lat,
          hdg: simSnapshot.heading_deg_true ?? simSnapshot.heading_deg_magnetic ?? 0,
        }
      : null;
  const effDepIcao = demo ? "EDDH" : activeFlight?.dpt_airport;
  const effArrIcao = demo ? "LEMD" : activeFlight?.arr_airport;

  const demoPhase = (() => {
    if (demoT < 0.03) return "Boarding";
    if (demoT < 0.06) return "TakeoffRoll";
    if (demoT < 0.16) return "Climb";
    if (demoT < 0.78) return "Cruise";
    if (demoT < 0.96) return "Descent";
    if (demoT < 0.99) return "Approach";
    return "Landing";
  })();
  const phaseLabel = demo ? demoPhase : (activeFlight?.phase ?? "—");

  // dataRef für die styledata-Re-Adds aktuell halten
  dataRef.current = { fixes: effFixes, track: effTrack, dep: effDep, arr: effArr };

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: readTheme() === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT,
      center: [6, 48],
      zoom: 4,
      attributionControl: { compact: true },
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
    mapRef.current = map;
    map.on("load", () => {
      addOverlays(map);
      setMapReady(true);
    });
    map.on("styledata", () => {
      if (map.isStyleLoaded()) addOverlays(map);
    });
    return () => {
      map.remove();
      mapRef.current = null;
      setMapReady(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Theme beobachten + Basemap umschalten ----
  useEffect(() => {
    const obs = new MutationObserver(() => {
      const next = readTheme();
      setTheme((prev) => (prev === next ? prev : next));
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => obs.disconnect();
  }, []);
  useEffect(() => {
    mapRef.current?.setStyle(theme === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT);
  }, [theme]);

  // ---- Overlays anlegen (idempotent) + aus dataRef füllen ----
  function addOverlays(map: maplibregl.Map) {
    const accent = cssVar("--accent", "#0a84ff");
    const trackColor = cssVar("--success", "#30d158");
    const empty: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };
    if (!map.getSource(SRC_ROUTE)) map.addSource(SRC_ROUTE, { type: "geojson", data: empty });
    if (!map.getSource(SRC_WPTS)) map.addSource(SRC_WPTS, { type: "geojson", data: empty });
    if (!map.getSource(SRC_TRACK)) map.addSource(SRC_TRACK, { type: "geojson", data: empty });
    if (!map.getLayer(LYR_ROUTE)) {
      map.addLayer({
        id: LYR_ROUTE,
        type: "line",
        source: SRC_ROUTE,
        paint: { "line-color": accent, "line-width": 2, "line-opacity": 0.6, "line-dasharray": [2, 2] },
      });
    }
    if (!map.getLayer(LYR_TRACK)) {
      map.addLayer({
        id: LYR_TRACK,
        type: "line",
        source: SRC_TRACK,
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": trackColor, "line-width": 3 },
      });
    }
    if (!map.getLayer(LYR_WPTS)) {
      map.addLayer({
        id: LYR_WPTS,
        type: "circle",
        source: SRC_WPTS,
        paint: {
          "circle-radius": ["case", ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]], 5, 3],
          "circle-color": [
            "case",
            ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]],
            cssVar("--warning", "#ff9f0a"),
            accent,
          ],
          "circle-stroke-width": 1,
          "circle-stroke-color": cssVar("--surface", "#ffffff"),
        },
      });
    }
    pushSources(map, dataRef.current);
  }

  function pushSources(
    map: maplibregl.Map,
    d: { fixes: RouteFix[]; track: [number, number][]; dep?: [number, number]; arr?: [number, number] },
  ) {
    const routeSrc = map.getSource(SRC_ROUTE) as maplibregl.GeoJSONSource | undefined;
    const wptSrc = map.getSource(SRC_WPTS) as maplibregl.GeoJSONSource | undefined;
    const trackSrc = map.getSource(SRC_TRACK) as maplibregl.GeoJSONSource | undefined;
    if (!routeSrc || !wptSrc || !trackSrc) return;
    let line: [number, number][] = d.fixes.map((f) => [f.lon, f.lat]);
    if (line.length < 2 && d.dep && d.arr) line = [d.dep, d.arr];
    routeSrc.setData({
      type: "FeatureCollection",
      features: line.length >= 2 ? [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: line } }] : [],
    });
    wptSrc.setData({
      type: "FeatureCollection",
      features: d.fixes.map((f) => ({
        type: "Feature",
        properties: { ident: f.ident, kind: f.ident === "TOC" || f.ident === "TOD" ? f.ident : f.kind },
        geometry: { type: "Point", coordinates: [f.lon, f.lat] },
      })),
    });
    trackSrc.setData({
      type: "FeatureCollection",
      features: d.track.length >= 2 ? [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: d.track } }] : [],
    });
  }

  // ---- Routen-Fixes / Dep-Arr laden (nur echt, nicht im Demo) ----
  useEffect(() => {
    let cancelled = false;
    if (demo || !pirepId) {
      setRouteFixes([]);
      return;
    }
    invoke<RouteFix[]>("flight_get_route_fixes")
      .then((fx) => !cancelled && setRouteFixes(fx ?? []))
      .catch(() => !cancelled && setRouteFixes([]));
    return () => {
      cancelled = true;
    };
  }, [pirepId, demo]);

  useEffect(() => {
    let cancelled = false;
    if (demo || !activeFlight) {
      setDepArr({});
      return;
    }
    const lookup = async (icao: string): Promise<[number, number] | undefined> => {
      try {
        const a = await invoke<{ lat?: number | null; lon?: number | null }>("airport_get", { icao });
        if (a?.lat != null && a?.lon != null) return [a.lon, a.lat];
      } catch {
        /* ignore */
      }
      return undefined;
    };
    void (async () => {
      const dep = await lookup(activeFlight.dpt_airport);
      const arr = await lookup(activeFlight.arr_airport);
      if (!cancelled) setDepArr({ dep, arr });
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight?.dpt_airport, activeFlight?.arr_airport, activeFlight, demo]);

  // ---- Redraw: Quellen + Flugzeug-Marker + Pins ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || view !== "own") return;
    pushSources(map, { fixes: effFixes, track: effTrack, dep: effDep, arr: effArr });

    // Flugzeug-Marker
    if (effAircraft) {
      const lngLat: [number, number] = [effAircraft.lon, effAircraft.lat];
      if (!acMarkerRef.current) {
        const el = document.createElement("div");
        el.className = "aa-ac-marker";
        el.innerHTML = planeSvg();
        acMarkerRef.current = new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat(lngLat).addTo(map);
      }
      acMarkerRef.current.setLngLat(lngLat).setRotation(effAircraft.hdg);
      if (follow) map.easeTo({ center: lngLat, duration: 380 });
    } else {
      acMarkerRef.current?.remove();
      acMarkerRef.current = null;
    }

    // Dep/Arr-Pins
    pinMarkersRef.current.forEach((m) => m.remove());
    pinMarkersRef.current = [];
    const mk = (coord: [number, number], label: string, kind: "dep" | "arr") => {
      const el = document.createElement("div");
      el.className = `aa-pin aa-pin--${kind}`;
      el.textContent = label;
      pinMarkersRef.current.push(new maplibregl.Marker({ element: el, anchor: "bottom" }).setLngLat(coord).addTo(map));
    };
    if (effDep && effDepIcao) mk(effDep, effDepIcao, "dep");
    if (effArr && effArrIcao) mk(effArr, effArrIcao, "arr");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapReady, view, demoT, demo, follow, simSnapshot, routeFixes, depArr.dep, depArr.arr]);

  // einmal auf die Route fitten, wenn nicht Follow
  const fittedRef = useRef<string | null>(null);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || view !== "own" || follow) return;
    const pts: [number, number][] = [
      ...effFixes.map((f) => [f.lon, f.lat] as [number, number]),
      ...(effDep ? [effDep] : []),
      ...(effArr ? [effArr] : []),
    ];
    const key = `${demo}-${effFixes.length}-${effDepIcao}-${effArrIcao}`;
    if (pts.length >= 2 && fittedRef.current !== key) {
      fittedRef.current = key;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 80, duration: 600, maxZoom: 8 });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapReady, view, follow, effFixes, effDepIcao, effArrIcao, demo]);

  // ---- VA-Übersicht ----
  useEffect(() => {
    if (view !== "va" || demo) {
      vaMarkersRef.current.forEach((m) => m.remove());
      vaMarkersRef.current = [];
      return;
    }
    let cancelled = false;
    const poll = async () => {
      try {
        const data = await invoke<{ flights?: VaFlight[] } | VaFlight[]>("va_live_flights");
        const flights = Array.isArray(data) ? data : data?.flights ?? [];
        if (!cancelled) setVaFlights(flights);
      } catch {
        if (!cancelled) setVaFlights([]);
      }
    };
    void poll();
    const id = setInterval(poll, 12000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [view, demo]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || view !== "va") return;
    vaMarkersRef.current.forEach((m) => m.remove());
    vaMarkersRef.current = [];
    const pts: [number, number][] = [];
    for (const f of vaFlights) {
      const lat = f.position?.lat;
      const lon = f.position?.lon;
      if (typeof lat !== "number" || typeof lon !== "number") continue;
      const el = document.createElement("div");
      el.className = "aa-ac-marker aa-ac-marker--va";
      el.innerHTML = planeSvg();
      el.title = `${f.ident ?? f.flight_number ?? "?"} · ${f.aircraft?.icao ?? ""} · ${f.dpt_airport_id ?? ""}→${f.arr_airport_id ?? ""}`;
      vaMarkersRef.current.push(
        new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat([lon, lat]).setRotation(f.position?.heading ?? 0).addTo(map),
      );
      pts.push([lon, lat]);
    }
    if (pts.length >= 1) {
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 60, duration: 600, maxZoom: 6 });
    }
  }, [vaFlights, view, mapReady]);

  // ---- Stats ----
  const stats = useMemo(() => {
    const fmt = (v: number | null | undefined, suffix: string) =>
      v == null || Number.isNaN(v) ? "—" : `${Math.round(v)}${suffix}`;
    if (demo) {
      // Synthetische Höhe/Speed je nach Phase fürs Look-and-Feel.
      const alt = demoT < 0.16 ? Math.round(demoT * 230000) : demoT > 0.78 ? Math.round((1 - demoT) * 168000) : 37000;
      const flLabel = alt >= 18000 ? `FL${Math.round(alt / 100)}` : `${alt} ft`;
      return {
        alt: flLabel,
        spd: demoT < 0.1 || demoT > 0.95 ? "180 kts" : "290 kts",
        hdg: effAircraft ? `${Math.round(effAircraft.hdg)}°` : "—",
        gs: demoT < 0.1 || demoT > 0.95 ? "200 kts" : "450 kts",
        dtg: `${Math.round((1 - demoT) * 980)} nm`,
      };
    }
    const s = simSnapshot;
    const flLabel =
      s?.altitude_msl_ft != null
        ? s.altitude_msl_ft >= 18000
          ? `FL${Math.round(s.altitude_msl_ft / 100)}`
          : `${Math.round(s.altitude_msl_ft)} ft`
        : "—";
    return {
      alt: flLabel,
      spd: fmt(s?.indicated_airspeed_kt, " kts"),
      hdg: s ? `${Math.round(s.heading_deg_magnetic)}°` : "—",
      gs: fmt(s?.groundspeed_kt, " kts"),
      dtg: activeFlight?.distance_nm != null ? `${Math.round(activeFlight.distance_nm)} nm` : "—",
    };
  }, [demo, demoT, simSnapshot, activeFlight, effAircraft]);

  const showOwnContent = view === "own" && (demo || activeFlight);

  return (
    <section className="aa-livemap">
      <div className="aa-livemap__topbar">
        <div className="aa-livemap__viewtoggle">
          <button type="button" className={`aa-seg ${view === "own" ? "aa-seg--active" : ""}`} onClick={() => setView("own")}>
            Mein Flug
          </button>
          <button
            type="button"
            className={`aa-seg ${view === "va" ? "aa-seg--active" : ""}`}
            onClick={() => {
              setDemo(false);
              setView("va");
            }}
          >
            VA-Übersicht
          </button>
        </div>

        {showOwnContent && (
          <div className="aa-livemap__stats">
            <Stat label="ALT" value={stats.alt} />
            <Stat label="IAS" value={stats.spd} />
            <Stat label="HDG" value={stats.hdg} />
            <Stat label="GS" value={stats.gs} />
            <Stat label="DTG" value={stats.dtg} />
            <Stat label="PHASE" value={phaseLabel} />
          </div>
        )}

        <div className="aa-livemap__right">
          {view === "own" && (
            <label className="aa-livemap__follow">
              <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
              Follow
            </label>
          )}
          {isDev && view === "own" && (
            <div className="aa-livemap__demo">
              <label className="aa-livemap__follow">
                <input
                  type="checkbox"
                  checked={demo}
                  onChange={(e) => {
                    setDemo(e.target.checked);
                    setDemoT(0);
                    setDemoPlaying(true);
                    setFollow(true);
                  }}
                />
                Demo
              </label>
              {demo && (
                <>
                  <button type="button" className="aa-seg" onClick={() => setDemoPlaying((p) => !p)}>
                    {demoPlaying ? "⏸" : "▶"}
                  </button>
                  <input
                    type="range"
                    min={0}
                    max={1000}
                    value={Math.round(demoT * 1000)}
                    onChange={(e) => {
                      setDemoPlaying(false);
                      setDemoT(Number(e.target.value) / 1000);
                    }}
                  />
                </>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {view === "own" && !demo && !activeFlight && (
            <div className="aa-livemap__empty">
              Kein aktiver Flug — starte einen Flug, um ihn live zu verfolgen.
              {isDev && <div style={{ marginTop: 8, fontSize: 13 }}>(Tipp: „Demo" oben zeigt den Look ohne Flug.)</div>}
            </div>
          )}
        </div>
        <aside className="aa-livemap__log">
          <ActivityLogPanel />
        </aside>
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="aa-stat">
      <span className="aa-stat__label">{label}</span>
      <span className="aa-stat__value">{value}</span>
    </div>
  );
}

function planeSvg(): string {
  return `<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
    <path fill="currentColor" d="M12 2l1.5 7.5L22 13v2l-8.5-2.2L13 21l2 1.5V24l-3-1-3 1v-1.5L11 21l-.5-8.2L2 15v-2l8.5-3.5L12 2z"/>
  </svg>`;
}
