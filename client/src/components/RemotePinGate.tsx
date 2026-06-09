// Full-screen PIN gate for the LAN browser build (v0.16.0).
//
// Rendered by main.tsx ONLY in the browser path when there is no valid remote
// token. The Tauri desktop build never mounts this. It:
//
//   1. on mount, tries the QR auto-PIN flow (`?pin=NNNNNN` in the URL),
//   2. otherwise shows a 6-digit PIN form that POSTs `/api/auth`,
//   3. re-appears automatically if a later request 401s (token expired) —
//      `onReauthNeeded` flips the parent back into "locked" state.
//
// Bilingual via the same i18next instance as the rest of the app (DE primary).

import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { authenticateWithPin } from "../lib/ipc";

interface Props {
  /** Called once a token has been obtained — parent re-renders the real app. */
  onAuthenticated: () => void;
}

export function RemotePinGate({ onAuthenticated }: Props) {
  const { t } = useTranslation();
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"bad_pin" | "network" | null>(null);

  async function attempt(value: string): Promise<boolean> {
    setBusy(true);
    setError(null);
    try {
      const token = await authenticateWithPin(value);
      if (token) {
        onAuthenticated();
        return true;
      }
      setError("bad_pin");
      return false;
    } catch {
      setError("network");
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (busy || pin.length < 4) return;
    const ok = await attempt(pin);
    if (!ok) setPin("");
  }

  // QR / deep-link case: main.tsx already consumed `?pin=` before mount, so by
  // the time we render the token usually exists and this gate never shows. But
  // if that auto-auth raced or failed, we still present the manual form.
  useEffect(() => {
    // no auto-action here; consumePinFromUrl runs in main.tsx bootstrap.
  }, []);

  return (
    <div className="remote-pin-gate">
      <form className="remote-pin-gate__card" onSubmit={onSubmit}>
        <h1 className="remote-pin-gate__title">{t("remote.gate.title")}</h1>
        <p className="remote-pin-gate__subtitle">{t("remote.gate.subtitle")}</p>
        <input
          className="remote-pin-gate__input"
          type="text"
          inputMode="numeric"
          pattern="[0-9]*"
          autoComplete="one-time-code"
          autoFocus
          maxLength={6}
          value={pin}
          onChange={(e) => setPin(e.target.value.replace(/\D/g, "").slice(0, 6))}
          placeholder="••••••"
          aria-label={t("remote.gate.pinLabel")}
          disabled={busy}
        />
        {error === "bad_pin" && (
          <p className="remote-pin-gate__error" role="alert">
            {t("remote.gate.badPin")}
          </p>
        )}
        {error === "network" && (
          <p className="remote-pin-gate__error" role="alert">
            {t("remote.gate.network")}
          </p>
        )}
        <button
          className="remote-pin-gate__submit"
          type="submit"
          disabled={busy || pin.length < 4}
        >
          {busy ? t("remote.gate.connecting") : t("remote.gate.unlock")}
        </button>
        <p className="remote-pin-gate__hint">{t("remote.gate.hint")}</p>
      </form>
    </div>
  );
}
