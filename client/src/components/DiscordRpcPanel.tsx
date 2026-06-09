// v0.9.0 (#Discord-RPC) — Settings-Panel-Section + Hook fuer Auto-Push.
//
// Spec: docs/spec/v0.9.0-discord-rich-presence.md
//
// Drei sichtbare Toggles + ein Test-Button + Status-Anzeige.
// Default = aus (Opt-In). Wenn der Pilot enabled = true setzt, fragt
// das Backend Discord ueber die IPC-Pipe an; wenn Discord nicht laeuft,
// landet das auf Status "NotFound" — kein Crash, der Pilot sieht's hier.

import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";

export interface DiscordPresenceSettings {
  enabled: boolean;
  anonymize_callsign: boolean;
  show_profile_button: boolean;
}

export interface DiscordPresenceState {
  status: "connected" | "not_found" | "disabled" | "error";
  last_connect_attempt_at: string | null;
  last_update_at: string | null;
  client_id: string;
  error_message: string | null;
}

export function DiscordRpcPanel() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<DiscordPresenceSettings | null>(null);
  const [status, setStatus] = useState<DiscordPresenceState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Initial-Load: aktuelles Settings-Objekt + Status vom Backend ziehen.
  useEffect(() => {
    let cancel = false;
    void (async () => {
      try {
        const s = await invoke<DiscordPresenceSettings>("discord_rpc_get_settings");
        const st = await invoke<DiscordPresenceState>("discord_rpc_get_status");
        if (!cancel) {
          setSettings(s);
          setStatus(st);
        }
      } catch (e) {
        if (!cancel) setError(String(e));
      }
    })();
    return () => {
      cancel = true;
    };
  }, []);

  // Wenn enabled aktiv ist: alle 5s den Status frisch ziehen.
  // So sieht der Pilot wenn Discord aufgeht/zugeht ohne Re-mount.
  useEffect(() => {
    if (!settings?.enabled) return;
    const id = window.setInterval(() => {
      void invoke<DiscordPresenceState>("discord_rpc_get_status")
        .then(setStatus)
        .catch(() => undefined);
    }, 5000);
    return () => window.clearInterval(id);
  }, [settings?.enabled]);

  if (!settings) {
    return (
      <div className="settings__section">
        <h3>{t("discord_rpc.section_title")}</h3>
        <p className="settings__row-hint">{t("discord_rpc.loading")}</p>
      </div>
    );
  }

  const update = async (patch: Partial<DiscordPresenceSettings>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const next = { ...settings, ...patch };
    setSettings(next); // optimistic
    try {
      const st = await invoke<DiscordPresenceState>("discord_rpc_set_settings", { settings: next });
      setStatus(st);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleTest = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("discord_rpc_send_test");
      // Nach 1s Status neu ziehen damit der Pilot sieht ob's geklappt hat
      window.setTimeout(() => {
        void invoke<DiscordPresenceState>("discord_rpc_get_status").then(setStatus);
      }, 1000);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Status-Anzeige: Farb-Dot + Text
  const statusInfo = (() => {
    if (!status) return { dot: "#888", text: t("discord_rpc.status_unknown") };
    switch (status.status) {
      case "connected":
        return { dot: "#4ade80", text: t("discord_rpc.status_connected") };
      case "not_found":
        return { dot: "#9ca3af", text: t("discord_rpc.status_not_found") };
      case "disabled":
        return { dot: "#6b7280", text: t("discord_rpc.status_disabled") };
      case "error":
        return {
          dot: "#f87171",
          text: t("discord_rpc.status_error", { msg: status.error_message ?? "?" }),
        };
    }
  })();

  // Diagnose-Zeile (v0.12.2): wann landete der letzte Presence-Push?
  // `last_update_at` setzt das Backend bei jedem erfolgreichen
  // `set_activity`. Während eines aktiven Flugs muss der Wert alle paar
  // Sekunden frisch werden — bleibt er stehen, kommt der Flug-Push nicht
  // durch (TAP533-Befund). Reine Anzeige, die Daten hat der Manager eh.
  const lastPushLabel = (() => {
    if (!status?.last_update_at) return t("discord_rpc.diag_push_never");
    const secs = Math.max(
      0,
      Math.round((Date.now() - new Date(status.last_update_at).getTime()) / 1000),
    );
    return secs < 120
      ? t("discord_rpc.diag_ago_seconds", { n: secs })
      : t("discord_rpc.diag_ago_minutes", { n: Math.round(secs / 60) });
  })();

  return (
    <div className="settings__section">
      <h3>{t("discord_rpc.section_title")}</h3>
      <p className="settings__row-hint">{t("discord_rpc.intro")}</p>

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => void update({ enabled: e.target.checked })}
          disabled={busy}
        />
        <span>
          <strong>{t("discord_rpc.enable_label")}</strong>
          <span
            className="settings__row-hint"
            dangerouslySetInnerHTML={{ __html: t("discord_rpc.enable_hint") }}
          />
        </span>
      </label>

      {/* Status-Indikator (Dot + Text) + Diagnose — nur wenn enabled */}
      {settings.enabled && (
        <div style={{ margin: "4px 0 12px 28px" }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontSize: "0.9rem",
            }}
          >
            <span
              style={{
                display: "inline-block",
                width: 10,
                height: 10,
                borderRadius: "50%",
                background: statusInfo.dot,
              }}
            />
            <span>{statusInfo.text}</span>
          </div>
          <div style={{ marginTop: 3, fontSize: "0.82rem", color: "#9ca3af" }}>
            {t("discord_rpc.diag_last_push", { when: lastPushLabel })}
          </div>
          {status?.error_message && (
            <div style={{ marginTop: 2, fontSize: "0.82rem", color: "#f87171" }}>
              {t("discord_rpc.diag_push_error", { msg: status.error_message })}
            </div>
          )}
        </div>
      )}

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={settings.anonymize_callsign}
          onChange={(e) => void update({ anonymize_callsign: e.target.checked })}
          disabled={busy || !settings.enabled}
        />
        <span>
          <strong>{t("discord_rpc.anonymize_label")}</strong>
          <span className="settings__row-hint">{t("discord_rpc.anonymize_hint")}</span>
        </span>
      </label>

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={settings.show_profile_button}
          onChange={(e) => void update({ show_profile_button: e.target.checked })}
          disabled={busy || !settings.enabled}
        />
        <span>
          <strong>{t("discord_rpc.profile_button_label")}</strong>
          <span className="settings__row-hint">{t("discord_rpc.profile_button_hint")}</span>
        </span>
      </label>

      {settings.enabled && (
        <div style={{ margin: "8px 0 0 28px" }}>
          <button type="button" onClick={() => void handleTest()} disabled={busy}>
            {t("discord_rpc.test_button")}
          </button>
          <span style={{ marginLeft: 10, fontSize: "0.85rem", color: "#9ca3af" }}>
            {t("discord_rpc.test_hint")}
          </span>
        </div>
      )}

      {error && (
        <p style={{ color: "#f87171", fontSize: "0.85rem", marginTop: 8 }}>{error}</p>
      )}
    </div>
  );
}
