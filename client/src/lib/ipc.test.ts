// Tests for the LAN-remote IPC seam (v0.16.0, #LAN-Remote).
//
// These exercise the BROWSER branch of `lib/ipc` — i.e. the path taken when
// there is no Tauri runtime. In jsdom `window.__TAURI_INTERNALS__` is absent,
// so `isTauri` is false and `invoke`/`listen` route over HTTP/WebSocket.
//
// Coverage:
//   - invoke() success (200 → parsed JSON), UiError (422 → thrown {code,message}),
//     401 (token cleared + re-auth signalled + thrown), 404 (thrown).
//   - listen() shared-WebSocket registry: one socket for N listeners, frame
//     dispatch to the right per-event callbacks, and unlisten cleanup.
//   - the QR `?pin=` consume flow + token storage helpers.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

// In-memory localStorage stub (jsdom's may be absent / restricted under node).
function installLocalStorage() {
  const mem = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => (mem.has(k) ? mem.get(k)! : null),
    setItem: (k: string, v: string) => void mem.set(k, String(v)),
    removeItem: (k: string) => void mem.delete(k),
    clear: () => mem.clear(),
    get length() {
      return mem.size;
    },
    key: (i: number) => Array.from(mem.keys())[i] ?? null,
  });
}

// A minimal controllable WebSocket double. Tracks instances so a test can
// assert "only one socket was opened" and drive onopen/onmessage/onclose.
class FakeWebSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  url: string;
  readyState = FakeWebSocket.CONNECTING;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }
  // Test driver helpers.
  fireOpen() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }
  fireMessage(obj: unknown) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
  close() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.();
  }
}

let ipc: typeof import("./ipc");

beforeEach(async () => {
  installLocalStorage();
  vi.stubGlobal("WebSocket", FakeWebSocket as unknown as typeof WebSocket);
  FakeWebSocket.instances = [];
  // Fresh module each test so the WS/registry singletons reset cleanly.
  vi.resetModules();
  ipc = await import("./ipc");
  ipc.__resetIpcForTests();
});

afterEach(() => {
  ipc.__resetIpcForTests();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function mockFetch(handler: (url: string, init?: RequestInit) => Response) {
  const fn = vi.fn(
    async (url: string, init?: RequestInit) => handler(url, init),
  );
  vi.stubGlobal("fetch", fn);
  return fn;
}

function jsonResponse(status: number, body?: unknown): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    json: async () => body,
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as unknown as Response;
}

describe("ipc.isTauri", () => {
  it("is false in jsdom (no Tauri runtime)", () => {
    expect(ipc.isTauri).toBe(false);
  });
});

describe("browser invoke()", () => {
  it("POSTs to /api/cmd/<cmd> with the token header and parses 200 JSON", async () => {
    ipc.setRemoteToken("tok-123");
    const fetchMock = mockFetch(() => jsonResponse(200, { ok: true, n: 7 }));

    const result = await ipc.invoke<{ ok: boolean; n: number }>("do_thing", {
      a: 1,
    });

    expect(result).toEqual({ ok: true, n: 7 });
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("/api/cmd/do_thing");
    expect(init?.method).toBe("POST");
    const headers = init?.headers as Record<string, string>;
    expect(headers["X-AeroACARS-Token"]).toBe("tok-123");
    expect(headers["Content-Type"]).toBe("application/json");
    expect(init?.body).toBe(JSON.stringify({ a: 1 }));
  });

  it("sends an empty-object body when no args are given", async () => {
    ipc.setRemoteToken("tok");
    const fetchMock = mockFetch(() => jsonResponse(200, null));
    await ipc.invoke("ping");
    const init = fetchMock.mock.calls[0]![1];
    expect(init?.body).toBe("{}");
  });

  it("throws the {code,message} UiError on 422 (mirrors Tauri reject)", async () => {
    ipc.setRemoteToken("tok");
    mockFetch(() =>
      jsonResponse(422, { code: "bad_state", message: "Kein aktiver Flug" }),
    );

    await expect(ipc.invoke("end_flight")).rejects.toEqual({
      code: "bad_state",
      message: "Kein aktiver Flug",
    });
  });

  it("clears the token, signals re-auth, and throws on 401", async () => {
    ipc.setRemoteToken("stale");
    expect(ipc.hasRemoteToken()).toBe(true);
    const reauth = vi.fn();
    ipc.onReauthNeeded(reauth);
    mockFetch(() => jsonResponse(401));

    await expect(ipc.invoke("whatever")).rejects.toMatchObject({
      code: "unauthorized",
    });
    expect(ipc.hasRemoteToken()).toBe(false);
    expect(reauth).toHaveBeenCalledTimes(1);
  });

  it("throws an unknown_command UiError on 404", async () => {
    ipc.setRemoteToken("tok");
    mockFetch(() => jsonResponse(404));
    await expect(ipc.invoke("nope")).rejects.toMatchObject({
      code: "unknown_command",
    });
  });
});

describe("browser listen() — shared WebSocket registry", () => {
  it("opens exactly one socket for multiple listeners and dispatches by event", async () => {
    ipc.setRemoteToken("tok");

    const aCb = vi.fn();
    const bCb = vi.fn();
    const flagCb = vi.fn();

    const unA = await ipc.listen("flight_status", aCb);
    const unB = await ipc.listen("flight_status", bCb);
    const unFlag = await ipc.listen("integrity-flag", flagCb);

    // One socket, regardless of listener count.
    expect(FakeWebSocket.instances.length).toBe(1);
    const sock = FakeWebSocket.instances[0]!;
    expect(sock.url).toContain("/ws?token=tok");
    sock.fireOpen();

    // A flight_status frame reaches both flight_status listeners, not the flag one.
    sock.fireMessage({ event: "flight_status", payload: { phase: "Cruise" } });
    expect(aCb).toHaveBeenCalledTimes(1);
    expect(aCb).toHaveBeenCalledWith({
      event: "flight_status",
      payload: { phase: "Cruise" },
    });
    expect(bCb).toHaveBeenCalledTimes(1);
    expect(flagCb).not.toHaveBeenCalled();

    // An integrity-flag frame reaches only the flag listener.
    sock.fireMessage({ event: "integrity-flag", payload: { severity: "anomaly" } });
    expect(flagCb).toHaveBeenCalledTimes(1);
    expect(aCb).toHaveBeenCalledTimes(1);

    // Unlistening one of two flight_status cbs keeps the other working.
    unA();
    sock.fireMessage({ event: "flight_status", payload: { phase: "Approach" } });
    expect(aCb).toHaveBeenCalledTimes(1);
    expect(bCb).toHaveBeenCalledTimes(2);

    unB();
    unFlag();
  });

  it("ignores malformed frames without throwing", async () => {
    ipc.setRemoteToken("tok");
    const cb = vi.fn();
    await ipc.listen("flight_status", cb);
    const sock = FakeWebSocket.instances[0]!;
    sock.fireOpen();
    expect(() => sock.onmessage?.({ data: "not json {{{" })).not.toThrow();
    expect(cb).not.toHaveBeenCalled();
  });

  it("does not open a socket until a token exists", async () => {
    // No token set.
    const cb = vi.fn();
    await ipc.listen("flight_status", cb);
    expect(FakeWebSocket.instances.length).toBe(0);
  });
});

describe("token helpers + QR ?pin= flow", () => {
  it("authenticateWithPin stores the token on success and returns null on 401", async () => {
    const fetchMock = mockFetch((url) => {
      expect(url).toBe("/api/auth");
      return jsonResponse(200, { token: "pin-token" });
    });
    const token = await ipc.authenticateWithPin("123456");
    expect(token).toBe("pin-token");
    expect(ipc.getRemoteToken()).toBe("pin-token");
    expect(fetchMock.mock.calls[0]![1]?.body).toBe(
      JSON.stringify({ pin: "123456" }),
    );

    ipc.clearRemoteToken();
    mockFetch(() => jsonResponse(401));
    expect(await ipc.authenticateWithPin("000000")).toBeNull();
    expect(ipc.getRemoteToken()).toBeNull();
  });

  it("consumePinFromUrl auto-authenticates and strips ?pin= from the URL", async () => {
    window.history.replaceState({}, "", "/?pin=654321&tab=cockpit");
    mockFetch((url) => {
      expect(url).toBe("/api/auth");
      return jsonResponse(200, { token: "qr-token" });
    });

    const ok = await ipc.consumePinFromUrl();
    expect(ok).toBe(true);
    expect(ipc.getRemoteToken()).toBe("qr-token");
    // PIN stripped, other params preserved.
    expect(window.location.search).not.toContain("pin=");
    expect(window.location.search).toContain("tab=cockpit");
  });

  it("consumePinFromUrl is a no-op without ?pin=", async () => {
    window.history.replaceState({}, "", "/?tab=map");
    const fetchMock = mockFetch(() => jsonResponse(200, { token: "x" }));
    expect(await ipc.consumePinFromUrl()).toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
