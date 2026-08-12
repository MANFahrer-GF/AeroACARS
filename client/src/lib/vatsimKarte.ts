// VATSIM-Overlay-Datenschicht der CLIENT-Karte.
//
// 1:1 aus der Live-Karten-Webapp uebernommen (eigener Code, dort seit dem
// VATSIM-Overlay im Einsatz) — einzige Abweichung: der Fallback auf die
// VA-Airport-Koordinaten entfaellt, der Client haelt keine solche Liste;
// die VATSpy-Weltliste deckt ohnehin alle VATSIM-Plaetze.
//
// Beim Aendern BEIDE Fassungen anfassen: webapp/src/data/vatsim.ts ist
// die Stammfassung.
// ---------------------------------------------------------------------------
// VATSIM-Datentypen (nur die Felder die wir brauchen — kein Vollschema)
// ---------------------------------------------------------------------------

export interface VatsimFlightPlan {
  departure?: string | null;
  arrival?: string | null;
  aircraft_short?: string | null;
}

export interface VatsimPilot {
  callsign: string;
  latitude: number;
  longitude: number;
  altitude: number;
  groundspeed: number;
  heading: number;
  flight_plan: VatsimFlightPlan | null;
}

export interface VatsimController {
  callsign: string;
  frequency: string;
  /** 0=OBS,1=FSS,2=DEL,3=GND,4=TWR,5=APP,6=CTR */
  facility: number;
  rating: number;
  // Controller haben in der VATSIM-Datafeed KEINE Koordinaten.
  latitude: number | null;
  longitude: number | null;
}

export interface VatsimData {
  pilots: VatsimPilot[];
  controllers: VatsimController[];
}

const VATSIM_DATA_URL = "https://data.vatsim.net/v3/vatsim-data.json";
const BOUNDARIES_URL =
  "https://cdn.jsdelivr.net/gh/vatsimnetwork/vatspy-data-project@master/Boundaries.geojson";
// VATSpy.dat: Welt-Airport-Coords (~17.9k) + FIR-Callsign-Präfix→Boundary-Map.
// Pflicht für die ATC-Airport-Dots: airportCoord() kennt nur VA-Airports, aber
// VATSIM-Controller sitzen an beliebigen Welt-Airports.
const VATSPY_URL =
  "https://cdn.jsdelivr.net/gh/vatsimnetwork/vatspy-data-project@master/VATSpy.dat";

// ---------------------------------------------------------------------------
// Live-Datafeed
// ---------------------------------------------------------------------------

/** Holt den VATSIM-Live-Datafeed und reduziert ihn auf die Felder die wir
 *  brauchen. Wirft bei HTTP-Fehler/Abort weiter (Caller fängt + loggt). */
export async function fetchVatsimData(signal?: AbortSignal): Promise<VatsimData> {
  const res = await fetch(VATSIM_DATA_URL, { signal });
  if (!res.ok) throw new Error(`vatsim-data HTTP ${res.status}`);
  const json = (await res.json()) as {
    pilots?: unknown[];
    controllers?: unknown[];
  };

  const pilots: VatsimPilot[] = [];
  for (const raw of Array.isArray(json.pilots) ? json.pilots : []) {
    const p = raw as Record<string, unknown>;
    const lat = p.latitude;
    const lon = p.longitude;
    if (typeof lat !== "number" || typeof lon !== "number") continue;
    if (!isFinite(lat) || !isFinite(lon)) continue;
    const fp = p.flight_plan as Record<string, unknown> | null | undefined;
    pilots.push({
      callsign: String(p.callsign ?? ""),
      latitude: lat,
      longitude: lon,
      altitude: typeof p.altitude === "number" ? p.altitude : 0,
      groundspeed: typeof p.groundspeed === "number" ? p.groundspeed : 0,
      heading: typeof p.heading === "number" ? p.heading : 0,
      flight_plan: fp
        ? {
            departure: (fp.departure as string | null) ?? null,
            arrival: (fp.arrival as string | null) ?? null,
            aircraft_short: (fp.aircraft_short as string | null) ?? null,
          }
        : null,
    });
  }

  const controllers: VatsimController[] = [];
  for (const raw of Array.isArray(json.controllers) ? json.controllers : []) {
    const c = raw as Record<string, unknown>;
    const facility = typeof c.facility === "number" ? c.facility : 0;
    controllers.push({
      callsign: String(c.callsign ?? ""),
      frequency: String(c.frequency ?? ""),
      facility,
      rating: typeof c.rating === "number" ? c.rating : 0,
      latitude: typeof c.latitude === "number" ? c.latitude : null,
      longitude: typeof c.longitude === "number" ? c.longitude : null,
    });
  }

  return { pilots, controllers };
}

// ---------------------------------------------------------------------------
// Facility-Helfer
// ---------------------------------------------------------------------------

/** Kurz-Tag aus dem facility-Int. */
export function facilityTag(facility: number): string {
  switch (facility) {
    case 1: return "FSS";
    case 2: return "DEL";
    case 3: return "GND";
    case 4: return "TWR";
    case 5: return "APP";
    case 6: return "CTR";
    default: return "OBS";
  }
}

/** Präfix vor dem ersten "_" (ICAO bei Airports, FIR-id bei Centern). */
function callsignPrefix(callsign: string): string {
  const i = callsign.indexOf("_");
  return (i >= 0 ? callsign.slice(0, i) : callsign).toUpperCase();
}

// ---------------------------------------------------------------------------
// HTML-Escaping für Popup-Inhalte
// ---------------------------------------------------------------------------

export function esc(s: unknown): string {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---------------------------------------------------------------------------
// Pilot-Features (rotierte Flugzeug-Symbole)
// ---------------------------------------------------------------------------

/** Baut die Pilot-FeatureCollection. Properties tragen alles für den Popup
 *  + `hdg` für die icon-rotate-Expression. */
export function buildPilotFeatures(
  pilots: VatsimPilot[]
): GeoJSON.FeatureCollection<GeoJSON.Point> {
  const features: GeoJSON.Feature<GeoJSON.Point>[] = pilots.map((p) => ({
    type: "Feature",
    geometry: { type: "Point", coordinates: [p.longitude, p.latitude] },
    properties: {
      callsign: p.callsign,
      hdg: p.heading,
      dep: p.flight_plan?.departure ?? "",
      arr: p.flight_plan?.arrival ?? "",
      actype: p.flight_plan?.aircraft_short ?? "",
      alt: p.altitude,
      gs: p.groundspeed,
    },
  }));
  return { type: "FeatureCollection", features };
}

// ---------------------------------------------------------------------------
// ATC-Airport-Aggregation (DEL/GND/TWR/APP) — ein Dot pro Airport
// ---------------------------------------------------------------------------

interface AtcPosition {
  tag: string;
  callsign: string;
  frequency: string;
}

/** Aggregiert alle Airport-Controller (facility 2/3/4/5) pro ICAO zu EINEM
 *  Dot. ICAO = Callsign-Präfix vor dem ersten "_". Airports ohne bekannte
 *  Coords (airportCoord === null) werden übersprungen.
 *
 *  Die `positions`-Liste landet als JSON-String im Feature-Property, weil
 *  MapLibre-Feature-Properties keine verschachtelten Arrays/Objekte sauber
 *  durch die Style-/Click-Pipeline tragen — der Click-Handler parsed sie. */
export function buildAtcAirportFeatures(
  controllers: VatsimController[],
  airportCoords?: Map<string, [number, number]>,
): GeoJSON.FeatureCollection<GeoJSON.Point> {
  const AIRPORT_FACILITIES = new Set([2, 3, 4, 5]);
  const byIcao = new Map<string, AtcPosition[]>();

  for (const c of controllers) {
    if (!AIRPORT_FACILITIES.has(c.facility)) continue;
    const icao = callsignPrefix(c.callsign);
    if (!icao) continue;
    const list = byIcao.get(icao) ?? [];
    list.push({
      tag: facilityTag(c.facility),
      callsign: c.callsign,
      frequency: c.frequency,
    });
    byIcao.set(icao, list);
  }

  const features: GeoJSON.Feature<GeoJSON.Point>[] = [];
  for (const [icao, positions] of byIcao) {
    // VATSpy-Welt-Airports bevorzugt (deckt alle VATSIM-Airports), sonst
    // die VA-Airport-Coords als Fallback.
    const coord = airportCoords?.get(icao);
    if (!coord) continue; // Airport unbekannt → kein Dot (besser als falsch).
    // Tone: "twr" wenn eine TWR/APP online ist (höhere Sichtbarkeit),
    // sonst "gnd". Rein kosmetisch für die Dot-Farbe.
    const hasTower = positions.some((p) => p.tag === "TWR" || p.tag === "APP");
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: coord },
      properties: {
        icao,
        count: positions.length,
        tone: hasTower ? "twr" : "gnd",
        positions: JSON.stringify(positions),
        // Radar-Vorbild (Thomas, 12.08.2026): welche Station online ist,
        // muss OHNE Klick erkennbar sein. Je Art ein fester Buchstaben-
        // Slot, damit die Kartenbeschriftung ihn fest einfaerben kann.
        t_del: positions.some((p) => p.tag === "DEL") ? "D " : "",
        t_gnd: positions.some((p) => p.tag === "GND") ? "G " : "",
        t_twr: positions.some((p) => p.tag === "TWR") ? "T " : "",
        t_app: positions.some((p) => p.tag === "APP") ? "A" : "",
      },
    });
  }

  return { type: "FeatureCollection", features };
}

// ---------------------------------------------------------------------------
// FIR-Grenzen (Boundaries.geojson) — einmal laden + cachen
// ---------------------------------------------------------------------------

type FirFeature = GeoJSON.Feature<GeoJSON.Polygon | GeoJSON.MultiPolygon>;

// Modul-lokaler Cache. Boundaries.geojson ist ~5 MB → genau EINMAL fetchen.
let boundariesCache: Map<string, FirFeature> | null = null;
let boundariesPromise: Promise<Map<string, FirFeature>> | null = null;

/** Lädt Boundaries.geojson (oder gibt den Cache zurück). Map keyed nach
 *  GROSSGESCHRIEBENER `properties.id` (FIR-id wie "EDGG","LON"). */
export async function loadBoundaries(
  signal?: AbortSignal
): Promise<Map<string, FirFeature>> {
  if (boundariesCache) return boundariesCache;
  if (boundariesPromise) return boundariesPromise;

  boundariesPromise = (async () => {
    const res = await fetch(BOUNDARIES_URL, { signal });
    if (!res.ok) throw new Error(`boundaries HTTP ${res.status}`);
    const fc = (await res.json()) as GeoJSON.FeatureCollection<
      GeoJSON.Polygon | GeoJSON.MultiPolygon
    >;
    const map = new Map<string, FirFeature>();
    for (const f of fc.features ?? []) {
      const id = (f.properties?.id as string | undefined)?.toUpperCase();
      if (!id) continue;
      // Erster Treffer gewinnt (es gibt vereinzelt Duplikate; egal welcher).
      if (!map.has(id)) map.set(id, f);
    }
    boundariesCache = map;
    return map;
  })();

  try {
    return await boundariesPromise;
  } catch (e) {
    // Bei Fehler/Abort den Promise-Cache löschen, damit ein Re-Enable
    // erneut versuchen kann.
    boundariesPromise = null;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// VATSpy.dat — Welt-Airport-Coords + FIR-Präfix-Map (einmal laden + cachen)
// ---------------------------------------------------------------------------

export interface VatSpyData {
  /** ICAO → [lon, lat] für ALLE Welt-Airports (nicht nur VA-Airports). */
  airports: Map<string, [number, number]>;
  /** Callsign-Präfix (bzw. FIR-ICAO wenn Präfix leer) → Boundary-id. */
  prefixToBoundary: Map<string, string>;
}

let vatspyCache: VatSpyData | null = null;
let vatspyPromise: Promise<VatSpyData> | null = null;

/** Lädt + parst VATSpy.dat (Pipe-getrennt, `;`-Kommentare, `[Section]`-Header).
 *  Cached modul-lokal (einmal pro Session). */
export async function loadVatSpy(signal?: AbortSignal): Promise<VatSpyData> {
  if (vatspyCache) return vatspyCache;
  if (vatspyPromise) return vatspyPromise;

  vatspyPromise = (async () => {
    const res = await fetch(VATSPY_URL, { signal });
    if (!res.ok) throw new Error(`vatspy HTTP ${res.status}`);
    const text = await res.text();
    const airports = new Map<string, [number, number]>();
    const prefixToBoundary = new Map<string, string>();
    let section = "";
    for (const rawLine of text.split(/\r?\n/)) {
      const line = rawLine.trim();
      if (!line || line.startsWith(";")) continue;
      if (line.startsWith("[")) { section = line.toUpperCase(); continue; }
      const parts = line.split("|");
      if (section === "[AIRPORTS]") {
        // ICAO|Name|Lat|Lon|IATA|FIR|IsPseudo
        if (parts.length < 4) continue;
        const icao = parts[0].toUpperCase();
        const lat = parseFloat(parts[2]);
        const lon = parseFloat(parts[3]);
        if (icao && isFinite(lat) && isFinite(lon)) airports.set(icao, [lon, lat]);
      } else if (section === "[FIRS]") {
        // ICAO|NAME|CALLSIGN PREFIX|FIR BOUNDARY
        if (parts.length < 4) continue;
        const icao = (parts[0] ?? "").toUpperCase();
        const prefix = (parts[2] ?? "").toUpperCase();
        const boundary = (parts[3] ?? "").toUpperCase();
        if (!boundary) continue;
        const key = prefix || icao; // Präfix leer → über ICAO matchen
        if (key && !prefixToBoundary.has(key)) prefixToBoundary.set(key, boundary);
      }
    }
    vatspyCache = { airports, prefixToBoundary };
    return vatspyCache;
  })();

  try {
    return await vatspyPromise;
  } catch (e) {
    vatspyPromise = null; // Re-Enable darf erneut versuchen
    throw e;
  }
}

// ---------------------------------------------------------------------------
// Sektor-Features (CTR/FSS → FIR-Polygone)
// ---------------------------------------------------------------------------

interface SectorMeta {
  callsign: string;
  frequency: string;
  rating: number;
}

/** Matched CTR(6)/FSS(1)-Controller gegen die FIR-Grenzen. FIR-id =
 *  Callsign-Präfix vor dem ersten "_" (z.B. "EDGG_CTR" → "EDGG",
 *  "LON_S_CTR" → "LON"), case-insensitive gegen `properties.id` gematcht.
 *
 *  Dedupliziert nach FIR-id: mehrere Controller auf derselben FIR teilen
 *  sich EIN Polygon, der Popup listet alle. Center deren Präfix NICHT in
 *  den Boundaries auftaucht (z.B. exotische/aufgeteilte Sektoren wie
 *  "LON_S_CTR" wenn nur "LON" existiert matched es, aber etwa US-TMU- oder
 *  manche Trainings-Callsigns nicht) werden BEST-EFFORT übersprungen —
 *  akzeptierter Trade-off laut Spec. */
export function buildSectorFeatures(
  controllers: VatsimController[],
  boundaries: Map<string, FirFeature>,
  prefixToBoundary?: Map<string, string>,
): GeoJSON.FeatureCollection<GeoJSON.Polygon | GeoJSON.MultiPolygon> {
  const CENTER_FACILITIES = new Set([1, 6]);
  const byFir = new Map<string, SectorMeta[]>();

  for (const c of controllers) {
    if (!CENTER_FACILITIES.has(c.facility)) continue;
    const prefix = callsignPrefix(c.callsign);
    if (!prefix) continue;
    // Offizielles VATSpy-Präfix→Boundary-Mapping bevorzugt; sonst das Präfix
    // direkt als Boundary-id versuchen (Fallback, deckt z.B. "EDGG"→"EDGG").
    const boundaryId =
      prefixToBoundary?.get(prefix) ?? (boundaries.has(prefix) ? prefix : null);
    if (!boundaryId || !boundaries.has(boundaryId)) continue; // unmatched → skip
    const list = byFir.get(boundaryId) ?? [];
    list.push({ callsign: c.callsign, frequency: c.frequency, rating: c.rating });
    byFir.set(boundaryId, list);
  }

  const features: FirFeature[] = [];
  for (const [firId, metas] of byFir) {
    const base = boundaries.get(firId);
    if (!base) continue;
    features.push({
      type: "Feature",
      geometry: base.geometry,
      properties: {
        fir: firId,
        controllers: JSON.stringify(metas),
      },
    });
  }

  return { type: "FeatureCollection", features };
}

/**
 * Beschriftungspunkte zu den Sektorflaechen: Kennung + Frequenz IN der
 * Flaeche statt greller Rahmen drumherum (Feldbefund 12.08.2026: "das was
 * live nutzt ist nicht schoen"). Der Punkt ist der Schwerpunkt der
 * groessten Aussenhuelle — fuer ein Label voellig ausreichend.
 */
export function buildSectorLabelFeatures(
  controllers: VatsimController[],
  boundaries: Map<string, FirFeature>,
  prefixToBoundary?: Map<string, string>,
): GeoJSON.FeatureCollection<GeoJSON.Point> {
  const CENTER_FACILITIES = new Set([1, 6]);
  const byFir = new Map<string, SectorMeta[]>();
  for (const c of controllers) {
    if (!CENTER_FACILITIES.has(c.facility)) continue;
    const prefix = callsignPrefix(c.callsign);
    if (!prefix) continue;
    const boundaryId =
      prefixToBoundary?.get(prefix) ?? (boundaries.has(prefix) ? prefix : null);
    if (!boundaryId || !boundaries.has(boundaryId)) continue;
    const list = byFir.get(boundaryId) ?? [];
    list.push({ callsign: c.callsign, frequency: c.frequency, rating: c.rating });
    byFir.set(boundaryId, list);
  }
  const features: GeoJSON.Feature<GeoJSON.Point>[] = [];
  for (const [firId, metas] of byFir) {
    const base = boundaries.get(firId);
    if (!base) continue;
    const g = base.geometry;
    let ring: GeoJSON.Position[] | null = null;
    if (g.type === "Polygon") ring = g.coordinates[0];
    else if (g.type === "MultiPolygon") {
      // Die groesste Teilflaeche traegt das Label.
      let beste = -1;
      for (const poly of g.coordinates) {
        if (poly[0] && poly[0].length > beste) { beste = poly[0].length; ring = poly[0]; }
      }
    }
    if (!ring || ring.length < 3) continue;
    const lon = ring.reduce((a, p) => a + p[0], 0) / ring.length;
    const lat = ring.reduce((a, p) => a + p[1], 0) / ring.length;
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [lon, lat] },
      properties: {
        fir: firId,
        freq: metas[0]?.frequency ?? "",
        anzahl: metas.length,
      },
    });
  }
  return { type: "FeatureCollection", features };
}

/**
 * Lotsen-Einstufung in Klartext. Im Klickfenster stand vorher "R11" —
 * eine Zahl, mit der nur VATSIM-Insider etwas anfangen. Unbekannte Werte
 * bleiben sichtbar (statt leer), damit ein neuer Rang auffaellt.
 */
export function ratingLabel(rating: number): string {
  const NAMEN = ["", "OBS", "S1", "S2", "S3", "C1", "C2", "C3", "I1", "I2", "I3", "SUP", "ADM"];
  return NAMEN[rating] ?? `R${rating}`;
}

/** Leere FeatureCollection — zum Source-Leeren beim Ausschalten. */
export function emptyFC(): GeoJSON.FeatureCollection {
  return { type: "FeatureCollection", features: [] };
}
