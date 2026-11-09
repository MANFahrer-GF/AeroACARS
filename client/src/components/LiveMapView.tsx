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
// Rein Anzeige — keine Wertung. Die Kartenebenen liegen als Bausteine in `livekarte/`.

import { BildGross } from "./bilder/BildGross";
import { useBildKarte, type BildFokus } from "./bilder/useBildKarte";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, Bid, SimSnapshot } from "../types";
import { aircraftSvg } from "../lib/aircraftIcon";
import { phaseColor, phaseLabel as formatPhase } from "../lib/phaseColors";
import { displayCallsign } from "../lib/callsign";
import { simKindLabel } from "../lib/simKind";
import { useMapEvents, LiveMapEventList } from "./LiveMapEvents";
import { LiveMapEmptyState, nextBidInfo, type NextBidInfo } from "./LiveMapEmptyState";
import { LiveRecordingIndicator } from "./LiveRecordingIndicator";
import { kartenAnfrage, useKartengrundlage } from "./BasemapContext";
import { grundkarteBauen, grundkarteHolen, startStil, type Grundkartensteuerung } from "./livekarte/grundkarte";
import { useLiveZugangTakt } from "../lib/liveInhalt";
import { ohneDatumsgrenzenSprung } from "../lib/datumsgrenze";
import { overlayFarben } from "./livekarte/farben";
import { FlugPanel } from "./livekarte/FlugPanel";
import { KartenLeiste } from "./livekarte/KartenLeiste";
import { flugDatenSetzen, flugEbenenAnlegen, LYR_ROUTE_CASING, TRACK_LAYERS, type RouteFix } from "./livekarte/flugEbenen";
import { useEigenerFlugDaten } from "./livekarte/eigenerFlug";
import { fremdeRoutenAnlegen } from "./livekarte/fremdeRouten";
import { netzEbenenAnlegen, useNetzKarte } from "./livekarte/netzKarte";
import { TAXI_LAYERS, taxiEbenenAnlegen, useTaxiKarte } from "./livekarte/taxiKarte";
import { useVaVerkehr } from "./livekarte/vaVerkehr";
import { distNm } from "./livekarte/werkzeug";

// Die Stil-Adressen kommen vom Server — siehe `BasemapContext`. CARTO
// verlangt seit dem 26.08.2026 einen Schlüssel, und der soll austauschbar
// sein, ohne dass jeder Pilot ein Update braucht. Aufbau: `livekarte/grundkarte.ts`.

interface Aircraft {
  lon: number;
  lat: number;
  hdg: number;
}

function readTheme(): "dark" | "light" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
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

interface Props {
  activeFlight: ActiveFlightInfo | null;
  simSnapshot: SimSnapshot | null;
  /** For the System-Zeile's "Recorder aktiv · {Simulator} · …" (README §7) —
   *  the same value App.tsx's own sidebar status pill already shows. */
  simKind: string | undefined;
  onSwitchToBriefing: () => void;
  /** Liegt der Karten-Reiter gerade vorn?
   *
   *  Die Karte bleibt seit v1.6.11 ueber Reiterwechsel hinweg eingehaengt —
   *  sie neu aufzubauen kostete jedes Mal Stil, Schriften und Kacheln von
   *  CARTO/ArcGIS, und genau das war die gemeldete Traegheit. Verdeckt braucht
   *  sie aber nicht weiterzupollen: die Streckenaufzeichnung laeuft ohnehin im
   *  Rust-Teil (`record_track_point`, fenster-unabhaengig), die Karte LIEST sie
   *  nur. Pausieren verliert also keine Daten.
   *
   *  Optional mit Vorgabe `true`, damit Einbindungen ohne diese Angabe
   *  (Tests, Vorschau) sich unveraendert verhalten. */
  sichtbar?: boolean;
  /** Stelle, die im Bilder-Tab angeklickt wurde (P-11.36): die Karte fliegt dorthin. */
  bildFokus?: BildFokus | null;
}

export function LiveMapView({ activeFlight, simSnapshot, simKind, onSwitchToBriefing, sichtbar = true, bildFokus = null }: Props) {
  const grundlage = useKartengrundlage();
  const zugangTakt = useLiveZugangTakt();
  // Die Karte wird einmalig gebaut; der Schlüssel kommt erst danach vom
  // Server. Über diesen Verweis liest `kartenAnfrage` bei jeder Kachel
  // den aktuellen Stand — siehe dort.
  const grundlageRef = useRef(grundlage);
  grundlageRef.current = grundlage;
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const acMarkerRef = useRef<maplibregl.Marker | null>(null);
  const pinMarkersRef = useRef<maplibregl.Marker[]>([]);
  const pinStandRef = useRef<{ karte: maplibregl.Map; schluessel: string } | null>(null); // wofuer die Pins zuletzt gebaut wurden
  const acCatRef = useRef<string | null>(null); // zuletzt gerendertes Eigen-Flieger-Icon (ICAO)
  // v0.15.8: „beim bewussten Einschalten von Follow EINMAL den phasen-passenden
  // Zoom setzen" — danach bleibt der manuelle Zoom des Nutzers erhalten (laufendes
  // Folgen schwenkt nur noch). Startet true, damit die erste Aktivierung greift.
  const followEngageRef = useRef(true);
  // v0.15.6: verhindert, dass ein Nutzer-Pan (der Follow ausschaltet) die
  // „auf-die-ganze-Route-zoomen"-Logik auslöst. Nur BEWUSSTES Abhaken von Follow
  // soll auf die Route fitten — ein Pan lässt die Ansicht einfach stehen.
  const suppressRouteFitRef = useRef(false);
  const grundkarteRef = useRef<Grundkartensteuerung | null>(null);

  const [mapReady, setMapReady] = useState(false);
  const [follow, setFollow] = useState(true);
  // Basemap: "auto" = theme-gekoppelt (dark/light), "sat" = Esri-Satellit (manuell).
  const [basemap, setBasemap] = useState<"auto" | "sat">(
    () => (typeof localStorage !== "undefined" && localStorage.getItem("aaLivemapBasemap") === "sat" ? "sat" : "auto"),
  );

  // Karte in Flugrichtung drehen (Track-up). BEWUSST abschaltbar und
  // standardmäßig AUS: Eine sich mitdrehende Karte ist beim Fliegen
  // hilfreich, beim Planen und Nachschauen aber hinderlich — Norden oben
  // ist die Erwartung, solange man nicht ausdrücklich etwas anderes will.
  const [trackUp, setTrackUp] = useState<boolean>(
    () => typeof localStorage !== "undefined" && localStorage.getItem("aaLivemapTrackUp") === "1",
  );
  const simSnapshotRef = useRef<SimSnapshot | null>(null);
  useEffect(() => { simSnapshotRef.current = simSnapshot; });
  const { netz, setNetz, flModus, setFlModus, flWert, setFlWert, netzZeit } = useNetzKarte({ mapRef, mapReady, sichtbar, zugangTakt, simSnapshotRef });
  const groundLoaded = useTaxiKarte(mapRef, mapReady);
  const [theme, setTheme] = useState<"dark" | "light">(readTheme());
  // Wird der Reiter wieder vorgeholt, muss MapLibre seine Leinwand neu
  // vermessen: verdeckt steht der Behaelter auf `display: none` und damit auf
  // 0x0. Ohne dieses `resize` bliebe die Karte beim Zurueckkommen leer oder
  // stark verzerrt — der klassische Fallstrick beim Verstecken statt Ausbauen.
  //
  // Zwei Bilder Verzoegerung, damit der Browser das Layout schon gerechnet hat:
  // ein `resize` im selben Bild misst noch die alten 0x0.
  useEffect(() => {
    if (!sichtbar || !mapReady) return;
    let id2 = 0;
    const id1 = requestAnimationFrame(() => {
      id2 = requestAnimationFrame(() => mapRef.current?.resize());
    });
    return () => {
      cancelAnimationFrame(id1);
      if (id2) cancelAnimationFrame(id2);
    };
  }, [sichtbar, mapReady]);

  // README §6/§2: next booked flight, shown in the idle empty-state card AND
  // the header's idle context line — fetched once per active-flight-identity
  // so both read the same answer instead of polling `phpvms_get_bids` twice.
  // `undefined` = loading.
  //
  // v1.4.7 (Feld-Report Thomas, Screenshot): dep-array war `[]` — fetchte
  // GENAU EINMAL beim Mount und nie wieder. Ein Pilot der die Karte offen
  // hatte, während er BTI358 flog und den PIREP absendete, sah danach
  // weiterhin BTI358 als "nächster gebuchter Flug" — nicht weil phpVMS den
  // Bid nicht entfernt hätte, sondern weil diese Komponente serverseitige
  // Änderungen gar nicht erst nachfragte. Jetzt an `activeFlight?.pirep_id`
  // gekoppelt: refetcht bei jedem Flug-Start (nicht mehr "next" relevant)
  // UND bei jedem Flug-Ende (genau der Moment, in dem "next" veraltet sein
  // könnte).
  const [next, setNext] = useState<NextBidInfo | null | undefined>(undefined);
  useEffect(() => {
    let cancelled = false;
    setNext(undefined);
    void (async () => {
      try {
        const bids = await invoke<Bid[]>("phpvms_get_bids");
        const bid = bids[0] ?? null;
        const flight = bid?.flight;
        // Field bug (2026-08-03): phpVMS's own `dpt_time` schedule field can
        // be null on a real, existing booking (verified — not a "no time
        // set" placeholder, genuinely absent on this booking's API
        // response). BidsList.tsx already has a fallback for exactly this
        // (`f.dpt_time ?? sched_out_epoch via bid_simbrief_preview`) —
        // replicated here rather than showing "no flight booked" for a
        // flight that plainly IS booked.
        let std: string | null = flight?.dpt_time ?? null;
        if (!std && bid) {
          try {
            const preview = await invoke<{ sched_out_epoch: number | null }>("bid_simbrief_preview", {
              bidId: bid.id,
            });
            if (preview?.sched_out_epoch != null) {
              const d = new Date(preview.sched_out_epoch * 1000);
              std = `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
            }
          } catch {
            // no SimBrief-direct preview either — std stays null, the card
            // just won't show a departure time for this booking.
          }
        }
        if (!cancelled) setNext(nextBidInfo(bid, std));
      } catch {
        if (!cancelled) setNext(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeFlight?.pirep_id]);
  // README §2: header UTC clock, ticking every second — same pattern as
  // PilotHeader.tsx's own zulu clock.
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    // QS-Befund 18.08.2026: seit die Karte ueber Reiterwechsel hinweg
    // eingehaengt bleibt, haette dieser Takt die (sehr grosse) Komponente
    // waehrend des GANZEN Fluges jede Sekunde neu gerendert — auch verdeckt.
    // Genau die Grundlast, die der Umbau vermeiden sollte. Beim Auftauchen
    // wird sofort einmal nachgestellt, damit die Uhr nicht nachgeht.
    if (!sichtbar) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [sichtbar]);
  const { routeFixes, depArr, trackPoints } = useEigenerFlugDaten(activeFlight, sichtbar);

  // v1.4.7 (Feld-Report Thomas, "bei Kurs oben zittert die Karte immer"):
  // rohe Telemetrie-Peilung ändert sich JEDEN 500ms-Poll um Bruchteile eines
  // Grades. Zwei Effekte (Orientierung unten + der Redraw-Effekt weiter unten
  // im File) lösen davon abhängig je einen eigenen 350–400ms `easeTo` auf die
  // Bahn aus — bei ~500ms Poll-Intervall wird das Ease nie fertig, bevor der
  // nächste Tick es mit einem minimal neuen Ziel unterbricht → sichtbares
  // Zittern der ganzen Karte (nur in Kurs-oben spürbar, weil Norden-oben die
  // Peilung dauerhaft auf 0 pinnt und diese Kette nie erneut anstößt).
  // Fix: auf ganze Grad runden, BEVOR der Wert als Dependency/Ziel verwendet
  // wird — Rauschen unter 0.5° löst dann gar keinen neuen Effect-Lauf mehr
  // aus (React vergleicht Deps per ===), echte Kursänderungen bleiben sofort
  // wirksam. Kein useMemo nötig: die Eingabewerte ändern sich praktisch bei
  // jedem Tick, die eigentliche Wirkung ist die STABILITÄT des gerundeten
  // Primitives für die Ziel-Effekte, nicht gesparte Rechenzeit.
  const rawHeading = simSnapshot?.heading_deg_true ?? simSnapshot?.heading_deg_magnetic;
  const trackUpHeadingDeg =
    typeof rawHeading === "number" && Number.isFinite(rawHeading) ? Math.round(rawHeading) : null;

  const pirepId = activeFlight?.pirep_id ?? null;
  // ---- effektive Daten (aktiver Flug) ----
  const effFixes = routeFixes;
  const effTrack: [number, number][] = trackPoints;
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

  // v0.15.13: die gerenderte Track-Linie immer bis zur LIVE-Position des
  // Flugzeugs ziehen. `effTrack` (aus dem trackStore) wird ausgedünnt, der
  // letzte aufgezeichnete Punkt liegt also bis zu ~220 m hinter dem Marker
  // → die weiße Linie „hinkte" sichtbar hinterher (Live-Report Thomas, BTX2222).
  // Wir hängen den aktuellen Sim-Punkt als zusätzliches Linienende an (nur
  // fürs Rendern; trackStore bleibt ausgedünnt persistiert).
  const effTrackLive: [number, number][] = effAircraft
    ? [...effTrack, [effAircraft.lon, effAircraft.lat]]
    : effTrack;

  const phaseLabel = formatPhase(activeFlight?.phase);

  // ---- Nächster Wegpunkt + ETA ----
  const nav = useMemo(() => {
    if (!effAircraft) return { nextIdent: undefined as string | undefined, nextLabel: "—", eta: "—" };
    const ac: [number, number] = [effAircraft.lon, effAircraft.lat];
    const acToArr = effArr ? distNm(ac, effArr) : Infinity;
    let next: RouteFix | null = null;
    let bestD = Infinity;
    for (const f of effFixes) {
      const fp: [number, number] = [f.lon, f.lat];
      // nur Fixe, die näher am Ziel sind als wir (= voraus), und nicht das Ziel selbst.
      if (effArr && distNm(fp, effArr) >= acToArr - 1) continue;
      const d = distNm(ac, fp);
      if (d < bestD) {
        bestD = d;
        next = f;
      }
    }
    const nextLabel = next ? `${next.ident} · ${Math.round(bestD)} nm` : "—";
    // v0.19.3: distance-to-GO, i.e. great-circle from the aircraft to the
    // destination — `acToArr`, computed above. This used to read
    // `activeFlight.distance_nm`, which is the accumulated distance already
    // FLOWN: the ETA therefore started near zero and grew for the whole flight,
    // telling a pilot on short final he had another hour to run.
    const dtgNm = Number.isFinite(acToArr) ? acToArr : null;
    const gs = simSnapshot?.groundspeed_kt;
    // README §4.1 wants an absolute UTC time ("15:50z"), not a countdown
    // duration — the Flugwerte card's ETA cell reads this directly.
    let eta = "—";
    if (dtgNm != null && gs != null && gs > 30) {
      const mins = Math.round((dtgNm / gs) * 60);
      const arrival = new Date(Date.now() + mins * 60_000);
      eta = `${arrival.toISOString().slice(11, 16)}z`;
    }
    return { nextIdent: next?.ident, nextLabel, eta, dtgNm };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effFixes, effAircraft, effArr, activeFlight?.distance_nm, simSnapshot?.groundspeed_kt]);

  // ---- Map einmalig erstellen ----
  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: startStil(grundlage.glyphen, readTheme() === "dark" ? "dunkel" : "hell"),
      center: [6, 48],
      zoom: 4,
      attributionControl: { compact: true },
      // Der Schlüssel gehört an JEDE Anfrage, nicht nur an die
      // Stil-Adresse — die style.json verweist selbst weiter auf
      // TileJSON, Kacheln, Schriften und Sprites. Siehe `kartenAnfrage`.
      transformRequest: kartenAnfrage(() => grundlageRef.current.schluessel),
    });
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-right");
    // `compact: true` startet in MapLibre aufgeklappt, sobald die erste Namensnennung da ist
    // (`_updateCompact` setzt `compact` und `compact-show` zusammen). Einmal zuklappen, danach
    // gehoert der (i)-Knopf dem Piloten. "sourcedata", weil die Quellen erst nach `load` kommen.
    const nennungZu = () => {
      const nennung = containerRef.current?.querySelector(".maplibregl-ctrl-attrib.maplibregl-compact-show");
      if (!nennung) return;
      nennung.classList.remove("maplibregl-compact-show");
      nennung.removeAttribute("open");
      map.off("sourcedata", nennungZu);
    };
    map.on("sourcedata", nennungZu);
    mapRef.current = map;
    // Follow nicht „einsperren", aber ehrlich: zieht der Nutzer die Karte selbst
    // weg (echter Pan = originalEvent gesetzt; unser easeTo/jumpTo löst KEIN
    // „dragstart" aus), schalten wir Follow sichtbar AUS (Haken weg) statt es
    // heimlich zu pausieren. Zoomen/Scrollen/+- lässt Follow an — man darf immer
    // zoomen, ohne den Flieger zu verlieren.
    map.on("dragstart", (e: { originalEvent?: unknown }) => {
      if (e.originalEvent) {
        // Pan = bewusst woanders hinschauen → Follow sichtbar aus + NICHT auf die
        // ganze Route fitten.
        suppressRouteFitRef.current = true;
        setFollow(false);
      }
    });
    map.on("load", () => {
      // Die Bereitschaft MUSS gesetzt werden, auch wenn das Anlegen der
      // Ebenen scheitert. Vorher stand sie dahinter: ein einziger
      // Fehler in addOverlays — etwa ein Ausdruck an einer Eigenschaft,
      // die keine nimmt — verhinderte sie, und damit lief der ganze
      // Datenabruf nie an. Auf der Karte blieb nur der Verkehr übrig,
      // der aus einer anderen Quelle kommt.
      try {
        grundkarteRef.current = grundkarteBauen(map);
        // Reihenfolge = Stapelung: Die Bodendaten liegen unter Route, Spur und Flugzeug.
        taxiEbenenAnlegen(map);
        netzEbenenAnlegen(map);
        fremdeRoutenAnlegen(map);
        flugEbenenAnlegen(map);
      } catch (e) {
        console.error("[karte] Ebenen konnten nicht angelegt werden:", e);
      } finally {
        setMapReady(true);
      }
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
    try {
      localStorage.setItem("aaLivemapBasemap", basemap);
    } catch {
      /* persist best-effort */
    }
    const map = mapRef.current;
    if (!map || !mapReady) return;
    grundkarteRef.current?.zeigen(basemap === "sat" ? "satellit" : theme === "dark" ? "dunkel" : "hell");
    overlayFarben(map, basemap === "sat");
  }, [theme, basemap, mapReady]);

  // Vektorstile holen, und neu, sobald der Server andere Adressen nennt.
  useEffect(() => {
    if (!mapReady || !grundkarteRef.current) return;
    return grundkarteHolen(
      grundkarteRef.current,
      { dunkel: grundlage.dunkel, hell: grundlage.hell },
      kartenAnfrage(() => grundlageRef.current.schluessel),
    );
  }, [mapReady, grundlage.dunkel, grundlage.hell, grundlage.schluessel]);

  // Kartenausrichtung an den Steuerkurs koppeln.
  //
  // `easeTo` statt `setBearing`: Ein harter Sprung bei jedem Telemetriepaket
  // sieht aus wie Ruckeln. Beim Ausschalten zurück auf Norden, sonst bliebe
  // die Karte in der zuletzt geflogenen Richtung stehen — der Pilot hätte
  // eine schief stehende Karte ohne erkennbaren Grund.
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    try { localStorage.setItem("aaLivemapTrackUp", trackUp ? "1" : "0"); } catch { /* egal */ }
    // Merken ja, Kamera bewegen nein: `trackUpHeadingDeg` folgt der Telemetrie,
    // ohne diese Sperre liefe verdeckt bei jedem Paket ein 400-ms-Schwenk auf
    // einer 0x0-Leinwand (QS-Befund 18.08.2026).
    if (!sichtbar) return;
    if (!trackUp) {
      map.easeTo({ bearing: 0, duration: 400 });
      return;
    }
    if (trackUpHeadingDeg !== null) {
      map.easeTo({ bearing: trackUpHeadingDeg, duration: 400 });
    }
  }, [trackUp, trackUpHeadingDeg, sichtbar]);

  // ---- Redraw: eigener Flug (Quellen + Flugzeug-Marker + Pins) ----
  // Läuft immer; VA-Flieger liegen als zusätzliche Marker mit drauf (eigene Effekte).
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    // QS-Befund 18.08.2026, der schwerste: dieser Effekt haengt an
    // `simSnapshot` und lief als einziger grosser Effekt ungesperrt weiter.
    // Mit der dauerhaft eingehaengten Karte haette er waehrend des GANZEN
    // Fluges Marker verschoben und `easeTo`/`jumpTo` auf einer unsichtbaren
    // Leinwand gefahren — MapLibre haelt dafuer eine Animationsschleife am
    // Laufen. Beim Auftauchen laeuft er neu und zeichnet alles frisch.
    if (!sichtbar) return;
    flugDatenSetzen(map, { fixes: effFixes, track: effTrackLive, dep: effDep, arr: effArr, nextIdent: nav.nextIdent });

    // Flugzeug-Marker (kategorieabhängiges Icon wie VPS/Stratos)
    if (effAircraft) {
      const lngLat: [number, number] = [effAircraft.lon, effAircraft.lat];
      const icao = simSnapshot?.aircraft_icao ?? null;
      if (!acMarkerRef.current) {
        const el = document.createElement("div");
        el.className = "aa-ac-marker";
        el.innerHTML = aircraftSvg(icao);
        acCatRef.current = icao;
        acMarkerRef.current = new maplibregl.Marker({ element: el, rotationAlignment: "map" }).setLngLat(lngLat).addTo(map);
      } else if (acCatRef.current !== icao) {
        // Muster wurde (nach-)geladen → Icon aktualisieren.
        acMarkerRef.current.getElement().innerHTML = aircraftSvg(icao);
        acCatRef.current = icao;
      }
      // Phasenabhängige Farbe (Fill + Glow + Pulse) wie auf dem VPS.
      acMarkerRef.current.getElement().style.setProperty("--ac-color", phaseColor(activeFlight?.phase));
      acMarkerRef.current.setLngLat(lngLat).setRotation(effAircraft.hdg);
      // was_just_resumed = der Resume-Gate wartet noch auf einen frischen
      // Sim-Snapshot. In dem Zustand kann X-Plane kurz eine Reload-/Lade-
      // position melden (z.B. Nordeuropa) → NICHT blind dorthin folgen.
      // Field feedback (2026-08-03, "Norden oben/Kurs oben gehen nur
      // sporadisch"): this effect's own easeTo/jumpTo calls (below) and the
      // separate orientation effect's `easeTo({bearing, duration:400})`
      // both fire off the SAME simSnapshot tick, independently, with no
      // coordination. MapLibre's easeTo isn't per-property-isolated — a
      // second call while the first is still mid-flight restarts the
      // animation from whatever the CURRENT interpolated values are, so a
      // center-only call here could freeze an in-flight bearing rotation
      // wherever it happened to be, and vice versa. Every camera call in
      // THIS effect now names its own target bearing explicitly (same
      // trackUp/heading source the orientation effect uses) so whichever
      // call "wins" the race, it's still driving toward the same target —
      // no more silent stalls.
      const targetBearing = trackUp ? (trackUpHeadingDeg ?? map.getBearing()) : 0;

      // Follow: bei gesetztem Haken IMMER auf den Flieger zentrieren — egal wie
      // weit man rausgezoomt hat. Gate NUR am `follow`-State (ein einziger Zustand,
      // nichts kann mehr desyncen). was_just_resumed unterdrückt das Folgen der
      // evtl. falschen Reload-Position direkt nach einem Resume.
      if (follow && !activeFlight?.was_just_resumed) {
        const c = map.getCenter();
        const far = Math.abs(c.lng - lngLat[0]) > 2 || Math.abs(c.lat - lngLat[1]) > 2;
        if (followEngageRef.current) {
          // Bewusstes (Wieder-)Einschalten von Follow (Erst-Mount oder der
          // "Auf Flug zentrieren"/"Folgen"-Knopf) → phasen-passenden Zoom
          // setzen, egal wie weit die Kamera gerade weg ist.
          if (far) {
            map.jumpTo({
              center: lngLat,
              zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
              bearing: targetBearing,
            });
          } else {
            map.easeTo({
              center: lngLat,
              zoom: targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft),
              bearing: targetBearing,
              duration: 350,
            });
          }
          followEngageRef.current = false;
        } else if (far) {
          // Field feedback (2026-08-03, "Map hängt, kann nicht rauszoomen,
          // zeigt nicht mehr alle Flüge"): laufendes Folgen, aber die Kamera
          // ist weit vom Flugzeug abgedriftet — das passierte auch schon von
          // einem simplen Rauszoomen per Mausrad: MapLibre zoomt um den
          // CURSOR, nicht um die Kartenmitte, also verschiebt Rauszoomen
          // abseits des Flugzeug-Icons `getCenter()` vom Flieger weg. Der
          // alte Code behandelte JEDE große Abweichung als "Spur verloren,
          // hart auf festen Phasen-Zoom zurücksetzen" — das hat ein
          // bewusstes Rauszoomen binnen einer Sekunde wieder rückgängig
          // gemacht, jedes Mal. Hier nur auf den Flieger zentrieren, OHNE
          // den Zoom anzufassen — der Phasen-Zoom-Override ist jetzt allein
          // dem bewussten (Wieder-)Einschalten oben vorbehalten.
          map.jumpTo({ center: lngLat, bearing: targetBearing });
        } else {
          // laufendes Folgen → NUR schwenken. Dein manueller Zoom bleibt erhalten
          // (kein Zurückziehen mehr auf den Phasen-Zoom — genau das war der Bug).
          map.easeTo({ center: lngLat, bearing: targetBearing, duration: 400 });
        }
      }
    } else {
      acMarkerRef.current?.remove();
      acMarkerRef.current = null;
    }

    // Dep/Arr-Pins: nur neu bauen, wenn sich Platz oder Kennung geaendert haben, nicht bei jedem Takt.
    const pinSchluessel = JSON.stringify([effDep, effDepIcao, effArr, effArrIcao]);
    if (pinStandRef.current?.karte !== map || pinStandRef.current.schluessel !== pinSchluessel) {
      pinStandRef.current = { karte: map, schluessel: pinSchluessel };
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
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    // was_just_resumed in den Deps: sobald die Resume-Warte-Phase endet, läuft
    // der Redraw erneut und zentriert (bei Follow) auf die frische Sim-Position.
    // v0.15.6: phase/dep/arr/nextIdent mit in die Deps — sonst frieren Phasen-
    // Zoom + Marker-Farbe + Pins/Highlight ein, falls simSnapshot mal stehen
    // bleibt (Sim-Pause/Resume), während sich die Phase noch ändert.
  }, [
    mapReady,
    sichtbar,
    follow,
    simSnapshot,
    routeFixes,
    // v0.15.x: trackPoints in den Deps, damit die geflogene Linie sicher neu
    // zeichnet, sobald der Backend-Poll neue Punkte liefert — auch wenn
    // simSnapshot im Hintergrund kurz steht.
    trackPoints,
    depArr.dep,
    depArr.arr,
    activeFlight?.was_just_resumed,
    activeFlight?.phase,
    activeFlight?.dpt_airport,
    activeFlight?.arr_airport,
    nav.nextIdent,
  ]);

  // einmal auf die eigene Route fitten, wenn Follow BEWUSST abgehakt wurde
  // (nicht, wenn ein Pan Follow ausgeschaltet hat — dann bleibt die Ansicht stehen).
  const fittedRef = useRef<string | null>(null);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady || !activeFlight || follow) return;
    if (suppressRouteFitRef.current) return;
    // In Flugreihenfolge und ueber die Datumsgrenze weitergezaehlt — sonst
    // umfasst der Ausschnitt einer Pazifikroute die ganze Welt.
    const pts = ohneDatumsgrenzenSprung([
      ...(effDep ? [effDep] : []),
      ...effFixes.map((f) => [f.lon, f.lat] as [number, number]),
      ...(effArr ? [effArr] : []),
    ]);
    // pirepId mit im Key: ein neuer Flug derselben Strecke (gleiche Fix-Anzahl +
    // gleiche Dep/Arr) soll wieder gefittet werden, nicht als „schon erledigt" gelten.
    const key = `${pirepId}-${effFixes.length}-${effDepIcao}-${effArrIcao}`;
    if (pts.length >= 2 && fittedRef.current !== key) {
      fittedRef.current = key;
      const b = pts.reduce((acc, p) => acc.extend(p), new maplibregl.LngLatBounds(pts[0], pts[0]));
      map.fitBounds(b, { padding: 80, duration: 600, maxZoom: 8 });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapReady, follow, effFixes, effDepIcao, effArrIcao, activeFlight]);

  // Steht vor dem VA-Effekt, der `bildKarte` beim Einpassen braucht; muss nach dem Anlegen der Karte bleiben.
  const bildKarte = useBildKarte(mapRef, bildFokus, activeFlight?.pirep_id ?? null, sichtbar);
  const { showVa, setShowVa, vaVisible, fremdeRouteHinweis, setFremdeRouteHinweis } = useVaVerkehr({
    mapRef, mapReady, sichtbar, activeFlight, bilderVorrang: bildKarte.bilderAn && bildKarte.marken > 0,
  });

  // README §3 EBENEN: Track / Taxiweg show/hide, two independent toggles
  // (field feedback, 2026-08-03 — split apart from the spec's original
  // combined "Track & Taxiweg" switch). Deliberately additive and separate
  // from `addOverlays` — the trackline/taxi-map DATA and how it's computed
  // stay exactly as they were (README §0), this only toggles the existing
  // layers' visibility. `setLayoutProperty` is a no-op-safe call guarded by
  // `getLayer`, so it can't fight the style-reload re-add logic above; it
  // just re-asserts the current toggle state whenever it changes.
  const [showTrack, setShowTrack] = useState(true);
  const [showTaxi, setShowTaxi] = useState(true);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    for (const id of TRACK_LAYERS) {
      // Die dunkle Unterlage braucht die Route nur auf dem bunten Satellitenbild.
      const an = showTrack && (id !== LYR_ROUTE_CASING || basemap === "sat");
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", an ? "visible" : "none");
    }
  }, [showTrack, basemap, mapReady]);
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    const vis = showTaxi ? "visible" : "none";
    for (const id of TAXI_LAYERS) {
      if (map.getLayer(id)) map.setLayoutProperty(id, "visibility", vis);
    }
  }, [showTaxi, mapReady]);

  // ---- Stats (Flugwerte card — README §4.1: HÖHE, GS, REST, ETA only) ----
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
      gs: fmt(s?.groundspeed_kt, " kts"),
      // v0.19.3: distance to GO (aircraft → destination), not the distance
      // already flown. Same source as the ETA above — see `nav.dtgNm`.
      dtg: nav.dtgNm != null ? `${Math.round(nav.dtgNm)} nm` : "—",
    };
  }, [simSnapshot, nav]);

  const showOwnContent = !!activeFlight;
  const ident = activeFlight
    ? displayCallsign(activeFlight.airline_icao, activeFlight.flight_number, activeFlight.callsign)
    : null;
  const clock = new Date(nowMs).toISOString().slice(11, 19);
  // QS-Runde 4: `useMapEvents` haengt zwei Takte an — das Aktivitaetsprotokoll
  // (alle 2 s) und den Hoppie-Nachrichtenfaden. Beide hingen nur am aktiven
  // Flug, liefen also mit der dauerhaft eingehaengten Karte den GANZEN Flug
  // lang weiter, obwohl niemand hinsieht. Der Hoppie-Teil geht dabei sogar ins
  // Netz. Beim Auftauchen wird ohnehin sofort einmal frisch geholt.
  const { events } = useMapEvents(sichtbar && showOwnContent);
  const flyToPosition = (position: [number, number]) => {
    mapRef.current?.flyTo({ center: position, duration: 700 });
  };
  const recorderState: "ok" | "off" | "danger" = !activeFlight
    ? "off"
    : activeFlight.connection_state === "blocked" || activeFlight.connection_state === "failing"
      ? "danger"
      : "ok";

  return (
    <section className="aa-livemap">
      {/* README §2 — no buttons here at all; every control lives in the
          Kartenschalter-Block over the map (§3). This header carries
          identity/context and clock only. */}
      <header className="aa-livemap__header">
        <div className="aa-livemap__header-left">
          <h1 className="aa-livemap__header-title">{t("livemap.title")}</h1>
          <span className="aa-livemap__header-sep" aria-hidden="true" />
          <div className="aa-livemap__header-context">
            {activeFlight ? (
              <>
                <span className="aa-livemap__header-callsign">{ident}</span>
                <span className="aa-livemap__header-route">
                  {t("livemap.header_context_flight", {
                    dep: activeFlight.dpt_airport,
                    arr: activeFlight.arr_airport,
                    phase: phaseLabel,
                  })}
                </span>
                {activeFlight.was_just_resumed && (
                  <span className="aa-livemap__resume-hint" title={t("livemap.resume_hint")}>
                    {" "}
                    ⏳
                  </span>
                )}
              </>
            ) : (
              <span className="aa-livemap__header-route">
                {next
                  ? t("livemap.header_idle_next", { ident: next.ident, std: `${next.std}z` })
                  : t("livemap.header_idle_none")}
              </span>
            )}
          </div>
        </div>
        <div className="aa-livemap__header-right">
          <span className="aa-livemap__header-vacount">{t("livemap.header_va_count", { count: vaVisible.length })}</span>
          <span className="aa-livemap__header-clock">
            {clock}
            <span className="aa-livemap__header-z">z</span>
          </span>
        </div>
      </header>

      <div className="aa-livemap__body">
        <div className="aa-livemap__map" ref={containerRef}>
          {/* Festes Raster statt gemessener Abstände: Leiste oben, Panel darunter, Systemzeile unten.
              Bricht die Leiste um, rückt alles von selbst nach. */}
          <div className="aa-livemap-ui">
            <KartenLeiste
              satellit={basemap === "sat"}
              setzeSatellit={(an) => setBasemap(an ? "sat" : "auto")}
              kugel={bildKarte.kugel}
              setzeKugel={bildKarte.setKugel}
              trackUp={trackUp}
              setzeTrackUp={setTrackUp}
              showTrack={showTrack}
              umschaltenTrack={() => setShowTrack((v) => !v)}
              showTaxi={showTaxi}
              umschaltenTaxi={() => setShowTaxi((v) => !v)}
              taxiPlaetze={groundLoaded}
              bilderMarken={bildKarte.marken}
              bilderAn={bildKarte.bilderAn}
              umschaltenBilder={() => bildKarte.setBilderAn((v) => !v)}
              showVa={showVa}
              umschaltenVa={() => setShowVa((v) => !v)}
              netz={netz}
              setzeNetz={setNetz}
              flAuto={flModus === "auto"}
              umschaltenFlAuto={() => setFlModus((m) => (m === "auto" ? "fest" : "auto"))}
              flWert={flWert}
              setzeFlWert={(fl) => { setFlModus("fest"); setFlWert(fl); }}
              flRegelbar={!(flModus === "auto" && simSnapshot != null)}
              flAnzeige={
                flModus === "auto" && simSnapshot != null
                  ? `FL${String(Math.max(0, Math.round((simSnapshot.altitude_msl_ft ?? 0) / 100))).padStart(3, "0")}`
                  : `FL${String(flWert).padStart(3, "0")}`
              }
              hatFlug={!!activeFlight}
              hatPosition={!!effAircraft}
              folgen={follow}
              umschaltenFolgen={() => {
                const neu = !follow;
                setFollow(neu);
                followEngageRef.current = neu;
                suppressRouteFitRef.current = neu;
              }}
              zentrieren={() => {
                const map = mapRef.current;
                if (!map || !effAircraft) return;
                followEngageRef.current = true;
                suppressRouteFitRef.current = true;
                setFollow(true);
                const tz = targetFollowZoom(activeFlight?.phase ?? "", simSnapshot?.altitude_msl_ft);
                map.easeTo({ center: [effAircraft.lon, effAircraft.lat], zoom: tz, duration: 500 });
              }}
            />

            {/* Kurzer Hinweis, wenn zu einem angeklickten Kollegen keine Route vorliegt: Sonst sähe
                der Klick aus, als täte er nichts. */}
            {fremdeRouteHinweis && (
              <div role="status" className="aa-livemap-hinweis aa-livemap-overlay" onClick={() => setFremdeRouteHinweis(null)}>
                {fremdeRouteHinweis}
              </div>
            )}

            {showOwnContent && (
              <FlugPanel
                flug={activeFlight!}
                phase={phaseLabel}
                werte={{ hoehe: stats.alt, gs: stats.gs, rest: stats.dtg, eta: nav.eta, restNm: nav.dtgNm }}
                ereignisse={<LiveMapEventList events={events} callsign={ident} onJumpTo={flyToPosition} />}
              />
            )}

            {/* README §7 — System-Zeile, overlay bottom-left. Field feedback
                (2026-08-03): the "Systemmeldungen (n)" count-link was replaced
                by the actual recorder connection detail (last send, total
                sent) — the same LiveRecordingIndicator the sidebar already
                shows, so "what/when/how much are we sending" lives where the
                pilot is actually looking, not behind a bare counter. */}
            <div className="aa-livemap-sysline aa-livemap-overlay">
              {/* Wie lange der letzte Netz-Durchlauf gebraucht hat, getrennt
                  nach Holen und Zeichnen. Beim ersten Zuschalten dauert es
                  20-25 s, und aus der Ferne war nicht zu klaeren, woran —
                  also misst es sich selbst. */}
              {netzZeit && (
                <span
                  className="aa-livemap-sysline__text"
                  style={{ opacity: 0.7, fontVariantNumeric: "tabular-nums" }}
                  title={t("livemap.netz_zeit_hint", "Holen der Daten und Zeichnen der Flächen beim letzten Durchlauf")}
                >
                  {netzZeit.erster ? "Netz (erstmals): " : "Netz: "}
                  {(netzZeit.abruf / 1000).toFixed(1)} s holen ·{" "}
                  {netzZeit.zeichnen === null
                    ? "zeichnet …"
                    : `${(netzZeit.zeichnen / 1000).toFixed(1)} s zeichnen`}
                </span>
              )}
              <span className={`aa-livemap-sysline__dot aa-livemap-sysline__dot--${recorderState}`} aria-hidden="true" />
              <span className="aa-livemap-sysline__text">
                {recorderState === "off"
                  ? t("livemap.sys_inactive")
                  : t("livemap.sys_active", {
                      sim: simKindLabel(simKind),
                      uplink: recorderState === "ok" ? t("livemap.sys_uplink_ok") : t("livemap.sys_uplink_lost"),
                    })}
              </span>
              {/* QS-Runde 5: `sichtbar` gehoert hier dazu, nicht nur `activeFlight`.
                  LiveRecordingIndicator haelt einen eigenen Sekundentakt fuer sein
                  "vor X Sekunden" — mit der dauerhaft eingehaengten Karte liefe
                  der sonst den ganzen Flug lang in einem unsichtbaren Teilbaum.
                  Verdeckt gar nicht erst einhaengen ist billiger als jede Sperre
                  im Inneren, und beim Auftauchen steht sofort der aktuelle Wert. */}
              {activeFlight && sichtbar && (
                <>
                  <span className="aa-livemap-sysline__sep" aria-hidden="true" />
                  <LiveRecordingIndicator
                    lastPositionAt={activeFlight.last_position_at}
                    queuedCount={activeFlight.queued_position_count}
                    positionCount={activeFlight.position_count}
                    connectionState={
                      activeFlight.connection_state === "blocked"
                        ? "blocked"
                        : activeFlight.connection_state === "failing"
                          ? "failing"
                          : "live"
                    }
                  />
                </>
              )}
            </div>
          </div>

          {/* Field feedback (2026-08-03): this card used to be shown whenever
              there was no own flight, VA traffic or not — it visibly blocked
              the map exactly where colleague aircraft render, which the
              pilot explicitly wants to see when they're not flying
              themselves. The OLD behaviour only showed the equivalent
              full-map sentence when there was NEITHER an own flight NOR any
              VA traffic — restoring that exact gate here. Bildpunkte zaehlen
              genauso: die Karte laege sonst ueber ihnen, sie waeren nicht klickbar. */}
          {!showOwnContent && vaVisible.length === 0 && !(bildKarte.bilderAn && bildKarte.marken > 0) && (
            <LiveMapEmptyState next={next} onGoToBriefing={onSwitchToBriefing} />
          )}

        </div>
      </div>
      {bildKarte.gross && <BildGross bilder={bildKarte.gross.bilder} startId={bildKarte.gross.id} onZu={bildKarte.grossZu} />}
    </section>
  );
}
