export type Theme = "dark" | "light";

const STORAGE_KEY = "aeroacars.theme";

/**
 * v0.19.x FIX: this was the only unguarded `localStorage` call in the lib
 * layer, and `applyTheme` runs at module-eval time in `main.tsx` — BEFORE
 * the Sentry `ErrorBoundary` wraps the tree. If `localStorage` throws here
 * (quota exceeded, private-browsing restrictions, a corrupted value), the
 * app fails to mount at all: a blank page with no error boundary to catch
 * it. Every other localStorage call in this codebase already wraps in
 * try/catch (see `lib/trackStore.ts`) — this brings theme handling in
 * line with that.
 */
export function getInitialTheme(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "dark" || stored === "light") return stored;
  } catch {
    /* localStorage unavailable — fall through to the media-query default */
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    /* best-effort — the theme still applies to the DOM this session,
       it just won't be remembered across a restart */
  }
}
