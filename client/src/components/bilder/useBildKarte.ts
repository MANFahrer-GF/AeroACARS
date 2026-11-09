// Bilder auf der Live-Karte (P-11.36): Marken auf der Karte, Fokus auf die Stelle, die im
// Bilder-Tab angeklickt wurde, und der Umschalter Karte/Weltkugel (`setProjection` mit `globe`,
// MapLibre 5). Welche Marken: ohne aktiven Flug alle anzeigbaren Bilder mit Position aller
// Fluege; mit aktivem Flug dessen Bilder plus die des Fluges aus dem Fokus (der Fokus
// verdraengt den aktiven Flug nicht). Haengt sich an die Karte von `LiveMapView`; ohne Bilder
// und ohne Fokus tut der Haken nichts. Ein Klick auf eine Marke liefert `gross` mit den Bildern
// DESSELBEN Flugs, das `LiveMapView` als Grossansicht zeigt. Die Projektion wird gesetzt, sobald
// der Stil steht; die Wahl "Weltkugel" bleibt in localStorage (`aaLivemapKugel`).
import { useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import { useTranslation } from "react-i18next";
import maplibregl from "maplibre-gl";
import { bilderStore, useBilder } from "./bilderStore";
import { bildZeitIso } from "./Kacheln";
import { anzeigbar, flugSchluessel } from "./logik/gruppen";
import { markeMitVorschau } from "../../flugkarte/markenVorschau";
import { useMarkenVorschau } from "./useMarkenVorschau";
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

const KUGEL_SCHLUESSEL = "aaLivemapKugel";

export function useBildKarte(mapRef: MutableRefObject<maplibregl.Map | null>, fokus: BildFokus | null, aktivePirep: string | null, sichtbar = true) {
  const { t } = useTranslation();
  const z = useBilder();
  const [kugel, setKugel] = useState(() => {
    try { return typeof localStorage !== "undefined" && localStorage.getItem(KUGEL_SCHLUESSEL) === "1"; } catch { return false; }
  });
  const [bilderAn, setBilderAn] = useState(true);
  const [gross, setGross] = useState<{ id: string; bilder: Eintrag[] } | null>(null);
  const fokusPirep = fokus?.pirep ?? null;
  // Die Liste fuellt sonst erst der Bilder-Tab; ohne sie blieben die Punkte aus, bis er einmal offen war.
  // `sichtbar` als Abhaengigkeit: Scheitert der erste Abruf beim Programmstart, holt ihn das Oeffnen der Karte nach.
  const ohneListe = !z.liste;
  useEffect(() => { if (ohneListe) void bilderStore.laden(); }, [ohneListe, sichtbar]);
  const flugBilder = useMemo(
    () =>
      (z.liste?.eintraege ?? [])
        .filter((e) => anzeigbar(e) && (aktivePirep === null || flugSchluessel(e) === aktivePirep || (fokusPirep !== null && flugSchluessel(e) === fokusPirep)))
        .sort((a, b) => (bildZeitIso(a) ?? "").localeCompare(bildZeitIso(b) ?? "")),
    [z.liste, aktivePirep, fokusPirep],
  );
  const marken = useMemo(
    () =>
      flugBilder
        .filter((e) => typeof e.sidecar?.meta?.breite === "number" && typeof e.sidecar?.meta?.laenge === "number")
        .map((e) => ({ id: e.id, flug: flugSchluessel(e), lat: e.sidecar!.meta!.breite!, lon: e.sidecar!.meta!.laenge!, zeit: e.sidecar!.meta!.zeit_utc?.slice(11, 16) ?? "" })),
    [flugBilder],
  );

  // Am Tablet kommt die Liste alle paar Sekunden neu: unveraenderte Marken nicht neu bauen, sonst geht ein Tippen verloren.
  const markenSchluessel = JSON.stringify(marken);
  const vorschauVon = useMarkenVorschau();
  const aktuell = useRef({ marken, flugBilder, vorschauVon });
  aktuell.current = { marken, flugBilder, vorschauVon };
  useEffect(() => {
    const map = mapRef.current;
    const { marken } = aktuell.current;
    if (!map || marken.length === 0 || !bilderAn) return;
    const gesetzt = marken.map((m) => {
      const el = document.createElement("button");
      el.type = "button";
      el.className = "bt-karte__marke" + (fokus?.id === m.id ? " bt-karte__marke--aktiv" : "");
      const name = t("bilder.karte.bild_marke", { name: m.zeit ? `${m.zeit}Z` : m.id });
      el.title = name;
      el.setAttribute("aria-label", name);
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        setGross({ id: m.id, bilder: aktuell.current.flugBilder.filter((e) => flugSchluessel(e) === m.flug) });
      });
      const vorschauZu = markeMitVorschau(el, map, name, () => aktuell.current.vorschauVon(aktuell.current.flugBilder.find((e) => e.id === m.id)));
      return { marke: new maplibregl.Marker({ element: el }).setLngLat([m.lon, m.lat]).addTo(map), vorschauZu };
    });
    return () => gesetzt.forEach((m) => { m.vorschauZu(); m.marke.remove(); });
  }, [mapRef, markenSchluessel, fokus?.id, t, bilderAn]);

  // Ohne Flug startet die Karte ueber Europa: Einmal auf die Punkte einpassen, sonst liegen etwa
  // Bilder aus Japan ausserhalb des Bildes. Nicht bei verdeckter Karte (Groesse 0) und nicht gegen einen Fokus.
  const eingepasst = useRef(false);
  useEffect(() => {
    if (aktivePirep !== null) { eingepasst.current = false; return; }
    const map = mapRef.current;
    const { marken } = aktuell.current;
    if (!map || !sichtbar || !bilderAn || fokus || eingepasst.current || marken.length === 0) return;
    eingepasst.current = true;
    const laengen = marken.map((m) => m.lon);
    const breiten = marken.map((m) => m.lat);
    map.fitBounds(
      [[Math.min(...laengen), Math.min(...breiten)], [Math.max(...laengen), Math.max(...breiten)]],
      { padding: { top: 120, bottom: 60, left: 60, right: 60 }, maxZoom: 6, duration: 0 },
    );
  }, [mapRef, aktivePirep, markenSchluessel, sichtbar, bilderAn, fokus]);

  useEffect(() => {
    if (fokus) setBilderAn(true); // sonst fliegt die Karte zu einem unsichtbaren Punkt
    const map = mapRef.current;
    if (!map || !fokus) return;
    map.flyTo({ center: [fokus.lon, fokus.lat], zoom: Math.max(map.getZoom(), 9), duration: 800 });
  }, [mapRef, fokus?.n]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    try { localStorage.setItem(KUGEL_SCHLUESSEL, kugel ? "1" : "0"); } catch { /* Speichern ist best-effort */ }
    const map = mapRef.current;
    if (!map) return;
    const setzen = () => {
      try { map.setProjection({ type: kugel ? "globe" : "mercator" }); } catch { /* Fassung ohne Projektionen oder Stil noch nicht da */ }
    };
    // Vor dem ersten style.load wirft setProjection: dann gilt dieser Aufruf.
    map.on("style.load", setzen);
    setzen();
    return () => { map.off("style.load", setzen); };
  }, [mapRef, kugel]);

  return { kugel, setKugel, bilderAn, setBilderAn, marken: marken.length, gross, grossZu: () => setGross(null) };
}
