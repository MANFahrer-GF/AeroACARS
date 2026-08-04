// v0.9.0 (#GlitchTip) — Sentry-Init fuer das Tauri-Frontend (React).
//
// Spec: docs/spec/v0.9.0-glitchtip-self-hosted.md
//   + docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (Datenschutz-Gates)
//
// Anders als Recorder/Webapp ist der Client ein Pilot-facing Tool
// → Opt-In ist Pflicht (DSGVO Art. 6 (1) a). Default = aus.
//
// DSN: Build-time via Vite-env VITE_SENTRY_DSN_CLIENT.
// Consent: localStorage `aeroacars.errorReporting.enabled` ("true"|"false")
//   + Mirror in den Rust-Backend via Tauri-Command `error_reporting_set_consent`.
//
// Privacy:
//   - beforeSend strippt nicht-allowlisted Tags, User-PII, Request-Daten
//   - Wenn !consent → Event verworfen (Defense-in-Depth: Rust hat eigene Atomic-Gate)

import * as Sentry from "@sentry/react";

export const STORAGE_KEY = "aeroacars.errorReporting.enabled";

const ALLOWED_TAG_KEYS: ReadonlySet<string> = new Set([
  "app.component",
  "app.version",
  "os",
  "os.version",
  "simulator",
  "aircraft",
  "airport",
  "runway",
  "pirep.id",
  "callsign",
  // v0.9.0 QS-Hotfix: "route" entfernt — Privacy-Hint sagt "Route NICHT
  // gesendet", also auch nicht erlauben.
  "phase",
  "pilot.hash",
  "forensics.version",
  "error.code",
  "error.status_code",
  "error.kind",
  "feature.flag",
  "distance.bucket",
]);

export function getConsent(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

/**
 * Pilot hat noch nie eine Entscheidung getroffen.
 * Wird vom First-Run-Banner abgefragt.
 */
export function consentIsUnset(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === null;
  } catch {
    return true;
  }
}

export function setConsent(enabled: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, enabled ? "true" : "false");
  } catch {
    // ignore
  }
}

/**
 * Initialisiert Sentry. No-Op wenn DSN nicht in build-time-env gesetzt war.
 * MUSS sehr frueh in main.tsx aufgerufen werden.
 */
export function initSentry(appVersion: string): boolean {
  const dsn = (import.meta.env.VITE_SENTRY_DSN_CLIENT as string | undefined)?.trim();
  if (!dsn) {
    // Kein Log noisy machen — viele Builds laufen ohne DSN (Dev, contributors).
    return false;
  }

  Sentry.init({
    dsn,
    release: `aeroacars-client@${appVersion}`,
    environment: import.meta.env.PROD ? "production" : "development",
    sendDefaultPii: false,
    tracesSampleRate: 0,
    // v0.9.0 QS-Hotfix F6: KEIN `integrations: []` mehr — das hatte ALLE
    // Default-Integrations deaktiviert. Pilots-JS-Crashes ausserhalb React-
    // ErrorBoundary (window.onerror, unhandled-promise-rejections) gingen
    // verloren. Jetzt: Defaults aktiv. Replay/Profiling sind opt-in,
    // werden also nicht aktiviert.
    beforeSend(event) {
      // Consent-Gate. Wird live aus localStorage gelesen, damit Toggle sofort wirkt.
      if (!getConsent()) return null;
      try {
        return redactEvent(event);
      } catch (err) {
        console.warn("[sentry] redactEvent failed, dropping:", err);
        return null;
      }
    },
    beforeBreadcrumb: redactBreadcrumb,
    initialScope: {
      tags: {
        "app.component": "client",
        "app.version": appVersion,
      },
    },
  });
  return true;
}

/**
 * v0.19.x FIX: strips query strings from breadcrumb URLs. Previously only
 * covered `fetch`/`xhr` breadcrumbs (`data.url`) — Sentry's browser history
 * integration also emits `navigation` breadcrumbs with `data.from`/`data.to`
 * URLs, which were NOT covered. `ipc.ts`'s `consumePinFromUrl()` uses
 * `history.replaceState` specifically to strip a `?pin=NNNNNN` LAN-pairing
 * PIN from the address bar as soon as the app reads it — but the PIN is
 * still in the URL at the instant that navigation happens, so it could
 * otherwise reach a future error report verbatim in a navigation
 * breadcrumb's `to` (or `from`) field.
 */
export function redactBreadcrumb(crumb: Sentry.Breadcrumb): Sentry.Breadcrumb {
  if (crumb.category === "fetch" || crumb.category === "xhr") {
    if (crumb.data?.url && typeof crumb.data.url === "string") {
      crumb.data.url = crumb.data.url.split("?")[0];
    }
  }
  if (crumb.category === "navigation" && crumb.data) {
    for (const key of ["from", "to"] as const) {
      const v = crumb.data[key];
      if (typeof v === "string") {
        crumb.data[key] = v.split("?")[0];
      }
    }
  }
  return crumb;
}

export function redactEvent(event: Sentry.ErrorEvent): Sentry.ErrorEvent {
  if (event.tags) {
    const cleanTags: Record<string, string | number | boolean> = {};
    for (const [key, value] of Object.entries(event.tags)) {
      if (ALLOWED_TAG_KEYS.has(key) && value != null) {
        cleanTags[key] = value as string | number | boolean;
      }
    }
    event.tags = cleanTags;
  }

  if (event.user) {
    event.user = {
      id: event.user.id && /^[a-f0-9]{1,16}$/i.test(String(event.user.id))
        ? event.user.id
        : undefined,
      email: undefined,
      username: undefined,
      ip_address: undefined,
    };
  }

  if (event.request) {
    event.request = {
      method: event.request.method,
      url: event.request.url?.split("?")[0],
    };
  }

  if (event.breadcrumbs) {
    event.breadcrumbs = event.breadcrumbs.map((crumb) => {
      if (crumb.data) {
        const cleanData: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(crumb.data)) {
          const kl = k.toLowerCase();
          if (
            kl.includes("token") ||
            kl.includes("password") ||
            kl.includes("authorization") ||
            kl.includes("cookie") ||
            kl === "body" ||
            kl === "request_body"
          ) {
            cleanData[k] = "[REDACTED]";
          } else {
            cleanData[k] = v;
          }
        }
        return { ...crumb, data: cleanData };
      }
      return crumb;
    });
  }

  if (event.exception?.values) {
    for (const ex of event.exception.values) {
      if (ex.stacktrace?.frames) {
        for (const frame of ex.stacktrace.frames) {
          delete frame.vars;
        }
      }
    }
  }

  return event;
}

export { Sentry };
