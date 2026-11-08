// Bilder auf der Live-Karte (P-11.36): Marken der Bilder desselben Flugs auf der Spur,
// Fokus auf die Stelle, die im Bilder-Tab angeklickt wurde, und der Umschalter
// Karte/Weltkugel (`setProjection` mit `globe`, MapLibre 5). Haengt sich an die Karte von
// `LiveMapView`; ohne Bilder und ohne Fokus tut der Haken nichts. Ein Klick auf eine Marke
// liefert `gross`, das `LiveMapView` als Grossansicht zeigt.
import { useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import { useTranslation } from "react-i18next";
import maplibregl from "maplibre-gl";
import { bilderStore, useBilder } from "./bilderStore";
import { bildZeitIso } from "./Kacheln";
import { anzeigbar, flugSchluessel } from "./logik/gruppen";
import type { Eintrag } from "./logik/typen";
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
  const { t } = useTranslation();
  const z = useBilder();
  const [kugel, setKugel] = useState(false);
  const [gross, setGross] = useState<{ id: string; bilder: Eintrag[] } | null>(null);
  const pirep = fokus?.pirep ?? aktivePirep;
  // Die Liste fuellt sonst erst der Bilder-Tab; ohne sie blieben die Punkte aus, bis er einmal offen war.
  const ohneListe = !z.liste;
  useEffect(() => { if (pirep !== null && ohneListe) void bilderStore.laden(); }, [pirep, ohneListe]);
  const flugBilder = useMemo(
    () =>
      (z.liste?.eintraege ?? [])
        .filter((e) => anzeigbar(e) && pirep !== null && flugSchluessel(e) === pirep)
        .sort((a, b) => (bildZeitIso(a) ?? "").localeCompare(bildZeitIso(b) ?? "")),
    [z.liste, pirep],
  );
  const marken = useMemo(
    () =>
      flugBilder
        .filter((e) => typeof e.sidecar?.meta?.breite === "number" && typeof e.sidecar?.meta?.laenge === "number")
        .map((e) => ({ id: e.id, lat: e.sidecar!.meta!.breite!, lon: e.sidecar!.meta!.laenge!, zeit: e.sidecar!.meta!.zeit_utc?.slice(11, 16) ?? "" })),
    [flugBilder],
  );

  // Am Tablet kommt die Liste alle paar Sekunden neu: unveraenderte Marken nicht neu bauen, sonst geht ein Tippen verloren.
  const markenSchluessel = JSON.stringify(marken);
  const aktuell = useRef({ marken, flugBilder });
  aktuell.current = { marken, flugBilder };
  useEffect(() => {
    const map = mapRef.current;
    const { marken } = aktuell.current;
    if (!map || marken.length === 0) return;
    const gesetzt = marken.map((m) => {
      const el = document.createElement("button");
      el.type = "button";
      el.className = "bt-karte__marke" + (fokus?.id === m.id ? " bt-karte__marke--aktiv" : "");
      const name = t("bilder.karte.bild_marke", { name: m.zeit ? `${m.zeit}Z` : m.id });
      el.title = name;
      el.setAttribute("aria-label", name);
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        setGross({ id: m.id, bilder: aktuell.current.flugBilder });
      });
      return new maplibregl.Marker({ element: el }).setLngLat([m.lon, m.lat]).addTo(map);
    });
    return () => gesetzt.forEach((m) => m.remove());
  }, [mapRef, markenSchluessel, fokus?.id, t]);

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

  return { kugel, setKugel, marken: marken.length, gross, grossZu: () => setGross(null) };
}
