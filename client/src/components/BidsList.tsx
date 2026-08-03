import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, openExternal as openUrl } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import { formatRefreshError } from "../lib/refreshErrorFormatter";
import { resolveFlightIdent } from "../lib/callsign";
import type {
  ActiveFlightInfo,
  AircraftInfo,
  AirportInfo,
  Bid,
  Flight,
  SimBriefOfp,
  SimConnectionState,
  SimSnapshot,
  UiError,
} from "../types";
import { ManualFlightModal } from "./ManualFlightModal";

/**
 * Maximum distance (in nautical miles) between the aircraft and the bid's
 * departure airport before "Start flight" is enabled. Mirrors the server-side
 * threshold in `aeroacars-app/src/lib.rs::MAX_START_DISTANCE_NM`.
 */
const MAX_START_DISTANCE_NM = 5.0;

/** Great-circle distance in nautical miles. */
function distanceNm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const R = 6_371_008.8; // metres
  const toRad = (d: number) => (d * Math.PI) / 180;
  const phi1 = toRad(lat1);
  const phi2 = toRad(lat2);
  const dphi = toRad(lat2 - lat1);
  const dlam = toRad(lon2 - lon1);
  const a =
    Math.sin(dphi / 2) ** 2 +
    Math.cos(phi1) * Math.cos(phi2) * Math.sin(dlam / 2) ** 2;
  const meters = 2 * R * Math.asin(Math.sqrt(a));
  return meters / 1852;
}

type State =
  | { kind: "loading" }
  | { kind: "error"; error: UiError }
  | { kind: "empty" }
  | { kind: "ready"; bids: Bid[] };

import type { Profile } from "../types";

interface Props {
  baseUrl: string;
  /** Sim connection state to gate the Start Flight button. */
  simState: SimConnectionState;
  /** Latest sim snapshot — used to compute distance to each bid's dpt airport. */
  simSnapshot: SimSnapshot | null;
  /** Whether a flight is already active (disables Start on every bid). */
  hasActiveFlight: boolean;
  /** Notify the parent when a bid is selected so it can drive the next step. */
  onSelect?: (bid: Bid | null) => void;
  /** Notify the parent that a flight just started. */
  onFlightStarted?: (flight: ActiveFlightInfo) => void;
  /** Called whenever the user hits the Refresh button — passes a fresh
   *  profile fetched from phpVMS so the parent can update the cached
   *  session and the PilotHeader picks up new curr_airport/etc. without
   *  requiring a logout/login cycle. v0.1.30. */
  onProfileRefreshed?: (profile: Profile) => void;
  /** v0.7.7: Bid-Tab-Refresh ruft `flight_refresh_simbrief` zusaetzlich
   *  auf wenn ein aktiver Flug existiert. Bei einem successful Refresh
   *  mit `changed=true` benachrichtigen wir den Parent, damit Cockpit +
   *  Loadsheet sofort neuen Plan sehen (statt erst nach 2s flight_status-
   *  Poll). Spec docs/spec/ofp-refresh-during-boarding.md §6.5b. */
  onActiveFlightUpdated?: () => void;
}

/** v0.7.7: Tauri-Backend liefert das nach erfolgreichem
 *  `flight_refresh_simbrief`. Spec §6.1 DTO-Split. */
interface SimBriefRefreshResult {
  ofp: {
    planned_block_fuel_kg: number;
    planned_burn_kg: number;
    planned_tow_kg: number;
    planned_ldw_kg: number;
    // weitere Felder verfuegbar, aber Bid-Tab braucht nur die fuer den Notice
  };
  previous_ofp_id: string | null;
  current_ofp_id: string;
  changed: boolean;
}

const KNOWN_ERROR_CODES = new Set([
  "not_logged_in",
  "network",
  "unauthenticated",
  "forbidden",
  "not_found",
  "rate_limited",
  "server",
  "bad_response",
]);

function errorKey(code: string): string {
  return KNOWN_ERROR_CODES.has(code)
    ? `bids.error.${code}`
    : "bids.error.unknown";
}

function formatFlightTime(minutes: number | null, locale: string): string {
  if (minutes == null) return "—";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return locale.startsWith("de") ? `${h}h ${m.toString().padStart(2, "0")}m` : `${h}h ${m}m`;
}

function formatDistanceNm(nmi: number | null, locale: string): string {
  if (nmi == null) return "—";
  return `${new Intl.NumberFormat(locale).format(Math.round(nmi))} nmi`;
}

function formatLevel(level: number | null): string | null {
  if (level == null || level <= 0) return null;
  return `FL${level.toString().padStart(3, "0")}`;
}

/** "14:12z" aus einem Unix-Timestamp (Briefing-2a §3/§4 Zeitstempel). */
function formatZuluHm(ms: number): string {
  const d = new Date(ms);
  const hh = d.getUTCHours().toString().padStart(2, "0");
  const mm = d.getUTCMinutes().toString().padStart(2, "0");
  return `${hh}:${mm}z`;
}

/** Briefing-2a §5.2 Countdown: "in 28 Min" vor STD, "überfällig seit n Min"
 *  danach. `std` ist "HH:MM" Zulu-Uhrzeit-of-day (kein Datum) — wir nehmen
 *  an, dass die Buchung innerhalb der naechsten 24h liegt und picken den
 *  naeheren der beiden Kandidaten (heute/morgen) relativ zu `nowMs`. */
function stdCountdown(
  std: string | null,
  nowMs: number,
): { minutes: number; overdue: boolean; crossesDay: boolean; dayDiff: number } | null {
  if (!std) return null;
  const m = /^(\d{1,2}):(\d{2})$/.exec(std.trim());
  if (!m) return null;
  const hh = Number(m[1]);
  const mm = Number(m[2]);
  if (hh > 23 || mm > 59) return null;
  const now = new Date(nowMs);
  const candidate = new Date(
    Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), hh, mm, 0),
  );
  let diffMin = Math.round((candidate.getTime() - nowMs) / 60_000);
  // Naeherer 24h-Nachbar: wenn STD vor >12h "in der Vergangenheit" waere,
  // ist es wahrscheinlicher der morgige Slot; wenn >12h in der Zukunft,
  // wahrscheinlicher der gestrige (= schon ueberfaellig).
  if (diffMin < -12 * 60) diffMin += 24 * 60;
  else if (diffMin > 12 * 60) diffMin -= 24 * 60;
  // phpVMS' dpt_time ist eine reine "HH:MM"-Tageszeit ohne Datum — es gibt
  // hier kein echtes Kalenderdatum zum Vergleichen, daher immer `false`
  // (bleibt bei der bisherigen Minuten-Anzeige, nie die Tage-Formulierung).
  return { minutes: Math.abs(diffMin), overdue: diffMin < 0, crossesDay: false, dayDiff: 0 };
}

/** "HH:MM" (UTC) aus einem Unix-Epoch (Sekunden) — SimBrief `sched_out`/
 *  `sched_in` Fallback wenn phpVMS' `flight.dpt_time`/`arr_time` fehlt. */
function formatEpochHm(epochSec: number): string {
  const d = new Date(epochSec * 1000);
  const hh = d.getUTCHours().toString().padStart(2, "0");
  const mm = d.getUTCMinutes().toString().padStart(2, "0");
  return `${hh}:${mm}`;
}

/** Countdown direkt aus einem Unix-Epoch — exakter als `stdCountdown`s
 *  HH:MM-Naeherung (kein 24h-Nachbar-Raten noetig). */
function epochCountdown(
  epochSec: number,
  nowMs: number,
): { minutes: number; overdue: boolean; crossesDay: boolean; dayDiff: number } {
  const diffMin = Math.round((epochSec * 1000 - nowMs) / 60_000);
  // Pilot-Feedback 2026-08-03: die Anzeige zeigt STD nur als "HH:MMz" ohne
  // Datum (formatEpochHm) — der Countdown rechnet aber mit dem VOLLEN
  // Zeitstempel. Der tueckische Fall ist NICHT einfach ">24h alt", sondern
  // wenn das OFP einen anderen Kalendertag hat als "jetzt" — das kann schon
  // bei deutlich unter 24h Differenz passieren (z.B. 18.8h alt, aber ueber
  // Mitternacht hinweg): die Minutenzahl sieht dann "plausibel klein" aus,
  // passt aber nicht zur naiven Kopfrechnung "jetzige Uhrzeit minus
  // angezeigte STD-Uhrzeit" (die waere "in N Std", nicht ueberfaellig).
  // Deshalb wird hier der ECHTE Kalendertag verglichen, nicht nur die
  // Minuten-Groessenordnung.
  const epochDate = new Date(epochSec * 1000);
  const nowDate = new Date(nowMs);
  const epochDayIndex = Date.UTC(epochDate.getUTCFullYear(), epochDate.getUTCMonth(), epochDate.getUTCDate());
  const nowDayIndex = Date.UTC(nowDate.getUTCFullYear(), nowDate.getUTCMonth(), nowDate.getUTCDate());
  const dayDiff = Math.round(Math.abs(epochDayIndex - nowDayIndex) / 86_400_000);
  return { minutes: Math.abs(diffMin), overdue: diffMin < 0, crossesDay: dayDiff > 0, dayDiff };
}

/// phpVMS-Flight-Type-Code → kurzes UI-Label.
/// Stage-E-Fix (2026-08-02): die vorherige Zuordnung war eine ungeprüfte
/// Annahme und stimmte an mehreren Stellen NICHT mit dem echten phpVMS-Core
/// überein — verifiziert gegen `app/Models/Enums/FlightType.php` auf dem
/// GSG-Server. Insbesondere: "T" ist TECHNICAL_TEST, nicht Training (echtes
/// Training ist "K"); "M" ist MAIL_SERVICE, nicht Military (echtes Military
/// ist "W"); "R" ist ADDTL_CARGO_IN_CABIN, keine Repositionierung (die ist
/// "X", TECHNICAL_STOP). "E" = VIP — nicht "ECON", das gibt es als Konzept
/// in diesem Enum gar nicht. Volle Liste, damit sowas nicht nochmal aus dem
/// Bauch heraus geraten wird.
function flightTypeLabel(type: string): string {
  switch (type.toUpperCase()) {
    case "J": return "PAX";
    case "F": return "CARGO";
    case "C": return "CHARTER";
    case "A": return "CARGO+";
    case "E": return "VIP";
    case "G": return "PAX+";
    case "H": return "CHARTER CARGO";
    case "I": return "AMBULANCE";
    case "K": return "TRAINING";
    case "M": return "MAIL";
    case "O": return "CHARTER SPECIAL";
    case "P": return "POSITIONING";
    case "T": return "TECH TEST";
    case "W": return "MILITARY";
    case "X": return "TECH STOP";
    case "S": return "SHUTTLE";
    case "B": return "SHUTTLE+";
    case "Q":
    case "R":
    case "L": return "CARGO IN CABIN";
    case "D": return "GA";
    case "N": return "AIR TAXI";
    case "Y": return "SPECIAL";
    case "Z": return "OTHER";
    default: return type.toUpperCase();
  }
}

// Briefing-2a §5.1: Flugtyp erscheint nur noch als ausgeschriebener Text
// in der Meta-Zeile (kein farbiges Badge mehr) — flightTypeKind() (Badge-
// Farbklasse) ist damit ueberfluessig, siehe git history falls die
// farbige Pill-Darstellung mal zurueckkommen soll.

// v0.5.29: flightRulesHint() entfernt — Auto-Kategorisierung war zu eng.
// Pilot entscheidet selbst ob IFR oder VFR (= klare Hinweis-Text-Box
// statt Pill). Falls künftig wieder gebraucht, im git history (v0.5.28).

function buildCallsigns(flight: Flight): string {
  const icao = flight.airline?.icao?.trim();
  const iata = flight.airline?.iata?.trim();
  const fnum = resolveFlightIdent(flight.flight_number, flight.callsign);
  const icaoCs = icao ? `${icao}${fnum}` : null;
  const iataCs = iata ? `${iata}${fnum}` : null;
  if (icaoCs && iataCs) return `${icaoCs} · ${iataCs}`;
  return icaoCs ?? iataCs ?? fnum;
}

/** Briefing-2a §5.1: ICAO-Callsign (Zeile 1) und IATA-Callsign (Zeile 2,
 *  hinter dem Airline-Namen) getrennt statt als ein kombinierter String. */
function splitCallsigns(flight: Flight): { icao: string | null; iata: string | null } {
  const icao = flight.airline?.icao?.trim();
  const iata = flight.airline?.iata?.trim();
  const fnum = resolveFlightIdent(flight.flight_number, flight.callsign);
  return {
    icao: icao ? `${icao}${fnum}` : null,
    iata: iata ? `${iata}${fnum}` : null,
  };
}

function asUiError(err: unknown): UiError {
  return typeof err === "object" && err !== null && "code" in err
    ? (err as UiError)
    : { code: "unknown", message: String(err) };
}

/** Two-letter monogram fallback when no logo is available. */
function airlineMonogram(flight: Flight): string {
  return (
    flight.airline?.iata?.toUpperCase() ??
    flight.airline?.icao?.slice(0, 2).toUpperCase() ??
    "—"
  );
}

// v0.7.7 → v1.5.2 refactor: Error-Notice-Mapping wurde in den shared
// Helper `formatRefreshError` ausgelagert (lib/refreshErrorFormatter.ts).
// Das hier bleibt fuer den Success-Pfad (changed=false → "OFP unchanged"-
// Notice). Verhindert duplizierte JSON-Parse-Logik in 3 Komponenten.
//
// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §8 Notice-Tabelle.
export function refreshSuccessNotice(
  result: SimBriefRefreshResult | null,
): { key: string; tone: "info" | "warn" } | null {
  if (result && !result.changed) {
    return { key: "bids.ofp_unchanged", tone: "info" };
  }
  // changed=true → kein Notice (Erfolg ist still, Cockpit + Loadsheet
  // zeigen die neuen Werte sofort).
  return null;
}

export function BidsList({
  baseUrl,
  simState,
  simSnapshot,
  hasActiveFlight,
  onActiveFlightUpdated,
  onSelect,
  onFlightStarted,
  onProfileRefreshed,
}: Props) {
  const { t, i18n } = useTranslation();
  const [state, setState] = useState<State>({ kind: "loading" });
  // Briefing-2a §6: genau EINE Hauptkarte (voller Umfang), alle anderen
  // gebuchten Fluege als Einzeiler. Default = der zeitlich naechste Bid
  // (state.bids[0]); ein Klick auf einen Einzeiler tauscht die Rolle.
  const [mainBidId, setMainBidId] = useState<number | null>(null);
  // Stage E redesign: IFR/VFR ist jetzt EIN Umschalter mit EINER
  // primären "Flug starten"-Aktion statt zwei gleichrangigen Buttons.
  // Default pro Bid: IFR wenn ein SimBrief-OFP gebunden ist (der
  // Normalfall), sonst VFR (IFR braucht zwingend einen OFP).
  const [modeByBid, setModeByBid] = useState<Record<number, "ifr" | "vfr">>({});
  const [refreshing, setRefreshing] = useState(false);
  // v0.7.7 → v1.5.2: Notice-Pill im Bid-Tab-Header nach Refresh.
  // Vereinfacht auf {text, tone} weil der shared formatRefreshError-Helper
  // bereits den fertig formatierten lokalisierten Text liefert. Success-
  // Pfad (changed=false) bekommt seinen Text inline via t() in handleRefresh.
  // Auto-clear nach 6s.
  const [refreshNotice, setRefreshNotice] = useState<
    { text: string; tone: "info" | "warn" | "err" | "ok" } | null
  >(null);
  useEffect(() => {
    if (!refreshNotice) return;
    const id = setTimeout(() => setRefreshNotice(null), 6000);
    return () => clearTimeout(id);
  }, [refreshNotice]);

  // v0.7.10: Pre-Flight SimBrief-Preview-Cache pro Bid. Wird durch
  // `bid_simbrief_preview` befuellt — uberschreibt die phpVMS-Snapshot-
  // Werte (Block, ZFW, TOW, LDW) in der Bid-Display-Anzeige.
  interface BidSimBriefPreview {
    request_id: string;
    planned_block_fuel_kg: number;
    planned_burn_kg: number;
    planned_reserve_kg: number;
    planned_zfw_kg: number;
    planned_tow_kg: number;
    planned_ldw_kg: number;
    alternate: string | null;
    ofp_flight_number: string;
    ofp_origin_icao: string;
    ofp_destination_icao: string;
    // v0.7.12: Pax/Cargo aus dem SimBrief-OFP-XML — die Bid-Card zeigt diese
    // statt der phpVMS-Bid-Pointer-Subfleet-Fares wenn Preview da ist.
    pax_count: number;
    /** Gepaeck + Fracht zusammen — fuer den Chip `freight_kg` nehmen. */
    cargo_kg: number;
    freight_kg: number;
    /** Briefing-2a: Fallback-Quellen wenn phpVMS-seitige Felder fehlen. */
    aircraft_icao: string | null;
    max_passengers: number | null;
    sched_out_epoch: number | null;
    sched_in_epoch: number | null;
    /** Restliche Fuel-Kategorien — mit Trip+Reserve ergibt die Summe Block. */
    planned_taxi_kg: number;
    planned_contingency_kg: number;
    planned_alternate_burn_kg: number;
    planned_extra_kg: number;
    /** OEW + Payload ergeben zusammen ZFW. */
    planned_oew_kg: number;
    planned_payload_kg: number;
    /** Pax-Gewicht + Gepaeck + Fracht ergeben zusammen Payload. */
    planned_pax_weight_kg: number;
    planned_baggage_kg: number;
    /** Operationeller (flug-spezifischer) MTOW-Grenzwert aus dem OFP. */
    planned_max_tow_kg: number;
    callsign_warning: { sb_callsign: string; active_callsigns: string } | null;
  }
  const [bidPreviews, setBidPreviews] = useState<Map<number, BidSimBriefPreview>>(
    new Map(),
  );
  /** Briefing-2a §3: OFP-Statuszeile braucht pro Bid, ob der Preview-Fetch
   *  fehlgeschlagen ist (fuer den FEHLER-Zustand) und wann zuletzt erfolgreich
   *  geladen wurde (fuer den BEREIT-Zeitstempel). */
  const [bidPreviewErrors, setBidPreviewErrors] = useState<Set<number>>(new Set());
  const [previewLoadedAt, setPreviewLoadedAt] = useState<number | null>(null);
  /** Briefing-2a §4: "Stand {hh:mm}z" — Zeitpunkt des letzten erfolgreichen
   *  Bid-Fetches (nicht des Previews). */
  const [bidsLoadedAt, setBidsLoadedAt] = useState<number | null>(null);
  /** Briefing-2a §5.2: Countdown-Minutentakt (README §8: "Minutentakt,
   *  nicht sekuendlich"). */
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  const [startingId, setStartingId] = useState<number | null>(null);
  const [startError, setStartError] = useState<{
    bidId: number;
    message: string;
  } | null>(null);
  // v0.8.3 (#7): Wenn Backend "aircraft_mismatch_warning" liefert,
  // zeigen wir hier ein gelbes Banner mit "Trotzdem starten"-Button —
  // statt der roten Hard-Block-Bar. Unterstuetzt Wetlease-Flows
  // (PaxStudio-Loadsheet erlaubt jedes aktive Aircraft).
  const [startWarning, setStartWarning] = useState<{
    bidId: number;
    message: string;
  } | null>(null);
  /** v0.5.27: Manual/VFR-Mode-Modal — Bid für den's gerade geöffnet ist. */
  const [manualModalBid, setManualModalBid] = useState<Bid | null>(null);
  /** v0.12.12-dev: Wetter-Briefing-Lade-Hinweis. Erscheint per Toast für 5 s
   *  beim Klick auf "Wetter-Briefing öffnen" und informiert den Pilot, dass
   *  die GSG-Seite ihre Daten live holt → Browser-Tab kann bis 30 s laden. */
  const [weatherLoadHint, setWeatherLoadHint] = useState(false);
  /** Cached airport coords keyed by uppercase ICAO. */
  const [airports, setAirports] = useState<Record<string, AirportInfo>>({});
  /** Tracks ICAOs we've already requested so we don't fetch the same one twice. */
  const requestedIcaosRef = useRef<Set<string>>(new Set());

  const fetchBids = useCallback(async () => {
    try {
      const bids = await invoke<Bid[]>("phpvms_get_bids");
      setState(bids.length === 0 ? { kind: "empty" } : { kind: "ready", bids });
      setBidsLoadedAt(Date.now());
      return bids;
    } catch (err: unknown) {
      setState({ kind: "error", error: asUiError(err) });
      return [];
    }
  }, []);

  // v0.7.10: holt SimBrief-direct-Preview fuer jeden Bid in der Liste
  // parallel. Schreibt Ergebnisse in `bidPreviews` damit Display-Werte
  // die phpVMS-Snapshot-Werte ueberschreiben. Stille Fehler (nur Logging)
  // — Pre-Flight ist optional, phpVMS-Snapshot bleibt als Fallback.
  // Returns success/error counts fuer Notice-Anzeige.
  const fetchPreviewsForBids = useCallback(
    async (bids: Bid[]): Promise<{ successCount: number; errorCount: number }> => {
      if (bids.length === 0) {
        setBidPreviews(new Map());
        return { successCount: 0, errorCount: 0 };
      }
      let results = await Promise.allSettled(
        bids.map((b) =>
          invoke<BidSimBriefPreview>("bid_simbrief_preview", { bidId: b.id }),
        ),
      );

      // Briefing-2a-Haertung (Pilot-Feedback 2026-08-03): direkt nach dem
      // Login laeuft dieser Fetch parallel zu App.tsx's eigenem useEffect,
      // der die SimBrief-Settings aus localStorage ins Backend synct
      // (`set_simbrief_settings`). Race-Condition: kommt dieser Fetch
      // zuerst dran, sieht das Backend noch KEINE Settings und bricht mit
      // `no_simbrief_settings` ab — ein reiner Timing-Fehler, kein
      // Netzwerkproblem (das haette ein Retry im Rust-Client nicht
      // behoben, da der Abbruch VOR jedem Netzwerk-Call passiert). Sync
      // braucht nur einen Wimpernschlag; ein einziger verzoegerter Retry
      // genau der betroffenen Bids reicht.
      const raceFailedIdx = results
        .map((r, i) => ({ r, i }))
        .filter(({ r }) => {
          if (r.status !== "rejected") return false;
          const reason = r.reason as { code?: string } | undefined;
          return reason?.code === "no_simbrief_settings";
        })
        .map(({ i }) => i);

      if (raceFailedIdx.length > 0) {
        await new Promise((resolve) => setTimeout(resolve, 600));
        const retried = await Promise.allSettled(
          raceFailedIdx.map((i) =>
            invoke<BidSimBriefPreview>("bid_simbrief_preview", { bidId: bids[i]!.id }),
          ),
        );
        raceFailedIdx.forEach((idx, j) => {
          results[idx] = retried[j]!;
        });
      }

      const next = new Map<number, BidSimBriefPreview>();
      const nextErrors = new Set<number>();
      let successCount = 0;
      let errorCount = 0;
      results.forEach((r, i) => {
        if (r.status === "fulfilled") {
          next.set(bids[i]!.id, r.value);
          successCount++;
        } else {
          // v0.13.7: console.log entfernt — Preview-Failure ist eine
          // erwartete Bedingung (kein OFP fuer den Bid generiert) und
          // sollte die DevTools nicht zumuellen. Bei Bedarf kann das
          // Backend ein log_activity-Entry liefern.
          // Briefing-2a §3: trotzdem gemerkt, damit die Statuszeile bei
          // einem Bid MIT gebundenem OFP dessen echtes Fehlschlagen
          // ("SimBrief nicht erreichbar") von "kein OFP gebunden"
          // unterscheiden kann (Aufrufer prueft flight.simbrief?.id dafuer).
          nextErrors.add(bids[i]!.id);
          errorCount++;
        }
      });
      setBidPreviews(next);
      setBidPreviewErrors(nextErrors);
      setPreviewLoadedAt(Date.now());
      return { successCount, errorCount };
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const bids = await invoke<Bid[]>("phpvms_get_bids");
        if (cancelled) return;
        setState(bids.length === 0 ? { kind: "empty" } : { kind: "ready", bids });
        // v0.7.10: nach Initial-Load gleich SimBrief-Previews holen
        // damit der erste Render schon die frischen Werte zeigt.
        void fetchPreviewsForBids(bids);
      } catch (err: unknown) {
        if (cancelled) return;
        setState({ kind: "error", error: asUiError(err) });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [fetchPreviewsForBids]);

  // Background-poll bids so a freshly booked flight on phpVMS appears in
  // the list almost immediately — the pilot has the app open precisely
  // because they're about to fly, so a 15 s tick feels live without
  // burning through the phpVMS rate-limit budget (60 req/min/IP).
  // Pauses while a flight is active (no new flight can start anyway).
  useEffect(() => {
    if (hasActiveFlight) return;
    const id = setInterval(() => {
      void fetchBids();
    }, 15_000);
    return () => clearInterval(id);
  }, [hasActiveFlight, fetchBids]);

  /** Combined refresh: bid list + sim-position cache + pilot profile.
   *
   *  v0.1.30 added the profile re-fetch: pilots reported the
   *  "Standort" (curr_airport) chip in the header staying wrong even
   *  after their PIREP filed and phpVMS server-side updated the
   *  pilot's location. Cause: the cached LoginResult only ever held
   *  the login-time profile; a manual logout/login was the only way
   *  to refresh it. Now hitting "Aktualisieren" also pulls a fresh
   *  profile and bubbles it up to the parent so the header updates.
   *
   *  All three calls fire in parallel — the slowest network call
   *  bounds the spinner duration; the sim resync and any profile
   *  failure are non-fatal.
   */
  async function handleRefresh() {
    if (refreshing) return;
    setRefreshing(true);
    setRefreshNotice(null);

    // v0.7.7: Benannte Promises statt Promise<unknown>[]-Array damit
    // TypeScript die Result-Typen behaelt (QS-Hint aus Spec v1.1 §8).
    // v0.7.10: fetchBids gibt jetzt die Bid-Liste zurueck damit wir
    // danach SimBrief-direct fuer jeden Bid holen koennen.
    const bidsP = fetchBids();
    const simP = invoke("sim_force_resync").catch(() => {
      // Adapter command failures only happen if the mutex is poisoned
      // (= app already broken). Don't fail the whole refresh because of it.
      return null;
    });
    const profileP = invoke<Profile | null>("phpvms_refresh_profile").catch(
      () => null,
    );

    // v0.7.7: Wenn ein aktiver Flug existiert, OFP-Refresh mit ausloesen.
    // Backend hat Phase-Gate (Preflight|Boarding|Pushback|TaxiOut) und
    // returnt `phase_locked` in spaeteren Phasen — wir ignorieren das
    // still, damit der Bid-Tab-Refresh in Cruise nicht spammt.
    //
    // W5-Realitaet: phpVMS-7 entfernt den Bid nach Prefile. In den
    // meisten Real-Boarding-Faellen wird flight_refresh_simbrief mit
    // `bid_not_found` antworten — refreshNoticeKey() macht daraus
    // einen ehrlichen Pilot-Hinweis (Spec §8).
    //
    // v0.7.10: nur bei aktivem Flug aufrufen — Pre-Flight wird jetzt
    // vom bid_simbrief_preview-Pfad gehandled (= grüner "Daten geladen"
    // Notice statt no_active_flight-Fehler).
    const refreshP: Promise<{
      result: SimBriefRefreshResult | null;
      error: { code?: string } | null;
    }> = hasActiveFlight
      ? invoke<SimBriefRefreshResult>("flight_refresh_simbrief")
          .then((result) => ({ result, error: null }))
          .catch((err: { code?: string; message?: string }) => ({
            result: null,
            error: err,
          }))
      : Promise.resolve({ result: null, error: null });

    const [freshBids, , freshProfile, refreshOutcome] = await Promise.all([
      bidsP,
      simP,
      profileP,
      refreshP,
    ]);

    if (freshProfile && onProfileRefreshed) {
      onProfileRefreshed(freshProfile);
    }

    // v0.7.10: pro Bid SimBrief-direct preview holen damit Display-Werte
    // vom frischen OFP kommen statt vom phpVMS-Snapshot. Result zaehlt
    // erfolgreiche Previews fuer den Pre-Flight-Notice unten.
    const previewSummary = await fetchPreviewsForBids(freshBids);

    // v0.7.9 QS-Round-2: Notice-Logik defensiv — IMMER ein Banner zeigen,
    // egal was passiert. Vorher gab es Pfade die `null` zurueckgaben
    // (success+changed=true, unknown-error-code) → Pilot sah gar nichts
    // und wusste nicht ob der Refresh ueberhaupt durchlief.
    // v0.13.7: Diagnose-console.log entfernt — Pilot bekommt die Outcome
    // bereits ueber das Notice-UI; Devs koennen via Tauri-Logs nachvollziehen.

    if (refreshOutcome.error) {
      const formatted = formatRefreshError(refreshOutcome.error, t);
      if (formatted) {
        setRefreshNotice({ text: formatted.text, tone: formatted.tone });
      } else {
        // Unbekannter Error-Code → roher Code als Fallback damit Pilot
        // was sieht statt silent fail.
        setRefreshNotice({
          text: t("bids.refresh_unknown_error", {
            code: refreshOutcome.error.code ?? "unknown",
          }),
          tone: "warn",
        });
      }
    } else if (refreshOutcome.result) {
      // Success-Pfad: 2 Faelle (changed=true / changed=false), plus
      // optional Callsign-Warning.
      let noticeSet = false;

      // v0.7.9: Callsign-Warning ueberschreibt alles andere wenn vorhanden
      // — es ist die wichtigste Info ("OFP geladen aber Callsign weicht ab")
      try {
        const warn = await invoke<{
          sb_callsign: string;
          active_callsigns: string;
          issued_at: string;
        } | null>("ofp_callsign_warning_get");
        if (warn) {
          setRefreshNotice({
            text: t("flight.ofp_callsign_warning", {
              sb_callsign: warn.sb_callsign,
              active_callsigns: warn.active_callsigns,
            }),
            tone: "warn",
          });
          noticeSet = true;
        }
      } catch {
        // noop — Warning ist Nice-to-have
      }

      if (!noticeSet) {
        if (refreshOutcome.result.changed) {
          // OFP wurde wirklich aktualisiert — grüner Erfolg-Notice.
          // Vorher silent (Spec sagte "Cockpit zeigt die neuen Werte"),
          // aber Pilot brauchte sichtbare Bestaetigung dass der Klick
          // was bewirkt hat.
          setRefreshNotice({
            text: t("bids.ofp_refreshed", {
              id: refreshOutcome.result.current_ofp_id ?? "",
            }),
            tone: "ok",
          });
        } else {
          // OFP-ID unveraendert → "kein Update" Notice (war vorher schon da)
          setRefreshNotice({
            text: t("bids.ofp_unchanged"),
            tone: "info",
          });
        }
      }
    } else {
      // v0.7.10 Pre-Flight: kein aktiver Flug → flight_refresh_simbrief
      // wurde gar nicht aufgerufen. Stattdessen rendert die Bid-Liste
      // jetzt SimBrief-direct-Werte via bid_simbrief_preview. Notice
      // zeigt wie viele Bids frische OFP-Daten geladen haben.
      if (previewSummary.successCount > 0) {
        setRefreshNotice({
          text: t("bids.preview_loaded", {
            count: previewSummary.successCount,
          }),
          tone: "ok",
        });
      } else if (previewSummary.errorCount > 0) {
        setRefreshNotice({
          text: t("bids.preview_failed_all"),
          tone: "warn",
        });
      }
      // Wenn beide 0: keine Bids in der Liste — kein Notice.
    }

    // v0.7.7 §6.5b: Bei `changed=true` Parent direkt benachrichtigen
    // damit Cockpit + Loadsheet sofort den neuen Plan sehen statt erst
    // nach 2s-flight_status-Poll.
    if (refreshOutcome.result?.changed) {
      onActiveFlightUpdated?.();
    }

    // Tiny visible spinner tail so the pilot sees confirmation even
    // when both calls return instantly.
    setTimeout(() => setRefreshing(false), 400);
  }

  // Whenever the bids change, fetch the coordinates of every unique departure
  // airport in the background. Results are cached server-side too, so this is
  // cheap on subsequent calls.
  useEffect(() => {
    if (state.kind !== "ready") return;
    const uniqueIcaos = new Set(
      state.bids.map((b) => b.flight.dpt_airport_id.trim().toUpperCase()),
    );
    for (const icao of uniqueIcaos) {
      if (!icao || requestedIcaosRef.current.has(icao)) continue;
      if (airports[icao]) continue;
      requestedIcaosRef.current.add(icao);
      void (async () => {
        try {
          const info = await invoke<AirportInfo>("airport_get", { icao });
          setAirports((prev) => ({ ...prev, [icao]: info }));
        } catch {
          // Leave the icao un-cached; the user will still get a clear error
          // if they try to start the flight (server-side check kicks in).
          requestedIcaosRef.current.delete(icao);
        }
      })();
    }
  }, [state, airports]);

  function handleSetMain(bid: Bid) {
    setMainBidId(bid.id);
    onSelect?.(bid);
  }

  async function openFlightPage(flight: Flight) {
    const url = `${baseUrl.replace(/\/$/, "")}/flights/${flight.id}`;
    try {
      await openUrl(url);
    } catch {
      // ignore — the user will see the URL didn't open and can retry
    }
  }

  async function openOfp(flight: Flight) {
    if (!flight.simbrief?.id) return;
    // paxstudio-theme convention; works on GSG and most paxstudio-based VAs.
    // TODO: per-VA configurable URL pattern in Phase 4.
    const url = `${baseUrl.replace(/\/$/, "")}/paxstudio/ofp/${flight.simbrief.id}`;
    try {
      await openUrl(url);
    } catch {
      // ignore
    }
  }

  async function startFlight(bid: Bid, acknowledgeAircraftMismatch = false) {
    if (startingId !== null || hasActiveFlight) return;
    setStartingId(bid.id);
    setStartError(null);
    setStartWarning(null);
    try {
      const result = await invoke<ActiveFlightInfo>("flight_start", {
        bidId: bid.id,
        // v0.8.3 (#7): bei "Trotzdem starten"-Klick wird der Aircraft-
        // Mismatch-Check serverseitig uebersprungen.
        acknowledgeAircraftMismatch,
      });
      onFlightStarted?.(result);
    } catch (err: unknown) {
      const ui = asUiError(err);
      // v0.8.3 (#7): aircraft_mismatch_warning ist KEIN Hard-Block —
      // gelbes Warn-Banner mit "Trotzdem starten"-Button anzeigen.
      if (ui.code === "aircraft_mismatch_warning") {
        setStartWarning({
          bidId: bid.id,
          message: `${t("flight.error.aircraft_mismatch_warning")} (${ui.message})`,
        });
        return;
      }
      // Map known backend error codes to localized messages; fall back to the
      // raw server-supplied message if the code is unfamiliar.
      const knownCodes = [
        "no_sim_snapshot",
        "not_on_ground",
        "not_at_departure",
        "missing_airline",
        "missing_aircraft",
        "flight_already_active",
        "bid_not_found",
        "aircraft_not_available",
        "aircraft_mismatch",
        "phpvms_error",
      ];
      const message = knownCodes.includes(ui.code)
        ? `${t(`flight.error.${ui.code}`)} (${ui.message})`
        : ui.message;
      setStartError({ bidId: bid.id, message });
    } finally {
      setStartingId(null);
    }
  }

  // Sim-position health check. v0.1.29 onwards we ONLY surface the
  // warning row when something actually looks wrong — in the happy
  // path (sim connected, position fresh and real) the row is
  // invisible so it doesn't clutter the page. Two failure modes
  // catch the bugs we've actually seen:
  //
  //   * sim says it's connected but lat/lon is exactly 0,0 →
  //     adapter's default-uninitialised state (= no real frame yet).
  //   * sim says it's connected and we have lat/lon, but the bid
  //     list shows "X nm from <airport>" with X far above any sane
  //     threshold (set by tooFar in the bid card below) AND the
  //     pilot tries to start anyway. We can't easily pre-compute
  //     this here because it depends on the airport coords cache,
  //     so the per-bid card is responsible for showing the warning
  //     inline (start-button title + start-disabled state). This
  //     row only catches the "no position at all" case.
  const hasSimPosition =
    simSnapshot !== null &&
    !(simSnapshot.lat === 0 && simSnapshot.lon === 0);
  const showPositionWarning = simState === "connected" && !hasSimPosition;

  // Briefing-2a §3: OFP-Statuszeile bezieht sich auf den NAECHSTEN Bid
  // (= die Hauptkarte, state.bids[0]) — das ist der Flug, den der Pilot
  // als naechstes starten wird.
  const primaryBid = state.kind === "ready" ? (state.bids[0] ?? null) : null;
  const primaryHasOfp = !!primaryBid?.flight.simbrief?.id;
  const primaryPreviewFailed =
    !!primaryBid && bidPreviewErrors.has(primaryBid.id);
  const ofpStatus: { tone: "ok" | "warn" | "danger"; word: string; text: string } | null =
    primaryBid === null
      ? null
      : primaryHasOfp && primaryPreviewFailed
        ? {
            tone: "danger",
            word: t("bids.ofp_status.error_word"),
            text: t("bids.ofp_status.error_text"),
          }
        : primaryHasOfp
          ? {
              tone: "ok",
              word: t("bids.ofp_status.ready_word"),
              text: t("bids.ofp_status.ready_text", {
                callsign: buildCallsigns(primaryBid.flight),
                time: previewLoadedAt ? formatZuluHm(previewLoadedAt) : "—",
              }),
            }
          : {
              tone: "warn",
              word: t("bids.ofp_status.none_word"),
              text: t("bids.ofp_status.none_text"),
            };

  return (
    <section className="bids">
      {ofpStatus && (
        <div className={`bids__ofp-status bids__ofp-status--${ofpStatus.tone}`}>
          <span className="bids__ofp-status-word">{ofpStatus.word}</span>
          <span className="bids__ofp-status-text">{ofpStatus.text}</span>
        </div>
      )}

      <header className="bids__header">
        <h2>
          {t("bids.title")}
          {state.kind === "ready" && (
            <span className="bids__count">{state.bids.length}</span>
          )}
        </h2>
        <div className="bids__header-right">
          {bidsLoadedAt !== null && (
            <span className="bids__stand">
              {t("bids.stand", { time: formatZuluHm(bidsLoadedAt) })}
            </span>
          )}
          <button
            type="button"
            className="bids__refresh"
            onClick={handleRefresh}
            disabled={refreshing || state.kind === "loading"}
          >
            {refreshing ? t("bids.refreshing") : t("bids.refresh")}
          </button>
        </div>
      </header>

      {/* v0.7.7 → v1.5.2: Notice nach Bid-Tab-Refresh. Text wird vom
          formatRefreshError-Helper geliefert (Error-Pfad) ODER vom
          inline-Render in handleRefresh (Success-Pfad mit changed=false).
          Auto-clear nach 6s. */}
      {/* v0.12.12-dev: 30-s-Lade-Hinweis fuer Wetter-Briefing — erscheint
          beim Klick auf "Wetter-Briefing öffnen" und blendet sich nach 5 s
          aus. */}
      {weatherLoadHint && (
        <div
          role="status"
          className="bids-refresh-notice bids-refresh-notice--info"
          style={{
            padding: "8px 12px",
            marginBottom: 10,
            borderRadius: 6,
            background: "rgba(56, 189, 248, 0.12)",
            border: "1px solid rgba(56, 189, 248, 0.35)",
            color: "#7dd3fc",
            fontSize: "0.88rem",
          }}
        >
          {t("bids.weather_briefing_load_hint")}
        </div>
      )}
      {refreshNotice && (
        <div
          role="status"
          className={`bids-refresh-notice bids-refresh-notice--${refreshNotice.tone}`}
          style={{
            padding: "6px 10px",
            marginBottom: 10,
            borderRadius: 6,
            // v0.7.10: 'ok'-Tone fuer gruene Success-Notice (Pre-Flight-
            // OFP-Refresh). Vorher fiel das auf info (blau) zurueck.
            background:
              refreshNotice.tone === "warn"
                ? "#3f2b0e"
                : refreshNotice.tone === "err"
                  ? "#3f0e0e"
                  : refreshNotice.tone === "ok"
                    ? "#0e3a1e"
                    : "#1e3a5f",
            border:
              refreshNotice.tone === "warn"
                ? "1px solid #b8842a"
                : refreshNotice.tone === "err"
                  ? "1px solid #c53030"
                  : refreshNotice.tone === "ok"
                    ? "1px solid #30d158"
                    : "1px solid #3b82f6",
            color:
              refreshNotice.tone === "warn"
                ? "#f5d68b"
                : refreshNotice.tone === "err"
                  ? "#fca5a5"
                  : refreshNotice.tone === "ok"
                    ? "#a7f3c2"
                    : "#cfe3ff",
            fontSize: "0.85rem",
          }}
        >
          {refreshNotice.text}
        </div>
      )}

      {showPositionWarning && (
        <div
          className="bids__sim-position bids__sim-position--warn"
          role="status"
        >
          <span className="bids__sim-position-message">
            {t("bids.sim_position_warning")}
          </span>
        </div>
      )}

      {/* v0.3.0: Auto-Start-Skip-Banner — zeigt warum Auto-Start
          gerade nicht greift, damit der Pilot nicht auf eine Meldung
          wartet die nie kommt. Pollt das Backend alle 3 s. */}
      <AutoStartSkipBanner />

      {/* Briefing-2a §9: Ladevorgang = Platzhalterflaechen in der
          Card-Sollhoehe statt Spinner-Overlay (keine Endlos-Animation
          auf diesem Screen). */}
      {state.kind === "loading" && (
        <div className="bid-card-skeleton" aria-hidden="true">
          <div className="bid-card-skeleton__head" />
          <div className="bid-card-skeleton__route" />
          <div className="bid-card-skeleton__rubrics" />
        </div>
      )}

      {/* Field feedback (2026-08-03): the Briefing-2a §9 "empty row, no
          illustration" choice read as unprofessional right next to
          Cockpit's own illustrated empty state — same icon+title+hint+
          button recipe as `.cockpit-empty` now. Header/tab bar above still
          render independently of state.kind, unchanged. */}
      {state.kind === "empty" && (
        <div className="bids-empty">
          <div className="bids-empty__icon" aria-hidden="true">
            🗓
          </div>
          <h2 className="bids-empty__title">{t("bids.empty_title")}</h2>
          <p className="bids-empty__hint">{t("bids.empty_hint")}</p>
          <button
            type="button"
            className="button button--primary"
            onClick={() => void openUrl(baseUrl).catch(() => {})}
          >
            {t("bids.empty_action")}
          </button>
        </div>
      )}

      {state.kind === "error" && (
        <div className="bids__error" role="alert">
          <p>{t(errorKey(state.error.code))}</p>
          {/* For decode failures the technical message tells us *which*
              bid / field broke parsing — Ralf hit this on the Eurowings
              test instance. Surface it in a <details> so the pilot can
              copy/paste the snippet without it dominating the panel. */}
          {state.error.code === "bad_response" && state.error.message && (
            <details className="bids__error-details">
              <summary>{t("bids.error.details_summary")}</summary>
              <code>{state.error.message}</code>
            </details>
          )}
        </div>
      )}

      {state.kind === "ready" && (() => {
        const bids = state.bids;
        const mainBid = bids.find((b) => b.id === mainBidId) ?? bids[0] ?? null;
        const restBids = bids.filter((b) => b.id !== mainBid?.id);
        // NÄCHSTER macht nur Sinn, wenn es ueberhaupt mehrere Buchungen gibt,
        // gegenueber denen dieser Flug "der naechste" ist — bei nur einer
        // Buchung waere das Badge bedeutungslose Deko (Pilot-Feedback 2026-08-03).
        const isNextFlight =
          bids.length > 1 && mainBid !== null && mainBid.id === bids[0]?.id;

        // Distanz/Sim-Gating braucht jeder Bid einzeln (Einzeiler zeigen
        // zwar kein Distanz-Badge, aber der Klick zum Main-Swap soll immer
        // moeglich sein — kein Gating dort).
        function derivedFor(bid: Bid) {
          const f = bid.flight;
          const dptIcao = f.dpt_airport_id.trim().toUpperCase();
          const dptCoords = airports[dptIcao];
          const hasRealPosition =
            simSnapshot !== null &&
            !(simSnapshot.lat === 0 && simSnapshot.lon === 0);
          let distanceToDptNm: number | null = null;
          if (
            hasRealPosition &&
            simSnapshot &&
            dptCoords &&
            dptCoords.lat !== null &&
            dptCoords.lon !== null
          ) {
            distanceToDptNm = distanceNm(
              simSnapshot.lat,
              simSnapshot.lon,
              dptCoords.lat,
              dptCoords.lon,
            );
          }
          const onGround = simSnapshot?.on_ground ?? false;
          const tooFar =
            distanceToDptNm !== null && distanceToDptNm > MAX_START_DISTANCE_NM;
          const noPositionYet = simState === "connected" && !hasRealPosition;
          const startDisabled =
            startingId !== null ||
            hasActiveFlight ||
            simState !== "connected" ||
            noPositionYet ||
            !onGround ||
            tooFar;

          let startTitle = "";
          if (simState !== "connected") {
            startTitle = t("bids.start_disabled_no_sim");
          } else if (hasActiveFlight) {
            startTitle = t("bids.start_disabled_active_flight");
          } else if (noPositionYet) {
            startTitle = t("bids.start_disabled_no_position");
          } else if (!onGround) {
            startTitle = t("bids.start_disabled_not_on_ground");
          } else if (tooFar && distanceToDptNm !== null) {
            startTitle = t("bids.start_disabled_too_far", {
              distance: distanceToDptNm.toFixed(1),
              airport: dptIcao,
            });
          }

          const mode = modeByBid[bid.id] ?? (f.simbrief?.id ? "ifr" : "vfr");
          const primaryDisabled = mode === "ifr" ? startDisabled : hasActiveFlight;
          const primaryHint =
            mode === "ifr"
              ? startTitle || t("bid_card.mode_ifr_hint")
              : t("bid_card.mode_vfr_hint");

          return { dptIcao, distanceToDptNm, tooFar, startDisabled, startTitle, mode, primaryDisabled, primaryHint };
        }

        return (
          <>
            {mainBid && (() => {
              const d = derivedFor(mainBid);
              return (
                <MainBidCard
                  bid={mainBid}
                  isNextFlight={isNextFlight}
                  preview={bidPreviews.get(mainBid.id) ?? null}
                  nowMs={nowMs}
                  mode={d.mode}
                  setMode={(next) => setModeByBid((prev) => ({ ...prev, [mainBid.id]: next }))}
                  primaryDisabled={d.primaryDisabled}
                  primaryHint={d.primaryHint}
                  startTitle={d.startTitle}
                  starting={startingId === mainBid.id}
                  startError={startError?.bidId === mainBid.id ? startError.message : null}
                  startWarning={startWarning?.bidId === mainBid.id ? startWarning.message : null}
                  distanceToDptNm={d.distanceToDptNm}
                  tooFar={d.tooFar}
                  dptIcao={d.dptIcao}
                  onStart={() => void startFlight(mainBid)}
                  onAcknowledgeMismatch={() => void startFlight(mainBid, true)}
                  onManualStart={() => setManualModalBid(mainBid)}
                  onOfp={() => void openOfp(mainBid.flight)}
                  onFlightPage={() => void openFlightPage(mainBid.flight)}
                  onWeather={() => {
                    setWeatherLoadHint(true);
                    window.setTimeout(() => setWeatherLoadHint(false), 5000);
                    void openUrl("https://german-sky-group.eu/weatherbriefing").catch(() => {});
                  }}
                />
              );
            })()}

            {restBids.length > 0 && (
              <ul className="bids__more-list">
                {restBids.map((bid) => {
                  const f = bid.flight;
                  const preview = bidPreviews.get(bid.id) ?? null;
                  const dpt = f.dpt_airport?.icao ?? f.dpt_airport_id;
                  const arr = f.arr_airport?.icao ?? f.arr_airport_id;
                  const { icao: icaoCallsign } = splitCallsigns(f);
                  const airlineLogo = f.airline?.logo?.trim() || null;
                  const monogram = airlineMonogram(f);
                  const aircraftType = f.simbrief?.subfleet?.type_ ?? preview?.aircraft_icao ?? undefined;
                  const dptTimeDisplay = f.dpt_time ?? (preview?.sched_out_epoch != null ? formatEpochHm(preview.sched_out_epoch) : null);
                  const arrTimeDisplay = f.arr_time ?? (preview?.sched_in_epoch != null ? formatEpochHm(preview.sched_in_epoch) : null);

                  return (
                    <li key={bid.id}>
                      <button
                        type="button"
                        className="bid-more"
                        onClick={() => handleSetMain(bid)}
                      >
                        <div className="bid-more__leg">
                          <span className="bid-more__icao">{dpt}</span>
                          <span className="bid-more__time">
                            {dptTimeDisplay ? `${dptTimeDisplay}z` : "--:--"}
                          </span>
                        </div>
                        <div className="bid-more__mid">
                          <div
                            className={`bid-more__logo ${airlineLogo ? "" : "bid-more__logo--placeholder"}`}
                          >
                            {airlineLogo ? (
                              <img src={airlineLogo} alt={f.airline?.name ?? icaoCallsign ?? ""} />
                            ) : (
                              <span>{f.airline?.icao ?? monogram}</span>
                            )}
                          </div>
                          <span className="bid-more__callsign">{icaoCallsign}</span>
                          <span className="bid-more__meta">
                            {[aircraftType, formatDistanceNm(f.distance?.nmi ?? null, i18n.language), formatFlightTime(f.flight_time, i18n.language)]
                              .filter(Boolean)
                              .join(" · ")}
                          </span>
                          {!f.simbrief?.id && (
                            <span className="bid-more__badge">{t("bid_card.no_ofp_badge")}</span>
                          )}
                        </div>
                        <div className="bid-more__leg bid-more__leg--arrival">
                          <span className="bid-more__time">
                            {arrTimeDisplay ? `${arrTimeDisplay}z` : "--:--"}
                          </span>
                          <span className="bid-more__icao">{arr}</span>
                        </div>
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </>
        );
      })()}

      {/* v0.5.27 Manual/VFR-Mode-Modal */}
      {manualModalBid && (
        <ManualFlightModal
          bid={manualModalBid}
          simHint={simSnapshot ? {
            aircraft_icao: simSnapshot.aircraft_icao,
            aircraft_registration: simSnapshot.aircraft_registration,
            fuel_total_kg: simSnapshot.fuel_total_kg,
          } : null}
          onClose={() => setManualModalBid(null)}
          onFlightStarted={(info) => {
            setManualModalBid(null);
            onFlightStarted?.(info);
          }}
        />
      )}
    </section>
  );
}

/**
 * Briefing-2a: die EINE Hauptflug-Karte (README §5). Ersetzt die
 * vorherige BidDetails-Ausklapp-Komponente — jetzt die komplette Karte
 * (Kopf + Streckenband + Rubriken + Aktionszeile) statt nur der
 * ausgeklappten Detail-Sektion, weil Logo/ALTN/Aircraft-Typ jetzt Teil
 * der Route sind statt einer separaten Kachel-Reihe.
 *
 * Aircraft-Info aus SimBrief-Subfleet (B738 · Boeing 737-800), Pax/Cargo-
 * Load aus den fares, Route-String aus phpVMS-Bid, SimBrief-Plan-
 * Vorschau (Block-Fuel, Trip-Burn, TOW, LDW, Reserve, ZFW, Alternate)
 * per `fetch_simbrief_preview` bzw. `preview`-Prop (SimBrief-direct).
 */
interface BidSimBriefPreviewProp {
  request_id: string;
  planned_block_fuel_kg: number;
  planned_burn_kg: number;
  planned_reserve_kg: number;
  planned_zfw_kg: number;
  planned_tow_kg: number;
  planned_ldw_kg: number;
  alternate: string | null;
  ofp_flight_number: string;
  ofp_origin_icao: string;
  ofp_destination_icao: string;
  pax_count: number;
  /** Gepaeck + Fracht zusammen — fuer den Chip `freight_kg` nehmen. */
  cargo_kg: number;
  freight_kg: number;
  aircraft_icao: string | null;
  max_passengers: number | null;
  sched_out_epoch: number | null;
  sched_in_epoch: number | null;
  planned_taxi_kg: number;
  planned_contingency_kg: number;
  planned_alternate_burn_kg: number;
  planned_extra_kg: number;
  planned_oew_kg: number;
  planned_payload_kg: number;
  planned_pax_weight_kg: number;
  planned_baggage_kg: number;
  planned_max_tow_kg: number;
  callsign_warning: { sb_callsign: string; active_callsigns: string } | null;
}

interface MainBidCardProps {
  bid: Bid;
  /** README §5.1: NÄCHSTER-Badge nur an der zeitlich ersten Karte, auch
   *  wenn der Pilot per Klick eine andere Buchung zur Hauptkarte macht. */
  isNextFlight: boolean;
  preview: BidSimBriefPreviewProp | null;
  nowMs: number;
  mode: "ifr" | "vfr";
  setMode: (next: "ifr" | "vfr") => void;
  primaryDisabled: boolean;
  primaryHint: string;
  startTitle: string;
  starting: boolean;
  startError: string | null;
  startWarning: string | null;
  distanceToDptNm: number | null;
  tooFar: boolean;
  dptIcao: string;
  onStart: () => void;
  onManualStart: () => void;
  onAcknowledgeMismatch: () => void;
  onOfp: () => void;
  onFlightPage: () => void;
  onWeather: () => void;
}

function MainBidCard({
  bid,
  isNextFlight,
  preview,
  nowMs,
  mode,
  setMode,
  primaryDisabled,
  primaryHint,
  startTitle,
  starting,
  startError,
  startWarning,
  distanceToDptNm,
  tooFar,
  dptIcao,
  onStart,
  onManualStart,
  onAcknowledgeMismatch,
  onOfp,
  onFlightPage,
  onWeather,
}: MainBidCardProps) {
  const { t, i18n } = useTranslation();
  const flight = bid.flight;
  const [plan, setPlan] = useState<SimBriefOfp | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  // Briefing-2a-Haertung (Pilot-Feedback 2026-08-03): startet "ladend" wenn
  // ein OFP gebunden ist, statt bei "false" — sonst rendert die Karte beim
  // allerersten Mount (bevor `preview` von BidsList ankommt) fuer 1-3s mit
  // `plan=null`, was in den Rubriken NICHT als Ladezustand aussah, sondern
  // wie fertige, aber unvollstaendige/falsche Werte (z.B. MTOW-Vergleich
  // fiel auf den strukturellen phpVMS-Wert zurueck statt dem echten
  // operationellen SimBrief-Wert zu zeigen — plausibel aussehend, aber
  // sachlich falsch).
  const [planLoading, setPlanLoading] = useState(() => !!flight.simbrief?.id);
  // v0.3.0: Aircraft-Reg — wenn die simbrief.aircraft_id vorhanden ist,
  // holen wir die konkrete Registrierung (z.B. "EI-ENI") über einen
  // separaten phpVMS-API-Call. Subfleet alleine sagt nur die Type-Klasse,
  // nicht das konkrete Flugzeug. Seit Briefing-2a traegt die Response
  // auch `mtow_kg` (§21) — Grundlage fuer die WEIGHTS-Einordnung.
  const [aircraft, setAircraft] = useState<AircraftInfo | null>(null);

  // OFP-Vorschau bei Bid-Selection holen. Nur einmal pro Render-Lifetime.
  // v0.7.10: `preview` (SimBrief-direct via bid_simbrief_preview Parent-Call)
  // hat Prioritaet — das sind die FRISCHEN Werte von simbrief.com. Der
  // `fetch_simbrief_preview` Pfad (via phpVMS-Bid-Pointer) bleibt als
  // Fallback wenn SimBrief-Settings fehlen oder direct-fetch failed.
  useEffect(() => {
    // Wenn Preview-Werte aus SimBrief-direct da sind, baue daraus ein
    // SimBriefOfp-kompatibles Objekt und nutze das. Kein phpVMS-Pointer-Call
    // noetig.
    if (preview) {
      setPlan({
        planned_block_fuel_kg: preview.planned_block_fuel_kg,
        planned_burn_kg: preview.planned_burn_kg,
        planned_reserve_kg: preview.planned_reserve_kg,
        planned_zfw_kg: preview.planned_zfw_kg,
        planned_tow_kg: preview.planned_tow_kg,
        planned_ldw_kg: preview.planned_ldw_kg,
        alternate: preview.alternate ?? undefined,
        ofp_flight_number: preview.ofp_flight_number,
        ofp_origin_icao: preview.ofp_origin_icao,
        ofp_destination_icao: preview.ofp_destination_icao,
        // Felder die SimBrief-direct nicht hat — auf Defaults
        route: undefined,
        waypoints: [],
        max_zfw_kg: 0,
        max_tow_kg: preview.planned_max_tow_kg,
        max_ldw_kg: 0,
        request_id: preview.request_id,
        // v0.7.12: Pax/Cargo aus dem OFP-XML — fuer die Bid-Card-Chips
        // wenn die phpVMS-Bid-Pointer-Subfleet-Fares leer sind.
        pax_count: preview.pax_count,
        cargo_kg: preview.cargo_kg,
        freight_kg: preview.freight_kg,
        aircraft_icao: preview.aircraft_icao,
        planned_oew_kg: preview.planned_oew_kg,
        planned_payload_kg: preview.planned_payload_kg,
        planned_pax_weight_kg: preview.planned_pax_weight_kg,
        planned_baggage_kg: preview.planned_baggage_kg,
      } as unknown as SimBriefOfp);
      setPlanLoading(false);
      setPlanError(null);
      return;
    }

    // Fallback v0.7.7-Pfad: phpVMS-Bid-Pointer
    const ofpId = flight.simbrief?.id;
    if (!ofpId) return;
    setPlanLoading(true);
    setPlanError(null);
    invoke<SimBriefOfp | null>("fetch_simbrief_preview", { ofpId })
      .then((result) => {
        setPlan(result);
        if (!result) {
          setPlanError(t("bids.simbrief_unavailable"));
        }
      })
      .catch((err: UiError) => {
        setPlanError(err.message ?? "Fehler");
      })
      .finally(() => setPlanLoading(false));
  }, [flight.simbrief?.id, t, preview]);

  // Aircraft-Reg holen wenn verfügbar. Fail silent (Pilot sieht halt nur
  // Subfleet-Name ohne Reg, kein Spam).
  useEffect(() => {
    const acId = flight.simbrief?.aircraft_id;
    if (!acId) return;
    invoke<AircraftInfo>("phpvms_get_aircraft", { aircraftId: acId })
      .then(setAircraft)
      .catch(() => setAircraft(null));
  }, [flight.simbrief?.aircraft_id]);

  // Pax + Cargo zusammenrechnen.
  // v0.7.12 (Bug-Fix): Bei aktivem Pre-Flight-Preview (v0.7.10) sind die
  // phpVMS-Bid-Pointer-Subfleet-Fares oft leer (Pilot hat noch keinen
  // OFP ueber phpVMS gebunden, sondern wir holen direkt von simbrief.com).
  // Dann zog die Bid-Card vorher 0 Pax / 0 Cargo aus den Fares und blendete
  // die Chips komplett aus — Pilot sah dann zwischen Aircraft-Zeile und
  // SimBrief-Plan-Block einen leeren Bereich. Fix: Preview-Werte
  // bevorzugen wenn da, sonst Fallback auf Bid-Subfleet-Fares.
  const fares = flight.simbrief?.subfleet?.fares ?? [];
  const faresPax = fares
    .filter((f) => (f.type ?? 0) === 0)
    .reduce((sum, f) => sum + (f.count ?? 0), 0);
  const faresCargoKg = fares
    .filter((f) => (f.type ?? 0) === 1)
    .reduce((sum, f) => sum + (f.count ?? 0), 0);
  const paxCount = preview && preview.pax_count > 0 ? preview.pax_count : faresPax;
  // Der Chip weist REINE Fracht aus. `preview.cargo_kg` waere Gepaeck +
  // Fracht (SimBrief `<weights><cargo>`) und stand vorher unter dem Label
  // "Cargo" — ein 150-Pax-Flug mit 3 t Fracht las sich damit als 6 t Fracht.
  // Der Fares-Fallback ist ohnehin schon reine Fracht, also passen beide
  // Pfade jetzt zueinander.
  const previewFreightKg =
    preview && preview.freight_kg > 0 ? preview.freight_kg : 0;
  const cargoKg = previewFreightKg > 0 ? previewFreightKg : faresCargoKg;

  // Briefing-2a: Fallback auf preview.aircraft_icao — der SimBrief-direct-
  // Pfad (Normalfall vor dem ersten phpVMS-OFP-Binding) hat keinen Bid-
  // Pointer-Subfleet, aber das SimBrief-OFP kennt den Aircraft-Typ selbst.
  const aircraftType = flight.simbrief?.subfleet?.type_ ?? preview?.aircraft_icao ?? undefined;
  const aircraftName = flight.simbrief?.subfleet?.name;
  const registration = aircraft?.registration ?? null;

  // v0.3.0: SimBrief-OFP-Mismatch-Detection (mehrere Signale).
  // Hintergrund: SimBrief liefert IMMER den letzten OFP des Pilot-
  // Accounts, egal ob der zur aktuellen phpVMS-Buchung passt. Wenn
  // der Pilot vergessen hat einen frischen OFP zu erstellen, sieht
  // er hier evtl. den Plan vom Vortag (falsche Airline/Aircraft/
  // Route). Wir prüfen mehrere Signale:
  //
  //   1. **Aircraft-Type:** OFP-Subfleet-ICAO vs. Bid-Aircraft-ICAO
  //      (z.B. OFP "A320" vs. Bid "B738")
  //   2. **Origin-Airport:** OFP-Origin vs. Bid-Departure (z.B. OFP
  //      "EDDF" vs. Bid "LOWS")
  //   3. **Destination-Airport:** dasselbe für Arrival
  //   4. **Flight-Number:** OFP-Callsign vs. Bid-Flightnumber (mit
  //      Airline-ICAO-Präfix; "RYR100" vs. "DLH123")
  //
  // Mindestens EIN klares Mismatch-Signal → Banner. Mehrere Signale
  // → eindeutig falscher OFP.
  // Briefing-2a-Fix: reiner ICAO-Typ (z.B. "A320") statt `aircraftType` —
  // seit dem Subfleet-Serde-Fix traegt `aircraftType` den VOLLEN phpVMS-
  // Subfleet-Namen ("VLG-A320-IAE-SL"), der hier gegen `aircraft.icao`
  // (reiner Typ) immer als Mismatch durchgefallen waere. `plan.aircraft_icao`
  // kommt direkt aus dem SimBrief-OFP als reiner Typ-Code.
  const ofpAcIcao = plan?.aircraft_icao?.toUpperCase().trim();
  const bidAcIcao = aircraft?.icao?.toUpperCase().trim();
  const acTypeMismatch =
    !!ofpAcIcao && !!bidAcIcao && ofpAcIcao !== bidAcIcao;

  const ofpOrigin = plan?.ofp_origin_icao?.toUpperCase().trim() ?? "";
  const ofpDest = plan?.ofp_destination_icao?.toUpperCase().trim() ?? "";
  const bidDpt = flight.dpt_airport_id.toUpperCase().trim();
  const bidArr = flight.arr_airport_id.toUpperCase().trim();
  const originMismatch = !!ofpOrigin && ofpOrigin !== bidDpt;
  const destMismatch = !!ofpDest && ofpDest !== bidArr;

  // v0.3.3+ — Flight-Number/Callsign-Mismatch fließt NICHT mehr in
  // den Banner-Trigger. Begründung: ein abweichender ATC-Callsign bei
  // richtiger Route + richtigem Aircraft ist fast immer ein legitimer
  // persönlicher Callsign (Pilot konfiguriert seinen Callsign in
  // SimBrief, nicht im phpVMS-Bid). Aircraft / Origin / Destination
  // sind die einzigen Signale stark genug für einen "altes OFP"-Befund.
  // Die Bid-Identität (`fullBidCallsign`) wird unten im Banner-Body
  // weiter angezeigt, deswegen behalten wir bidAirlineIcao/bidFnum/
  // bidCallsign.
  const bidAirlineIcao = flight.airline?.icao?.toUpperCase().trim() ?? "";
  const bidFnum = flight.flight_number.toUpperCase().replace(/\s/g, "");
  const bidCallsign = flight.callsign?.toUpperCase().replace(/\s/g, "") ?? "";

  // v0.3.0: Voller ATC-Callsign für die Banner-Anzeige. Wenn der Pilot
  // im phpVMS-Bid einen Callsign-Suffix hinterlegt hat (z.B. "4TK"),
  // ergänzen wir den Airline-Prefix damit's lesbar ist ("RYR4TK").
  // Sonst nehmen wir was direkt im Bid steht.
  const fullBidCallsign = bidCallsign
    ? bidCallsign.startsWith(bidAirlineIcao)
      ? bidCallsign
      : `${bidAirlineIcao}${bidCallsign}`
    : "";

  // Mindestens ein "starkes" Signal → Banner. Aircraft / Origin /
  // Destination sind hart — wenn die abweichen, ist der OFP nachweisbar
  // für einen anderen Flug. Flight-Number alleine ist zu schwach
  // (siehe `fnumMismatch`-Comment oben). Sie fließt nur in den
  // ausführlichen Banner-Body wenn er sowieso schon offen ist.
  const ofpMismatch = acTypeMismatch || originMismatch || destMismatch;

  const hasAircraft = !!(aircraftType || aircraftName);
  const hasLoad = paxCount > 0 || cargoKg > 0;
  const hasSimBriefId = !!flight.simbrief?.id;
  const hasRoute = !!flight.route;

  // Briefing-2a: Trip+Reserve allein ergaben keine nachvollziehbare Summe zu
  // Block (Pilot-Feedback 2026-08-02) — Taxi/Contingency/Alternate/Extra
  // ergaenzen die Rechnung (alle sechs Werte zusammen = Block).
  const fuelRows = [
    plan && plan.planned_burn_kg > 0
      ? `Trip ${Math.round(plan.planned_burn_kg).toLocaleString("de-DE")}`
      : null,
    plan && plan.planned_reserve_kg > 0
      ? `Reserve ${Math.round(plan.planned_reserve_kg).toLocaleString("de-DE")}`
      : null,
    preview && preview.planned_taxi_kg > 0
      ? `Taxi ${Math.round(preview.planned_taxi_kg).toLocaleString("de-DE")}`
      : null,
    preview && preview.planned_contingency_kg > 0
      ? `Conting. ${Math.round(preview.planned_contingency_kg).toLocaleString("de-DE")}`
      : null,
    preview && preview.planned_alternate_burn_kg > 0
      ? `Alt-Fuel ${Math.round(preview.planned_alternate_burn_kg).toLocaleString("de-DE")}`
      : null,
    preview && preview.planned_extra_kg > 0
      ? `Extra ${Math.round(preview.planned_extra_kg).toLocaleString("de-DE")}`
      : null,
  ].filter((v): v is string => v !== null);
  // Briefing-2a: OEW+Payload ergaenzen ZFW/LDW — zusammen ergibt
  // OEW+Payload exakt ZFW (Pilot-Feedback 2026-08-03, analog zur Fuel-
  // Aufschluesselung, wo Trip+Reserve+Taxi+... exakt Block ergibt).
  const weightRows = [
    plan && plan.planned_zfw_kg > 0
      ? `ZFW ${Math.round(plan.planned_zfw_kg).toLocaleString("de-DE")}`
      : null,
    plan && plan.planned_ldw_kg > 0
      ? `LDW ${Math.round(plan.planned_ldw_kg).toLocaleString("de-DE")}`
      : null,
    plan?.planned_oew_kg && plan.planned_oew_kg > 0
      ? `OEW ${Math.round(plan.planned_oew_kg).toLocaleString("de-DE")}`
      : null,
    plan?.planned_payload_kg && plan.planned_payload_kg > 0
      ? `Payload ${Math.round(plan.planned_payload_kg).toLocaleString("de-DE")}`
      : null,
  ].filter((v): v is string => v !== null);
  // Briefing-2a: Pax-Gewicht + Gepaeck ergaenzen die reine Fracht — analog
  // zu Fuel/Weights ergibt die Summe (Pax-Gewicht + Gepaeck + Fracht)
  // exakt Payload (Pilot-Feedback 2026-08-03). Einheitlich in KG, nicht
  // Cargo in Tonnen und Pax/Bags in KG gemischt (Pilot-Feedback 2026-08-03).
  const loadRows = [
    cargoKg > 0 ? `${t("bid_card.cargo_short")} ${Math.round(cargoKg).toLocaleString("de-DE")}` : null,
    plan?.planned_pax_weight_kg && plan.planned_pax_weight_kg > 0
      ? `Pax ${Math.round(plan.planned_pax_weight_kg).toLocaleString("de-DE")}`
      : null,
    plan?.planned_baggage_kg && plan.planned_baggage_kg > 0
      ? `Bags ${Math.round(plan.planned_baggage_kg).toLocaleString("de-DE")}`
      : null,
    aircraftType,
    registration,
  ].filter((v): v is string => v !== null);
  const hasRubrics = !ofpMismatch && (hasSimBriefId || hasAircraft || hasLoad);

  // Briefing-2a §5.3: WEIGHTS-Einordnung ist Pflicht sobald MTOW bekannt
  // ist (§21-Verify: `phpvmsaircraft.mtow` fliesst jetzt via
  // `phpvms_get_aircraft` → `aircraft.mtow_kg` durch). Sitzplatz-Total
  // fuer "PAX von N" bleibt bewusst weg — verifiziert (§21-Verify-Agent):
  // kein Sitzplatz-Feld in phpVMS-Core, PaxStudios `seating`-JSON ist
  // browser-session-only und ueber die Pilot-API nicht erreichbar.
  const towKg = plan && plan.planned_tow_kg > 0 ? Math.round(plan.planned_tow_kg) : null;
  // Briefing-2a-Korrektur (Pilot-Feedback 2026-08-03): der OPERATIONELLE
  // MTOW-Grenzwert aus dem OFP (bahnlaengen-/temperaturlimitiert fuer
  // DIESEN Flug) ist oft enger als der STRUKTURELLE Grenzwert aus
  // phpVMS' `aircraft.mtow` — der strukturelle Wert zeigte einen viel
  // groesseren, irrefuehrenden Puffer. Operationeller Wert hat Prioritaet,
  // struktureller bleibt Fallback (z.B. VFR ohne OFP).
  const mtowKg =
    plan?.max_tow_kg && plan.max_tow_kg > 0
      ? Math.round(plan.max_tow_kg)
      : aircraft?.mtow_kg && aircraft.mtow_kg > 0
        ? Math.round(aircraft.mtow_kg)
        : null;
  let weightsInset: string | null = null;
  let weightsDanger = false;
  if (towKg !== null && mtowKg !== null) {
    const diff = mtowKg - towKg;
    weightsInset =
      diff >= 0
        ? t("bid_card.tow_under_mtow", { n: diff.toLocaleString("de-DE") })
        : t("bid_card.tow_over_mtow", { n: Math.abs(diff).toLocaleString("de-DE") });
    weightsDanger = diff < 0;
  }

  const dpt = flight.dpt_airport?.icao ?? flight.dpt_airport_id;
  const arr = flight.arr_airport?.icao ?? flight.arr_airport_id;
  const dptName = flight.dpt_airport?.name ?? null;
  const arrName = flight.arr_airport?.name ?? null;
  const { icao: icaoCallsign, iata: iataCallsign } = splitCallsigns(flight);
  const airlineName = flight.airline?.name ?? null;
  const airlineLogo = flight.airline?.logo?.trim() || null;
  const cruiseLevel = formatLevel(flight.level);
  // Briefing-2a §8: STD/STA sind Pflichtfeld der Anzeige. phpVMS'
  // flight.dpt_time/arr_time hat Prioritaet; fehlt es (viele Test-/manuell
  // erstellte Buchungen haben kein Schedule-Feld gesetzt), faellt es auf
  // SimBrief's <times><sched_out>/<sched_in> zurueck (verifiziert per
  // Live-XML-Dump 2026-08-02) — erst wenn beides fehlt, "--:--".
  const dptTimeDisplay = flight.dpt_time ?? (preview?.sched_out_epoch != null ? formatEpochHm(preview.sched_out_epoch) : null);
  const arrTimeDisplay = flight.arr_time ?? (preview?.sched_in_epoch != null ? formatEpochHm(preview.sched_in_epoch) : null);
  const dptCountdown = flight.dpt_time
    ? stdCountdown(flight.dpt_time, nowMs)
    : preview?.sched_out_epoch != null
      ? epochCountdown(preview.sched_out_epoch, nowMs)
      : null;

  return (
    <article className="bid-card bid-card--main">
      <div className="bid-card__head">
        <div className="bid-card__brand">
          <div className={`bid-card__logo ${airlineLogo ? "" : "bid-card__logo--placeholder"}`}>
            {airlineLogo ? (
              <img src={airlineLogo} alt={airlineName ?? icaoCallsign ?? ""} />
            ) : (
              <span>{flight.airline?.icao ?? airlineMonogram(flight)}</span>
            )}
          </div>
          <div className="bid-card__title">
            <div className="bid-card__title-row">
              {isNextFlight && (
                <span className="bid-card__next-badge">{t("bid_card.next_badge")}</span>
              )}
              <span className="bid-card__callsign">{icaoCallsign}</span>
            </div>
            <span className="bid-card__airline-line">
              {[airlineName, iataCallsign].filter(Boolean).join(" · ")}
            </span>
          </div>
        </div>
        <div className="bid-card__meta">
          {flight.flight_type && <span>{flightTypeLabel(flight.flight_type)}</span>}
          <span>{formatDistanceNm(flight.distance?.nmi ?? null, i18n.language)}</span>
          <span>{formatFlightTime(flight.flight_time, i18n.language)}</span>
        </div>
      </div>

      <div className="bid-card__route-grid">
        <div className="bid-card__route-leg">
          <div className="bid-card__route-line1">
            <span className="bid-card__route-icao">{dpt}</span>
            <span className="bid-card__route-time">
              {dptTimeDisplay ? `${dptTimeDisplay}z` : "--:--"}
            </span>
          </div>
          <div className="bid-card__route-line2">
            {dptName && <span>{dptName}</span>}
            {dptCountdown && (
              <span className={dptCountdown.overdue ? "bid-card__route-countdown--overdue" : undefined}>
                {dptName && " · "}
                {/* Pilot-Feedback 2026-08-03: STD zeigt nur die Uhrzeit ohne
                    Datum ("11:55z") — faellt das OFP auf einen anderen
                    Kalendertag als "jetzt" (auch bei < 24h Differenz, z.B.
                    ueber Mitternacht hinweg), las sich "überfällig seit
                    1129 Min" neben der aktuellen Uhr wie ein Rechenfehler
                    (naive Kopfrechnung "jetzt minus 11:55" ergibt etwas
                    ganz anderes). Bei Tagessprung auf eine Tage-Formulierung
                    wechseln macht den Datumssprung explizit. */}
                {dptCountdown.crossesDay
                  ? dptCountdown.overdue
                    ? t("bid_card.overdue_days", { count: dptCountdown.dayDiff })
                    : t("bid_card.countdown_days", { count: dptCountdown.dayDiff })
                  : dptCountdown.overdue
                    ? t("bid_card.overdue", { n: dptCountdown.minutes })
                    : t("bid_card.countdown", { n: dptCountdown.minutes })}
              </span>
            )}
          </div>
        </div>

        <div className="bid-card__route-mid">
          <div className="bid-card__route-track" aria-hidden="true">
            <span className="bid-card__route-dot bid-card__route-dot--filled" />
            <span className="bid-card__route-tline" />
            {(aircraftType || registration) && (
              <span className="bid-card__route-type">
                {[aircraftType, registration].filter(Boolean).join(" · ")}
              </span>
            )}
            <span className="bid-card__route-tline" />
            <span className="bid-card__route-dot bid-card__route-dot--hollow" />
          </div>
          {!ofpMismatch && (plan?.alternate || cruiseLevel) && (
            <div className="bid-card__route-altn">
              {plan?.alternate && (
                <>
                  <span className="bid-card__route-altn-label">{t("bid_card.altn_label")}</span>{" "}
                  <span className="bid-card__route-altn-value">{plan.alternate}</span>
                </>
              )}
              {plan?.alternate && cruiseLevel && " · "}
              {cruiseLevel && <span>CRZ {cruiseLevel}</span>}
            </div>
          )}
        </div>

        <div className="bid-card__route-leg bid-card__route-leg--arrival">
          <div className="bid-card__route-line1">
            <span className="bid-card__route-time">
              {arrTimeDisplay ? `${arrTimeDisplay}z` : "--:--"}
            </span>
            <span className="bid-card__route-icao">{arr}</span>
          </div>
          <div className="bid-card__route-line2">{arrName}</div>
        </div>
      </div>

      {/* Briefing-2a-Haertung: solange der (einzige) OFP-Preview-Fetch noch
          laeuft, einen echten Ladezustand zeigen statt der Rubriken mit
          `plan=null` — sonst rendert kurz ein scheinbar fertiger, aber
          unvollstaendiger/irrefuehrender Zwischenstand (z.B. MTOW-Vergleich
          faellt auf den strukturellen statt operationellen Wert zurueck). */}
      {planLoading ? (
        <div className="bid-card__rubrics" aria-hidden="true">
          <div className="bid-card__rubric-loading" />
          <div className="bid-card__rubric-loading" />
          <div className="bid-card__rubric-loading" />
        </div>
      ) : (
        hasRubrics && (
          <div className="bid-card__rubrics">
            <Rubric
              title={t("bid_card.rubric_fuel")}
              unit="KG"
              headline={plan && plan.planned_block_fuel_kg > 0 ? Math.round(plan.planned_block_fuel_kg).toLocaleString("de-DE") : null}
              headlineLabel="Block"
              rows={fuelRows}
            />
            <Rubric
              title={t("bid_card.rubric_weights")}
              unit="KG"
              headline={towKg !== null ? towKg.toLocaleString("de-DE") : null}
              headlineLabel="TOW"
              inset={weightsInset}
              insetDanger={weightsDanger}
              rows={weightRows}
            />
            <Rubric
              title={t("bid_card.rubric_load")}
              headline={paxCount > 0 ? String(paxCount) : null}
              headlineLabel="PAX"
              inset={preview?.max_passengers ? t("bid_card.pax_of_total", { n: preview.max_passengers }) : null}
              rows={loadRows}
            />
          </div>
        )
      )}

      {!hasSimBriefId && (
        <div className="bid-card__ofp-mismatch bid-card__ofp-mismatch--info">
          <div className="bid-card__ofp-mismatch-text">
            <strong>{t("bids.no_ofp_title")}</strong>
            <span className="bid-card__ofp-mismatch-hint">{t("bids.no_ofp_hint")}</span>
          </div>
        </div>
      )}

      {ofpMismatch && plan && (
        <div className="bid-card__ofp-mismatch">
          <div className="bid-card__ofp-mismatch-text">
            <strong>{t("bids.ofp_mismatch_title")}</strong>
            <div className="bid-card__ofp-compare">
              <div>
                <span className="bid-card__ofp-compare-label">{t("bids.ofp_mismatch_bid")}</span>
                <code>
                  {fullBidCallsign || `${bidAirlineIcao ? `${bidAirlineIcao} ` : ""}${bidFnum}`}
                  {" · "}
                  {bidDpt} → {bidArr}
                  {bidAcIcao && ` · ${bidAcIcao}`}
                </code>
              </div>
              <div>
                <span className="bid-card__ofp-compare-label">{t("bids.ofp_mismatch_ofp")}</span>
                <code>
                  {plan.ofp_flight_number || "—"}
                  {plan.ofp_origin_icao && plan.ofp_destination_icao &&
                    ` · ${plan.ofp_origin_icao} → ${plan.ofp_destination_icao}`}
                  {ofpAcIcao && ` · ${ofpAcIcao}`}
                </code>
              </div>
            </div>
            <span className="bid-card__ofp-mismatch-hint">{t("bids.ofp_mismatch_hint")}</span>
          </div>
        </div>
      )}

      {!ofpMismatch && hasSimBriefId && (planLoading || (planError && !plan)) && (
        <div className="bid-card__simbrief-status">
          {planLoading && <span className="bid-card__simbrief-loading">{t("bids.simbrief_plan")} …</span>}
          {planError && !plan && <span className="bid-card__simbrief-error">{planError}</span>}
        </div>
      )}

      {hasRoute && (
        <div className="bid-card__route-text">
          <span className="bid-card__route-label">{t("bids.route")}</span>{" "}
          <code>{flight.route}</code>
        </div>
      )}

      <div className="bid-card__actions">
        <div className="bid-card__mode-toggle" role="group" aria-label={t("bid_card.mode_toggle_label")}>
          <button
            type="button"
            className={`bid-card__mode-btn ${mode === "ifr" ? "bid-card__mode-btn--active" : ""}`}
            onClick={() => setMode("ifr")}
            disabled={!hasSimBriefId}
            title={!hasSimBriefId ? t("bid_card.mode_ifr_needs_ofp") : undefined}
          >
            {t("bid_card.mode_ifr")}
          </button>
          <button
            type="button"
            className={`bid-card__mode-btn ${mode === "vfr" ? "bid-card__mode-btn--active" : ""}`}
            onClick={() => setMode("vfr")}
          >
            {t("bid_card.mode_vfr")}
          </button>
        </div>
        <span className="bid-card__mode-active-hint">{primaryHint}</span>
        <span className="bid-card__actions-spacer" />
        <div className="bid-card__links">
          {hasSimBriefId && (
            <button type="button" className="bid-card__link" onClick={onOfp}>
              {t("bids.open_ofp")}
            </button>
          )}
          <button type="button" className="bid-card__link" onClick={onFlightPage}>
            {t("bids.open_flight_page")}
          </button>
          <button
            type="button"
            className="bid-card__link"
            onClick={onWeather}
            title={t("bids.open_weather_briefing_hint")}
          >
            {t("bids.open_weather_briefing")}
          </button>
        </div>
        <button
          type="button"
          className="button button--primary bid-card__start"
          onClick={mode === "ifr" ? onStart : onManualStart}
          disabled={primaryDisabled}
          title={mode === "ifr" ? startTitle || undefined : undefined}
        >
          {starting ? t("bids.starting") : t("bid_card.start_flight")}
        </button>
      </div>

      {distanceToDptNm !== null && (
        <span
          className={`bid-card__distance ${tooFar ? "bid-card__distance--far" : "bid-card__distance--near"}`}
          title={t("bids.distance_to_departure", { airport: dptIcao })}
        >
          {distanceToDptNm < 1
            ? `< 1 nm ${t("bids.from_airport", { airport: dptIcao })}`
            : `${distanceToDptNm.toFixed(1)} nm ${t("bids.from_airport", { airport: dptIcao })}`}
        </span>
      )}

      {startError && (
        <p className="bid-card__start-error" role="alert">
          {startError}
        </p>
      )}
      {startWarning && (
        <div className="manual-modal__warning" role="alert" style={{ marginTop: 8 }}>
          <div className="manual-modal__warning-title">{t("manual_flight.warning_title")}</div>
          <div className="manual-modal__warning-text">{startWarning}</div>
          <div style={{ marginTop: 8 }}>
            <button
              type="button"
              className="button button--primary"
              onClick={onAcknowledgeMismatch}
              style={{ background: "#fbbf24", borderColor: "#fbbf24", color: "#1f1f1f" }}
            >
              {t("manual_flight.start_anyway")}
            </button>
          </div>
        </div>
      )}
    </article>
  );
}

interface RubricProps {
  /** Aviation-Rubrik-Titel — bewusst NICHT übersetzt (FUEL/WEIGHTS/LOAD
   *  sind wie ZFW/TOW/LDW feststehende Fachbegriffe). */
  title: string;
  unit?: string;
  /** Der EINE Leitwert der Rubrik (Block/TOW/PAX) — null blendet die
   *  Rubrik-Kachel komplett aus, außer wenn wenigstens ein Nebenwert da ist. */
  headline: string | null;
  headlineLabel: string;
  /** Briefing-2a §5.3: Einordnung neben dem Leitwert — bei WEIGHTS Pflicht
   *  sobald MTOW bekannt ist ("3.983 kg unter MTOW"). */
  inset?: string | null;
  insetDanger?: boolean;
  rows: string[];
}

/** Eine Rubrik-Kachel (Fuel/Weights/Load) — ein Leitwert + Nebenwerte
 *  statt sechs gleich großer Einzelkacheln. */
function Rubric({ title, unit, headline, headlineLabel, inset, insetDanger, rows }: RubricProps) {
  if (headline == null && rows.length === 0) return null;
  return (
    <div className="bid-card__rubric">
      <div className="bid-card__rubric-title">
        {title}
        {unit && <span className="bid-card__rubric-unit"> · {unit}</span>}
      </div>
      {headline != null && (
        <div className={`bid-card__rubric-headline ${insetDanger ? "bid-card__rubric-headline--danger" : ""}`}>
          {headline}
          <span className="bid-card__rubric-headline-label"> {headlineLabel}</span>
          {inset && <span className="bid-card__rubric-inset"> · {inset}</span>}
        </div>
      )}
      {rows.length > 0 && (
        <div className="bid-card__rubric-rows">
          {rows.map((row, i) => (
            <span key={i} className="bid-card__rubric-row">
              {row}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Auto-Start-Skip-Banner (v0.3.0).
 *
 * Pollt alle 3 s das Backend `auto_start_skip_status` und zeigt einen
 * gelben Banner mit der Begründung, wenn Auto-Start gerade nicht
 * greifen kann (z.B. Triebwerke an, Flugzeug rollt, in der Luft).
 * Liefert das Backend `null` (Auto-Start aus, oder Voraussetzungen
 * passen, oder älter als 10 s), wird kein Banner gerendert.
 *
 * Spiegelung der Activity-Log-Einträge die der Watcher schreibt —
 * der Pilot wartet im Briefing-Tab auf eine Meldung, schaut nicht
 * im Settings-Log nach.
 */
interface AutoStartSkipDto {
  reason: string;
  age_secs: number;
}

function AutoStartSkipBanner() {
  const { t } = useTranslation();
  const [skip, setSkip] = useState<AutoStartSkipDto | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const next = await invoke<AutoStartSkipDto | null>(
          "auto_start_skip_status",
        );
        if (!cancelled) setSkip(next);
      } catch {
        // IPC-Fehler beim Hot-Reload sind normal — ignorieren.
      }
    }
    void poll();
    const id = window.setInterval(poll, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  if (!skip) return null;

  // Reason-Code → lokalisiertes Label + Erklärung. Backend liefert nur
  // den Code (no_bid_match / missing_aircraft / engines_on / …); Frontend
  // macht die Übersetzung selbst, damit Theme/Sprache greift.
  // v0.15.13: defaultValue-Fallback, falls das Backend mal einen reason-Code
  // ohne i18n-Key liefert — vorher wurde dann der rohe Key angezeigt
  // (Live-Report Thomas: „bids.auto_start_skip.no_bid_match"). Jetzt erscheint
  // im Worst Case ein generischer Text statt des Schlüssels.
  const reasonKey = `bids.auto_start_skip.${skip.reason}`;
  const reasonText = t(reasonKey, {
    defaultValue: t("bids.auto_start_skip.generic"),
  });
  return (
    <div
      className="bids__auto-start-skip"
      role="status"
      aria-live="polite"
    >
      <div className="bids__auto-start-skip-text">
        <strong>{t("bids.auto_start_skip.title")}</strong>
        <span>{reasonText}</span>
      </div>
    </div>
  );
}
