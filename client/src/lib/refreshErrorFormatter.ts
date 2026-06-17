/**
 * v0.7.8 v1.5.2: Shared Formatter fuer `flight_refresh_simbrief`-Fehler.
 *
 * Drei Call-Sites rufen heute `flight_refresh_simbrief`:
 *   1. BidsList "Aktualisieren"-Button (Bid-Tab)
 *   2. ActiveFlightPanel "OFP refreshen"-Button (Cockpit-Tab)
 *   3. LoadsheetMonitor inline-Refresh-Button (v0.5.46 Adrian-Fix)
 *
 * Ohne diesen gemeinsamen Helper wuerde Cockpit + Loadsheet bei einem
 * Mismatch-Fehler einfach `err.message` anzeigen — was bei v0.7.8
 * structured-JSON-Details (siehe `lib.rs` flight_refresh_simbrief
 * Mismatch-Handler) bedeuten wuerde dass der Pilot rohes JSON sieht
 * wie `{"active_callsigns":"CFG1504","active_dpt":"EDDF",...}`.
 *
 * Dieser Helper macht aus der JSON-Encoded UiError eine lesbare
 * lokalisierte Pilot-Notice. Spec docs/spec/ofp-refresh-simbrief-direct-
 * v0.7.8.md §8 + Thomas-QS v1.5.1.
 */

import type { TFunction } from "i18next";

/** Tauri-UiError-Shape — `code` + `message` Tupel. */
export interface TauriRefreshError {
  code?: string;
  message?: string;
}

export interface FormattedNotice {
  /** Lokalisierter Pilot-lesbarer Text. */
  text: string;
  /** Notice-Tonalitaet — Frontend rendert das passend (rot/gelb/blau). */
  tone: "info" | "warn" | "err";
}

/**
 * Welcher Aufrufer ruft den Helper? Bestimmt das Verhalten fuer
 * benigne Codes (`phase_locked` + `no_simbrief_link`):
 *
 * - **bidlist**: still (null) — der Bid-Tab-"Aktualisieren"-Button
 *   loest auch in Cruise/etc. mit den anderen Refreshes (Bids,
 *   Sim-Resync, Profile) aus; `phase_locked` ist erwartet und soll
 *   keinen Notice produzieren. Genauso ist `no_simbrief_link` da
 *   Pilot-spezifisch normal.
 *
 * - **cockpit**: lesbare Notice — der Cockpit-/Loadsheet-Refresh-
 *   Button ist Pilot-initiiert. Wenn er aus Race-Conditions in
 *   `phase_locked` rennt oder fuer einen Bid ohne SimBrief klickt,
 *   muss der Pilot eine vernuenftige Antwort sehen statt
 *   `[object Object]` oder einer schweigenden UI.
 */
export type RefreshErrorContext = "bidlist" | "cockpit";

/**
 * Formatiert einen `flight_refresh_simbrief`-Error in eine lokalisierte
 * Pilot-Notice. Bekannte error.codes werden ueber i18n-Templates mit
 * Parametern gerendert; unbekannte Codes fallen auf den `err.message`
 * (= raw) zurueck.
 *
 * **Wichtig:** Diese Funktion ist die EINZIGE Stelle die JSON-Details
 * aus `err.message` parsed. Wenn das JSON-Encoding-Format im Backend
 * mal aendert, ist hier der zentrale Anpassungspunkt — nicht ueber
 * 3 Komponenten verteilt.
 */
export function formatRefreshError(
  err: TauriRefreshError | null | undefined,
  t: TFunction,
  context: RefreshErrorContext = "bidlist",
): FormattedNotice | null {
  if (!err) return null;

  // ─── ofp_does_not_match_active_flight ──────────────────────────────
  // Backend kodiert active_callsigns/dpt/arr + sb_callsign/origin/dest
  // als JSON im message-Feld.
  if (err.code === "ofp_does_not_match_active_flight") {
    // v1.5.2: active_callsigns ist eine "/" -getrennte Liste aller
    // validen Kandidaten (z.B. "CFG1504 / CFG4TK / 4TK"), gebaut vom
    // Backend via build_candidate_callsigns. Pilot sieht damit ALLE
    // valid-Formen statt nur airline+flight_number.
    const defaults: Record<string, string> = {
      active_callsigns: "—",
      active_dpt: "—",
      active_arr: "—",
      sb_callsign: "—",
      sb_origin: "—",
      sb_dest: "—",
    };
    const params: Record<string, string> = { ...defaults };
    if (err.message) {
      try {
        const parsed = JSON.parse(err.message) as Record<string, unknown>;
        for (const k of Object.keys(defaults)) {
          const v = parsed[k];
          if (typeof v === "string" && v.length > 0) params[k] = v;
        }
      } catch {
        // JSON-Parse-Fehler → Defaults ("—") bleiben. Pilot sieht
        // unspezifischen Notice, kein Crash, kein rohes JSON.
      }
    }
    return {
      text: t("bids.ofp_does_not_match_active_flight", params),
      tone: "warn",
    };
  }

  // ─── Bekannte SimBrief-direct-Error-Codes ──────────────────────────
  // Alle haben i18n-Keys ohne Parameter (= einfache Strings).
  const KNOWN_CODES: Record<string, { key: string; tone: FormattedNotice["tone"] }> = {
    simbrief_user_not_found: { key: "bids.simbrief_user_not_found", tone: "warn" },
    simbrief_unavailable_and_bid_gone: {
      key: "bids.simbrief_unavailable_and_bid_gone",
      tone: "warn",
    },
    simbrief_direct_failed: { key: "bids.simbrief_direct_failed", tone: "warn" },
    bid_not_found: { key: "bids.ofp_bid_gone", tone: "warn" },
    // v0.16.23: flight_refresh_route_only ohne konfigurierten SimBrief-
    // Identifier. Actionable Notice — der Pilot muss seinen SimBrief-
    // Username in Settings hinterlegen damit Route-Sync den AKTUELLEN
    // OFP direkt ziehen kann (der Pointer-Pfad würde nur die stale Route
    // liefern, deshalb gibt es hier keinen Fallback).
    no_simbrief_identifier: { key: "bids.no_simbrief_identifier", tone: "warn" },
    // v0.16.23: SimBrief gerade nicht erreichbar (Route-Sync, kein
    // Pointer-Fallback). Eigener Code damit der Notice "versuch's gleich
    // nochmal" statt "Bid weg" sagt.
    simbrief_unavailable: {
      key: "bids.simbrief_unavailable_and_bid_gone",
      tone: "warn",
    },
  };
  if (err.code && KNOWN_CODES[err.code]) {
    const { key, tone } = KNOWN_CODES[err.code];
    return { text: t(key), tone };
  }

  // ─── phase_locked + no_simbrief_link ───────────────────────────────
  //
  // v1.5.3 (Thomas-QS P2): Kontext-abhaengig.
  //
  // - BidsList: still (= kein Notice). Refresh kombiniert mehrere
  //   unabhaengige Calls (Bids, Sim, Profile, evtl. OFP). `phase_locked`
  //   ist ein erwarteter benigner Outcome in spaeteren Flugphasen — die
  //   Bid-Liste wird trotzdem aktualisiert, also kein User-Notice noetig.
  //   Gleiches fuer `no_simbrief_link`.
  //
  // - Cockpit/Loadsheet: lesbare Notice. Der Pilot hat den Refresh-
  //   Button explizit gedrueckt. `phase_locked` koennte ueber Race-
  //   Condition (Phase wechselt waehrend Button-Click) auftreten;
  //   `no_simbrief_link` kann real passieren weil der Cockpit-Button
  //   NICHT abhaengig vom SimBrief-Link gegated ist. Pilot muss
  //   eine lesbare Antwort kriegen, sonst sieht er sonst `[object
  //   Object]` aus dem String(err)-Fallback im Caller.
  if (err.code === "phase_locked") {
    if (context === "bidlist") return null;
    return { text: t("bids.phase_locked"), tone: "info" };
  }
  if (err.code === "no_simbrief_link") {
    if (context === "bidlist") return null;
    return { text: t("bids.no_simbrief_link"), tone: "info" };
  }
  // v0.7.10: no_active_flight — Pilot drueckte "Aktualisieren" im Bid-Tab
  // OHNE einen aktiven Flug. SimBrief-OFP-Refresh ist im aktuellen
  // Backend nur waehrend Boarding/Preflight verfuegbar (eigener Bid-
  // Preview kommt in v0.7.11). Pilot bekommt klaren Hinweis statt
  // generischem "Refresh ohne Antwort" oder silent fail.
  if (err.code === "no_active_flight") {
    return { text: t("bids.no_active_flight_hint"), tone: "info" };
  }

  // ─── Unbekannte Codes: rohe message als err-Tone ───────────────────
  // Letzter Fallback — sollte nur fuer interne Bugs / unerwartete
  // Backend-Errors greifen.
  if (err.message && err.message.length > 0) {
    return { text: err.message, tone: "err" };
  }
  return null;
}
