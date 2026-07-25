// v1.3.0 (#Hoppie-PDC-CPDLC) — CPDLC section.
//
// Unlike PDC, CPDLC is a session: the pilot logs on to a named facility
// and stays connected to it until handing over to the next one. The
// logon state (NO LINK / LOGON SENT / CONNECTED <facility>) is the
// header, mirroring a DCDU's status line, and every uplink that needs an
// answer carries its answer keys inline.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { formatDatalinkText } from "../lib/datalink";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";
import { CpdlcQuickReply } from "./CpdlcQuickReply";
import { CpdlcComposer } from "./CpdlcComposer";
import { useMessageLog } from "../hooks/useMessageLog";
import { useStationOnline } from "../hooks/useStationOnline";
import { StationBadge } from "./StationBadge";

/** GOLD response codes that actually demand an answer from the pilot.
 *  N (not required) and NE (none expected) are answered by nobody. */
const REPLYABLE = ["WU", "AN", "R", "Y"];

/** The centre the pilot typed but hasn't logged on to yet.
 *
 *  Field feedback: pilots typed a centre, got nothing back, and found the
 *  box empty again ("Center" placeholder). Cause was this component's own
 *  lifetime — the input lived in component state seeded from `station`,
 *  which is null until the logon succeeds. Every unmount (switching to the
 *  PDC tab, or the panel's mount condition in App.tsx flipping) therefore
 *  reset it to empty. Persisting the draft outlives all of those. */
const STATION_DRAFT_KEY = "aeroacars.cpdlc.station_draft";

/** How long the logon button stays blocked after a request went out.
 *
 *  Not a timeout — a double-click guard. Procedure has you send the logon
 *  BEFORE entering the sector, and the controller commonly only accepts it
 *  once they actually take you, so minutes of silence are normal and not an
 *  error. What went wrong in the field was the pilot getting no feedback at
 *  all, clicking again out of uncertainty, and ATC receiving a second
 *  REQUEST LOGON — which vSMR re-flags as a fresh request because it scans
 *  for "REQ" first. One poll cycle (max 75 s) plus headroom is enough for an
 *  answer to have arrived; after that a deliberate retry stays possible,
 *  which the field case needed: logon, no reaction, called on voice, retry. */
const LOGON_COOLDOWN_SECS = 90;

function loadStationDraft(): string {
  try {
    return localStorage.getItem(STATION_DRAFT_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveStationDraft(value: string): void {
  try {
    if (value) localStorage.setItem(STATION_DRAFT_KEY, value);
    else localStorage.removeItem(STATION_DRAFT_KEY);
  } catch {
    // Private-mode / quota — the field just falls back to its old
    // per-mount behaviour, which is no worse than before.
  }
}

interface Props {
  online: boolean;
  loggedOn: boolean;
  station: string | null;
  logonSent: boolean;
  /** Logon sent but nothing came back — see LOGON_TIMEOUT_SECS. */
  logonTimedOut: boolean;
  messages: ThreadEntry[];
  onChanged: () => void;
}

export function CpdlcView({
  online,
  loggedOn,
  station,
  logonSent,
  logonTimedOut,
  messages,
  onChanged,
}: Props) {
  const { t } = useTranslation();
  // An active session wins; otherwise fall back to the not-yet-sent draft
  // so a typed centre survives tab switches and panel remounts.
  const [stationInput, setStationInputState] = useState(() => station ?? loadStationDraft());
  const setStationInput = (value: string) => {
    setStationInputState(value);
    saveStationDraft(value);
  };
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [composerOpen, setComposerOpen] = useState(false);

  // Ticks only while a cooldown is actually running, so the panel isn't
  // re-rendering once a second for the entire flight.
  const [cooldownLeft, setCooldownLeft] = useState(0);
  const cooldownRunning = cooldownLeft > 0;
  useEffect(() => {
    if (!cooldownRunning) return;
    const id = window.setInterval(
      () => setCooldownLeft((s) => Math.max(0, s - 1)),
      1000,
    );
    return () => window.clearInterval(id);
  }, [cooldownRunning]);

  const sendLogon = async () => {
    if (busy || cooldownLeft > 0) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_logon_request", { station: stationInput.trim().toUpperCase() });
      setCooldownLeft(LOGON_COOLDOWN_SECS);
      onChanged();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const sendLogoff = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_logoff");
      onChanged();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  // Re-sending LOGON to the facility we're already on does nothing
  // useful and just confuses the controller — the button only unlocks
  // once a different centre is entered (i.e. an actual handover).
  const alreadyOn =
    loggedOn && station !== null && stationInput.trim().toUpperCase() === station.toUpperCase();

  const linkState = loggedOn ? "connected" : logonSent ? "sent" : "none";
  const linkLabel = loggedOn
    ? t("cpdlc.link_connected", { station: station ?? stationInput })
    : logonSent
      ? t("cpdlc.link_sent")
      : t("cpdlc.link_none");

  const cpdlc = messages.filter((m) => m.kind === "cpdlc");
  const { logRef, onScroll, toggle, isCollapsed } = useMessageLog(cpdlc.length);
  const stationStatus = useStationOnline(stationInput, online);

  return (
    <div className="cpdlc-section">
      <div className="cpdlc-section__side">
      <header className="cpdlc-section__bar">
        <span className={`cpdlc-link cpdlc-link--${linkState}`}>
          <span className="cpdlc-link__dot" aria-hidden="true" />
          {linkLabel}
        </span>
        <label className="cpdlc-field">
          <span>{t("cpdlc.center_label")}</span>
          <input
            type="text"
            value={stationInput}
            onChange={(e) => setStationInput(e.target.value.toUpperCase())}
            placeholder={t("cpdlc.center_placeholder")}
            disabled={busy}
          />
          <StationBadge status={stationStatus} />
        </label>
        <button
          type="button"
          className="button button--primary"
          disabled={
            !online || busy || stationInput.trim() === "" || alreadyOn || cooldownRunning
          }
          title={
            alreadyOn
              ? t("cpdlc.logon_already", { station })
              : cooldownRunning
                ? t("cpdlc.logon_cooldown", { seconds: cooldownLeft })
                : undefined
          }
          onClick={() => void sendLogon()}
        >
          {/* One constant label. Swapping to "log on to next centre"
              resized the button and shoved the row around; the centre
              field right next to it already says WHERE we're logging on. */}
          {t("cpdlc.logon_send")}
        </button>
        <button
          type="button"
          className="button"
          disabled={!online || !loggedOn}
          onClick={() => setComposerOpen((v) => !v)}
        >
          {t("cpdlc.composer_open")}
        </button>
        {/* Ends the session deliberately without dropping the ACARS link.
            After this no facility is responsible for the aircraft until
            the pilot logs on to the next one. */}
        <button
          type="button"
          className="button"
          disabled={!online || busy || (!loggedOn && !logonSent && !logonTimedOut)}
          onClick={() => void sendLogoff()}
        >
          {t("cpdlc.logoff")}
        </button>
      </header>

      {logonTimedOut && (
        <p className="cpdlc-link-bar__warn">{t("cpdlc.logon_no_answer", { station: stationInput })}</p>
      )}
      <p className="cpdlc-section__explain">{t("cpdlc.cpdlc_explain")}</p>
      {error && <p className="cpdlc-panel__error">{error}</p>}
      </div>

      {/* The composer takes over the wide column instead of squeezing into
          the narrow one, where it was cut off and unusable. Same as
          EasyCPDLC, which swaps its compose panel in for the message log
          rather than showing both at once. */}
      {composerOpen ? (
        <CpdlcComposer
          connected={online && loggedOn}
          onSent={() => {
            setComposerOpen(false);
            onChanged();
          }}
          onClose={() => setComposerOpen(false)}
        />
      ) : (
      <ul className="cpdlc-log" ref={logRef} onScroll={onScroll}>
        {cpdlc.length === 0 && <li className="cpdlc-log__empty">{t("cpdlc.cpdlc_log_empty")}</li>}
        {cpdlc.map((m, i) => {
          const isUplink = m.direction === "received";
          // "Open" means ATC is actually waiting. `closed` alone isn't
          // enough: an uplink that never required a response (LOGON
          // ACCEPTED, advisories — response N/NE) also never gets closed,
          // and would otherwise sit there forever demanding an answer.
          const open = isUplink && !m.closed && REPLYABLE.includes(m.response ?? "");
          // Our own request still waiting on ATC — different state, and
          // explicitly NOT something the pilot has to act on.
          const awaitingAtc = !isUplink && !m.closed && REPLYABLE.includes(m.response ?? "");
          const key = `${m.at}-${i}`;
          const collapsed = isCollapsed(i, key, open || awaitingAtc);
          return (
            <li
              key={key}
              className={`cpdlc-msg cpdlc-msg--${isUplink ? "up" : "down"} ${
                open ? "cpdlc-msg--open" : ""
              } ${collapsed ? "cpdlc-msg--collapsed" : ""}`}
              onClick={collapsed ? () => toggle(key) : undefined}
              role={collapsed ? "button" : undefined}
              tabIndex={collapsed ? 0 : undefined}
              onKeyDown={
                collapsed
                  ? (e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        toggle(key);
                      }
                    }
                  : undefined
              }
              title={collapsed ? t("cpdlc.expand_hint") : undefined}
            >
              <p className="cpdlc-msg__meta">
                <span className="cpdlc-msg__who">
                  {isUplink ? station ?? t("cpdlc.thread_received") : t("cpdlc.thread_sent")}
                </span>
                {m.min != null && <span className="cpdlc-msg__min">MIN {m.min}</span>}
                <span className="cpdlc-msg__time">{new Date(m.at).toLocaleTimeString()}</span>
                {open && <span className="cpdlc-msg__flag">{t("cpdlc.awaiting_reply")}</span>}
                {/* Our own request that ATC hasn't answered yet. The DCDU
                    shows this as SENT so a pilot doesn't re-send it. */}
                {awaitingAtc && (
                  <span className="cpdlc-msg__pending">{t("cpdlc.awaiting_atc")}</span>
                )}
                {isUplink && m.closed && (
                  <span className="cpdlc-msg__done" aria-hidden="true">
                    ✓
                  </span>
                )}
              </p>
              <p className="cpdlc-msg__text">
                {collapsed ? m.text.replace(/@/g, " ") : formatDatalinkText(m.text)}
              </p>
              {open && (
                <CpdlcQuickReply
                  min={m.min}
                  response={m.response}
                  deferred={Boolean(m.deferred)}
                  onReplied={onChanged}
                />
              )}
            </li>
          );
        })}
      </ul>
      )}
    </div>
  );
}
