// v1.3.0 (#Hoppie-PDC-CPDLC) — PDC/CPDLC tab root.
//
// Three levels, deliberately kept apart because conflating them is
// confusing:
//   1. ACARS reception — the Hoppie network link itself (logon code +
//      callsign). No station involved; without it nothing arrives at all.
//   2. PDC — a telex to the delivery station. No logon.
//   3. CPDLC — a session logged on to a named ATC facility, re-flown at
//      every sector handover.
//
// The panel is a fixed-height flex column: only the message log scrolls,
// so there is never a scrollbar inside a scrollbar.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { useCpdlcMessages } from "../hooks/useCpdlcMessages";
import { PdcView } from "./PdcView";
import { CpdlcView } from "./CpdlcView";

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
  pending_uplink_count: number;
  last_error: string | null;
  logon_verified: VerifyOutcome | null;
  station_id: string | null;
  logon_pending: boolean;
  logon_timed_out: boolean;
}

interface FlightContext {
  callsign: string | null;
}

const STATUS_POLL_MS = 5000;

type Section = "pdc" | "cpdlc";

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
  const [section, setSection] = useState<Section>("pdc");
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

  const online = Boolean(status?.connected);
  const { messages, refresh: refreshMessages } = useCpdlcMessages(online);

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

  /// Blur-save. Deliberately does NOT touch `busy`: the connect button is
  /// disabled while busy, so flipping it here meant the very click that
  /// caused the blur landed on a disabled button and was swallowed — the
  /// pilot typed a callsign, pressed "start reception", and nothing
  /// happened. `toggleLink` re-saves anyway, so a lost race is harmless.
  const saveCallsign = async () => {
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
      // Persist a freshly typed callsign BEFORE connecting. Blur-saving
      // alone races the click: the button fires while the settings write
      // is still in flight, the backend reads the old (usually empty)
      // value, and the connect fails with "no callsign configured".
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

  // Nothing is entered and we'd fall back to the VA callsign from the
  // flight plan — which is almost never what the pilot is connected to
  // the network as. Worth saying out loud, because the failure mode is
  // silent: the controller simply never sees the request.
  const mismatchedCallsign =
    callsignInput.trim() === "" && flightCallsign !== null && flightCallsign !== "";

  return (
    <section className="cpdlc-panel">
      <header className="cpdlc-link-bar">
        <div className="cpdlc-link-bar__main">
          <span className={`cpdlc-link cpdlc-link--${online ? "connected" : "none"}`}>
            <span className="cpdlc-link__dot" aria-hidden="true" />
            {online ? t("cpdlc.acars_online") : t("cpdlc.acars_offline")}
          </span>
          <label className="cpdlc-field">
            <span>{t("cpdlc.callsign_label")}</span>
            <input
              type="text"
              value={callsignInput}
              onChange={(e) => {
                // Normalize the value, not just its rendering — the CSS
                // uppercase transform left the stored value lowercase.
                setCallsignInput(e.target.value.toUpperCase());
                setCallsignDirty(true);
              }}
              onBlur={() => void saveCallsign()}
              placeholder={flightCallsign ?? t("cpdlc.callsign_placeholder")}
              disabled={busy || online}
              title={t("cpdlc.callsign_network_hint")}
            />
          </label>
          <button type="button" className="button" disabled={busy} onClick={() => void toggleLink()}>
            {online ? t("cpdlc.acars_stop") : t("cpdlc.acars_start")}
          </button>
        </div>
        <p className="cpdlc-link-bar__explain">{t("cpdlc.acars_explain")}</p>
        {/* The controller's client looks the aircraft up by this exact
            string (vSMR matches it against the flight plan callsign). A
            VA callsign from the phpVMS flight plan will not match what
            the pilot is connected to the network as, and the request then
            never appears on the controller's screen. */}
        {mismatchedCallsign && (
          <p className="cpdlc-link-bar__warn">
            {t("cpdlc.callsign_mismatch", {
              entered: callsignInput,
              flight: flightCallsign,
            })}
          </p>
        )}
      </header>

      {status?.last_error && <p className="cpdlc-panel__error">{status.last_error}</p>}
      {error && <p className="cpdlc-panel__error">{error}</p>}

      <nav className="cpdlc-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={section === "pdc"}
          className={`cpdlc-tabs__tab ${section === "pdc" ? "cpdlc-tabs__tab--active" : ""}`}
          onClick={() => setSection("pdc")}
        >
          {t("cpdlc.section_pdc")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={section === "cpdlc"}
          className={`cpdlc-tabs__tab ${section === "cpdlc" ? "cpdlc-tabs__tab--active" : ""}`}
          onClick={() => setSection("cpdlc")}
        >
          {t("cpdlc.section_cpdlc")}
          {status && status.pending_uplink_count > 0 && (
            <span className="cpdlc-tabs__badge">{status.pending_uplink_count}</span>
          )}
        </button>
      </nav>

      {section === "pdc" ? (
        <PdcView
          online={online}
          callsign={callsignInput || flightCallsign}
          messages={messages}
          onChanged={onChanged}
        />
      ) : (
        <CpdlcView
          online={online}
          loggedOn={Boolean(status?.logged_on)}
          station={status?.station_id ?? null}
          logonSent={Boolean(status?.logon_pending)}
          logonTimedOut={Boolean(status?.logon_timed_out)}
          messages={messages}
          onChanged={onChanged}
        />
      )}
    </section>
  );
}
