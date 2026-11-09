// VA-Verkehr auf der Karte: Marker der Kollegen, Infokarte und ihre Routen.
import { useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import { useTranslation } from "react-i18next";
import maplibregl from "maplibre-gl";
import { invoke } from "../../lib/ipc";
import type { ActiveFlightInfo } from "../../types";
import { aircraftSvg } from "../../lib/aircraftIcon";
import { phaseColor } from "../../lib/phaseColors";
import { punkteAufKleinstemBogen } from "../../lib/datumsgrenze";
import { FREMDE_ROUTEN_EBENEN, fremdeRoutenZeichnen, type FremderPunkt } from "./fremdeRouten";
import { distNm } from "./werkzeug";

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

/** Eine Kollegen-Marke; `flug` und `lngLat` folgen dem letzten Abruf. */
interface VaMarke {
  marker: maplibregl.Marker;
  karte: maplibregl.Map;
  flug: VaFlight;
  lngLat: [number, number];
  muster: string | undefined;
}

function escHtml(s: unknown): string {
  return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string);
}
function flLabel(msl?: number | null): string {
  if (msl == null || Number.isNaN(msl)) return "—";
  return msl >= 18000 ? `FL${Math.round(msl / 100)}` : `${Math.round(msl)} ft`;
}
/** README §5 — Kollegen-Karteichen: header (callsign+pilot), route row
 *  (DEP→ARR + phase), 3-col grid (HÖHE/GS/ETA), footer (Muster). `eta` is
 *  computed outside this function (needs an async airport-coordinate
 *  lookup — see `vaEtaFor` at the call site) and passed in, defaulting to
 *  "—" until resolved so opening the popup never blocks on it. */
function vaPopupHtml(f: VaFlight, eta = "—"): string {
  const cs = f.ident || [f.airline?.icao, f.flight_number].filter(Boolean).join("") || f.flight_number || "—";
  const ac = [f.aircraft?.icao, f.aircraft?.registration].filter(Boolean).join(" · ");
  const pilot = f.user?.name ?? "";
  const phaseTxt = f.status_text ?? "";
  // Field feedback (2026-08-03): the colored phase pill from the old popup
  // ("fand ich richtig mega") comes back exactly as it was — same
  // phaseColor() source as the marker itself, not the plain dim text this
  // rewrite briefly had instead. Text + color together (not color alone)
  // already satisfies README §9's "Zustand nie nur über Farbe".
  const pcol = phaseColor(f.status_text ?? (f.phase != null ? String(f.phase) : null));
  const pos = f.position ?? {};
  const gs = pos.gs != null ? `${Math.round(pos.gs)}` : "—";
  const cell = (k: string, v: string, u = "") =>
    `<div class="aa-vapop__cell"><span class="aa-vapop__k">${k}</span><span class="aa-vapop__v">${escHtml(v)}${u ? `<i>${u}</i>` : ""}</span></div>`;
  return (
    `<div class="aa-vapop__head">` +
    `<span class="aa-vapop__cs">${escHtml(cs)}</span>` +
    (pilot ? `<span class="aa-vapop__pilot">${escHtml(pilot)}</span>` : "") +
    `</div>` +
    `<div class="aa-vapop__route">` +
    `<span>${escHtml(f.dpt_airport_id ?? "—")}<span class="aa-vapop__arrow">→</span>${escHtml(f.arr_airport_id ?? "—")}</span>` +
    (phaseTxt ? `<span class="aa-vapop__phase" style="--p:${pcol}">${escHtml(phaseTxt)}</span>` : "") +
    `</div>` +
    `<div class="aa-vapop__grid">` +
    cell("HÖHE", flLabel(pos.altitude_msl ?? pos.altitude)) +
    cell("GS", gs, " kt") +
    cell("ETA", eta) +
    `</div>` +
    (ac ? `<div class="aa-vapop__sub">${escHtml(ac)}</div>` : "")
  );
}

interface VaAngaben {
  mapRef: MutableRefObject<maplibregl.Map | null>;
  mapReady: boolean;
  sichtbar: boolean;
  activeFlight: ActiveFlightInfo | null;
  /** Eigene Bildpunkte gehen vor: Dann wird nicht auf die Kollegen eingepasst. */
  bilderVorrang: boolean;
}

export function useVaVerkehr({ mapRef, mapReady, sichtbar, activeFlight, bilderVorrang }: VaAngaben) {
  const { t } = useTranslation();
  const [showVa, setShowVa] = useState(true); // VA-Verkehr ein-/ausblenden
  const vaMarkersRef = useRef<Map<string, VaMarke>>(new Map());
  // Die eingeblendeten Routen der Kollegen: Kennung → Punkte.
  //
  // Thomas, 20.09.2026: „auf die anderen Flieger klicken und zusätzlich die
  // Route angezeigt bekommen, nach einem erneuten Klick wieder aus."
  // Mehrere gleichzeitig sind erlaubt — alle in derselben gedämpften Farbe
  // und dünner als die eigene, damit man sie nicht verwechselt.
  //
  // Bewusst ein Ref und kein State: Die Marker werden in einem Effekt
  // gebaut, der an `vaFlights` hängt. Ein State würde ihn bei jedem
  // Ein- und Ausblenden neu laufen lassen.
  const fremdeRoutenRef = useRef<Map<string, FremderPunkt[]>>(new Map());
  // Sagt, ob die Kollegen-Liste aus einem erfolgreichen Abruf stammt.
  // Eine leere Liste nach einem Fehler bedeutet nichts; eine leere Liste
  // nach einem erfolgreichen Abruf heisst „niemand mehr unterwegs".
  const vaListeAktuellRef = useRef(false);
  const vaPopupRef = useRef<maplibregl.Popup | null>(null);
  const vaPopupIdRef = useRef<string | null>(null); // welcher VA-Flug das offene Popup zeigt
  // README §5 — colleague popup ETA needs the arrival airport's coordinate,
  // which /api/acars doesn't send (only the ICAO). Cached per-ICAO so
  // reopening/refreshing a popup for the same destination doesn't re-fetch.
  const airportCoordCacheRef = useRef(new Map<string, [number, number] | null>());
  const vaFittedRef = useRef(false);
  // Als Ref: Die Liste der Bilder kommt oft erst nach dem Start des Abruf-Effekts an.
  const bilderVorrangRef = useRef(bilderVorrang);
  bilderVorrangRef.current = bilderVorrang;
  // Dead-Reckoning: VA-Marker zwischen den 12-s-Polls flüssig weiterrechnen.
  const vaDrRef = useRef<{ marker: maplibregl.Marker; lat: number; lon: number; hdg: number; gs: number }[]>([]);
  const vaDrT0Ref = useRef(0);
  const [vaFlights, setVaFlights] = useState<VaFlight[]>([]);
  // Kurzer Hinweis, wenn eine Route fehlt oder nicht geladen werden kann.
  const [fremdeRouteHinweis, setFremdeRouteHinweis] = useState<string | null>(null);
  // Ist die Kollegen-Anzeige aus, sollen auch ihre Routen verschwinden —
  // nur verstecken, nicht vergessen: Beim Wiedereinschalten sind sie
  // wieder da (Abnahme 20.09.2026).
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || !map.getLayer("fremde-routen-line")) return;
    // BEIDE Ebenen — Linie und Wegpunkte. Eine davon zu vergessen hiesse,
    // dass Punkte ohne Linie stehen bleiben (oder umgekehrt).
    for (const ebene of FREMDE_ROUTEN_EBENEN) {
      if (!map.getLayer(ebene)) continue;
      map.setLayoutProperty(ebene, "visibility", showVa ? "visible" : "none");
    }
  }, [showVa, mapReady]);

  useEffect(() => {
    if (!fremdeRouteHinweis) return;
    const id = window.setTimeout(() => setFremdeRouteHinweis(null), 4000);
    return () => window.clearTimeout(id);
  }, [fremdeRouteHinweis]);
  // VA-Flieger ohne den eigenen Flug (steckt auch in /api/acars).
  const vaVisible = useMemo(
    () => (showVa ? vaFlights.filter((f) => !activeFlight || String(f.id ?? "") !== activeFlight.pirep_id) : []),
    [showVa, vaFlights, activeFlight],
  );

  // ---- VA-Verkehr: pollt /api/acars (wenn eingeblendet) ----
  useEffect(() => {
    if (!showVa) {
      // Die Liste wird geleert, weil niemand sie sehen will — das ist
      // KEINE Aussage ueber gelandete Kollegen. Wird die Anzeige gleich
      // wieder eingeschaltet, liegt die leere Liste noch vor, waehrend
      // der erste Abruf laeuft; ohne dieses Zuruecksetzen raeumte das
      // Aufraeumen in diesem Moment alle Routen weg.
      vaListeAktuellRef.current = false;
      setVaFlights([]);
      return;
    }
    // Verdeckt nicht weiterpollen — die Blasen der Kollegen sieht ohnehin
    // niemand. Der Bestand bleibt stehen und wird beim Wiederauftauchen
    // sofort einmal aufgefrischt.
    if (!sichtbar) return;
    let cancelled = false;
    const poll = async () => {
      try {
        // /api/acars liefert { data: [...] } — defensiv auch flights / Array.
        const data = await invoke<{ data?: VaFlight[]; flights?: VaFlight[] } | VaFlight[]>("va_live_flights");
        const flights = Array.isArray(data) ? data : data?.data ?? data?.flights ?? [];
        if (!cancelled) {
          // Die Liste ist AKTUELL — auch wenn sie leer ist.
          vaListeAktuellRef.current = true;
          setVaFlights(flights);
        }
      } catch {
        if (!cancelled) {
          // Ein Netz-Haenger heisst NICHT „alle sind gelandet".
          vaListeAktuellRef.current = false;
          setVaFlights([]);
        }
      }
    };
    void poll();
    // Field feedback (2026-08-03): "nothing happened in 5 seconds", explicit
    // ask to speed this up. handoff_livemap_4a §8 actually states 15-30s as
    // the target for this exact poll — slower than even the pre-existing
    // 12s — so this is a deliberate override of the written spec, confirmed
    // with the user (asked 5s/8s/leave-at-12s; they picked 8s: faster than
    // today without going as aggressive as the un-throttled 5s option).
    const id = setInterval(poll, 8000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [showVa, sichtbar]);

  async function resolveAirportCoord(icao: string): Promise<[number, number] | null> {
    const cache = airportCoordCacheRef.current;
    if (cache.has(icao)) return cache.get(icao)!;
    try {
      const a = await invoke<{ lat?: number | null; lon?: number | null }>("airport_get", { icao });
      const coord: [number, number] | null = a.lat != null && a.lon != null ? [a.lon, a.lat] : null;
      // Nur ERFOLGE dauerhaft merken. Ein "der Flughafen hat keine
      // Koordinaten" ist eine echte, stabile Antwort — die darf gecacht
      // werden.
      cache.set(icao, coord);
      return coord;
    } catch {
      // QS 2026-08-04: hier wurde vorher `cache.set(icao, null)` gesetzt —
      // ein einmaliger Aussetzer (`airport_get` ist ein Result-Command und
      // wirft auch bei fehlendem Client/Netz-Hänger/Timeout, nicht nur bei
      // "Flughafen unbekannt") hat sich damit für die ganze Sitzung
      // eingebrannt: jede Kollegen-Blase zu diesem Ziel zeigte danach
      // dauerhaft "—" als ETA, obwohl ein Versuch eine Sekunde später
      // geklappt hätte. Fehlschläge werden jetzt NICHT gemerkt, der
      // nächste Aufruf versucht es neu — so wie es die Schwester-Abfrage
      // für den eigenen Flug (`lookup()`) oben auch schon macht.
      return null;
    }
  }
  /**
   * Route eines Kollegen ein- oder ausblenden.
   *
   * Zweiter Klick auf denselben Flieger nimmt sie wieder weg. Gibt es
   * keine Route (Flug ohne SimBrief-Plan), sagt die Karte das kurz —
   * sonst sähe es nach einem kaputten Klick aus.
   */
  async function fremdeRouteUmschalten(pirepId: string, map: maplibregl.Map) {
    if (!pirepId) return;
    if (fremdeRoutenRef.current.has(pirepId)) {
      fremdeRoutenRef.current.delete(pirepId);
      fremdeRoutenZeichnen(map, fremdeRoutenRef.current);
      return;
    }
    // Platz SOFORT belegen, noch vor dem Abruf. Sonst findet ein zweiter
    // Klick den Schlüssel nicht (der erste ist noch am Laden), lädt
    // ebenfalls, und die Route bleibt an, obwohl der Pilot sie
    // ausgeschaltet hat (Abnahme 20.09.2026).
    fremdeRoutenRef.current.set(pirepId, []);
    try {
      const punkte = await invoke<FremderPunkt[]>("fremde_flugroute", { pirepId });
      // Hat der Pilot inzwischen wieder ausgeschaltet, nichts zeichnen.
      if (!fremdeRoutenRef.current.has(pirepId)) return;
      if (!punkte || punkte.length < 2) {
        fremdeRoutenRef.current.delete(pirepId);
        setFremdeRouteHinweis(
          t("livemap.fremde_route_fehlt", {
            defaultValue: "Für diesen Flug liegt keine Route vor.",
          }),
        );
        return;
      }
      fremdeRoutenRef.current.set(pirepId, punkte);
      fremdeRoutenZeichnen(map, fremdeRoutenRef.current);
    } catch {
      fremdeRoutenRef.current.delete(pirepId);
      setFremdeRouteHinweis(
        t("livemap.fremde_route_fehler", {
          defaultValue: "Route konnte nicht geladen werden.",
        }),
      );
    }
  }

  /** README §5's ETA cell for a colleague aircraft — same great-circle +
   *  groundspeed math as the own-flight ETA (`nav`), just resolving the
   *  destination coordinate on demand instead of from the loaded route. */
  async function vaEtaFor(f: VaFlight): Promise<string> {
    const pos = f.position;
    if (!pos || pos.lat == null || pos.lon == null || pos.gs == null || pos.gs <= 30 || !f.arr_airport_id) return "—";
    const arrCoord = await resolveAirportCoord(f.arr_airport_id);
    if (!arrCoord) return "—";
    const dtg = distNm([pos.lon, pos.lat], arrCoord);
    const mins = Math.round((dtg / pos.gs) * 60);
    const arrival = new Date(Date.now() + mins * 60_000);
    return `${arrival.toISOString().slice(11, 16)}z`;
  }

  // ---- VA-Marker rendern (liegen mit auf der einen Karte) ----
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    // Routen von Kollegen aufräumen, die nicht mehr unterwegs sind.
    // Sonst bliebe die Linie als Geisterspur stehen, nachdem der Marker
    // verschwunden ist — und ohne Marker könnte niemand sie ausschalten
    // (Abnahme 20.09.2026).
    // NUR aufräumen, wenn die Liste auch etwas aussagt.
    //
    // `vaVisible` ist leer, wenn die Kollegen-Anzeige ausgeschaltet ist —
    // und ebenso nach einem fehlgeschlagenen Abruf (der Poll setzt die
    // Liste dann auf leer). Beides heisst NICHT „alle sind gelandet".
    // Ohne diese Bedingung verlor der Pilot seine eingeblendeten Routen,
    // sobald er die Anzeige kurz ausschaltete (Abnahme 20.09.2026).
    // Aufraeumen nur, wenn die Liste etwas aussagt: Sie muss aus einem
    // erfolgreichen Abruf stammen. Dann ist auch eine LEERE Liste eine
    // echte Aussage („der letzte Kollege ist gelandet") — sonst bliebe
    // seine Route als Geisterspur stehen, und ohne Marker liesse sie sich
    // nicht mehr abschalten (Abnahme 20.09.2026, dritte Runde).
    if (fremdeRoutenRef.current.size > 0 && showVa && vaListeAktuellRef.current) {
      const nochDa = new Set(vaVisible.map((f) => String(f.id ?? "")));
      let entfernt = false;
      for (const id of [...fremdeRoutenRef.current.keys()]) {
        if (!nochDa.has(id)) {
          fremdeRoutenRef.current.delete(id);
          entfernt = true;
        }
      }
      if (entfernt) fremdeRoutenZeichnen(map, fremdeRoutenRef.current);
    }
    if (vaVisible.length === 0) {
      vaPopupRef.current?.remove();
      vaPopupRef.current = null;
      vaFittedRef.current = false;
    }
    const pts: [number, number][] = [];
    const dr: { marker: maplibregl.Marker; lat: number; lon: number; hdg: number; gs: number }[] = [];
    const popupTargets = new Map<string, { lngLat: [number, number]; f: VaFlight }>();
    const nochSichtbar = new Set<string>();
    vaVisible.forEach((f, index) => {
      const lat = f.position?.lat;
      const lon = f.position?.lon;
      if (typeof lat !== "number" || typeof lon !== "number") return;
      const lngLat: [number, number] = [lon, lat];
      const kennung = String(f.id ?? f.ident ?? f.flight_number ?? "");
      popupTargets.set(kennung, { lngLat, f });
      // Dieselbe Marke weiterverwenden: Ein Neubau bei jedem 8-s-Abruf nahm dem
      // Tippen auf dem Tablet das Element unter dem Finger weg.
      const schluessel = kennung || `ohne-kennung-${index}`;
      nochSichtbar.add(schluessel);
      let marke = vaMarkersRef.current.get(schluessel);
      if (marke && marke.karte !== map) {
        marke.marker.remove();
        marke = undefined;
      }
      if (!marke) {
      const el = document.createElement("div");
      el.className = "aa-ac-marker aa-ac-marker--va";
      const neu: VaMarke = { marker: new maplibregl.Marker({ element: el, rotationAlignment: "map" }), karte: map, flug: f, lngLat, muster: undefined };
      // Klick → Popup mit Flugdaten (ersetzt ein evtl. offenes Popup).
      el.addEventListener("click", (ev) => {
        ev.stopPropagation();
        // Immer der Stand des letzten Abrufs, die Marke lebt länger als ein Takt.
        const { flug: f, lngLat } = neu;
        const thisId = String(f.id ?? f.ident ?? f.flight_number ?? "");
        // Route des Kollegen ein- oder ausblenden (Thomas, 20.09.2026).
        void fremdeRouteUmschalten(String(f.id ?? ""), map);
        const warOffen = vaPopupIdRef.current === thisId && vaPopupRef.current != null;
        vaPopupRef.current?.remove();
        // Zweiter Klick auf DENSELBEN Kollegen: Karte zu, Route aus —
        // ein Umschalter, der in beide Richtungen dasselbe tut. Vorher
        // ging die Karte dabei wieder auf und verdeckte die Route, und
        // man musste woanders hinklicken, um sie loszuwerden (Thomas,
        // 20.09.2026: „das Infofeld vom Piloten verdeckt es … kmm").
        if (warOffen) {
          vaPopupRef.current = null;
          vaPopupIdRef.current = null;
          return;
        }
        vaPopupIdRef.current = thisId;
        // Etwas weiter vom Flieger weg als die 16 px vorher: Die Route
        // laeuft durch den Marker, und die Karte sass direkt darauf.
        const popup = new maplibregl.Popup({ offset: 26, closeButton: true, className: "aa-vapop", maxWidth: "260px" })
          .setLngLat(lngLat)
          .setHTML(vaPopupHtml(f))
          .addTo(map);
        // manuelles Schließen (X) → Refs aufräumen, sonst zeigt der Rebuild ins Leere.
        popup.on("close", () => {
          if (vaPopupRef.current === popup) {
            vaPopupRef.current = null;
            vaPopupIdRef.current = null;
          }
        });
        vaPopupRef.current = popup;
        // ETA needs an airport-coordinate lookup the popup can't wait on —
        // show "—" first, fill it in once resolved, but only if this exact
        // popup is still the one open (pilot may have clicked elsewhere or
        // closed it by the time the lookup returns).
        void vaEtaFor(f).then((eta) => {
          if (vaPopupRef.current === popup && vaPopupIdRef.current === thisId) popup.setHTML(vaPopupHtml(f, eta));
        });
      });
      neu.marker.setLngLat(lngLat).addTo(map);
      vaMarkersRef.current.set(schluessel, neu);
      marke = neu;
      }
      marke.flug = f;
      marke.lngLat = lngLat;
      const el = marke.marker.getElement();
      if (marke.muster !== (f.aircraft?.icao ?? "")) {
        marke.muster = f.aircraft?.icao ?? "";
        el.innerHTML = aircraftSvg(f.aircraft?.icao);
      }
      el.style.setProperty("--ac-color", phaseColor(f.status_text ?? (f.phase != null ? String(f.phase) : null)));
      el.title = `${f.ident ?? f.flight_number ?? "?"} · ${f.aircraft?.icao ?? ""} · ${f.dpt_airport_id ?? ""}→${f.arr_airport_id ?? ""}`;
      const marker = marke.marker;
      marker.setLngLat(lngLat).setRotation(f.position?.heading ?? 0);
      dr.push({ marker, lat, lon, hdg: f.position?.heading ?? 0, gs: f.position?.gs ?? 0 });
      pts.push(lngLat);
    });
    for (const [schluessel, marke] of vaMarkersRef.current) {
      if (nochSichtbar.has(schluessel)) continue;
      marke.marker.remove();
      vaMarkersRef.current.delete(schluessel);
    }
    vaDrRef.current = dr;
    vaDrT0Ref.current = Date.now();
    // v0.15.6: offenes VA-Popup beim Rebuild an den passenden Flug nachführen
    // (frische Position + Daten) statt es an einem gerade entfernten Marker
    // verwaisen zu lassen; ist der Flug weg, Popup schließen.
    if (vaPopupRef.current && vaPopupIdRef.current) {
      const t = popupTargets.get(vaPopupIdRef.current);
      if (t) {
        const popup = vaPopupRef.current;
        const openId = vaPopupIdRef.current;
        popup.setLngLat(t.lngLat).setHTML(vaPopupHtml(t.f));
        void vaEtaFor(t.f).then((eta) => {
          if (vaPopupRef.current === popup && vaPopupIdRef.current === openId) popup.setHTML(vaPopupHtml(t.f, eta));
        });
      } else {
        vaPopupRef.current.remove();
        vaPopupRef.current = null;
        vaPopupIdRef.current = null;
      }
    }
    // Nur auf die VA-Flieger einpassen, wenn KEIN eigener Flug die Kamera führt
    // (sonst würde es dich vom eigenen Flug wegziehen). Und nur einmal. Eigene
    // Bildpunkte gehen vor: useBildKarte passt ohne Flug auf sie ein.
    if (pts.length >= 1 && !vaFittedRef.current && !activeFlight) {
      // Auch bei Vorrang als erledigt merken: Sonst sprange die Kamera, sobald „Bilder“ ausgeht.
      vaFittedRef.current = true;
      if (bilderVorrangRef.current) return;
      // Kleinster Bogen: Kollegen beiderseits der Datumsgrenze ergeben sonst
      // einen Ausschnitt ueber fast die ganze Welt.
      const bogen = punkteAufKleinstemBogen(pts);
      const b = bogen.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(bogen[0], bogen[0]));
      map.fitBounds(b, { padding: 60, duration: 600, maxZoom: 6 });
    }
  }, [vaVisible, mapReady, activeFlight]);

  // Dead-Reckoning: VA-Marker zwischen den 12-s-Polls flüssig entlang
  // Heading+Groundspeed weiterbewegen (snappen beim nächsten Poll auf die
  // echte Position). Gibt den Live-Eindruck ohne separaten Live-Endpoint.
  useEffect(() => {
    if (!mapReady) return;
    // Koppelnavigation der VA-Blasen: verdeckt sinnlos, der Bestand wird beim
    // Auftauchen ohnehin frisch geholt.
    if (!sichtbar) return;
    const id = setInterval(() => {
      const dtH = (Date.now() - vaDrT0Ref.current) / 3600000; // Stunden seit Poll
      // Bleibt der Poll aus (Netz weg, Tab im Hintergrund), nicht endlos
      // weiterrechnen: nach langer Pause lag die Breite jenseits ±90° und MapLibre
      // warf „Invalid LngLat“ (GlitchTip #230, Client 2.1.13, 26.10.2026).
      if (dtH <= 0 || dtH > 60 / 3600) return;
      for (const e of vaDrRef.current) {
        if (e.gs < 30) continue; // am Boden/langsam → nicht extrapolieren
        const distNm = e.gs * dtH;
        const th = (e.hdg * Math.PI) / 180;
        const dLat = (distNm * Math.cos(th)) / 60;
        const dLon = (distNm * Math.sin(th)) / (60 * Math.max(0.2, Math.cos((e.lat * Math.PI) / 180)));
        e.marker.setLngLat([e.lon + dLon, e.lat + dLat]);
      }
    }, 1000);
    return () => clearInterval(id);
  }, [mapReady, sichtbar]);

  return { showVa, setShowVa, vaVisible, fremdeRouteHinweis, setFremdeRouteHinweis };
}
