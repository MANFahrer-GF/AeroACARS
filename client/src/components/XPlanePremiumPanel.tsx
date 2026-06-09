import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { SimConnectionState, XPlanePremiumStatus } from "../types";

interface PluginInstallResult {
  installed_at: string;
  bytes_written: number;
  files_written: number;
}

/**
 * Settings → Debug: AeroACARS X-Plane Plugin status + auto-install
 * card (v0.5.0+).
 *
 * Visual states:
 *
 *  1. **Active** — packets flowing right now (green badge,
 *     "📡 live"). Touchdown captures come from the plugin.
 *  2. **Inactive** (no plugin packets ever this session) — grey
 *     badge, plus the **install panel** that lets the pilot:
 *       a) Auto-detect their X-Plane root via `xplane_detect_install_path`
 *       b) Or paste / edit the path manually
 *       c) One-click "Install plugin" → downloads matching zip
 *          from this version's GitHub release, extracts to
 *          `<root>/Resources/plugins/AeroACARS/`.
 *  3. **Bind error** (port 52000 held by something else) — red
 *     warning panel.
 *
 * Architectural twin of `PmdgPremiumPanel`. Polls
 * `xplane_premium_status` every 2 s.
 */
interface Props {
  simState: SimConnectionState;
}

export function XPlanePremiumPanel({ simState }: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<XPlanePremiumStatus | null>(null);
  const [installPath, setInstallPath] = useState<string>("");
  const [installing, setInstalling] = useState(false);
  const [installMessage, setInstallMessage] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  // Poll status every 2 s — cheap, mutex read on the adapter side.
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const next = await invoke<XPlanePremiumStatus>(
          "xplane_premium_status",
        );
        if (!cancelled) setStatus(next);
      } catch {
        // IPC errors transient on dev rebuilds.
      }
    }
    void poll();
    const id = window.setInterval(poll, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  // Auto-detect the X-Plane install path on mount so the pilot
  // sees a prefilled value when they expand the install panel.
  useEffect(() => {
    let cancelled = false;
    void invoke<string | null>("xplane_detect_install_path").then((path) => {
      if (!cancelled && path && installPath === "") {
        setInstallPath(path);
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!status) return null;

  const hasError = !!status.last_error;
  const stateClass = hasError
    ? "pmdg-panel--warn"
    : status.active
      ? "pmdg-panel--active"
      : "pmdg-panel--inactive";

  async function handleInstall() {
    setInstalling(true);
    setInstallMessage(null);
    setInstallError(null);
    try {
      const result = await invoke<PluginInstallResult>(
        "xplane_install_plugin",
        { installDir: installPath },
      );
      setInstallMessage(
        t("xplane_premium_panel.install_success", {
          path: result.installed_at,
          files: result.files_written,
        }),
      );
    } catch (err) {
      setInstallError(String(err));
    } finally {
      setInstalling(false);
    }
  }

  async function handleDetect() {
    try {
      const path = await invoke<string | null>("xplane_detect_install_path");
      if (path) {
        setInstallPath(path);
        setInstallError(null);
        setInstallMessage(t("xplane_premium_panel.detect_success"));
      } else {
        setInstallMessage(null);
        setInstallError(t("xplane_premium_panel.detect_failed"));
      }
    } catch (err) {
      setInstallError(String(err));
    }
  }

  return (
    <section className={`pmdg-panel ${stateClass}`}>
      <header className="pmdg-panel__header">
        <span className="pmdg-panel__title">
          {t("xplane_premium_panel.title")}
        </span>
        {status.active && (
          <span className="pmdg-panel__variant">📡 live</span>
        )}
      </header>

      {/* Bind error — port 52000 held by something else */}
      {hasError && (
        <div className="pmdg-panel__warning">
          <p className="pmdg-panel__warning-title">
            ⚠️ {t("xplane_premium_panel.bind_error_title")}
          </p>
          <p>{t("xplane_premium_panel.bind_error_explanation")}</p>
          <pre className="pmdg-panel__code-block">{status.last_error}</pre>
        </div>
      )}

      {/* Active — packets flowing */}
      {!hasError && status.active && (
        <div className="pmdg-panel__active">
          <div className="pmdg-panel__metrics">
            <div className="pmdg-panel__metric">
              <span className="pmdg-panel__metric-label">
                {t("xplane_premium_panel.packets_label")}
              </span>
              <span className="pmdg-panel__metric-value">
                {status.packet_count.toLocaleString()}
              </span>
            </div>
          </div>
          <p className="pmdg-panel__hint">
            {t("xplane_premium_panel.active_hint")}
          </p>
        </div>
      )}

      {/* Inactive — show install panel */}
      {!hasError && !status.active && (
        <div className="pmdg-panel__active">
          <p className="pmdg-panel__hint">
            {simState === "connected"
              ? t("xplane_premium_panel.inactive_hint_xp_running")
              : t("xplane_premium_panel.inactive_hint_xp_not_running")}
          </p>

          <div style={{ marginTop: "0.75rem" }}>
            <label
              htmlFor="xplane-install-path"
              className="pmdg-panel__metric-label"
              style={{ display: "block", marginBottom: "0.25rem" }}
            >
              {t("xplane_premium_panel.install_path_label")}
            </label>
            <div style={{ display: "flex", gap: "0.5rem" }}>
              <input
                id="xplane-install-path"
                type="text"
                value={installPath}
                onChange={(e) => setInstallPath(e.target.value)}
                placeholder={t(
                  "xplane_premium_panel.install_path_placeholder",
                )}
                style={{ flex: 1 }}
                disabled={installing}
              />
              <button
                type="button"
                onClick={handleDetect}
                disabled={installing}
              >
                {t("xplane_premium_panel.detect_button")}
              </button>
            </div>
            <button
              type="button"
              onClick={handleInstall}
              disabled={installing || installPath.trim() === ""}
              style={{ marginTop: "0.5rem" }}
            >
              {installing
                ? t("xplane_premium_panel.installing")
                : t("xplane_premium_panel.install_button")}
            </button>
            {installMessage && (
              <p
                className="pmdg-panel__hint"
                style={{ color: "var(--success-color, #2a8b2a)" }}
              >
                ✅ {installMessage}
              </p>
            )}
            {installError && (
              <p
                className="pmdg-panel__hint"
                style={{ color: "var(--error-color, #c53030)" }}
              >
                ⚠️ {installError}
              </p>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
