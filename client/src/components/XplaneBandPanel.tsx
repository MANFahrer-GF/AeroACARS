// Settings → X-Plane-HUD-Band (ADR-0005). Ein Schalter „X-Plane-Band senden"
// (Standard an). Persistiert backend-seitig (xplane_band.json), wirkt sofort:
// beim Ausschalten blendet die App das Band im Sim einmal aus und sendet
// danach nichts mehr.
//
// Tauri-only; SettingsPanel guarded den Mount, dieses Panel no-opt zusätzlich.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri, formatIpcError } from "../lib/ipc";
import { BAND_MIN_PLUGIN, bandBrauchtUpdate } from "../lib/pluginVersion";
import type { XPlanePremiumStatus } from "../types";

export function XplaneBandPanel() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Plugin-Stand: ist ein zu altes Plugin verbunden, sendet die App kein Band
  // (QS-Hinweis H1) — das soll der Pilot hier sehen, nicht raten.
  const [plugin, setPlugin] = useState<XPlanePremiumStatus | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    invoke<boolean>("xplane_band_get_enabled")
      .then(setEnabled)
      .catch(() => setEnabled(true));
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let aus = false;
    const holen = () =>
      invoke<XPlanePremiumStatus>("xplane_premium_status")
        .then((s) => {
          if (!aus) setPlugin(s);
        })
        .catch(() => {});
    void holen();
    const id = window.setInterval(holen, 5000);
    return () => {
      aus = true;
      window.clearInterval(id);
    };
  }, []);

  async function handleToggle(next: boolean) {
    setBusy(true);
    setError(null);
    try {
      const v = await invoke<boolean>("xplane_band_set_enabled", {
        enabled: next,
      });
      setEnabled(v);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  }

  if (!isTauri) return null;

  return (
    <div className="settings__section">
      <h3>{t("xplane_band.section_title")}</h3>
      <p className="settings__row-hint">{t("xplane_band.intro")}</p>
      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={enabled ?? true}
          disabled={busy || enabled === null}
          onChange={(e) => void handleToggle(e.target.checked)}
        />
        <span>
          <strong>{t("xplane_band.toggle_label")}</strong>
          <span className="settings__row-hint">{t("xplane_band.toggle_hint")}</span>
        </span>
      </label>
      {bandBrauchtUpdate(plugin) && (
        <p className="settings__row-hint">
          ⚠️{" "}
          {t("xplane_band.plugin_too_old", {
            version: plugin?.plugin_version ?? "",
            min: BAND_MIN_PLUGIN,
          })}
        </p>
      )}
      {error && <p className="settings__row-hint">{error}</p>}
    </div>
  );
}
