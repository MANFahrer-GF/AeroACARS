// v1.3.0 (#Hoppie-PDC-CPDLC) — CPDLC/PDC tab root.
//
// The callsign used for `from=` on every Hoppie request lives here, not in
// Settings — a pilot checks/adjusts it right where they connect, prefilled
// from the active flight when there is one. Settings only keeps the
// enable/simulation/notification toggles + the logon code.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { useCpdlcMessages } from "../hooks/useCpdlcMessages";
import { PdcRequestForm } from "./PdcRequestForm";
import { CpdlcMessageThread } from "./CpdlcMessageThread";
import { CpdlcComposer } from "./CpdlcComposer";

interface HoppieSettings {
  enabled: boolean;
  callsign_override: string | null;
  notify_sound: boolean;
}

interface VerifyOutcome {
  valid: boolean;
  reason: string | null;
}

interface HoppieStatus {
  connected: boolean;
  logged_on: boolean;
  pending_response_count: number;
  last_error: string | null;
  logon_verified: VerifyOutcome | null;
  mock_mode: boolean;
}

interface FlightContext {
  callsign: string | null;
}

const STATUS_POLL_MS = 5000;

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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  const { messages, refresh: refreshMessages } = useCpdlcMessages(
    Boolean(status?.connected),
    Boolean(settings?.notify_sound),
  );

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

  const saveCallsign = async () => {
    if (busy || !callsignDirty) return;
    setBusy(true);
    setError(null);
    const trimmed = callsignInput.trim().toUpperCase();
    try {
      const next = { ...settings, callsign_override: trimmed === "" ? null : trimmed };
      await invoke<HoppieSettings>("hoppie_set_settings", { settings: next });
      setSettings(next);
      setCallsignInput(trimmed);
      setCallsignDirty(false);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<HoppieStatus>("hoppie_connect");
      setStatus(s);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<HoppieStatus>("hoppie_disconnect");
      setStatus(s);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const sendLogonRequest = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<HoppieStatus>("hoppie_send_logon_request");
      setStatus(s);
      refreshMessages();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="cpdlc-panel">
      <div className="cpdlc-panel__callsign">
        <label>
          <span className="settings__field-label">{t("cpdlc.callsign_label")}</span>
          <input
            type="text"
            value={callsignInput}
            onChange={(e) => {
              setCallsignInput(e.target.value);
              setCallsignDirty(true);
            }}
            onBlur={() => void saveCallsign()}
            placeholder={flightCallsign ?? t("cpdlc.callsign_placeholder")}
            disabled={busy || Boolean(status?.connected)}
          />
        </label>
        {callsignDirty && (
          <button type="button" className="button" disabled={busy} onClick={() => void saveCallsign()}>
            {t("cpdlc.callsign_save")}
          </button>
        )}
        {!callsignInput && flightCallsign && (
          <span className="settings__row-hint">{t("cpdlc.callsign_from_flight", { callsign: flightCallsign })}</span>
        )}
        {status?.connected && (
          <span className="settings__row-hint">{t("cpdlc.callsign_locked_hint")}</span>
        )}
      </div>

      <header className="cpdlc-panel__header">
        <span
          className={`status-pill status-pill--${status?.connected ? "online" : "offline"}`}
        >
          <span className="status-pill__dot" />
          {status?.connected ? t("cpdlc.status_connected") : t("cpdlc.status_offline")}
        </span>
        {status?.connected && (
          <span
            className={`status-pill status-pill--${status.logged_on ? "online" : "offline"}`}
          >
            <span className="status-pill__dot" />
            {status.logged_on ? t("cpdlc.status_logged_on") : t("cpdlc.status_not_logged_on")}
          </span>
        )}
        {status?.mock_mode && (
          <span className="cpdlc-panel__mock-badge">{t("cpdlc.mock_mode_badge")}</span>
        )}
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => void (status?.connected ? disconnect() : connect())}
        >
          {status?.connected ? t("cpdlc.disconnect") : t("cpdlc.connect")}
        </button>
        {status?.connected && !status.logged_on && (
          <button type="button" className="button button--primary" disabled={busy} onClick={() => void sendLogonRequest()}>
            {t("cpdlc.send_logon_request")}
          </button>
        )}
      </header>

      {status?.last_error && <p className="cpdlc-panel__error">{status.last_error}</p>}
      {error && <p className="cpdlc-panel__error">{error}</p>}

      <PdcRequestForm connected={Boolean(status?.connected)} onSent={refreshMessages} />

      <CpdlcComposer connected={Boolean(status?.connected)} onSent={refreshMessages} />

      <div className="cpdlc-panel__thread">
        <h3>{t("cpdlc.thread_title")}</h3>
        <CpdlcMessageThread messages={messages} onResponded={refreshMessages} />
      </div>
    </section>
  );
}
