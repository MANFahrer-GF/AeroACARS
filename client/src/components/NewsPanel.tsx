// v0.12.12-dev: VA-News-Tab. Holt `GET /api/news` ueber den Tauri-
// `news_fetch`-Command (re-used phpVMS-API-Key aus dem Login). Read-
// State client-side via localStorage. Auto-Refresh alle 5 Minuten.
//
// HTML-Sanitization: phpVMS news.body ist HTML (Paragraphs, Bold, Links).
// Wir nutzen DOMParser + Allowlist statt einer 3rd-party-Lib (DOMPurify
// waere 50 KB+ fuer das eine Feature). Allowlist: p, br, strong, em, b,
// i, u, a, ul, ol, li, h1-h6, blockquote, code, pre. Script/iframe/style
// werden komplett entfernt. on*-Attribute werden gestrippt. href bei a-
// Tags wird auf http/https validiert.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";

export interface NewsItem {
  id: number;
  subject: string;
  body: string;
  created_at?: string | null;
  updated_at?: string | null;
  author?: string | null;
  user?: { name?: string | null } | null;
}

type State =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; items: NewsItem[] };

const READ_STORAGE_KEY = "aeroacars.readNewsIds";
const REFRESH_INTERVAL_MS = 5 * 60 * 1000;

// Allowlist fuer Tags + Attribute. Alles andere wird stripped/escaped.
const ALLOWED_TAGS = new Set([
  "P", "BR", "STRONG", "EM", "B", "I", "U", "A", "UL", "OL", "LI",
  "H1", "H2", "H3", "H4", "H5", "H6", "BLOCKQUOTE", "CODE", "PRE",
  "SPAN", "DIV", "HR",
]);
const ALLOWED_ATTRS_BY_TAG: Record<string, Set<string>> = {
  A: new Set(["href", "title"]),
};

function loadReadIds(): Set<number> {
  try {
    const raw = localStorage.getItem(READ_STORAGE_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return new Set();
    const out = new Set<number>();
    for (const v of parsed) {
      if (typeof v === "number" && Number.isFinite(v)) out.add(v);
    }
    return out;
  } catch {
    return new Set();
  }
}

function saveReadIds(ids: Set<number>): void {
  try {
    localStorage.setItem(READ_STORAGE_KEY, JSON.stringify(Array.from(ids)));
  } catch {
    // localStorage voll oder disabled — egal, naechster Run probiert's erneut.
  }
}

/** Sanitize HTML via DOMParser. Walk tree, drop disallowed tags
 *  (keep their text), strip on*-handlers + javascript:-urls. */
function sanitizeHtml(html: string): string {
  if (!html) return "";
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, "text/html");
  } catch {
    return "";
  }
  function walk(node: Element): void {
    // Children-Snapshot ziehen — wir modifizieren die Liste waehrend wir iterieren.
    const children = Array.from(node.children);
    for (const child of children) {
      const tag = child.tagName.toUpperCase();
      if (!ALLOWED_TAGS.has(tag)) {
        // Tag rauswerfen, aber Textinhalt behalten: ersetze Element
        // durch seinen TextContent.
        const text = doc.createTextNode(child.textContent ?? "");
        child.replaceWith(text);
        continue;
      }
      // Attribute saeubern.
      const allowed = ALLOWED_ATTRS_BY_TAG[tag] ?? new Set<string>();
      for (const attr of Array.from(child.attributes)) {
        const name = attr.name.toLowerCase();
        if (!allowed.has(name)) {
          child.removeAttribute(attr.name);
          continue;
        }
        if (name === "href") {
          const url = attr.value.trim();
          if (!/^(https?:|mailto:)/i.test(url)) {
            child.removeAttribute(attr.name);
          } else {
            // Externe Links sicher oeffnen.
            child.setAttribute("rel", "noopener noreferrer");
            child.setAttribute("target", "_blank");
          }
        }
      }
      walk(child);
    }
  }
  walk(doc.body);
  return doc.body.innerHTML;
}

function authorName(item: NewsItem): string | null {
  const flat = item.author?.trim();
  if (flat) return flat;
  const nested = item.user?.name?.trim();
  if (nested) return nested;
  return null;
}

/** Relative-Zeit-Formatter (de). Faellt auf Datum zurueck wenn > 7 Tage. */
function formatRelative(iso: string | null | undefined, t: (k: string) => string, locale: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const sec = Math.floor(diffMs / 1000);
  if (sec < 60) return t("news.relative_just_now");
  const rtf = new Intl.RelativeTimeFormat(locale || "de", { numeric: "auto" });
  const min = Math.floor(sec / 60);
  if (min < 60) return rtf.format(-min, "minute");
  const hr = Math.floor(min / 60);
  if (hr < 24) return rtf.format(-hr, "hour");
  const day = Math.floor(hr / 24);
  if (day < 7) return rtf.format(-day, "day");
  return d.toLocaleDateString(locale || "de", { year: "numeric", month: "short", day: "numeric" });
}

/** Internal fetch helper — used by Panel + useUnreadNewsCount. */
async function fetchNews(): Promise<NewsItem[]> {
  return await invoke<NewsItem[]>("news_fetch");
}

/** Lightweight hook fuer den Tab-Badge. Pollt alle 5 Minuten, plus
 *  bei Storage-Event (anderer Tab/Komponente hat read-state geaendert).
 *  Bewusst ohne Sharing-State — der Cost (1 IPC alle 5 min) ist niedrig
 *  und wir vermeiden globale State-Loesung. */
export function useUnreadNewsCount(loggedIn: boolean): number {
  const [items, setItems] = useState<NewsItem[]>([]);
  const [readIds, setReadIds] = useState<Set<number>>(() => loadReadIds());

  useEffect(() => {
    if (!loggedIn) {
      setItems([]);
      return;
    }
    let cancelled = false;
    async function tick() {
      try {
        const fresh = await fetchNews();
        if (!cancelled) setItems(fresh);
      } catch {
        // egal — Badge bleibt einfach unveraendert.
      }
    }
    void tick();
    const id = setInterval(tick, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [loggedIn]);

  // Re-load read-state bei storage-Event (NewsPanel hat als gelesen markiert).
  useEffect(() => {
    function onStorage(e: StorageEvent) {
      if (e.key === READ_STORAGE_KEY) {
        setReadIds(loadReadIds());
      }
    }
    function onCustom() {
      setReadIds(loadReadIds());
    }
    window.addEventListener("storage", onStorage);
    window.addEventListener("aeroacars:news-read-changed", onCustom);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("aeroacars:news-read-changed", onCustom);
    };
  }, []);

  return useMemo(
    () => items.filter((it) => !readIds.has(it.id)).length,
    [items, readIds],
  );
}

export function NewsPanel() {
  const { t, i18n } = useTranslation();
  const [state, setState] = useState<State>({ kind: "loading" });
  const [readIds, setReadIds] = useState<Set<number>>(() => loadReadIds());
  const [expandedIds, setExpandedIds] = useState<Set<number>>(() => new Set());
  const mountedRef = useRef(true);

  const load = useCallback(async () => {
    setState({ kind: "loading" });
    try {
      const items = await fetchNews();
      if (!mountedRef.current) return;
      // Neueste zuerst (phpVMS sortiert vermutlich schon so, aber sicher ist sicher).
      const sorted = [...items].sort((a, b) => {
        const ta = a.created_at ? new Date(a.created_at).getTime() : 0;
        const tb = b.created_at ? new Date(b.created_at).getTime() : 0;
        return tb - ta;
      });
      setState({ kind: "ready", items: sorted });
    } catch (e) {
      if (!mountedRef.current) return;
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e);
      setState({ kind: "error", message: msg });
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void load();
    const id = setInterval(() => void load(), REFRESH_INTERVAL_MS);
    return () => {
      mountedRef.current = false;
      clearInterval(id);
    };
  }, [load]);

  function markRead(id: number): void {
    setReadIds((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      saveReadIds(next);
      // Custom-Event damit useUnreadNewsCount im gleichen Tab den Wert
      // aktualisiert (storage-Event feuert nur cross-tab).
      window.dispatchEvent(new Event("aeroacars:news-read-changed"));
      return next;
    });
  }

  // v0.12.12-dev: „Alle als gelesen markieren" — fuegt alle aktuell
  // sichtbaren News-IDs in den read-Set ein. Badge oben in der Tab-Leiste
  // verschwindet automatisch (useUnreadNewsCount horcht auf das Event).
  function markAllRead(): void {
    if (state.kind !== "ready" || state.items.length === 0) return;
    setReadIds((prev) => {
      const next = new Set(prev);
      let added = false;
      for (const item of state.items) {
        if (!next.has(item.id)) {
          next.add(item.id);
          added = true;
        }
      }
      if (!added) return prev;
      saveReadIds(next);
      window.dispatchEvent(new Event("aeroacars:news-read-changed"));
      return next;
    });
  }

  function toggleExpanded(id: number): void {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        markRead(id);
      }
      return next;
    });
  }

  const locale = i18n.language || "de";

  return (
    <section className="news-panel">
      <div className="news-panel__header">
        <h2 className="news-panel__title">{t("news.tab")}</h2>
        <div className="news-panel__actions">
          {/* v0.12.12-dev: Alle-als-gelesen-Knopf — nur sichtbar wenn
              wirklich noch ungelesene News in der Liste sind. */}
          {state.kind === "ready" &&
            state.items.some((item) => !readIds.has(item.id)) && (
              <button
                type="button"
                className="news-panel__mark-all"
                onClick={markAllRead}
              >
                {t("news.mark_all_read")}
              </button>
            )}
          <button
            type="button"
            className="news-panel__refresh"
            onClick={() => void load()}
            disabled={state.kind === "loading"}
          >
            {t("news.refresh")}
          </button>
        </div>
      </div>

      {state.kind === "loading" && (
        <div className="news-panel__loading">
          <span className="news-panel__spinner" aria-hidden="true" />
          <span>{t("news.refresh")}…</span>
        </div>
      )}

      {state.kind === "error" && (
        <div className="news-empty news-empty--error">
          <p>{t("news.load_error")}</p>
          <p className="news-empty__detail">{state.message}</p>
        </div>
      )}

      {state.kind === "ready" && state.items.length === 0 && (
        <div className="news-empty">
          <div className="news-empty__icon" aria-hidden="true">📭</div>
          <p>{t("news.empty")}</p>
        </div>
      )}

      {state.kind === "ready" && state.items.length > 0 && (
        <ul className="news-list">
          {state.items.map((item) => {
            const unread = !readIds.has(item.id);
            const expanded = expandedIds.has(item.id);
            const cleaned = sanitizeHtml(item.body);
            const author = authorName(item);
            return (
              <li
                key={item.id}
                className={`news-card${unread ? " news-card--unread" : ""}`}
              >
                <button
                  type="button"
                  className="news-card__header"
                  onClick={() => toggleExpanded(item.id)}
                  aria-expanded={expanded}
                >
                  <div className="news-card__title-row">
                    {unread && (
                      <span
                        className="news-card__unread-dot"
                        aria-label={t("news.new_badge")}
                      />
                    )}
                    <h3 className="news-card__title">{item.subject || "—"}</h3>
                  </div>
                  <div className="news-card__meta">
                    {author && <span className="news-card__author">{author}</span>}
                    {item.created_at && (
                      <span className="news-card__time" title={item.created_at}>
                        {formatRelative(item.created_at, t, locale)}
                      </span>
                    )}
                    <span
                      className={`news-card__chevron${expanded ? " news-card__chevron--open" : ""}`}
                      aria-hidden="true"
                    >
                      ›
                    </span>
                  </div>
                </button>
                {expanded && (
                  <div
                    className="news-card__body"
                    // Sanitized via DOMParser-Allowlist oben.
                    dangerouslySetInnerHTML={{ __html: cleaned }}
                  />
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

export default NewsPanel;
