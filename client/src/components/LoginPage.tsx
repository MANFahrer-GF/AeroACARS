import { type FormEvent, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type { LoginResult, UiError } from "../types";

interface Props {
  initialUrl?: string;
  /** v0.12.1 (Stream B LE6): pre-seeded error — set when a session
   *  restore was blocked by the pilot-status gate, so the form opens
   *  with the status-specific message instead of blank. */
  initialError?: UiError | null;
  onSuccess: (result: LoginResult) => void;
}

const KNOWN_ERROR_CODES = new Set([
  "invalid_url",
  "network",
  "unauthenticated",
  "forbidden",
  "not_found",
  "rate_limited",
  "server",
  "bad_response",
  "keyring",
  "config_path",
  "config_read",
  "config_write",
  "config_parse",
  // v0.12.1 (Stream B) — pilot-status gate
  "pilot_pending",
  "pilot_rejected",
  "pilot_on_leave",
  "pilot_suspended",
  "pilot_state_unknown",
]);

function errorKey(code: string): string {
  return KNOWN_ERROR_CODES.has(code)
    ? `login.error.${code}`
    : "login.error.unknown";
}

function isUiError(value: unknown): value is UiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value
  );
}

/**
 * Hardcoded phpVMS host this build is locked to. Mirrors the
 * backend's `ALLOWED_PHPVMS_HOST` constant — the backend ignores
 * whatever URL the form sent and always uses this, so we just
 * surface it read-only in the UI for transparency.
 */
const LOCKED_HOST = "https://german-sky-group.eu";

export function LoginPage({
  initialUrl: _initialUrl = "",
  initialError = null,
  onSuccess,
}: Props) {
  const { t } = useTranslation();
  const [apiKey, setApiKey] = useState("");
  const [submitting, setSubmitting] = useState(false);
  // v0.12.1 (Stream B LE6): seed with the restore-block error if present.
  const [error, setError] = useState<UiError | null>(initialError);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const result = await invoke<LoginResult>("phpvms_login", {
        // The backend ignores `url` and uses ALLOWED_PHPVMS_HOST,
        // but we still pass the locked value for clarity in any
        // future logging / debugging.
        url: LOCKED_HOST,
        apiKey: apiKey.trim(),
      });
      onSuccess(result);
    } catch (err: unknown) {
      if (isUiError(err)) {
        setError(err);
      } else {
        setError({ code: "unknown", message: String(err) });
      }
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <section className="login">
      <h2>{t("login.title")}</h2>
      <p className="login__description">{t("login.description")}</p>

      <form className="login__form" onSubmit={handleSubmit}>
        <label className="field">
          <span className="field__label">{t("login.url_label")}</span>
          <input
            type="url"
            value={LOCKED_HOST}
            readOnly
            disabled
          />
        </label>

        <label className="field">
          <span className="field__label">{t("login.api_key_label")}</span>
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            required
            placeholder={t("login.api_key_placeholder")}
            value={apiKey}
            onChange={(e) => setApiKey(e.currentTarget.value)}
            disabled={submitting}
          />
        </label>

        {error && (
          <div className="login__error" role="alert">
            {t(errorKey(error.code))}
          </div>
        )}

        <button
          type="submit"
          className="button button--primary"
          disabled={submitting || !apiKey}
        >
          {submitting ? t("login.submitting") : t("login.submit")}
        </button>
      </form>
    </section>
  );
}
