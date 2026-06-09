// LAN remote-control IPC seam (v0.16.0).
//
// **Why this file exists.** The SAME React bundle has to run in TWO places:
//
//   1. the Tauri desktop app (the PC running the sim), where `invoke`/`listen`
//      talk to the Rust backend over the native Tauri IPC bridge, and
//   2. a plain LAN browser (a tablet on the same Wi-Fi), where there is NO
//      Tauri runtime — the same calls must go over HTTP/WebSocket to the
//      companion axum server the desktop app hosts.
//
// Every call site imports `invoke`/`listen` from HERE instead of directly from
// `@tauri-apps/api`, so the environment switch happens in one place. Call sites
// keep identical names/args/return-shapes — including the reject shape: the
// backend returns a `{code,message}` UiError as HTTP 422, which we THROW so it
// matches Tauri's `invoke()` rejection (callers already `.catch()` on that).
//
// The switch is a RUNTIME decision (`isTauri`), NOT a build-time one — the same
// `client/dist` bundle is served to both. Therefore this module must NOT do a
// top-level static `import` of `@tauri-apps/api/*` (that would pull the Tauri
// runtime into the browser path and can throw at module-eval time when the
// Tauri globals are absent). Instead we lazy `import()` the real APIs only on
// the Tauri branch.

/** True when running inside the Tauri webview (native IPC available). */
export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ---------------------------------------------------------------------------
// Token storage + re-auth signalling (browser only).
// ---------------------------------------------------------------------------

const TOKEN_KEY = "aa-remote-token";

/** Read the stored LAN remote token (browser). Null in Tauri / when unset. */
export function getRemoteToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

/** Persist the LAN remote token (browser). */
export function setRemoteToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, token);
  } catch {
    /* localStorage disabled — token lives only in memory for this load */
    memoryToken = token;
  }
}

/** Drop the stored token (e.g. after a 401) and ask the UI to re-auth. */
export function clearRemoteToken(): void {
  try {
    localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* noop */
  }
  memoryToken = null;
  // Tear down the live socket — it is now authenticated with a dead token.
  closeSocket();
  notifyReauth();
}

// Fallback when localStorage is unavailable (private-mode Safari etc.).
let memoryToken: string | null = null;

function currentToken(): string | null {
  return getRemoteToken() ?? memoryToken;
}

// The PIN-gate component subscribes here; when the token is cleared we flip it
// back into "needs PIN" state without a full reload.
type ReauthListener = () => void;
const reauthListeners = new Set<ReauthListener>();

/** Subscribe to re-auth requests (PIN gate uses this). Returns unsubscribe. */
export function onReauthNeeded(cb: ReauthListener): () => void {
  reauthListeners.add(cb);
  return () => reauthListeners.delete(cb);
}

function notifyReauth(): void {
  for (const cb of reauthListeners) {
    try {
      cb();
    } catch {
      /* a bad listener must not break token handling */
    }
  }
}

/** Whether the browser build currently has a usable token. */
export function hasRemoteToken(): boolean {
  return currentToken() != null;
}

/**
 * POST a PIN to `/api/auth`. On success stores + returns the token; on a 401
 * (bad PIN) returns null. Throws only on transport errors.
 */
export async function authenticateWithPin(pin: string): Promise<string | null> {
  const res = await fetch("/api/auth", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pin }),
  });
  if (res.status === 401) return null;
  if (!res.ok) throw new Error(`auth failed: HTTP ${res.status}`);
  const data = (await res.json()) as { token: string };
  setRemoteToken(data.token);
  return data.token;
}

/**
 * QR-flow bootstrap: if the URL carries `?pin=NNNNNN`, auto-authenticate with
 * it and strip the param from the address bar (so the PIN is not left in
 * history / shared links). Returns true if a token was obtained this way.
 *
 * Safe to call unconditionally on load — it no-ops in Tauri and when there is
 * no `?pin=`.
 */
export async function consumePinFromUrl(): Promise<boolean> {
  if (isTauri || typeof window === "undefined") return false;
  let pin: string | null = null;
  try {
    const url = new URL(window.location.href);
    pin = url.searchParams.get("pin");
    if (pin) {
      // Strip it regardless of auth outcome — never leave it in the URL.
      url.searchParams.delete("pin");
      window.history.replaceState(
        {},
        document.title,
        url.pathname + (url.search ? url.search : "") + url.hash,
      );
    }
  } catch {
    return false;
  }
  if (!pin) return false;
  try {
    const token = await authenticateWithPin(pin);
    return token != null;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// invoke()
// ---------------------------------------------------------------------------

/** Reject shape contract: a backend UiError (HTTP 422) is thrown as-is. */
export interface UiError {
  code: string;
  message: string;
}

// Lazily-resolved real Tauri invoke (only ever loaded inside Tauri).
let tauriInvoke:
  | (<T>(cmd: string, args?: Record<string, unknown>) => Promise<T>)
  | null = null;
let tauriInvokeLoad: Promise<void> | null = null;

async function ensureTauriInvoke(): Promise<void> {
  if (tauriInvoke) return;
  if (!tauriInvokeLoad) {
    tauriInvokeLoad = import("@tauri-apps/api/core").then((m) => {
      tauriInvoke = m.invoke as <T>(
        cmd: string,
        args?: Record<string, unknown>,
      ) => Promise<T>;
    });
  }
  await tauriInvokeLoad;
}

async function browserInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const token = currentToken();
  const res = await fetch(`/api/cmd/${cmd}`, {
    method: "POST",
    headers: {
      "X-AeroACARS-Token": token ?? "",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(args ?? {}),
  });

  if (res.status === 200) {
    // 204 / empty body → undefined; otherwise parse JSON.
    const text = await res.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }
  if (res.status === 422) {
    // UiError — throw the {code,message} object to mirror Tauri's reject.
    let err: UiError;
    try {
      err = (await res.json()) as UiError;
    } catch {
      err = { code: "unknown", message: `HTTP 422 (${cmd})` };
    }
    throw err;
  }
  if (res.status === 401) {
    // Stale/missing token — drop it and ask the UI to re-auth.
    clearRemoteToken();
    throw { code: "unauthorized", message: "Session abgelaufen" } satisfies UiError;
  }
  if (res.status === 404) {
    throw {
      code: "unknown_command",
      message: `Unbekannter Befehl: ${cmd}`,
    } satisfies UiError;
  }
  throw new Error(`invoke ${cmd} failed: HTTP ${res.status}`);
}

/**
 * Drop-in replacement for Tauri's `invoke`. In Tauri it forwards to the native
 * bridge; in a LAN browser it POSTs to `/api/cmd/{cmd}` with the bearer token.
 */
export async function invoke<T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri) {
    await ensureTauriInvoke();
    return tauriInvoke!<T>(cmd, args);
  }
  return browserInvoke<T>(cmd, args);
}

// ---------------------------------------------------------------------------
// listen()
// ---------------------------------------------------------------------------

/** Mirrors Tauri's event payload envelope so call sites are unchanged. */
export interface IpcEvent<T> {
  event: string;
  payload: T;
}

/** Mirrors Tauri's `UnlistenFn`. */
export type UnlistenFn = () => void;

type AnyCb = (event: IpcEvent<unknown>) => void;

// ----- Tauri branch: forward to the real listen -----

let tauriListen:
  | (<T>(
      event: string,
      handler: (e: { event: string; payload: T }) => void,
    ) => Promise<() => void>)
  | null = null;
let tauriListenLoad: Promise<void> | null = null;

async function ensureTauriListen(): Promise<void> {
  if (tauriListen) return;
  if (!tauriListenLoad) {
    tauriListenLoad = import("@tauri-apps/api/event").then((m) => {
      tauriListen = m.listen as typeof tauriListen;
    });
  }
  await tauriListenLoad;
}

// ----- Browser branch: one shared WebSocket + a per-event cb registry -----

const browserRegistry = new Map<string, Set<AnyCb>>();
let ws: WebSocket | null = null;
let wsWantOpen = false;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000; // backs off to 15s

function closeSocket(): void {
  wsWantOpen = false;
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (ws) {
    try {
      ws.onclose = null; // don't trigger our reconnect on an intentional close
      ws.close();
    } catch {
      /* noop */
    }
    ws = null;
  }
}

function scheduleReconnect(): void {
  if (!wsWantOpen || reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    openSocket();
  }, reconnectDelay);
  reconnectDelay = Math.min(reconnectDelay * 2, 15000);
}

function openSocket(): void {
  if (typeof window === "undefined") return;
  const token = currentToken();
  if (!token) {
    // No token yet — wait; a fresh listen() call after auth re-triggers this.
    return;
  }
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
    return;
  }
  wsWantOpen = true;

  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${proto}//${window.location.host}/ws?token=${encodeURIComponent(token)}`;

  let socket: WebSocket;
  try {
    socket = new WebSocket(url);
  } catch {
    scheduleReconnect();
    return;
  }
  ws = socket;

  socket.onopen = () => {
    reconnectDelay = 1000; // reset backoff on a clean connect
  };
  socket.onmessage = (msg) => {
    let parsed: IpcEvent<unknown>;
    try {
      parsed = JSON.parse(msg.data as string) as IpcEvent<unknown>;
    } catch {
      return; // ignore malformed frames
    }
    const cbs = browserRegistry.get(parsed.event);
    if (!cbs) return;
    for (const cb of cbs) {
      try {
        cb(parsed);
      } catch {
        /* a bad handler must not kill the dispatch loop */
      }
    }
  };
  socket.onclose = () => {
    if (ws === socket) ws = null;
    scheduleReconnect();
  };
  socket.onerror = () => {
    // onclose fires after onerror — reconnect is handled there.
    try {
      socket.close();
    } catch {
      /* noop */
    }
  };
}

function browserListen<T>(
  event: string,
  cb: (e: IpcEvent<T>) => void,
): UnlistenFn {
  let set = browserRegistry.get(event);
  if (!set) {
    set = new Set();
    browserRegistry.set(event, set);
  }
  const wrapped = cb as AnyCb;
  set.add(wrapped);

  // Ensure the shared socket is up (or coming up).
  openSocket();

  return () => {
    const s = browserRegistry.get(event);
    if (s) {
      s.delete(wrapped);
      if (s.size === 0) browserRegistry.delete(event);
    }
    // If absolutely nothing is listening anymore, drop the socket so a logged-
    // out tablet doesn't keep a dead connection alive.
    if (browserRegistry.size === 0) closeSocket();
  };
}

/**
 * Drop-in replacement for Tauri's `listen`. In Tauri it forwards to the native
 * event bus; in a LAN browser it multiplexes a single shared WebSocket and
 * dispatches `{event,payload}` frames to per-event callbacks.
 *
 * Returns a Promise<UnlistenFn> to keep the exact Tauri signature (call sites
 * already `await` it or `.then(f => f())`).
 */
export async function listen<T = unknown>(
  event: string,
  cb: (e: IpcEvent<T>) => void,
): Promise<UnlistenFn> {
  if (isTauri) {
    await ensureTauriListen();
    return tauriListen!<T>(event, cb as (e: { event: string; payload: T }) => void);
  }
  return browserListen<T>(event, cb);
}

/**
 * Open a portable external URL. In Tauri this routes through the opener plugin
 * (native browser); in a LAN browser it opens a new tab. Centralised here so
 * the plugin import never reaches the browser bundle's eval path.
 */
export async function openExternal(url: string): Promise<void> {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

// Test-only reset hook (no-op in production paths). Lets unit tests clear the
// browser WebSocket/registry state between cases.
export function __resetIpcForTests(): void {
  closeSocket();
  browserRegistry.clear();
  reconnectDelay = 1000;
  memoryToken = null;
  reauthListeners.clear();
}
