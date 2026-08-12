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
 * v0.19.x FIX: a 429 (rate-limited) response used to throw a plain `Error`,
 * indistinguishable from a genuine transport failure — RemotePinGate showed
 * "network error, are you on the same WiFi?" for a rate-limit, and the user
 * kept retrying, extending their own lockout. Callers can now check
 * `err instanceof HttpStatusError && err.status === 429`.
 */
export class HttpStatusError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "HttpStatusError";
  }
}

/**
 * POST a PIN to `/api/auth`. On success stores + returns the token; on a 401
 * (bad PIN) returns null. Throws only on transport errors or a non-2xx/401
 * HTTP status (see HttpStatusError for the 429 case specifically).
 */
export async function authenticateWithPin(pin: string): Promise<string | null> {
  const res = await fetch("/api/auth", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pin }),
  });
  if (res.status === 401) return null;
  if (!res.ok) throw new HttpStatusError(res.status, `auth failed: HTTP ${res.status}`);
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

/**
 * Format an `invoke()` rejection for display. A Tauri command's `Err(UiError)`
 * arrives as the plain `{code, message}` object itself (not a JS `Error`), so
 * `String(e)` on it renders the useless `"[object Object]"` instead of the
 * actual message — always go through this instead of `String(e)`/`String(err)`
 * at a catch site that might see a `UiError`.
 */
export function formatIpcError(e: unknown): string {
  if (e && typeof e === "object" && "message" in e && typeof (e as UiError).message === "string") {
    return (e as UiError).message;
  }
  if (e instanceof Error) return e.message;
  return String(e);
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

// ---------------------------------------------------------------------------
// Buendelung (v1.5.9, #lan-traegheit Teil 3)
// ---------------------------------------------------------------------------
//
// **Warum.** Eine Ansicht fragt beim Aufbau fuenf bis zwoelf Werte ab. Am PC
// ist das native Bruecke — Mikrosekunden. Im LAN-Browser ist jede Abfrage ein
// eigener HTTP-Rundlauf; auf dem Tablet summiert sich das sichtbar. Vorladen
// und Zwischenspeicher (v1.5.7) helfen beim ZWEITEN Mal; beim ersten Aufbau
// einer Ansicht gibt es nichts vorzuladen und nichts im Speicher.
//
// **Wie.** Alles, was im selben Arbeitsschritt (Tick) an Abfragen anfaellt,
// geht in EINER Anfrage raus. Die Sammelphase ist eine Mikroaufgabe — kein
// Timer, keine kuenstliche Wartezeit: der Aufruf verzoegert sich um weniger
// als eine Millisekunde und spart dafuer bis zu elf WLAN-Rundlaeufe.
//
// **Was NICHT gebuendelt wird:** der Tauri-Pfad (dort ist ein Aufruf ohnehin
// billig) und alles, was ueber die Obergrenze hinausgeht — der Rest geht als
// naechstes Buendel. Faellt die Sammelroute aus (aelterer Client-Server, der
// die Route nicht kennt), fallen die Aufrufe still auf Einzelanfragen
// zurueck; die Bruecke bleibt damit abwaertskompatibel.

/** Muss zur Obergrenze im Rust-Router passen (BATCH_MAX). */
const BUENDEL_MAX = 24;

interface OffenerAufruf {
  cmd: string;
  args?: Record<string, unknown>;
  fertig: (wert: unknown) => void;
  fehler: (e: unknown) => void;
}

let sammlung: OffenerAufruf[] = [];
let sammlungGeplant = false;
/** Wird auf true gesetzt, sobald die Bruecke die Sammelroute nicht kennt. */
let buendelnMoeglich = true;

function fehlerAus(status: number, cmd: string, body: unknown): unknown {
  if (status === 422 || status === 404) {
    const e = body as UiError | undefined;
    if (e && typeof e.code === "string") return e;
    return { code: "unknown", message: `HTTP ${status} (${cmd})` } satisfies UiError;
  }
  if (status === 401) {
    clearRemoteToken();
    return { code: "unauthorized", message: "Session abgelaufen" } satisfies UiError;
  }
  return new Error(`invoke ${cmd} failed: HTTP ${status}`);
}

async function sammlungAbschicken(): Promise<void> {
  const stapel = sammlung.slice(0, BUENDEL_MAX);
  sammlung = sammlung.slice(BUENDEL_MAX);
  sammlungGeplant = false;
  if (sammlung.length > 0) planeSammlung();
  if (stapel.length === 0) return;

  // Ein einzelner Aufruf gewinnt durch die Sammelroute nichts.
  if (stapel.length === 1) {
    const a = stapel[0];
    einzelInvoke(a.cmd, a.args).then(a.fertig, a.fehler);
    return;
  }

  try {
    const token = currentToken();
    const res = await fetch("/api/cmd-batch", {
      method: "POST",
      headers: {
        "X-AeroACARS-Token": token ?? "",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(stapel.map((a) => ({ name: a.cmd, args: a.args ?? {} }))),
    });
    if (res.status === 404 || res.status === 405) {
      // Die Gegenstelle kennt die Sammelroute nicht — ab jetzt einzeln.
      buendelnMoeglich = false;
      for (const a of stapel) einzelInvoke(a.cmd, a.args).then(a.fertig, a.fehler);
      return;
    }
    if (res.status !== 200) {
      const fehler = fehlerAus(res.status, "cmd-batch", undefined);
      for (const a of stapel) a.fehler(fehler);
      return;
    }
    const teile = (await res.json()) as Array<{
      status: number; value?: unknown; error?: UiError;
    }>;
    stapel.forEach((a, i) => {
      const t = teile[i];
      if (!t) { a.fehler(new Error(`invoke ${a.cmd}: keine Antwort im Buendel`)); return; }
      if (t.status === 200) a.fertig(t.value);
      else a.fehler(fehlerAus(t.status, a.cmd, t.error));
    });
  } catch (e) {
    // Netzfehler: nicht das ganze Buendel verlieren, sondern einzeln
    // nachfassen. Ist die Leitung wirklich tot, scheitern die auch — dann
    // aber mit dem Fehler, den die Aufrufer ohnehin erwarten.
    for (const a of stapel) einzelInvoke(a.cmd, a.args).then(a.fertig, a.fehler);
    void e;
  }
}

function planeSammlung(): void {
  if (sammlungGeplant) return;
  sammlungGeplant = true;
  queueMicrotask(() => { void sammlungAbschicken(); });
}

function browserInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!buendelnMoeglich) return einzelInvoke<T>(cmd, args);
  return new Promise<T>((fertig, fehler) => {
    sammlung.push({
      cmd, args,
      fertig: (w) => fertig(w as T),
      fehler,
    });
    planeSammlung();
  });
}

/** Nur fuer Tests: Sammelzustand zuruecksetzen. */
export function _buendelungZuruecksetzen(): void {
  sammlung = [];
  sammlungGeplant = false;
  buendelnMoeglich = true;
}

async function einzelInvoke<T>(
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
// ---------------------------------------------------------------------------
// Antwort-Zwischenspeicher (v1.5.7, #lan-traegheit)
// ---------------------------------------------------------------------------
//
// **Warum.** Feldbefund Thomas: Der Tab-Wechsel auf dem Tablet ist träge. Eine
// Ansicht fragt beim Öffnen 5–12 Werte einzeln ab; am PC ist das native IPC
// (Mikrosekunden), über die LAN-Brücke sind es 5–12 HTTP-Runden durchs WLAN.
// Wer zwischen Karte und Cockpit hin und her wechselt, bezahlt sie JEDES MAL
// neu — obwohl sich Flughafendaten, Logbuchseiten oder Flottenlisten in
// diesen Sekunden nicht ändern.
//
// **Wie.** Für ausgewählte LESENDE Befehle wird die letzte Antwort kurz
// behalten. Ein Wiederholungsaufruf bekommt sie SOFORT und stößt im
// Hintergrund eine Auffrischung an ("stale while revalidate") — die Ansicht
// steht also augenblicklich da und aktualisiert sich still.
//
// **Sicherheitsnetz.** Der Zwischenspeicher ist eine ausdrückliche Liste,
// KEINE Namensheuristik. Wer künftig einen Befehl hinzufügt und ihn nicht
// einträgt, bekommt exakt das heutige Verhalten — nie ein falsches Ergebnis.
// Schreibende Befehle (flight_start, hoppie_connect, …) stehen niemals drin.

/** Wie lange eine Antwort ohne Rückfrage weiterverwendet werden darf. */
const CACHE_TTL_MS = 20_000;

/**
 * Absolute Obergrenze fürs Ausliefern eines veralteten Werts.
 *
 * QS-Befund v1.5.7: Ohne diese Grenze konnte ein Wert BELIEBIG alt werden.
 * Die Auffrischung schreibt nur in den Speicher zurück — eine Ansicht, die
 * beim Öffnen einmal lädt (der Normalfall, alle Reiter werden beim Wechsel
 * neu aufgebaut), zeigte den alten Stand und korrigierte sich nie. Jenseits
 * dieser Grenze wird deshalb gewartet, bis frische Daten da sind.
 */
const CACHE_HARD_MAX_MS = 120_000;

/**
 * Lesende Befehle, deren Antwort sich in Sekundenfrist nicht sinnvoll
 * ändert. AUSDRÜCKLICHE Liste, keine Namensheuristik: Wer künftig einen
 * Befehl hinzufügt und ihn nicht einträgt, bekommt exakt das heutige
 * Verhalten — nie ein falsches Ergebnis.
 *
 * QS-Befund v1.5.7 — die Liste war zu weit gefasst und hat mehrere
 * absichtlich gebaute Aktualisierungswege gebrochen. Entfernt wurden:
 *
 *   `divert_nearest_airports` — GEFÄHRLICH: nimmt die Position NICHT als
 *      Argument (sie kommt aus dem Programmzustand), der Schlüssel war also
 *      sitzungsweit derselbe. Beim zweiten Flug einer Sitzung zeigte das
 *      Divert-Fenster die Liste des ERSTEN Flugs — und der gewählte Platz
 *      wandert als Zielflughafen ins PIREP.
 *   `metar_get` — den Aktualisieren-Knopf drückt man GENAU DANN, wenn sich
 *      das Wetter geändert hat (SPECI, Scherung, Sichtabfall).
 *   `landing_list` — nach dem Löschen einer Landung kam sie zurück; und
 *      der bewusste 5-s-Takt während des Ausrollens wäre ausgebremst.
 *   `phpvms_get_bids` — hätte den v1.4.7-Fix rückgängig gemacht (Flug
 *      erscheint nach dem Einreichen noch als nächster Flug).
 *   `va_live_flights` — 8-s-Takt ist nach Feldrückmeldung so gewollt.
 *   `logbook_stats` / `logbook_pireps` — direkt nach der Landung soll das
 *      Logbuch den neuen Flug zeigen, nicht den Stand von vorher.
 *   `news_fetch` — geringer Gewinn, aber verzögerte Meldungen.
 *
 * Übrig bleibt, was sich frühestens im Tagesrhythmus ändert.
 */
const CACHEABLE = new Set<string>([
  // Flughafen-Stammdaten — ändern sich nicht im Flug
  "airport_get",
  // Flugzeug-Stammdaten aus phpVMS
  "phpvms_get_aircraft",
  // Ein ABGESCHLOSSENER Flugbericht ändert sich nicht mehr
  "logbook_pirep",
]);

interface CacheSlot {
  at: number;
  value: unknown;
  /** Aus welcher Cache-Generation dieser Wert stammt (siehe oben). */
  gen: number;
  /** Läuft gerade eine Auffrischung? Verhindert Anfrage-Lawinen. */
  refreshing?: boolean;
}

const cache = new Map<string, CacheSlot>();

/**
 * Zähler gegen den Wettlauf beim Piloten-/VA-Wechsel (QS-Befund v1.5.7).
 *
 * Eine Auffrischung, die VOR dem Abmelden losgeschickt wurde, kam nach
 * `clearIpcCache()` zurück und schrieb ihre Antwort in den frisch
 * geleerten Speicher — der nächste Pilot sah dann Daten seines Vorgängers.
 * Jede Antwort merkt sich jetzt, aus welcher "Generation" sie stammt;
 * nach dem Leeren zählt der Wert hoch und verspätete Antworten werden
 * verworfen.
 */
let cacheGeneration = 0;

function cacheKey(cmd: string, args?: Record<string, unknown>): string {
  return args && Object.keys(args).length > 0
    ? `${cmd}:${JSON.stringify(args)}`
    : cmd;
}

/** Zwischenspeicher leeren — nach Abmelden/VA-Wechsel aufrufen. */
export function clearIpcCache(): void {
  cache.clear();
  // Alles, was noch unterwegs ist, gehört ab jetzt zur alten Generation
  // und darf den Speicher nicht mehr befüllen.
  cacheGeneration++;
}

async function rawInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri) {
    await ensureTauriInvoke();
    return tauriInvoke!<T>(cmd, args);
  }
  return browserInvoke<T>(cmd, args);
}

/**
 * Befehle, die die Sitzung beenden — danach darf NICHTS aus dem
 * Zwischenspeicher des vorigen Piloten überleben.
 *
 * QS-Runde 4: Vorher hing das an einem einzelnen Aufruf in der
 * Abmelde-Funktion. Den kann man löschen, ohne dass ein Test es merkt
 * (bewiesen per Mutation) — genau der Fehler, der schon einmal passiert
 * war (Leerfunktion existierte, wurde nirgends gerufen). Jetzt hängt das
 * Leeren am Befehl selbst: Wer sich abmeldet, leert den Speicher, egal
 * von welcher Stelle im Programm aus.
 */
const SESSION_ENDING = new Set<string>(["phpvms_logout"]);

export async function invoke<T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (SESSION_ENDING.has(cmd)) {
    try {
      return await rawInvoke<T>(cmd, args);
    } finally {
      // Auch wenn das Abmelden serverseitig scheitert: die lokale
      // Sitzung ist beendet, die Daten dürfen nicht liegen bleiben.
      clearIpcCache();
    }
  }
  if (!CACHEABLE.has(cmd)) return rawInvoke<T>(cmd, args);

  const key = cacheKey(cmd, args);
  // Generation bei Aufrufbeginn festhalten — siehe `cacheGeneration`.
  const gen = cacheGeneration;
  const slot = cache.get(key);

  if (slot && Date.now() - slot.at < CACHE_TTL_MS) {
    // Frisch genug — direkt zurück, ohne das Netz anzufassen.
    return slot.value as T;
  }

  // QS-Befund v1.5.7: jenseits der harten Grenze NICHT mehr ausliefern —
  // lieber kurz warten als beliebig alte Zahlen zeigen.
  // QS-Runde 4: `slot.gen` wurde geschrieben, aber nie gelesen — ein
  // Feld, das wie Schutz aussieht und keiner ist. Jetzt zählt es: Ein
  // Eintrag aus einer früheren Sitzung wird verworfen, auch wenn die
  // Leerfunktion ihn (etwa nach einem Wettlauf) übersehen hat.
  if (slot && slot.gen !== gen) {
    cache.delete(key);
    const fresh = await rawInvoke<T>(cmd, args);
    cache.set(key, { at: Date.now(), value: fresh, gen });
    return fresh;
  }

  if (slot && Date.now() - slot.at >= CACHE_HARD_MAX_MS) {
    cache.delete(key);
    const fresh = await rawInvoke<T>(cmd, args);
    if (gen === cacheGeneration) {
      cache.set(key, { at: Date.now(), value: fresh, gen });
    }
    return fresh;
  }

  if (slot) {
    // Vorhanden, aber alt: SOFORT ausliefern und still auffrischen. Genau
    // das nimmt dem Tab-Wechsel die Wartezeit — die Ansicht ist da, die
    // Zahlen aktualisieren sich einen Wimpernschlag später.
    if (!slot.refreshing) {
      slot.refreshing = true;
      void rawInvoke<T>(cmd, args)
        .then((fresh) => {
          // Verspätete Antwort aus einer alten Sitzung: verwerfen.
          if (gen !== cacheGeneration) return;
          cache.set(key, { at: Date.now(), value: fresh, gen });
        })
        .catch(() => {
          // Auffrischung fehlgeschlagen: alten Wert behalten und die
          // Sperre lösen, damit der nächste Aufruf es erneut versucht.
          // (Der Wert altert weiter; die harte Grenze oben zieht ihn
          // irgendwann ohnehin aus dem Verkehr.)
          const s = cache.get(key);
          if (s) s.refreshing = false;
        });
    }
    return slot.value as T;
  }

  // Erstaufruf — normal holen und merken. Fehler NICHT zwischenspeichern.
  const value = await rawInvoke<T>(cmd, args);
  if (gen === cacheGeneration) {
    cache.set(key, { at: Date.now(), value, gen });
  }
  return value;
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
