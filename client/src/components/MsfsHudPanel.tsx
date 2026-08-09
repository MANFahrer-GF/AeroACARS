// Settings → MSFS-In-Sim-HUD (v1.5.0, #msfs-hud, QS 09.08.2026).
//
// Genau EIN Schalter: der lokale Panel-Server (Port 47847), von dem das
// Flow-Pro-HUD im Simulator seine Daten holt. Persistiert backend-seitig
// (panel_server.json), wirkt beim NÄCHSTEN App-Start — bewusst kein
// Laufzeit-Stopp/-Start, siehe panel_server::set_enabled.
//
// Der Schalter ist primär ein Diagnose-Werkzeug: die Beta-Abstürze sind
// ungeklärt, und hiermit lässt sich per A/B-Test prüfen, ob sie mit dem
// Panel-Server zusammenhängen — ohne auf eine alte Version zurückzugehen.
//
// Tauri-only (ein Browser hostet keinen Server); SettingsPanel guarded den
// Mount, dieses Panel no-opt zusätzlich defensiv.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri, formatIpcError } from "../lib/ipc";

export function MsfsHudPanel() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Erst nach einer Änderung zeigen — beim bloßen Öffnen der Einstellungen
  // wäre „wirkt nach Neustart" Rauschen.
  const [restartHint, setRestartHint] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    invoke<boolean>("panel_server_get_enabled")
      .then(setEnabled)
      .catch(() => setEnabled(true));
  }, []);

  async function handleToggle(next: boolean) {
    setBusy(true);
    setError(null);
    try {
      const v = await invoke<boolean>("panel_server_set_enabled", {
        enabled: next,
      });
      setEnabled(v);
      setRestartHint(true);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  }

  if (!isTauri) return null;

  return (
    <div className="settings__section">
      <h3>{t("msfs_hud.section_title")}</h3>
      <p className="settings__row-hint">{t("msfs_hud.intro")}</p>

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={enabled ?? true}
          disabled={busy || enabled === null}
          onChange={(e) => void handleToggle(e.target.checked)}
        />
        <span>
          <strong>{t("msfs_hud.toggle_label")}</strong>
          <span className="settings__row-hint">{t("msfs_hud.toggle_hint")}</span>
        </span>
      </label>

      {restartHint && (
        <p className="settings__row-hint">
          <strong>{t("msfs_hud.restart_hint")}</strong>
        </p>
      )}
      {error && <p className="settings__row-hint">{error}</p>}
    </div>
  );
}
