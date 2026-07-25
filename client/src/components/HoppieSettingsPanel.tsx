// v1.3.0 (#Hoppie-PDC-CPDLC) — Settings-Panel-Section fuer den Hoppie-
// ACARS-PDC/CPDLC-Client. Mirrort DiscordRpcPanel.tsx im Aufbau.
//
// Default = aus (Opt-In). Logon-Code ist write-only (wird nie zurueck-
// geladen) mit einem "Code pruefen"-Button, der per side-effekt-freiem
// Hoppie-`ping` den Code live gegen den echten Server testet, BEVOR der
// Pilot ueberhaupt speichert/verbindet.

import { useEffect, useState } from "react";
import { invoke, formatIpcError } from "../lib/ipc";
import { useTranslation } from "react-i18next";

export interface HoppieSettings {
  enabled: boolean;
  callsign_override: string | null;
  station_id: string;
  notify_os: boolean;
  notify_sound: boolean;
}

export function HoppieSettingsPanel() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<HoppieSettings | null>(null);
  const [hasCode, setHasCode] = useState(false);
  const [codeInput, setCodeInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<HoppieSettings>("hoppie_get_settings").then(setSettings);
    void invoke<boolean>("hoppie_has_logon_code").then(setHasCode);
  }, []);

  if (!settings) {
    return (
      <div className="settings__section">
        <h3>{t("hoppie_settings.section_title")}</h3>
      </div>
    );
  }

  const update = async (patch: Partial<HoppieSettings>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      await invoke<HoppieSettings>("hoppie_set_settings", { settings: next });
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };



  const saveCode = async () => {
    if (busy || codeInput.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_set_logon_code", { code: codeInput });
      setHasCode(true);
      setCodeInput("");
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const clearCode = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_clear_logon_code");
      setHasCode(false);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings__section">
      <h3>{t("hoppie_settings.section_title")}</h3>
      <p className="settings__row-hint">{t("hoppie_settings.intro")}</p>

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => void update({ enabled: e.target.checked })}
          disabled={busy}
        />
        <span>
          <strong>{t("hoppie_settings.enable_label")}</strong>
          <span className="settings__row-hint">{t("hoppie_settings.enable_hint")}</span>
        </span>
      </label>

      {settings.enabled && (
        <div style={{ margin: "4px 0 12px 28px", display: "flex", flexDirection: "column", gap: 12 }}>
          <p className="settings__row-hint">{t("hoppie_settings.callsign_moved_hint")}</p>

          <div className="settings__field">
            <span className="settings__field-label">{t("hoppie_settings.logon_code_label")}</span>
            {hasCode && (
              <p className="settings__row-hint">{t("hoppie_settings.logon_code_stored")}</p>
            )}
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                type="password"
                value={codeInput}
                onChange={(e) => {
                  setCodeInput(e.target.value);
                }}
                placeholder={t("hoppie_settings.logon_code_placeholder")}
                disabled={busy}
              />
              <button
                type="button"
                className="button button--primary"
                disabled={busy || codeInput.trim() === ""}
                onClick={() => void saveCode()}
              >
                {t("hoppie_settings.save_button")}
              </button>
              {hasCode && (
                <button type="button" className="button button--ghost" disabled={busy} onClick={() => void clearCode()}>
                  {t("hoppie_settings.clear_button")}
                </button>
              )}
            </div>
          </div>

          <label className="settings__checkbox">
            <input
              type="checkbox"
              checked={settings.notify_os}
              onChange={(e) => void update({ notify_os: e.target.checked })}
              disabled={busy}
            />
            <span>
              <strong>{t("hoppie_settings.notify_os_label")}</strong>
              <span className="settings__row-hint">{t("hoppie_settings.notify_os_hint")}</span>
            </span>
          </label>

          <label className="settings__checkbox">
            <input
              type="checkbox"
              checked={settings.notify_sound}
              onChange={(e) => void update({ notify_sound: e.target.checked })}
              disabled={busy}
            />
            <span>
              <strong>{t("hoppie_settings.notify_sound_label")}</strong>
            </span>
          </label>
        </div>
      )}

      {error && <p className="cpdlc-panel__error">{error}</p>}
    </div>
  );
}
