// v1.3.5 (#Datalink-3a) — PDC/CPDLC tab root.
//
// Datalink is a radio exchange, not a form: the message that comes back
// from the station is the point, not the sending. See
// handoff_datalink_3a/README.md §0 for the four decisions this rebuild
// carries through — two fixed columns (composer/history), the uplink
// parsed into large values, PDC/CPDLC as one connection with a shared
// history (only the composer swaps), and replies sitting at the message
// they answer.
//
// This shell owns exactly what's shared across both modes: the 66px
// connection status row (ACARS link, callsign) and the two-column
// workspace grid. Composer and history each own their own concerns.
//
// CPDLC logon STATE lives here (this is where `online`/`loggedOn` are
// already computed, and DatalinkHistory needs the station too), but its
// UI moved into DatalinkComposer (only rendered in CPDLC mode) — it used
// to sit dimmed-but-visible in this status row for both modes, which
// crammed variable-width, CPDLC-only content into a row that also has to
// carry things that are ALWAYS relevant. That one row produced three
// separate real layout bugs in a single afternoon; see the comment at
// the logon block's new home in DatalinkComposer.tsx for the full list.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { useCpdlcMessages } from "../hooks/useCpdlcMessages";
import { useStationOnline } from "../hooks/useStationOnline";
import { DatalinkComposer } from "./DatalinkComposer";
import { DatalinkHistory } from "./DatalinkHistory";

interface HoppieSettings {
  enabled: boolean;
  callsign_override: string | null;
  notify_sound: boolean;
}

interface HoppieStatus {
  connected: boolean;
  logged_on: boolean;
  pending_response_count: number;
  pending_uplink_count: number;
  last_error: string | null;
  station_id: string | null;
  logon_pending: boolean;
  logon_timed_out: boolean;
}

interface FlightContext {
  callsign: string | null;
}

const STATUS_POLL_MS = 5000;

/** How long the logon button stays blocked after a request went out — a
 *  double-click guard, not a timeout. See the CPDLC logon block below;
 *  ported unchanged from the previous CpdlcView. */
const LOGON_COOLDOWN_SECS = 90;

/** Persists a typed-but-not-yet-sent centre across tab switches and panel
 *  remounts — same reasoning as the previous CpdlcView draft key.
 *
 *  v1.3.5 fix: this used to live in `localStorage`, which never expires and
 *  was never cleared once the draft was actually sent — a station typed in
 *  ONE session (verified: "EDDF", almost certainly typed during this
 *  session's own testing of the Datalink-3a rewrite) kept resurfacing on
 *  every app start indefinitely, forcing the pilot to delete it every time.
 *  `sessionStorage` still survives a tab switch/remount within one running
 *  app instance (the original point of this cache) but is gone on the next
 *  launch. Also now cleared the moment a logon request actually goes out
 *  (see `sendLogon` below) — at that point the field's job (holding an
 *  unsent draft) is done. */
const STATION_DRAFT_KEY = "aeroacars.cpdlc.station_draft";

function loadStationDraft(): string {
  try {
    return sessionStorage.getItem(STATION_DRAFT_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveStationDraft(value: string): void {
  try {
    if (value) sessionStorage.setItem(STATION_DRAFT_KEY, value);
    else sessionStorage.removeItem(STATION_DRAFT_KEY);
  } catch {
    // Private-mode / quota — falls back to per-mount behaviour, no worse
    // than before this existed.
  }
}

/** Clears the PERSISTED draft only — leaves `stationInput` itself alone, so
 *  the just-typed station stays visible through the cooldown/pending
 *  period. Called once a logon request actually goes out. */
function clearStationDraft(): void {
  try {
    sessionStorage.removeItem(STATION_DRAFT_KEY);
  } catch {
    // Nothing to clean up if storage isn't available in the first place.
  }
}

export type DatalinkMode = "pdc" | "cpdlc";

interface Props {
  onOpenSettings: () => void;
}

export function CpdlcPanel({ onOpenSettings }: Props) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<HoppieSettings | null>(null);
  const [status, setStatus] = useState<HoppieStatus | null>(null);
  const [flightCallsign, setFlightCallsign] = useState<string | null>(null);

  const [callsignInput, setCallsignInput] = useState("");
  const [callsignDirty, setCallsignDirty] = useState(false);
  const [callsignEditing, setCallsignEditing] = useState(false);

  const [mode, setMode] = useState<DatalinkMode>("pdc");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Mirrors DatalinkComposer's PDC recipient field — see the prop doc on
  // DatalinkComposer for why the history needs a copy of it.
  const [pdcRecipient, setPdcRecipient] = useState("");

  // --- CPDLC logon: state lives here, UI rendered by DatalinkComposer
  // (CPDLC mode only — see that file). ---
  const [stationInput, setStationInputState] = useState(() => loadStationDraft());
  const setStationInput = (value: string) => {
    setStationInputState(value);
    saveStationDraft(value);
  };
  const [stationEditing, setStationEditing] = useState(false);
  const [logonBusy, setLogonBusy] = useState(false);
  const [logonError, setLogonError] = useState<string | null>(null);
  const [cooldownLeft, setCooldownLeft] = useState(0);
  const cooldownRunning = cooldownLeft > 0;

  useEffect(() => {
    if (!cooldownRunning) return;
    const id = window.setInterval(() => setCooldownLeft((s) => Math.max(0, s - 1)), 1000);
    return () => window.clearInterval(id);
  }, [cooldownRunning]);

  useEffect(() => {
    void invoke<HoppieSettings>("hoppie_get_settings").then((s) => {
      setSettings(s);
      setCallsignInput(s.callsign_override ?? "");
    });
    void invoke<FlightContext>("hoppie_get_flight_context").then((ctx) =>
      setFlightCallsign(ctx.callsign),
    );
  }, []);

  const refreshStatus = () => {
    void invoke<HoppieStatus>("hoppie_status").then(setStatus).catch(() => undefined);
  };

  useEffect(() => {
    if (!settings?.enabled) return;
    refreshStatus();
    const id = window.setInterval(refreshStatus, STATUS_POLL_MS);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings?.enabled]);

  const online = Boolean(status?.connected);
  const { messages, refresh: refreshMessages, lastFetchedAt } = useCpdlcMessages(online);
  const cpdlcStationStatus = useStationOnline(stationInput, online);

  // "Letzte Abfrage vor {n} s" — ticks every second, jumps to 0 the
  // moment a poll actually lands (README §2, right-hand readout).
  const [pollAgeSec, setPollAgeSec] = useState(0);
  useEffect(() => {
    if (lastFetchedAt == null) return;
    setPollAgeSec(0);
    const id = window.setInterval(() => {
      setPollAgeSec(Math.max(0, Math.round((Date.now() - lastFetchedAt) / 1000)));
    }, 1000);
    return () => window.clearInterval(id);
  }, [lastFetchedAt]);

  if (!settings) return null;

  if (!settings.enabled) {
    return (
      <section className="cpdlc-panel cpdlc-panel--disabled">
        <p>{t("cpdlc.disabled_hint")}</p>
        <button type="button" className="button button--primary" onClick={onOpenSettings}>
          {t("cpdlc.disabled_open_settings")}
        </button>
      </section>
    );
  }

  /// Blur-save. Deliberately does NOT touch `busy` — see the equivalent
  /// note that used to live here before the rebuild: flipping it meant
  /// the very click that caused the blur landed on a disabled button.
  const saveCallsign = async () => {
    setCallsignEditing(false);
    if (!callsignDirty) return;
    const trimmed = callsignInput.trim().toUpperCase();
    try {
      const next = { ...settings, callsign_override: trimmed === "" ? null : trimmed };
      await invoke<HoppieSettings>("hoppie_set_settings", { settings: next });
      setSettings(next);
      setCallsignInput(trimmed);
      setCallsignDirty(false);
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  const toggleLink = async () => {
    setBusy(true);
    setError(null);
    try {
      if (online) {
        setStatus(await invoke<HoppieStatus>("hoppie_disconnect"));
        return;
      }
      // Persist a freshly typed callsign BEFORE connecting — blur-saving
      // alone races the click (backend would read the old, usually
      // empty, value and refuse with "no callsign configured").
      const trimmed = callsignInput.trim().toUpperCase();
      if (callsignDirty) {
        const next = { ...settings, callsign_override: trimmed === "" ? null : trimmed };
        await invoke<HoppieSettings>("hoppie_set_settings", { settings: next });
        setSettings(next);
        setCallsignInput(trimmed);
        setCallsignDirty(false);
      }
      setStatus(await invoke<HoppieStatus>("hoppie_connect"));
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const onChanged = () => {
    refreshMessages();
    refreshStatus();
  };

  const loggedOn = Boolean(status?.logged_on);
  const logonSent = Boolean(status?.logon_pending);
  const logonTimedOut = Boolean(status?.logon_timed_out);
  const currentStation = status?.station_id ?? null;
  const alreadyOn =
    loggedOn && currentStation !== null && stationInput.trim().toUpperCase() === currentStation.toUpperCase();

  const sendLogon = async () => {
    if (logonBusy || cooldownLeft > 0) return;
    setLogonBusy(true);
    setLogonError(null);
    try {
      await invoke("hoppie_send_logon_request", { station: stationInput.trim().toUpperCase() });
      setCooldownLeft(LOGON_COOLDOWN_SECS);
      setStationEditing(false);
      clearStationDraft();
      onChanged();
    } catch (e) {
      setLogonError(formatIpcError(e));
    } finally {
      setLogonBusy(false);
    }
  };

  const sendLogoff = async () => {
    if (logonBusy) return;
    setLogonBusy(true);
    setLogonError(null);
    try {
      await invoke("hoppie_send_logoff");
      onChanged();
    } catch (e) {
      setLogonError(formatIpcError(e));
    } finally {
      setLogonBusy(false);
    }
  };

  // Nothing typed and no flight to fall back to a VA callsign from —
  // worth saying out loud since the failure mode is silent: the
  // controller never sees the request.
  const mismatchedCallsign =
    callsignInput.trim() === "" && flightCallsign !== null && flightCallsign !== "";

  const linkKind: "connected" | "error" | "none" = online
    ? "connected"
    : status?.last_error
      ? "error"
      : "none";
  const linkLabel =
    linkKind === "connected"
      ? t("cpdlc.acars_online")
      : linkKind === "error"
        ? t("cpdlc.link_error")
        : t("cpdlc.acars_offline");

  // The backend distinguishes "logged on", "request sent, no answer yet"
  // and "sent, no answer within the poll window" — there is no signal
  // for an explicit ATC rejection (Hoppie/vSMR give no such reply), so
  // that word from README §2d is not reachable from real status today.
  const logonWord: "ok" | "pending" | "none" = loggedOn ? "ok" : logonSent && !logonTimedOut ? "pending" : "none";
  const logonValue = loggedOn ? (currentStation ?? stationInput) : stationInput;
  const hasLogonOrPending = loggedOn || logonSent || logonTimedOut;
  // Nothing sent yet at all: the field IS the way to log on in the first
  // place, so it can't be gated behind "Wechseln" — that button only
  // exists once there's something to hand over FROM. Bug found in field
  // testing: with the old `stationEditing`-only gate, a pilot who had
  // never logged on saw a bare, un-editable value and no way to type a
  // centre at all.
  const showStationInput = stationEditing || !hasLogonOrPending;

  const resolvedCallsign = callsignInput || flightCallsign;

  return (
    <section className="cpdlc-panel">
      <header className="datalink-status">
        <div className="datalink-status__left">
          {/* Grouped per README §2 (a/b/d): 22px separates these groups from
              each other, but a block stays tight (12px) against the button
              that belongs to it — a flat, uniform 22px gap across every
              child here (dot+text, its own button, block, its own button, …)
              was what made the row read as cramped-yet-sparse at once: too
              little room between unrelated groups showed up as visual noise
              right where the field feedback ("alles so gequetscht") points,
              once the actual overlap bug (input width) no longer forced this
              row into a squeeze-everything band-aid. */}
          <div className="datalink-status__group">
            <span className={`datalink-link datalink-link--${linkKind}`}>
              <span className="datalink-link__dot" aria-hidden="true" />
              {linkLabel}
            </span>
            <button
              type="button"
              className={`button datalink-connect-btn ${online ? "" : "button--primary"}`}
              disabled={busy}
              onClick={() => void toggleLink()}
            >
              {online ? t("cpdlc.acars_stop") : t("cpdlc.acars_start")}
            </button>
          </div>

          <div className="datalink-status__group">
            <div className="datalink-block">
              <label className="datalink-block__label" htmlFor="datalink-callsign-input">
                {t("cpdlc.callsign_label")}
              </label>
              {callsignEditing ? (
                <input
                  id="datalink-callsign-input"
                  autoFocus
                  type="text"
                  className="datalink-block__input"
                  value={callsignInput}
                  onChange={(e) => {
                    setCallsignInput(e.target.value.toUpperCase());
                    setCallsignDirty(true);
                  }}
                  onBlur={() => void saveCallsign()}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void saveCallsign();
                    if (e.key === "Escape") setCallsignEditing(false);
                  }}
                  placeholder={flightCallsign ?? t("cpdlc.callsign_placeholder")}
                  disabled={busy || online}
                  title={t("cpdlc.callsign_network_hint")}
                />
              ) : (
                <span
                  id="datalink-callsign-input"
                  className="datalink-block__value"
                  title={t("cpdlc.callsign_network_hint")}
                >
                  {resolvedCallsign || t("cpdlc.callsign_placeholder")}
                </span>
              )}
            </div>
            {!callsignEditing && (
              <button
                type="button"
                className="button button--secondary"
                disabled={busy || online}
                onClick={() => setCallsignEditing(true)}
              >
                {t("cpdlc.callsign_change")}
              </button>
            )}
          </div>
        </div>
        <span className="datalink-status__poll">
          {lastFetchedAt != null && t("cpdlc.last_poll", { seconds: pollAgeSec })}
        </span>
      </header>

      {(status?.last_error || error || logonError || logonTimedOut || mismatchedCallsign) && (
        <div className="datalink-alerts">
          {status?.last_error && <p className="cpdlc-panel__error">{status.last_error}</p>}
          {error && <p className="cpdlc-panel__error">{error}</p>}
          {logonError && <p className="cpdlc-panel__error">{logonError}</p>}
          {logonTimedOut && (
            <p className="cpdlc-link-bar__warn">{t("cpdlc.logon_no_answer", { station: stationInput })}</p>
          )}
          {mismatchedCallsign && (
            <p className="cpdlc-link-bar__warn">
              {t("cpdlc.callsign_mismatch", { entered: callsignInput, flight: flightCallsign })}
            </p>
          )}
        </div>
      )}

      <div className="datalink-workspace">
        <DatalinkComposer
          online={online}
          mode={mode}
          setMode={setMode}
          callsign={resolvedCallsign}
          loggedOn={loggedOn}
          onChanged={onChanged}
          onRecipientChange={setPdcRecipient}
          stationInput={stationInput}
          onStationInputChange={setStationInput}
          stationEditing={stationEditing}
          onStationEditingChange={setStationEditing}
          logonBusy={logonBusy}
          logonWord={logonWord}
          logonValue={logonValue}
          hasLogonOrPending={hasLogonOrPending}
          showStationInput={showStationInput}
          cpdlcStationStatus={cpdlcStationStatus}
          alreadyOn={alreadyOn}
          cooldownRunning={cooldownRunning}
          cooldownLeft={cooldownLeft}
          onSendLogon={() => void sendLogon()}
          onSendLogoff={() => void sendLogoff()}
        />
        <DatalinkHistory
          callsign={resolvedCallsign}
          cpdlcStation={currentStation ?? stationInput}
          pdcRecipient={pdcRecipient}
          messages={messages}
          onChanged={onChanged}
        />
      </div>
    </section>
  );
}
