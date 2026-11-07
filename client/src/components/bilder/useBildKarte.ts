// Bilder auf der Live-Karte (P-11.36): Marken der Bilder desselben Flugs auf der Spur,
// Fokus auf die Stelle, die im Bilder-Tab angeklickt wurde, und der Umschalter
// Karte/Weltkugel (`setProjection` mit `globe`, MapLibre 5). Haengt sich an die Karte von
// `LiveMapView`; ohne Bilder und ohne Fokus tut der Haken nichts.
import { useEffect, useMemo, useState, type MutableRefObject } from "react";
import maplibregl from "maplibre-gl";
import { useBilder } from "./bilderStore";
import { anzeigbar, flugSchluessel } from "./logik/gruppen";
import "./bilder.css";

export interface BildFokus {
  lat: number;
  lon: number;
  id: string;
  pirep: string | null;
  /** Zaehler: ein neuer Klick auf dieselbe Stelle fliegt erneut hin. */
  n: number;
}

export function useBildKarte(mapRef: MutableRefObject<maplibregl.Map | null>, fokus: BildFokus | null, aktivePirep: string | null) {
  const z = useBilder();
  const [kugel, setKugel] = useState(false);
  const pirep = fokus?.pirep ?? aktivePirep;
  const marken = useMemo(
    () =>
      (z.liste?.eintraege ?? [])
        .filter((e) => anzeigbar(e) && pirep !== null && flugSchluessel(e) === pirep)
        .filter((e) => typeof e.sidecar?.meta?.breite === "number" && typeof e.sidecar?.meta?.laenge === "number")
        .map((e) => ({ id: e.id, lat: e.sidecar!.meta!.breite!, lon: e.sidecar!.meta!.laenge!, zeit: e.sidecar!.meta!.zeit_utc?.slice(11, 16) ?? "" })),
    [z.liste, pirep],
  );

  useEffect(() => {
    const map = mapRef.current;
    if (!map || marken.length === 0) return;
    const gesetzt = marken.map((m) => {
      const el = document.createElement("div");
      el.className = "bt-karte__marke" + (fokus?.id === m.id ? " bt-karte__marke--aktiv" : "");
      el.title = m.zeit ? `${m.zeit}Z` : "";
      return new maplibregl.Marker({ element: el }).setLngLat([m.lon, m.lat]).addTo(map);
    });
    return () => gesetzt.forEach((m) => m.remove());
  }, [mapRef, marken, fokus?.id]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !fokus) return;
    map.flyTo({ center: [fokus.lon, fokus.lat], zoom: Math.max(map.getZoom(), 9), duration: 800 });
  }, [mapRef, fokus?.n]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const setzen = () => {
      try { map.setProjection({ type: kugel ? "globe" : "mercator" }); } catch { /* Fassung ohne Projektionen */ }
    };
    if (map.isStyleLoaded()) setzen();
    else map.once("load", setzen);
  }, [mapRef, kugel]);

  return { kugel, setKugel, marken: marken.length };
}
