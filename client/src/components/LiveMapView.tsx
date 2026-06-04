// v0.13.x — In-App Live-Map (Stratos-orientiert, AeroACARS-Identität).
//
// Zwei Ansichten:
//   • "own" — eigener aktiver Flug: geplante Route (gestrichelt + Wegpunkt-Dots
//     + TOC/TOD aus dem SimBrief-Navlog), geflogener Track (solide, live
//     akkumuliert), Flugzeug-Marker (heading-gedreht), Dep/Arr-Pins, Stats-
//     Leiste, Log-Panel, Phase/ETA/DTG.
//   • "va"  — VA-Übersicht: alle aktiven Piloten (Proxy auf phpVMS /api/acars).
//
// Theme-aware: dunkle (dark-matter) bzw. helle (positron) CARTO-Basemap, die
// mit dem App-Theme (data-theme) umschaltet; Overlay-Farben aus den CSS-Vars.
// Dev/Beta-only (Tab in App.tsx hinter import.meta.env.DEV) — kein Pilot-Rollout.
//
// Hinweis: rein Anzeige. Keine Wertung/Statistik hängt an dieser Ansicht.

import { useEffect, useMemo, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "@tauri-apps/api/core";
import type { ActiveFlightInfo, SimSnapshot } from "../types";
import { ActivityLogPanel } from "./ActivityLogPanel";

// ---- Basemap-Styles (CARTO GL, kostenlos, kein API-Key) ----
const BASEMAP_DARK =
  "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json";
const BASEMAP_LIGHT =
  "https://basemaps.cartocdn.com/gl/positron-gl-style/style.json";

// Backend-`flight_get_route_fixes` → api_client::RouteFix.
interface RouteFix {
  ident: string;
  lat: number;
  lon: number;
  kind: string;
}

// Lockerer Typ für die /api/acars-Flüge (VA-Übersicht). Wir greifen defensiv zu.
interface VaFlight {
  ident?: string;
  flight_number?: string;
  user_id?: number | string;
  aircraft?: { icao?: string; registration?: string } | null;
  dpt_airport_id?: string;
  arr_airport_id?: string;
  phase?: string;
  status_text?: string;
  position?: {
    lat?: number;
    lon?: number;
    heading?: number;
    altitude_msl?: number;
    gs?: number;
  } | null;
}

type View = "own" | "va";

/** Aktuelles App-Theme aus dem <html data-theme>-Attribut lesen. */
function readTheme(): "dark" | "light" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

/** CSS-Var aus dem Root lesen (für Overlay-Farben passend zum Theme). */
function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
  return v || fallback;
}

// Track wird pro PIREP loop-lokal akkumuliert und übersteht Tab-Wechsel
// (Modul-Store), aber bewusst NICHT persistent über App-Neustart.
const trackStore = new Map<string, [number, number][]>();

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
  const overlaysReadyRef = useRef(false);

  const [view, setView] = useState<View>("own");
  const [follow, setFollow] = useState(true);
  const [theme, setTheme] = useState<"dark" | "light">(readTheme());
  const [routeFixes, setRouteFixes] = useState<RouteFix[]>([]);
  const [depArr, setDepArr] = useState<{
    dep?: [number, number];
    arr?: [number, number];
  }>({});
  const [vaFlights, setVaFlights] = useState<VaFlight[]>([]);

  const pirepId = activeFlight?.pirep_id ?? null;

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: readTheme() === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT,
      center: [10.4515, 51.1657], // DE-Mitte als Default
      zoom: 4,
      attributionControl: { compact: true },
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
    mapRef.current = map;
    map.on("load", () => {
      addOverlays(map);
      overlaysReadyRef.current = true;
    });
    // Beim Theme-Wechsel (setStyle) werden die Custom-Layer entfernt →
    // nach jedem Style-Load neu anlegen.
    map.on("styledata", () => {
      if (map.isStyleLoaded()) addOverlays(map);
    });
    return () => {
      map.remove();
      mapRef.current = null;
      overlaysReadyRef.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Theme beobachten ----
  useEffect(() => {
    const obs = new MutationObserver(() => {
      const next = readTheme();
      setTheme((prev) => (prev === next ? prev : next));
    });
    obs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => obs.disconnect();
  }, []);

  // Theme → Basemap umschalten.
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    map.setStyle(theme === "dark" ? BASEMAP_DARK : BASEMAP_LIGHT);
  }, [theme]);

  // ---- Overlays (Sources + Layer) anlegen, idempotent ----
  function addOverlays(map: maplibregl.Map) {
    const accent = cssVar("--accent", "#0a84ff");
    const trackColor = cssVar("--success", "#30d158");
    const empty: GeoJSON.FeatureCollection = {
      type: "FeatureCollection",
      features: [],
    };
    if (!map.getSource(SRC_ROUTE)) {
      map.addSource(SRC_ROUTE, { type: "geojson", data: empty });
    }
    if (!map.getSource(SRC_WPTS)) {
      map.addSource(SRC_WPTS, { type: "geojson", data: empty });
    }
    if (!map.getSource(SRC_TRACK)) {
      map.addSource(SRC_TRACK, { type: "geojson", data: empty });
    }
    if (!map.getLayer(LYR_ROUTE)) {
      map.addLayer({
        id: LYR_ROUTE,
        type: "line",
        source: SRC_ROUTE,
        paint: {
          "line-color": accent,
          "line-width": 2,
          "line-opacity": 0.65,
          "line-dasharray": [2, 2],
        },
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
          "circle-radius": [
            "case",
            ["in", ["get", "kind"], ["literal", ["TOC", "TOD"]]],
            5,
            3,
          ],
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
    // Nach (Re-)Add direkt mit aktuellen Daten füllen.
    pushRouteData(map);
    pushTrackData(map);
  }

  // ---- Routen-Fixes laden, wenn ein Flug aktiv ist ----
  useEffect(() => {
    let cancelled = false;
    if (!pirepId) {
      setRouteFixes([]);
      return;
    }
    invoke<RouteFix[]>("flight_get_route_fixes")
      .then((fx) => {
        if (!cancelled) setRouteFixes(fx ?? []);
      })
      .catch(() => {
        if (!cancelled) setRouteFixes([]);
      });
    return () => {
      cancelled = true;
    };
  }, [pirepId]);

  // ---- Dep/Arr-Koordinaten laden ----
  useEffect(() => {
    let cancelled = false;
    if (!activeFlight) {
      setDepArr({});
      return;
    }
    async function lookup(icao: string): Promise<[number, number] | undefined> {
      try {
        const a = await invoke<{ lat?: number | null; lon?: number | null }>(
          "airport_get",
          { icao },
        );
        if (a?.lat != null && a?.lon != null) return [a.lon, a.lat];
      } catch {
        /* ignore */
      }
      return undefined;
    }
    void (async () => {
      const dep = await lookup(activeFlight.dpt_airport);
      const arr = await lookup(activeFlight.arr_airport);
      if (!cancelled) setDepArr({ dep, arr });
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight?.dpt_airport, activeFlight?.arr_airport, activeFlight]);

  // ---- Track akkumulieren aus dem Snapshot-Stream ----
  useEffect(() => {
    if (!pirepId || !simSnapshot) return;
    const { lat, lon } = simSnapshot;
    if (typeof lat !== "number" || typeof lon !== "number") return;
    const arr = trackStore.get(pirepId) ?? [];
    const last = arr[arr.length - 1];
    // Nur loggen, wenn die Position sich nennenswert geändert hat (~Punkte
    // ausdünnen, damit lange Flüge die Linie nicht überladen).
    if (!last || Math.abs(last[0] - lon) > 0.002 || Math.abs(last[1] - lat) > 0.002) {
      arr.push([lon, lat]);
      trackStore.set(pirepId, arr);
      const map = mapRef.current;
      if (map && overlaysReadyRef.current) pushTrackData(map);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [simSnapshot, pirepId]);

  // ---- geplante Route + Wegpunkte → GeoJSON ----
  function pushRouteData(map: maplibregl.Map) {
    const routeSrc = map.getSource(SRC_ROUTE) as maplibregl.GeoJSONSource | undefined;
    const wptSrc = map.getSource(SRC_WPTS) as maplibregl.GeoJSONSource | undefined;
    if (!routeSrc || !wptSrc) return;

    // Linie: Navlog-Fixes, sonst Great-Circle-Fallback Dep→Arr.
    let lineCoords: [number, number][] = routeFixes.map((f) => [f.lon, f.lat]);
    if (lineCoords.length < 2 && depArr.dep && depArr.arr) {
      lineCoords = [depArr.dep, depArr.arr];
    }
    routeSrc.setData({
      type: "FeatureCollection",
      features:
        lineCoords.length >= 2
          ? [
              {
                type: "Feature",
                properties: {},
                geometry: { type: "LineString", coordinates: lineCoords },
              },
            ]
          : [],
    });

    wptSrc.setData({
      type: "FeatureCollection",
      features: routeFixes.map((f) => ({
        type: "Feature",
        properties: { ident: f.ident, kind: f.ident === "TOC" || f.ident === "TOD" ? f.ident : f.kind },
        geometry: { type: "Point", coordinates: [f.lon, f.lat] },
      })),
    });
  }

  // ---- geflogener Track → GeoJSON ----
  function pushTrackData(map: maplibregl.Map) {
    const src = map.getSource(SRC_TRACK) as maplibregl.GeoJSONSource | undefined;
    if (!src) return;
    const coords = pirepId ? trackStore.get(pirepId) ?? [] : [];
    src.setData({
      type: "FeatureCollection",
      features:
        coords.length >= 2
          ? [
              {
                type: "Feature",
                properties: {},
                geometry: { type: "LineString", coordinates: coords },
              },
            ]
          : [],
    });
  }

  // Routen-/DepArr-Änderung → neu zeichnen + einmalig fitten.
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !overlaysReadyRef.current || view !== "own") return;
    pushRouteData(map);
    // Auf Route/Endpunkte zoomen, wenn (noch) kein Follow.
    const pts: [number, number][] = [
      ...routeFixes.map((f) => [f.lon, f.lat] as [number, number]),
      ...(depArr.dep ? [depArr.dep] : []),
      ...(depArr.arr ? [depArr.arr] : []),
    ];
    if (pts.length >= 2 && !follow) {
      const b = pts.reduce(
        (acc, p) => acc.extend(p),
        new maplibregl.LngLatBounds(pts[0], pts[0]),
      );
      map.fitBounds(b, { padding: 80, duration: 600, maxZoom: 8 });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeFixes, depArr, view]);

  // ---- Flugzeug-Marker (Position + Heading) ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    if (view !== "own" || !simSnapshot || typeof simSnapshot.lat !== "number") {
      acMarkerRef.current?.remove();
      acMarkerRef.current = null;
      return;
    }
    const lngLat: [number, number] = [simSnapshot.lon, simSnapshot.lat];
    const hdg = simSnapshot.heading_deg_true ?? simSnapshot.heading_deg_magnetic ?? 0;
    if (!acMarkerRef.current) {
      const el = document.createElement("div");
      el.className = "aa-ac-marker";
      el.innerHTML = planeSvg();
      acMarkerRef.current = new maplibregl.Marker({ element: el, rotationAlignment: "map" })
        .setLngLat(lngLat)
        .addTo(map);
    }
    acMarkerRef.current.setLngLat(lngLat);
    acMarkerRef.current.setRotation(hdg);
    if (follow) map.easeTo({ center: lngLat, duration: 400 });
  }, [simSnapshot, follow, view]);

  // ---- Dep/Arr-Pins ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    pinMarkersRef.current.forEach((m) => m.remove());
    pinMarkersRef.current = [];
    if (view !== "own") return;
    const mk = (coord: [number, number], label: string, kind: "dep" | "arr") => {
      const el = document.createElement("div");
      el.className = `aa-pin aa-pin--${kind}`;
      el.textContent = label;
      const m = new maplibregl.Marker({ element: el, anchor: "bottom" })
        .setLngLat(coord)
        .addTo(map);
      pinMarkersRef.current.push(m);
    };
    if (depArr.dep && activeFlight) mk(depArr.dep, activeFlight.dpt_airport, "dep");
    if (depArr.arr && activeFlight) mk(depArr.arr, activeFlight.arr_airport, "arr");
  }, [depArr, view, activeFlight]);

  // ---- VA-Übersicht: /api/acars pollen, Marker setzen ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map || view !== "va") {
      vaMarkersRef.current.forEach((m) => m.remove());
      vaMarkersRef.current = [];
      return;
    }
    let cancelled = false;
    async function poll() {
      try {
        const data = await invoke<{ flights?: VaFlight[] } | VaFlight[]>("va_live_flights");
        const flights: VaFlight[] = Array.isArray(data)
          ? data
          : data?.flights ?? [];
        if (!cancelled) setVaFlights(flights);
      } catch {
        if (!cancelled) setVaFlights([]);
      }
    }
    void poll();
    const id = setInterval(poll, 12000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [view]);

  // VA-Flüge → Marker rendern.
  useEffect(() => {
    const map = mapRef.current;
    if (!map || view !== "va") return;
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
      const m = new maplibregl.Marker({ element: el, rotationAlignment: "map" })
        .setLngLat([lon, lat])
        .setRotation(f.position?.heading ?? 0)
        .addTo(map);
      vaMarkersRef.current.push(m);
      pts.push([lon, lat]);
    }
    if (pts.length >= 1) {
      const b = pts.reduce(
        (acc, p) => acc.extend(p),
        new maplibregl.LngLatBounds(pts[0], pts[0]),
      );
      map.fitBounds(b, { padding: 60, duration: 600, maxZoom: 6 });
    }
  }, [vaFlights, view]);

  // ---- Stats-Leiste (eigener Flug) ----
  const stats = useMemo(() => {
    const s = simSnapshot;
    const fmt = (v: number | null | undefined, suffix: string, digits = 0) =>
      v == null || Number.isNaN(v) ? "—" : `${v.toFixed(digits)}${suffix}`;
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
  }, [simSnapshot, activeFlight]);

  return (
    <section className="aa-livemap">
      <div className="aa-livemap__topbar">
        <div className="aa-livemap__viewtoggle">
          <button
            type="button"
            className={`aa-seg ${view === "own" ? "aa-seg--active" : ""}`}
            onClick={() => setView("own")}
          >
            Mein Flug
          </button>
          <button
            type="button"
            className={`aa-seg ${view === "va" ? "aa-seg--active" : ""}`}
            onClick={() => setView("va")}
          >
            VA-Übersicht
          </button>
        </div>
        {view === "own" && activeFlight && (
          <div className="aa-livemap__stats">
            <Stat label="ALT" value={stats.alt} />
            <Stat label="IAS" value={stats.spd} />
            <Stat label="HDG" value={stats.hdg} />
            <Stat label="GS" value={stats.gs} />
            <Stat label="DTG" value={stats.dtg} />
          </div>
        )}
        {view === "own" && (
          <label className="aa-livemap__follow">
            <input
              type="checkbox"
              checked={follow}
              onChange={(e) => setFollow(e.target.checked)}
            />
            Follow
          </label>
        )}
      </div>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {view === "own" && !activeFlight && (
            <div className="aa-livemap__empty">Kein aktiver Flug — starte einen Flug, um ihn live zu verfolgen.</div>
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
  // Einfaches, neutrales Flugzeug-Symbol (zeigt nach Norden/oben → Rotation
  // via Marker-rotation = Heading).
  return `<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
    <path fill="currentColor" d="M12 2l1.5 7.5L22 13v2l-8.5-2.2L13 21l2 1.5V24l-3-1-3 1v-1.5L11 21l-.5-8.2L2 15v-2l8.5-3.5L12 2z"/>
  </svg>`;
}
