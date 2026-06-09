// Settings → LAN-Fernbedienung / LAN remote (v0.16.0, #LAN-Remote).
//
// Tauri-ONLY panel (the desktop app hosts the server; a browser obviously
// can't host one). SettingsPanel guards the mount with `isTauri`, but this
// component also no-ops defensively if rendered outside Tauri.
//
// Controls:
//   - master toggle → remote_server_start / remote_server_stop
//   - port number input → remote_server_set_port (persisted backend-side),
//     applied on blur; when the server is running the new port takes effect
//     after a stop/start, which we do transparently.
//   - when running: the candidate LAN URL(s), the pairing QR (rendered from
//     the backend's qr_svg data-URL), and the 6-digit PIN shown large.
//   - a firewall note + a "anyone on the LAN with the PIN controls the
//     real flight" warning.
//
// Bilingual via i18next (DE primary). All copy lives under `remote.*`.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri } from "../lib/ipc";
import type { RemoteServerStatus } from "../types";

const DEFAULT_PORT = 8765;
const MIN_PORT = 1024;
const MAX_PORT = 65535;

export function RemoteServerPanel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RemoteServerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Local port-input buffer so the user can type freely; committed on blur.
  const [portInput, setPortInput] = useState<string>(String(DEFAULT_PORT));
  // "PIN kopiert" / "URL kopiert" transient feedback.
  const [copied, setCopied] = useState<"pin" | "url" | null>(null);
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const applyStatus = useCallback((s: RemoteServerStatus) => {
    setStatus(s);
    setPortInput(String(s.port));
  }, []);

  // Initial status fetch (also reflects a server already running from a
  // previous app session / auto-restart).
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    void (async () => {
      try {
        const s = await invoke<RemoteServerStatus>("remote_server_status");
        if (!cancelled) applyStatus(s);
      } catch (e) {
        if (!cancelled) setError(errText(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [applyStatus]);

  useEffect(
    () => () => {
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
    },
    [],
  );

  const running = status?.running ?? false;

  async function handleToggle(next: boolean) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<RemoteServerStatus>(
        next ? "remote_server_start" : "remote_server_stop",
      );
      applyStatus(s);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  // Commit a new port: validate, persist via remote_server_set_port, and —
  // if the server is currently running — bounce it so the new port binds.
  async function commitPort() {
    const parsed = Number(portInput);
    if (
      !Number.isInteger(parsed) ||
      parsed < MIN_PORT ||
      parsed > MAX_PORT
    ) {
      // Reject + revert to the last good value.
      setError(t("remote.server.port_invalid", { min: MIN_PORT, max: MAX_PORT }));
      setPortInput(String(status?.port ?? DEFAULT_PORT));
      return;
    }
    if (parsed === status?.port) return; // no change
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("remote_server_set_port", { port: parsed });
      if (running) {
        // Rebind: stop then start so the new port takes effect now.
        await invoke<RemoteServerStatus>("remote_server_stop");
        const s = await invoke<RemoteServerStatus>("remote_server_start");
        applyStatus(s);
      } else {
        // Not running — just refresh status to reflect the stored port.
        const s = await invoke<RemoteServerStatus>("remote_server_status");
        applyStatus(s);
      }
    } catch (e) {
      setError(errText(e));
      setPortInput(String(status?.port ?? DEFAULT_PORT));
    } finally {
      setBusy(false);
    }
  }

  function flashCopied(what: "pin" | "url") {
    setCopied(what);
    if (copiedTimer.current) clearTimeout(copiedTimer.current);
    copiedTimer.current = setTimeout(() => setCopied(null), 1800);
  }

  async function copyText(text: string, what: "pin" | "url") {
    try {
      await navigator.clipboard.writeText(text);
      flashCopied(what);
    } catch {
      /* clipboard blocked — ignore, the value is shown anyway */
    }
  }

  // Defensive: never render outside Tauri (SettingsPanel already guards).
  if (!isTauri) return null;

  const primaryUrl = status?.urls?.[0] ?? null;

  return (
    <div className="settings__section remote-panel">
      <h3>{t("remote.server.section_title")}</h3>
      <p className="settings__row-hint">{t("remote.server.intro")}</p>

      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={running}
          disabled={busy}
          onChange={(e) => void handleToggle(e.target.checked)}
        />
        <span>
          <strong>{t("remote.server.toggle_label")}</strong>
          <span className="settings__row-hint">
            {t("remote.server.toggle_hint")}
          </span>
        </span>
      </label>

      <label className="settings__field">
        <span className="settings__field-label">
          {t("remote.server.port_label")}
        </span>
        <input
          type="number"
          inputMode="numeric"
          min={MIN_PORT}
          max={MAX_PORT}
          value={portInput}
          disabled={busy}
          onChange={(e) => setPortInput(e.target.value.replace(/[^0-9]/g, ""))}
          onBlur={() => void commitPort()}
          className="remote-panel__port-input"
        />
        <small className="settings__row-hint">
          {t("remote.server.port_hint")}
        </small>
      </label>

      {error && (
        <p className="remote-panel__error" role="alert">
          {error}
        </p>
      )}

      {running && status && (
        <div className="remote-panel__live">
          {/* PIN — shown large; this is the primary thing a pilot reads off
              to a tablet. */}
          <div className="remote-panel__pin-block">
            <span className="settings__field-label">
              {t("remote.server.pin_label")}
            </span>
            <div className="remote-panel__pin-row">
              <code className="remote-panel__pin">{status.pin}</code>
              <button
                type="button"
                className="remote-panel__copy"
                onClick={() => void copyText(status.pin, "pin")}
              >
                {copied === "pin"
                  ? t("remote.server.copied")
                  : t("remote.server.copy")}
              </button>
            </div>
          </div>

          {/* QR — backend gives an <svg> data-URL; render it directly. */}
          {status.qr_svg && (
            <div className="remote-panel__qr-block">
              <span className="settings__field-label">
                {t("remote.server.qr_label")}
              </span>
              <img
                className="remote-panel__qr"
                src={status.qr_svg}
                alt={t("remote.server.qr_alt")}
                width={180}
                height={180}
              />
              <small className="settings__row-hint">
                {t("remote.server.qr_hint")}
              </small>
            </div>
          )}

          {/* Candidate LAN URLs. */}
          <div className="remote-panel__urls-block">
            <span className="settings__field-label">
              {t("remote.server.url_label")}
            </span>
            {status.urls.length === 0 ? (
              <p className="settings__row-hint">
                {t("remote.server.no_urls")}
              </p>
            ) : (
              <ul className="remote-panel__urls">
                {status.urls.map((url) => (
                  <li key={url} className="remote-panel__url-row">
                    <code className="remote-panel__url">{url}</code>
                  </li>
                ))}
              </ul>
            )}
            {primaryUrl && (
              <button
                type="button"
                className="remote-panel__copy"
                onClick={() => void copyText(primaryUrl, "url")}
              >
                {copied === "url"
                  ? t("remote.server.copied")
                  : t("remote.server.copy_url")}
              </button>
            )}
          </div>
        </div>
      )}

      {/* Firewall note — applies on first start regardless of running state. */}
      <p className="remote-panel__note">{t("remote.server.firewall_note")}</p>
      {/* Security warning — loud, always visible. */}
      <p className="remote-panel__warning" role="note">
        {t("remote.server.security_warning")}
      </p>
    </div>
  );
}

/** Extract a human string from a thrown UiError ({code,message}) or any other
 *  error shape. */
function errText(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}
