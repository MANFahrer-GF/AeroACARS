import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { formatRefreshError } from "../lib/refreshErrorFormatter";
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

/// phpVMS-Flight-Type-Code → kurzes UI-Label.
/// Codes laut phpVMS-Core: J=Sched.Pax, F=Sched.Cargo, C=Charter,
/// X=Reposition, I=Special, T=Training, M=Military, R=Repositioning.
function flightTypeLabel(type: string): string {
  switch (type.toUpperCase()) {
    case "J": return "PAX";
    case "F": return "CARGO";
    case "C": return "CHARTER";
    case "X":
    case "R": return "REPO";
    case "T": return "TRAINING";
    case "M": return "MIL";
    case "I": return "SPECIAL";
    default: return type.toUpperCase();
  }
}

/// CSS-Class-Suffix abhängig vom Flight-Type — steuert die Badge-Farbe.
/// Pax = blau, Cargo = orange, Charter = lila, Repo = grau.
function flightTypeKind(type: string): string {
  switch (type.toUpperCase()) {
    case "J": return "pax";
    case "F": return "cargo";
    case "C": return "charter";
    case "X":
    case "R": return "repo";
    default: return "other";
  }
}

// v0.5.29: flightRulesHint() entfernt — Auto-Kategorisierung war zu eng.
// Pilot entscheidet selbst ob IFR oder VFR (= klare Hinweis-Text-Box
// statt Pill). Falls künftig wieder gebraucht, im git history (v0.5.28).

function buildCallsigns(flight: Flight): string {
  const icao = flight.airline?.icao?.trim();
  const iata = flight.airline?.iata?.trim();
  const fnum = flight.flight_number;
  const icaoCs = icao ? `${icao}${fnum}` : null;
  const iataCs = iata ? `${iata}${fnum}` : null;
  if (icaoCs && iataCs) return `${icaoCs} · ${iataCs}`;
  return icaoCs ?? iataCs ?? fnum;
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
    "✈"
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
  const [selectedId, setSelectedId] = useState<number | null>(null);
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
    cargo_kg: number;
    callsign_warning: { sb_callsign: string; active_callsigns: string } | null;
  }
  const [bidPreviews, setBidPreviews] = useState<Map<number, BidSimBriefPreview>>(
    new Map(),
  );
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
   *  beim Klick auf den 🌦-Button und informiert den Pilot, dass die GSG-
   *  Seite ihre Daten live holt → Browser-Tab kann bis 30 s laden. */
  const [weatherLoadHint, setWeatherLoadHint] = useState(false);
  /** Cached airport coords keyed by uppercase ICAO. */
  const [airports, setAirports] = useState<Record<string, AirportInfo>>({});
  /** Tracks ICAOs we've already requested so we don't fetch the same one twice. */
  const requestedIcaosRef = useRef<Set<string>>(new Set());

  const fetchBids = useCallback(async () => {
    try {
      const bids = await invoke<Bid[]>("phpvms_get_bids");
      setState(bids.length === 0 ? { kind: "empty" } : { kind: "ready", bids });
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
      const results = await Promise.allSettled(
        bids.map((b) =>
          invoke<BidSimBriefPreview>("bid_simbrief_preview", { bidId: b.id }),
        ),
      );
      const next = new Map<number, BidSimBriefPreview>();
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
          errorCount++;
        }
      });
      setBidPreviews(next);
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

  function handleSelect(bid: Bid) {
    const next = bid.id === selectedId ? null : bid.id;
    setSelectedId(next);
    onSelect?.(next === null ? null : bid);
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

  return (
    <section className="bids">
      <header className="bids__header">
        <h2>{t("bids.title")}</h2>
        <button
          type="button"
          className="bids__refresh"
          onClick={handleRefresh}
          disabled={refreshing || state.kind === "loading"}
          aria-label={t("bids.refresh")}
          title={t("bids.refresh")}
        >
          {refreshing ? "…" : "⟳"} <span>{t("bids.refresh")}</span>
        </button>
      </header>

      {/* v0.7.7 → v1.5.2: Notice nach Bid-Tab-Refresh. Text wird vom
          formatRefreshError-Helper geliefert (Error-Pfad) ODER vom
          inline-Render in handleRefresh (Success-Pfad mit changed=false).
          Auto-clear nach 6s. */}
      {/* v0.12.12-dev: 30-s-Lade-Hinweis fuer Wetter-Briefing — erscheint
          beim Klick auf den 🌦-Button und blendet sich nach 5 s aus. */}
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
          🌦 {t("bids.weather_briefing_load_hint")}
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
          {refreshNotice.tone === "warn"
            ? "⚠ "
            : refreshNotice.tone === "err"
              ? "✖ "
              : refreshNotice.tone === "ok"
                ? "✓ "
                : "ℹ︎ "}
          {refreshNotice.text}
        </div>
      )}

      {showPositionWarning && (
        <div
          className="bids__sim-position bids__sim-position--warn"
          role="status"
        >
          <span className="bids__sim-position-icon" aria-hidden="true">⚠️</span>
          <span className="bids__sim-position-message">
            {t("bids.sim_position_warning")}
          </span>
        </div>
      )}

      {/* v0.3.0: Auto-Start-Skip-Banner — zeigt warum Auto-Start
          gerade nicht greift, damit der Pilot nicht auf eine Meldung
          wartet die nie kommt. Pollt das Backend alle 3 s. */}
      <AutoStartSkipBanner />

      {state.kind === "loading" && <p className="bids__hint">{t("bids.loading")}</p>}

      {state.kind === "empty" && <p className="bids__hint">{t("bids.empty")}</p>}

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

      {state.kind === "ready" && (
        <ul className="bids__list">
          {state.bids.map((bid) => {
            const f = bid.flight;
            const dpt = f.dpt_airport?.icao ?? f.dpt_airport_id;
            const arr = f.arr_airport?.icao ?? f.arr_airport_id;
            const dptName = f.dpt_airport?.name ?? null;
            const arrName = f.arr_airport?.name ?? null;
            const callsign = buildCallsigns(f);
            const airlineName = f.airline?.name ?? null;
            const airlineLogo = f.airline?.logo?.trim() || null;
            const monogram = airlineMonogram(f);
            const level = formatLevel(f.level);
            const isSelected = bid.id === selectedId;

            // Compute distance from the aircraft to this bid's dpt airport (if
            // we have both pieces of info). Drives the proactive gating: the
            // Start button is enabled only when the aircraft is on the ground
            // and within MAX_START_DISTANCE_NM of the departure airport.
            const dptIcao = f.dpt_airport_id.trim().toUpperCase();
            const dptCoords = airports[dptIcao];
            // Defensive: a snapshot reporting EXACTLY 0,0 lat/lon is
            // never a real airport — both adapters return that as
            // their default uninitialised value, and the open ocean
            // off the African coast is the only real point matching
            // (no real flight starts there). Treat as "no position
            // yet" rather than computing a 5000-nm phantom distance.
            // Belt-and-braces alongside the backend stale-snapshot
            // clear (commits in adapter.rs) so a brief race window
            // between Connected-state and first-real-position can't
            // surface a wrong "too far" gate to the pilot.
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
              distanceToDptNm !== null &&
              distanceToDptNm > MAX_START_DISTANCE_NM;
            const noPositionYet =
              simState === "connected" && !hasRealPosition;

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

            return (
              <li key={bid.id}>
                <article
                  className={`bid-card ${isSelected ? "bid-card--selected" : ""}`}
                >
                  <button
                    type="button"
                    className="bid-card__body"
                    onClick={() => handleSelect(bid)}
                    aria-pressed={isSelected}
                    aria-expanded={isSelected}
                  >
                    <div className="bid-card__top">
                      <div className="bid-card__brand">
                        <div
                          className={`bid-card__logo ${
                            airlineLogo ? "" : "bid-card__logo--placeholder"
                          }`}
                        >
                          {airlineLogo ? (
                            <img src={airlineLogo} alt={airlineName ?? callsign} />
                          ) : (
                            <span>{monogram}</span>
                          )}
                        </div>
                        <div className="bid-card__title">
                          <span className="bid-card__callsign">{callsign}</span>
                          {airlineName && (
                            <span className="bid-card__airline">{airlineName}</span>
                          )}
                        </div>
                      </div>
                      <div className="bid-card__meta">
                        <span title={t("bids.flight_time")}>
                          ⏱ {formatFlightTime(f.flight_time, i18n.language)}
                        </span>
                        <span title={t("bids.distance")}>
                          📏 {formatDistanceNm(f.distance?.nmi ?? null, i18n.language)}
                        </span>
                        {level && (
                          <span title={t("bids.cruise_level")}>✈ {level}</span>
                        )}
                        {f.flight_type && (
                          <span
                            className={`bid-card__type-badge bid-card__type-badge--${flightTypeKind(f.flight_type)}`}
                            title={t("bids.flight_type")}
                          >
                            {flightTypeLabel(f.flight_type)}
                          </span>
                        )}
                        {/* v0.5.29: IFR/VFR-Auto-Detection-Pills entfernt
                            — Pilot entscheidet selbst, Auto-Kategorisierung
                            war zu eng. Hinweis steht jetzt als Text-Line
                            unter den Action-Buttons. */}
                      </div>
                    </div>

                    <div className="bid-card__route">
                      <div className="bid-card__leg">
                        <span className="bid-card__icao">{dpt}</span>
                        {dptName && (
                          <span className="bid-card__airport-name">{dptName}</span>
                        )}
                      </div>
                      <div className="bid-card__path" aria-hidden="true">
                        <span className="bid-card__plane">✈</span>
                      </div>
                      <div className="bid-card__leg bid-card__leg--arrival">
                        <span className="bid-card__icao">{arr}</span>
                        {arrName && (
                          <span className="bid-card__airport-name">{arrName}</span>
                        )}
                      </div>
                    </div>
                  </button>

                  {/* v0.3.0: Bid-Details (Aircraft + SimBrief-Plan + Route)
                      ZUERST — die wichtigen Plan-Werte vor den Action-
                      Buttons, damit der Pilot den Plan sieht bevor er auf
                      "Flug starten" klickt. Bei GSG kann der Pilot eh nur
                      einen Flug buchen, also immer sichtbar. */}
                  <BidDetails flight={f} bidId={bid.id} preview={bidPreviews.get(bid.id) ?? null} />
                  {isSelected && null}

                  {/* Action-Zeile am Ende — Flug starten / OFP / Flugseite.
                      Bewusst NACH den Plan-Werten, damit der Pilot zuerst
                      den OFP-Plan im Blick hat und dann entscheidet. */}
                  <div className="bid-card__actions">
                    <button
                      type="button"
                      className="button button--primary bid-card__start"
                      onClick={() => void startFlight(bid)}
                      disabled={startDisabled}
                      title={startTitle ?? "Standard-Flug nach IFR-Regeln, basiert auf deinem SimBrief-OFP. Block-Fuel, Route, Weights und Alternates kommen aus dem OFP."}
                    >
                      🛫 {startingId === bid.id
                        ? t("bids.starting")
                        : "IFR Start (SimBrief)"}
                    </button>
                    {distanceToDptNm !== null && (
                      <span
                        className={`bid-card__distance ${
                          tooFar ? "bid-card__distance--far" : "bid-card__distance--near"
                        }`}
                        title={t("bids.distance_to_departure", {
                          airport: dptIcao,
                        })}
                      >
                        {tooFar ? "✕ " : "✓ "}
                        {distanceToDptNm < 1
                          ? `< 1 nm ${t("bids.from_airport", { airport: dptIcao })}`
                          : `${distanceToDptNm.toFixed(1)} nm ${t("bids.from_airport", { airport: dptIcao })}`}
                      </span>
                    )}
                    {f.simbrief?.id && (
                      <button
                        type="button"
                        className="button"
                        onClick={() => void openOfp(f)}
                      >
                        {t("bids.open_ofp")} ↗
                      </button>
                    )}
                    <button
                      type="button"
                      className="button"
                      onClick={() => void openFlightPage(f)}
                    >
                      {t("bids.open_flight_page")} ↗
                    </button>
                    {/* v0.12.12-dev: GSG-Wetter-Briefing extern oeffnen.
                        Login-basiert — die Seite zieht den aktiven Bid
                        automatisch sobald der Pilot in phpVMS eingeloggt
                        ist (gleiche Logik wie OFP/Flugseite). Der Lade-
                        Hinweis erscheint per Toast beim Klick statt als
                        permanenter Schild. */}
                    <button
                      type="button"
                      className="button"
                      onClick={() => {
                        setWeatherLoadHint(true);
                        window.setTimeout(() => setWeatherLoadHint(false), 5000);
                        void openUrl("https://german-sky-group.eu/weatherbriefing").catch(() => {});
                      }}
                      title={t("bids.open_weather_briefing_hint")}
                    >
                      🌦 {t("bids.open_weather_briefing")} ↗
                    </button>
                    {/* v0.5.27 VFR/Manual-Mode-Button: immer verfuegbar
                        wenn kein aktiver Flug laeuft. Pilot entscheidet
                        ob er IFR (oben mit SB) oder VFR (hier manuell)
                        fliegen will — keine harte Enforcement.
                        v0.5.28: konsistenter Label "VFR Start (manuell)"
                        unabhaengig ob SB existiert. */}
                    {hasActiveFlight ? null : (
                      <button
                        type="button"
                        className="button"
                        onClick={() => setManualModalBid(bid)}
                        title={t("bid_card.vfr_start_tooltip")}
                      >
                        {t("bid_card.vfr_start")}
                      </button>
                    )}
                  </div>
                  {/* v0.5.31: Klare Regel-Erklaerung statt
                      Marketing-Sprech. IFR = SB-Pflicht, VFR = SB-frei. */}
                  {!hasActiveFlight && (
                    <div className="bid-card__mode-hint">
                      <div className="bid-card__mode-hint-title">
                        {t("bid_card.mode_hint_title")}
                      </div>
                      <div className="bid-card__mode-hint-row bid-card__mode-hint-row--ifr">
                        <span className="bid-card__mode-hint-icon">🛫</span>
                        <span className="bid-card__mode-hint-key">{t("bid_card.mode_hint_ifr_key")}</span>
                        <span className="bid-card__mode-hint-rule">
                          <strong>{t("bid_card.mode_hint_ifr_rule_strong")}</strong>{t("bid_card.mode_hint_ifr_rule_rest")}
                        </span>
                      </div>
                      <div className="bid-card__mode-hint-row bid-card__mode-hint-row--vfr">
                        <span className="bid-card__mode-hint-icon">🛩</span>
                        <span className="bid-card__mode-hint-key">{t("bid_card.mode_hint_vfr_key")}</span>
                        <span className="bid-card__mode-hint-rule">
                          <strong>{t("bid_card.mode_hint_vfr_rule_strong")}</strong>{t("bid_card.mode_hint_vfr_rule_rest")}
                        </span>
                      </div>
                    </div>
                  )}

                  {startError?.bidId === bid.id && (
                    <p className="bid-card__start-error" role="alert">
                      {startError.message}
                    </p>
                  )}
                  {/* v0.8.3 (#7): Wetlease-/Mismatch-Warning mit
                      Override-Button (gelb) — analog ManualFlightModal. */}
                  {startWarning?.bidId === bid.id && (
                    <div className="manual-modal__warning" role="alert" style={{ marginTop: 8 }}>
                      <div className="manual-modal__warning-title">
                        {t("manual_flight.warning_title")}
                      </div>
                      <div className="manual-modal__warning-text">
                        {startWarning.message}
                      </div>
                      <div style={{ marginTop: 8 }}>
                        <button
                          type="button"
                          className="button button--primary"
                          onClick={() => void startFlight(bid, true)}
                          disabled={startingId !== null || hasActiveFlight}
                          style={{ background: "#fbbf24", borderColor: "#fbbf24", color: "#1f1f1f" }}
                        >
                          {t("manual_flight.start_anyway")}
                        </button>
                      </div>
                    </div>
                  )}
                </article>
              </li>
            );
          })}
        </ul>
      )}

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
 * Ausgeklappte Bid-Card-Details (v0.3.0):
 * - Aircraft-Info aus SimBrief-Subfleet (B738 · Boeing 737-800)
 * - Pax/Cargo-Load aus den fares
 * - Route-String (kommt aus phpVMS-Bid)
 * - SimBrief-Plan-Vorschau (Block-Fuel, Trip-Burn, TOW, LDW, Reserve,
 *   ZFW, Alternate) wird per `fetch_simbrief_preview` Tauri-Command
 *   geholt sobald die Card ausgeklappt wird. Lädt asynchron, kein
 *   Blocking — Pilot sieht erstmal die Route, der Plan-Block flutscht
 *   in 1-2s nach.
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
  cargo_kg: number;
  callsign_warning: { sb_callsign: string; active_callsigns: string } | null;
}

function BidDetails({
  flight,
  bidId: _bidId,
  preview,
}: {
  flight: Flight;
  bidId: number;
  preview: BidSimBriefPreviewProp | null;
}) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<SimBriefOfp | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [planLoading, setPlanLoading] = useState(false);
  // v0.3.0: Aircraft-Reg — wenn die simbrief.aircraft_id vorhanden ist,
  // holen wir die konkrete Registrierung (z.B. "EI-ENI") über einen
  // separaten phpVMS-API-Call. Subfleet alleine sagt nur die Type-Klasse,
  // nicht das konkrete Flugzeug.
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
        max_tow_kg: 0,
        max_ldw_kg: 0,
        request_id: preview.request_id,
        // v0.7.12: Pax/Cargo aus dem OFP-XML — fuer die Bid-Card-Chips
        // wenn die phpVMS-Bid-Pointer-Subfleet-Fares leer sind.
        pax_count: preview.pax_count,
        cargo_kg: preview.cargo_kg,
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
  const cargoKg = preview && preview.cargo_kg > 0 ? preview.cargo_kg : faresCargoKg;

  const aircraftType = flight.simbrief?.subfleet?.type_;
  const aircraftName = flight.simbrief?.subfleet?.name;

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
  const ofpAcIcao = aircraftType?.toUpperCase().trim();
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

  // Wenn weder Aircraft-Info noch SimBrief-Plan noch Route da ist,
  // rendern wir die Sektion gar nicht (würde sonst leer aussehen).
  const hasAircraft = !!(aircraftType || aircraftName);
  const hasLoad = paxCount > 0 || cargoKg > 0;
  const hasSimBriefId = !!flight.simbrief?.id;
  const hasRoute = !!flight.route;
  if (!hasAircraft && !hasLoad && !hasSimBriefId && !hasRoute) return null;

  return (
    <div className="bid-card__details">
      {/* v0.3.0: Bei OFP-Mismatch sind ALLE OFP-Werte unzuverlässig
          (Aircraft/Subfleet, Pax/Cargo aus den Fares, Plan-Block-
          Fuel/TOW etc.). Wir blenden sie aus, um keine falschen
          Werte zu zeigen — nur das Banner + die phpVMS-eigene Route
          bleiben sichtbar. Pilot kennt seine Buchung, AeroACARS hat
          nichts Verlässliches zu zeigen bis ein neuer OFP da ist. */}
      {/* Header: Aircraft-Info + Load-Chips in einer Zeile */}
      {!ofpMismatch && (hasAircraft || hasLoad) && (
        <div className="bid-card__aircraft-row">
          {hasAircraft && (
            <div className="bid-card__aircraft">
              <span className="bid-card__detail-label">
                {t("bids.aircraft")}
              </span>
              {aircraftType && <code>{aircraftType}</code>}
              {aircraftName && (
                <span className="bid-card__aircraft-name">{aircraftName}</span>
              )}
              {/* v0.3.0: Konkrete Registrierung wenn verfügbar — z.B.
                  "RYR-B738-WL · Boeing 737-800 · EI-ENI" */}
              {aircraft?.registration && (
                <span className="bid-card__aircraft-reg">
                  {aircraft.registration}
                </span>
              )}
            </div>
          )}
          {hasLoad && (
            <div className="bid-card__load">
              {paxCount > 0 && (
                <span className="bid-card__load-chip bid-card__load-chip--pax">
                  👥 {paxCount} PAX
                </span>
              )}
              {cargoKg > 0 && (
                <span className="bid-card__load-chip bid-card__load-chip--cargo">
                  📦 {(cargoKg / 1000).toFixed(1)} t Cargo
                </span>
              )}
            </div>
          )}
        </div>
      )}

      {/* v0.3.3: Wenn der Bid noch GAR KEINEN SimBrief-OFP gebunden hat,
          klarer Hinweis statt einfach nichts zu rendern. Vorher rätselte
          der Pilot warum die Plan-Cards leer sind. */}
      {!hasSimBriefId && (
        <div className="bid-card__ofp-mismatch bid-card__ofp-mismatch--info">
          <span className="bid-card__ofp-mismatch-icon">ℹ️</span>
          <div className="bid-card__ofp-mismatch-text">
            <strong>{t("bids.no_ofp_title")}</strong>
            <span className="bid-card__ofp-mismatch-hint">
              {t("bids.no_ofp_hint")}
            </span>
          </div>
        </div>
      )}

      {/* v0.3.0: OFP-Mismatch-Warnung. SimBrief liefert immer den
          letzten OFP des Pilot-Accounts — wenn der zur aktuellen
          Buchung passt, alles gut. Wenn nicht (Aircraft / Route /
          Flightnumber abweichen), zeigen wir einen klaren Vergleich
          und die Anleitung zum Reparieren. */}
      {ofpMismatch && plan && (
        <div className="bid-card__ofp-mismatch">
          <span className="bid-card__ofp-mismatch-icon">⚠</span>
          <div className="bid-card__ofp-mismatch-text">
            <strong>{t("bids.ofp_mismatch_title")}</strong>
            <div className="bid-card__ofp-compare">
              <div>
                <span className="bid-card__ofp-compare-label">
                  {t("bids.ofp_mismatch_bid")}
                </span>
                <code>
                  {/* Bevorzugt ATC-Callsign mit Airline-Prefix
                      (z.B. "RYR4TK"). phpVMS speichert oft nur den
                      Suffix, deshalb ergänzen wir den Prefix in
                      `fullBidCallsign`. Sonst Airline-ICAO + Flight-
                      Number als Standardformat. */}
                  {fullBidCallsign
                    || `${bidAirlineIcao ? `${bidAirlineIcao} ` : ""}${bidFnum}`}
                  {" · "}
                  {bidDpt} → {bidArr}
                  {bidAcIcao && ` · ${bidAcIcao}`}
                </code>
              </div>
              <div>
                <span className="bid-card__ofp-compare-label">
                  {t("bids.ofp_mismatch_ofp")}
                </span>
                <code>
                  {plan.ofp_flight_number || "—"}
                  {plan.ofp_origin_icao && plan.ofp_destination_icao &&
                    ` · ${plan.ofp_origin_icao} → ${plan.ofp_destination_icao}`}
                  {ofpAcIcao && ` · ${ofpAcIcao}`}
                </code>
              </div>
            </div>
            <span className="bid-card__ofp-mismatch-hint">
              {t("bids.ofp_mismatch_hint")}
            </span>
          </div>
        </div>
      )}

      {/* SimBrief-Plan: Card-Grid mit großen Werten — bei Mismatch
          NICHT zeigen (siehe oben Kommentar). */}
      {!ofpMismatch && hasSimBriefId && (
        <div className="bid-card__simbrief">
          <div className="bid-card__simbrief-header">
            <span>📋 {t("bids.simbrief_plan")}</span>
            {planLoading && <span className="bid-card__simbrief-loading">…</span>}
          </div>
          {plan && (
            <div className="bid-card__simbrief-cards">
              {/* Reihenfolge nach EFB-Konvention:
                  Fuel-Block: Block → Trip → Reserve
                  Weight-Block (mathematisch ZFW + Block - Taxi = TOW,
                  TOW - Trip = LDW): ZFW → TOW → LDW
                  Alt am Ende */}
              <PlanCard
                label="Block"
                kg={plan.planned_block_fuel_kg}
                accent="primary"
              />
              <PlanCard
                label="Trip"
                kg={plan.planned_burn_kg}
                accent="primary"
              />
              <PlanCard label="Reserve" kg={plan.planned_reserve_kg} />
              <PlanCard label="ZFW" kg={plan.planned_zfw_kg} accent="weight" />
              <PlanCard label="TOW" kg={plan.planned_tow_kg} accent="weight" />
              <PlanCard label="LDW" kg={plan.planned_ldw_kg} accent="weight" />
              {plan.alternate && (
                <div className="bid-card__plan-card bid-card__plan-card--alt">
                  <span className="bid-card__plan-card-label">Alt</span>
                  <span className="bid-card__plan-card-value">
                    {plan.alternate}
                  </span>
                </div>
              )}
            </div>
          )}
          {planError && !plan && (
            <div className="bid-card__simbrief-error">{planError}</div>
          )}
        </div>
      )}

      {/* Route-String aus phpVMS-Bid */}
      {hasRoute && (
        <div className="bid-card__route-text">
          <span className="bid-card__route-label">{t("bids.route")}</span>{" "}
          <code>{flight.route}</code>
        </div>
      )}
    </div>
  );
}

interface PlanCardProps {
  label: string;
  kg: number;
  /** Visual emphasis. `primary` = Block/Trip (wichtigste), `weight` = TOW/LDW/
   *  ZFW, default = sonst (Reserve). Treibt nur Farbe, kein Layout. */
  accent?: "primary" | "weight";
}

function PlanCard({ label, kg, accent }: PlanCardProps) {
  if (kg <= 0) return null;
  const accentClass =
    accent === "primary"
      ? "bid-card__plan-card--primary"
      : accent === "weight"
        ? "bid-card__plan-card--weight"
        : "";
  return (
    <div className={`bid-card__plan-card ${accentClass}`}>
      <span className="bid-card__plan-card-label">{label}</span>
      <span className="bid-card__plan-card-value">
        {Math.round(kg).toLocaleString("de-DE")}
        <span className="bid-card__plan-card-unit"> kg</span>
      </span>
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
      <span className="bids__auto-start-skip-icon">🤖</span>
      <div className="bids__auto-start-skip-text">
        <strong>{t("bids.auto_start_skip.title")}</strong>
        <span>{reasonText}</span>
      </div>
    </div>
  );
}
