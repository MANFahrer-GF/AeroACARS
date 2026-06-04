// v0.13.x — In-App Live-Map (Stratos-orientiert, AeroACARS-Identität).
//
// Ansichten:
//   • "own" — eigener aktiver Flug: geplante Route (gestrichelt + Wegpunkt-Dots
//     + TOC/TOD aus dem SimBrief-Navlog), geflogener Track (solide, app-weit ab
//     Flugstart akkumuliert), Flugzeug-Marker (heading-gedreht), Dep/Arr-Pins,
//     Stats-Leiste, Log-Panel, Follow-Zoom.
//   • "va"  — VA-Übersicht: alle aktiven Piloten (Proxy auf phpVMS /api/acars).
//
// Theme-aware: dunkle (dark-matter) bzw. helle (positron) CARTO-Basemap, die mit
// dem App-Theme (data-theme) umschaltet; Overlay-Farben aus CSS-Vars.
// Rein Anzeige — keine Wertung.

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
  id?: number | string;
  ident?: string;
  flight_number?: string;
  status_text?: string;
  phase?: number | string;
  airline?: { icao?: string; iata?: string } | null;
  aircraft?: { icao?: string; registration?: string; name?: string } | null;
  dpt_airport_id?: string;
  arr_airport_id?: string;
  user?: { name?: string; ident?: string; pilot_id?: string } | null;
  position?: {
    lat?: number;
    lon?: number;
    heading?: number;
    altitude?: number;
    altitude_msl?: number;
    altitude_agl?: number;
    gs?: number;
    ias?: number;
    vs?: number;
  } | null;
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

// Phasenabhängiger Folge-Zoom: am Boden nah dran, im Reiseflug weit.
// Abgestimmt auf die FlightPhase-Strings (snake_case) aus dem Backend.
// Gibt null zurück, wenn die Phase unbekannt ist → Höhen-Fallback.
function zoomForPhase(phase: string): number | null {
  const p = phase.toLowerCase();
  if (/preflight|boarding|board|pushback|blocks_on|arrived|pirep|gate|park|stand/.test(p)) return 14;
  if (/taxi/.test(p)) return 13;
  if (/takeoff|take-off|departure/.test(p)) return 11;
  if (/climb/.test(p)) return 7.5;
  if (/cruise/.test(p)) return 5;
  if (/descent|descend/.test(p)) return 7.5;
  if (/approach|final/.test(p)) return 9;
  if (/landing|flare|rollout|touch/.test(p)) return 12.5;
  // holding & alles andere → Höhen-Fallback (richtet sich nach der Flughöhe)
  return null;
}
// Folge-Zoom für echte Flüge: Phase zuerst, sonst nach Höhe (MSL ft).
function targetFollowZoom(phase: string, altMslFt?: number | null): number {
  const z = zoomForPhase(phase);
  if (z != null) return z;
  if (altMslFt == null || Number.isNaN(altMslFt)) return 6.5;
  if (altMslFt < 1500) return 12.5;
  if (altMslFt < 10000) return 9;
  if (altMslFt < 22000) return 7.5;
  return 5;
}

function escHtml(s: unknown): string {
  return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string);
}
function flLabel(msl?: number | null): string {
  if (msl == null || Number.isNaN(msl)) return "—";
  return msl >= 18000 ? `FL${Math.round(msl / 100)}` : `${Math.round(msl)} ft`;
}
/** Theme-fähiges Popup-HTML mit den Flugdaten aus /api/acars. */
function vaPopupHtml(f: VaFlight): string {
  const cs = f.ident || [f.airline?.icao, f.flight_number].filter(Boolean).join("") || f.flight_number || "—";
  const ac = [f.aircraft?.icao, f.aircraft?.registration].filter(Boolean).join(" · ") || "—";
  const route = `${f.dpt_airport_id ?? "—"} → ${f.arr_airport_id ?? "—"}`;
  const pos = f.position ?? {};
  const gs = pos.gs != null ? `${Math.round(pos.gs)} kt` : "—";
  const ias = pos.ias != null ? `${Math.round(pos.ias)} kt` : "—";
  const hdg = pos.heading != null ? `${Math.round(pos.heading)}°` : "—";
  const vs = pos.vs != null ? `${Math.round(pos.vs)} fpm` : "—";
  const pilot = f.user?.name ?? "";
  const row = (k: string, v: string) => `<span class="aa-vapop__k">${k}</span><span class="aa-vapop__v">${escHtml(v)}</span>`;
  return (
    `<div class="aa-vapop__title">${escHtml(cs)}</div>` +
    `<div class="aa-vapop__sub">${escHtml(ac)}${pilot ? ` · ${escHtml(pilot)}` : ""}</div>` +
    `<div class="aa-vapop__grid">` +
    row("Route", route) +
    row("Phase", f.status_text ?? "—") +
    row("ALT", flLabel(pos.altitude_msl ?? pos.altitude)) +
    row("HDG", hdg) +
    row("GS", gs) +
    row("IAS", ias) +
    row("V/S", vs) +
    `</div>`
  );
}

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
  const vaPopupRef = useRef<maplibregl.Popup | null>(null);
  const vaFittedRef = useRef(false);
  const zoomTargetRef = useRef<number | null>(null);
  const zoomingRef = useRef(false);
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

  const pirepId = activeFlight?.pirep_id ?? null;

  // ---- effektive Daten (aktiver Flug) ----
  const effFixes = routeFixes;
  const effTrack: [number, number][] = getTrack(pirepId);
  const effDep = depArr.dep;
  const effArr = depArr.arr;
  const effAircraft: Aircraft | null =
    simSnapshot && typeof simSnapshot.lat === "number"
      ? {
          lon: simSnapshot.lon,
          lat: simSnapshot.lat,
          hdg: simSnapshot.heading_deg_true ?? simSnapshot.heading_deg_magnetic ?? 0,
        }
      : null;
  const effDepIcao = activeFlight?.dpt_airport;
  const effArrIcao = activeFlight?.arr_airport;

  const phaseLabel = activeFlight?.phase ?? "—";

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

  // ---- Routen-Fixes / Dep-Arr laden ----
  useEffect(() => {
    let cancelled = false;
    if (!pirepId) {
      setRouteFixes([]);
      return;
    }
    invoke<RouteFix[]>("flight_get_route_fixes")
      .then((fx) => !cancelled && setRouteFixes(fx ?? []))
      .catch(() => !cancelled && setRouteFixes([]));
    return () => {
      cancelled = true;
    };
  }, [pirepId]);

  useEffect(() => {
    let cancelled = false;
    if (!activeFlight) {
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
  }, [activeFlight?.dpt_airport, activeFlight?.arr_airport, activeFlight]);

  // ---- Redraw: Quellen + Flugzeug-Marker + Pins ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    if (view !== "own") {
      // Eigen-Flug-Layer leeren, damit Route/Track/Marker nicht unter der VA-Übersicht durchscheinen.
      const empty: GeoJSON.FeatureCollection = { type: "FeatureCollection", features: [] };
      (map.getSource(SRC_ROUTE) as maplibregl.GeoJSONSource | undefined)?.setData(empty);
      (map.getSource(SRC_WPTS) as maplibregl.GeoJSONSource | undefined)?.setData(empty);
      (map.getSource(SRC_TRACK) as maplibregl.GeoJSONSource | undefined)?.setData(empty);
      acMarkerRef.current?.remove();
      acMarkerRef.current = null;
      pinMarkersRef.current.forEach((m) => m.remove());
      pinMarkersRef.current = [];
      zoomTargetRef.current = null; // beim Zurückkehren Zoom neu setzen
      zoomingRef.current = false;
      return;
    }
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
      if (follow) {
        // Phasenabhängiger Zoom (Boden nah, Reiseflug weit).
        const tz = targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft);
        const c = map.getCenter();
        const farJump = Math.abs(c.lng - lngLat[0]) > 3 || Math.abs(c.lat - lngLat[1]) > 3;
        if (farJump) {
          // großer Sprung (erster Frame / Teleport): hart setzen.
          map.jumpTo({ center: lngLat, zoom: tz });
          zoomTargetRef.current = tz;
          zoomingRef.current = false;
        } else {
          // Phasenwechsel → neues Zoomziel ansteuern, bis es erreicht ist.
          if (zoomTargetRef.current == null || Math.abs(zoomTargetRef.current - tz) > 0.25) {
            zoomTargetRef.current = tz;
            zoomingRef.current = true;
          }
          if (zoomingRef.current) {
            // WICHTIG: Zoom MIT ansteuern, sonst bricht das 100-ms-Folge-easeTo
            // die Zoomfahrt ab und sie bleibt auf halbem Weg stecken.
            map.easeTo({ center: lngLat, zoom: tz, duration: 250 });
            if (Math.abs(map.getZoom() - tz) < 0.1) zoomingRef.current = false;
          } else {
            // Ziel erreicht → nur noch schwenken (manuelles +/- bleibt erhalten).
            map.easeTo({ center: lngLat, duration: 380 });
          }
        }
      } else {
        zoomTargetRef.current = null;
        zoomingRef.current = false;
      }
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
  }, [mapReady, view, follow, simSnapshot, routeFixes, depArr.dep, depArr.arr]);

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
    const key = `${effFixes.length}-${effDepIcao}-${effArrIcao}`;
    if (pts.length >= 2 && fittedRef.current !== key) {
      fittedRef.current = key;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 80, duration: 600, maxZoom: 8 });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapReady, view, follow, effFixes, effDepIcao, effArrIcao]);

  // ---- VA-Übersicht ----
  useEffect(() => {
    if (view !== "va") {
      vaMarkersRef.current.forEach((m) => m.remove());
      vaMarkersRef.current = [];
      vaPopupRef.current?.remove();
      vaPopupRef.current = null;
      vaFittedRef.current = false;
      return;
    }
    let cancelled = false;
    const poll = async () => {
      try {
        // /api/acars liefert { data: [...] } — defensiv auch flights / Array.
        const data = await invoke<{ data?: VaFlight[]; flights?: VaFlight[] } | VaFlight[]>("va_live_flights");
        const flights = Array.isArray(data) ? data : data?.data ?? data?.flights ?? [];
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
  }, [view]);

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
      const lngLat: [number, number] = [lon, lat];
      const el = document.createElement("div");
      el.className = "aa-ac-marker aa-ac-marker--va";
      el.innerHTML = planeSvg();
      el.title = `${f.ident ?? f.flight_number ?? "?"} · ${f.aircraft?.icao ?? ""} · ${f.dpt_airport_id ?? ""}→${f.arr_airport_id ?? ""}`;
      // Klick → Popup mit Flugdaten (ersetzt ein evtl. offenes Popup).
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        vaPopupRef.current?.remove();
        vaPopupRef.current = new maplibregl.Popup({ offset: 16, closeButton: true, className: "aa-vapop", maxWidth: "260px" })
          .setLngLat(lngLat)
          .setHTML(vaPopupHtml(f))
          .addTo(map);
      });
      vaMarkersRef.current.push(
        new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat(lngLat).setRotation(f.position?.heading ?? 0).addTo(map),
      );
      pts.push(lngLat);
    }
    // Nur einmal je VA-Sitzung einpassen, sonst zuckt die Karte alle 12 s.
    if (pts.length >= 1 && !vaFittedRef.current) {
      vaFittedRef.current = true;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 60, duration: 600, maxZoom: 6 });
    }
  }, [vaFlights, view, mapReady]);

  // ---- Stats ----
  const stats = useMemo(() => {
    const fmt = (v: number | null | undefined, suffix: string) =>
      v == null || Number.isNaN(v) ? "—" : `${Math.round(v)}${suffix}`;
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
  }, [simSnapshot, activeFlight]);

  const showOwnContent = view === "own" && !!activeFlight;

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
            onClick={() => setView("va")}
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
        </div>
      </div>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {view === "own" && !activeFlight && (
            <div className="aa-livemap__empty">
              Kein aktiver Flug — starte einen Flug, um ihn live zu verfolgen.
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
