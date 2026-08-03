// v1.3.5 (#Datalink-3a) — history, right column (README §4).
//
// The point of this screen: what came back from the station, not what
// was sent. A downlink is a compact record; an uplink is parsed into
// large values with the original text always still there underneath.
// PDC and CPDLC share this one log — only the composer differs by mode
// (README §0).

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatDatalinkText } from "../lib/datalink";
import { parseUplink, formatCtot, type ParsedUplink } from "../lib/datalinkParse";
import { CpdlcQuickReply, TelexQuickReply } from "./CpdlcQuickReply";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";

/** GOLD response codes that actually demand an answer (ported from the
 *  previous CpdlcView — N/NE are answered by nobody). */
const REPLYABLE = ["WU", "AN", "R", "Y"];

/** Same detection EasyCPDLC/vSMR use — see the previous TelexQuickReply
 *  docs. A telex that isn't a clearance has nothing to WILCO. */
function isTelexClearance(text: string): boolean {
  return /\bCLR?D? TO\b|\bCLEARED TO\b/i.test(text);
}

function isCpdlcOpenUplink(m: ThreadEntry): boolean {
  return m.kind === "cpdlc" && m.direction === "received" && !m.closed && REPLYABLE.includes(m.response ?? "");
}

/** Across the WHOLE unified thread, the single most recent uplink still
 *  waiting on an answer — README §4.3.e: "Nur die jüngste unbeantwortete
 *  Uplink-Nachricht zeigt diese Zeile." Scans from the newest entry
 *  backwards so an old, never-answered clearance doesn't grow a
 *  permanent action row once something newer needs one instead. */
function findLatestUnansweredUplink(all: ThreadEntry[]): ThreadEntry | null {
  const telex = all.filter((m) => m.kind === "telex");
  const lastTelex = telex.length > 0 ? telex[telex.length - 1] : null;
  for (let i = all.length - 1; i >= 0; i--) {
    const m = all[i];
    if (isCpdlcOpenUplink(m)) return m;
    if (m.kind === "telex" && m === lastTelex && m.direction === "received" && isTelexClearance(m.text)) {
      return m;
    }
  }
  return null;
}

/** The reply that closed `uplink`, if any — CPDLC threads it via MRN;
 *  telex has no threading, so the reply is simply the next SENT telex
 *  entry after it (there is at most one, since sending one is exactly
 *  what stops `uplink` from being "the last telex" any more). */
function findReplyEntry(uplink: ThreadEntry, all: ThreadEntry[]): ThreadEntry | null {
  if (uplink.kind === "cpdlc") {
    return all.find((m) => m.kind === "cpdlc" && m.direction === "sent" && m.mrn === uplink.min) ?? null;
  }
  const idx = all.indexOf(uplink);
  for (let i = idx + 1; i < all.length; i++) {
    if (all[i].kind === "telex" && all[i].direction === "sent") return all[i];
  }
  return null;
}

function isAnswered(m: ThreadEntry, all: ThreadEntry[]): boolean {
  if (m.kind === "cpdlc") return Boolean(m.closed);
  return findReplyEntry(m, all) !== null;
}

/** UTC, always — README §6: "Alle Zeiten in UTC mit nachgestelltem z." */
function formatUtcHms(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${d.toISOString().slice(11, 19)}z`;
}

const SQUAWK_MEMO_KEY = "aeroacars.transponder.squawk_memo";
function loadSquawkMemo(): string | null {
  try {
    return localStorage.getItem(SQUAWK_MEMO_KEY);
  } catch {
    return null;
  }
}
function saveSquawkMemo(value: string): void {
  try {
    localStorage.setItem(SQUAWK_MEMO_KEY, value);
  } catch {
    // Private-mode / quota — the button just won't remember across a
    // restart, which is no worse than the feature not existing.
  }
}

function SquawkTakeover({ squawk }: { squawk: string }) {
  const { t } = useTranslation();
  const [stored, setStored] = useState(loadSquawkMemo);
  const taken = stored === squawk;
  return (
    <button
      type="button"
      className="button button--secondary"
      disabled={taken}
      onClick={() => {
        saveSquawkMemo(squawk);
        setStored(squawk);
      }}
    >
      {taken ? t("cpdlc.squawk_taken", { squawk }) : t("cpdlc.squawk_take", { squawk })}
    </button>
  );
}

/** Grid cell — always rendered, even when its value is missing, so the
 *  4-column layout never reflows (README §4.3.b/c: "die Zelle wird nie
 *  ausgeblendet"). */
function ValueCell({ label, value, size, warn }: { label: string; value: string | null; size: "lg" | "md"; warn?: boolean }) {
  return (
    <div className="datalink-uplink__cell">
      <span className="datalink-uplink__cell-label">{label}</span>
      <span
        className={`datalink-uplink__cell-value datalink-uplink__cell-value--${size} ${warn ? "datalink-uplink__cell-value--warn" : ""}`}
      >
        {value ?? "—"}
      </span>
    </div>
  );
}

type Filter = "all" | "pdc" | "cpdlc";

/** How many of the most recent entries in the current filter stay fully
 *  expanded by default — the rest fold behind "Ältere Nachrichten (n)"
 *  (README §4.4). A message still awaiting an answer (either direction)
 *  never folds, however old, same principle the previous per-item
 *  collapse used. */
const DEFAULT_VISIBLE = 2;

interface Props {
  callsign: string | null;
  cpdlcStation: string | null;
  pdcRecipient: string;
  messages: ThreadEntry[];
  onChanged: () => void;
}

export function DatalinkHistory({ callsign, cpdlcStation, pdcRecipient, messages, onChanged }: Props) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<Filter>("all");
  const [showOlder, setShowOlder] = useState(false);
  const [collapsedOriginal, setCollapsedOriginal] = useState<Set<string>>(new Set());
  const logRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const lastHeight = useRef(0);

  useEffect(() => setShowOlder(false), [filter]);

  // README §6: a genuinely NEW uplink fades in once — not the whole
  // history on every mount/tab-switch. `seenUplinkAtRef` is this
  // component's own mount-scoped baseline: lazily set to every uplink
  // already present the first time this renders (nothing here is "new"),
  // then grown after each commit so a later render of the SAME entry
  // never re-qualifies as fresh (no replay, no flicker on unrelated
  // re-renders). Keyed on `m.at` rather than the array index used for
  // React `key`s below, because that index shifts with `filter`.
  const seenUplinkAtRef = useRef<Set<string> | null>(null);
  if (seenUplinkAtRef.current === null) {
    seenUplinkAtRef.current = new Set(messages.filter((m) => m.direction === "received").map((m) => m.at));
  }
  const freshUplinkAt = new Set(
    messages
      .filter((m) => m.direction === "received" && !seenUplinkAtRef.current!.has(m.at))
      .map((m) => m.at),
  );
  useEffect(() => {
    for (const m of messages) {
      if (m.direction === "received") seenUplinkAtRef.current!.add(m.at);
    }
  });

  const unansweredUplink = findLatestUnansweredUplink(messages);

  const filtered = messages.filter(
    (m) => filter === "all" || (filter === "pdc" && m.kind === "telex") || (filter === "cpdlc" && m.kind === "cpdlc"),
  );

  const isLive = (m: ThreadEntry) =>
    m === unansweredUplink ||
    (m.kind === "cpdlc" && m.direction === "sent" && !m.closed && REPLYABLE.includes(m.response ?? ""));

  const n = filtered.length;
  const olderIndices = new Set<number>();
  filtered.forEach((m, i) => {
    if (!isLive(m) && i < n - DEFAULT_VISIBLE) olderIndices.add(i);
  });
  const olderCount = olderIndices.size;

  const scrollToEnd = () => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  };
  // Re-assert the bottom whenever content height changes, not just when
  // a message is added — an uplink's answer row can render a beat later
  // and grow, and the pilot needs THAT in view too.
  useEffect(() => {
    const el = logRef.current;
    if (!el) return;
    if (el.scrollHeight === lastHeight.current) return;
    lastHeight.current = el.scrollHeight;
    if (stick.current) scrollToEnd();
  });
  useEffect(() => {
    stick.current = true;
    // scrollTop, not scrollIntoView — README §6.
    scrollToEnd();
  }, [n]);
  const onScroll = () => {
    const el = logRef.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  };

  const toggleOriginal = (key: string) => {
    setCollapsedOriginal((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const renderUplinkCard = (m: ThreadEntry, key: string, station: string | null) => {
    const parsed: ParsedUplink = parseUplink(m.text);
    const answered = isAnswered(m, messages);
    const reply = answered ? findReplyEntry(m, messages) : null;
    const showAnswerRow = m === unansweredUplink;
    const title = !parsed.recognized
      ? t("cpdlc.uplink_title_unparsed")
      : parsed.squawk || parsed.rwy || parsed.sid
        ? t("cpdlc.uplink_title_clearance")
        : t("cpdlc.uplink_title_generic");
    const originalCollapsed = collapsedOriginal.has(key);
    const conditionsText = parsed.conditions.join(" · ");

    const trailing = parsed.squawk ? <SquawkTakeover squawk={parsed.squawk} /> : undefined;

    return (
      <article
        key={key}
        className={`datalink-uplink ${freshUplinkAt.has(m.at) ? "datalink-uplink--fresh" : ""}`}
        aria-label={`${t("cpdlc.thread_received")} ${station ?? ""} ${formatUtcHms(m.at)}`}
      >
        <header className="datalink-uplink__head">
          <span className="datalink-uplink__tag">{t("cpdlc.uplink_tag")}</span>
          <span className="datalink-uplink__station">{station ?? t("cpdlc.thread_received")}</span>
          <span className="datalink-uplink__title">{title}</span>
          <span className="datalink-uplink__time">{formatUtcHms(m.at)}</span>
        </header>

        {parsed.recognized ? (
          <>
            <div className="datalink-uplink__grid">
              <ValueCell label={t("cpdlc.field_squawk")} value={parsed.squawk} size="lg" />
              <ValueCell label={t("cpdlc.field_sid")} value={parsed.sid} size="md" />
              <ValueCell label={t("cpdlc.field_initial_climb")} value={parsed.initialClimb} size="lg" />
              <ValueCell label={t("cpdlc.field_dep_freq")} value={parsed.depFreq} size="md" />
            </div>
            <div className="datalink-uplink__grid datalink-uplink__grid--row2">
              <ValueCell label={t("cpdlc.field_rwy")} value={parsed.rwy} size="md" />
              <ValueCell
                label={t("cpdlc.field_ctot")}
                value={parsed.ctot ? formatCtot(parsed.ctot) : null}
                size="md"
                warn
              />
              <ValueCell label={t("cpdlc.field_qnh")} value={parsed.qnh} size="md" />
              <ValueCell label={t("cpdlc.field_atis")} value={parsed.atis} size="md" />
            </div>
          </>
        ) : (
          <p className="datalink-uplink__raw">{formatDatalinkText(m.text)}</p>
        )}

        {parsed.recognized && conditionsText !== "" && (
          <div className="datalink-uplink__conditions">
            <span className="datalink-uplink__conditions-label">{t("cpdlc.conditions_label")}</span>
            <p>{conditionsText}</p>
          </div>
        )}

        {showAnswerRow &&
          (m.kind === "cpdlc" ? (
            <CpdlcQuickReply
              min={m.min}
              response={m.response}
              deferred={Boolean(m.deferred)}
              onReplied={onChanged}
              trailing={trailing}
            />
          ) : (
            <TelexQuickReply recipient={pdcRecipient || null} clearance={m.text} onReplied={onChanged} trailing={trailing} />
          ))}

        {!showAnswerRow && answered && reply && (
          <p className="datalink-uplink__answered">
            {t("cpdlc.answered_status", { answer: reply.text.trim(), time: formatUtcHms(reply.at) })}
          </p>
        )}

        {parsed.recognized && (
          <div className="datalink-uplink__original">
            <div className="datalink-uplink__original-head">
              <span>{t("cpdlc.original_label")}</span>
              <button type="button" className="datalink-uplink__original-toggle" onClick={() => toggleOriginal(key)}>
                {originalCollapsed ? t("cpdlc.original_expand") : t("cpdlc.original_collapse")}
              </button>
            </div>
            {!originalCollapsed && <p className="datalink-uplink__original-text">{formatDatalinkText(m.text)}</p>}
          </div>
        )}
      </article>
    );
  };

  const renderDownlinkCard = (m: ThreadEntry, key: string, station: string | null) => (
    <article key={key} className="datalink-downlink" aria-label={`${t("cpdlc.thread_sent")} ${station ?? ""} ${formatUtcHms(m.at)}`}>
      <p className="datalink-downlink__meta">
        <span>{t("cpdlc.sent_to", { station: station ?? "—", time: formatUtcHms(m.at) })}</span>
        {/* The wire gives no send-in-progress/failed state to show here — a
            send that fails throws before ever reaching this list (surfaced
            instead as the composer's own error banner), so anything that
            got this far genuinely reached the addressee's mailbox. */}
        <span className="datalink-downlink__delivered">{t("cpdlc.delivered")}</span>
      </p>
      <p className="datalink-downlink__text">{formatDatalinkText(m.text)}</p>
    </article>
  );

  const renderOlderLine = (m: ThreadEntry, key: string, station: string | null) => (
    <p key={key} className="datalink-older-line">
      <span className="datalink-older-line__dir">
        {m.direction === "sent" ? t("cpdlc.thread_sent") : (station ?? t("cpdlc.thread_received"))}
      </span>
      <span className="datalink-older-line__time">{formatUtcHms(m.at)}</span>
      <span className="datalink-older-line__text">{formatDatalinkText(m.text)}</span>
    </p>
  );

  return (
    <div className="datalink-history">
      <header className="datalink-history__head">
        <span className="datalink-history__title">{t("cpdlc.history_title", { callsign: callsign ?? "—" })}</span>
        <nav className="datalink-history__filters">
          {(["all", "pdc", "cpdlc"] as Filter[]).map((f) => (
            <button
              key={f}
              type="button"
              className={`datalink-filter ${filter === f ? "datalink-filter--active" : ""}`}
              onClick={() => setFilter(f)}
            >
              {t(`cpdlc.filter_${f}`)}
            </button>
          ))}
        </nav>
      </header>

      <div className="datalink-history__log" ref={logRef} onScroll={onScroll} role="log" aria-live="polite">
        {filtered.length === 0 && <p className="cpdlc-log__empty">{t("cpdlc.history_empty")}</p>}
        {showOlder &&
          filtered.map((m, i) => {
            if (!olderIndices.has(i)) return null;
            const key = `${m.at}-${i}`;
            const station = m.kind === "cpdlc" ? cpdlcStation : pdcRecipient;
            return (
              <div key={key} className="datalink-older">
                {renderOlderLine(m, key, station)}
              </div>
            );
          })}
        {filtered.map((m, i) => {
          if (olderIndices.has(i)) return null;
          const key = `${m.at}-${i}`;
          const station = m.kind === "cpdlc" ? cpdlcStation : pdcRecipient;
          return m.direction === "received" ? renderUplinkCard(m, key, station) : renderDownlinkCard(m, key, station);
        })}
      </div>

      <footer className="datalink-history__footer">
        <span>{t("cpdlc.history_footer_count", { count: n })}</span>
        {olderCount > 0 && (
          <button type="button" className="datalink-history__older-toggle" onClick={() => setShowOlder((v) => !v)}>
            {showOlder ? t("cpdlc.history_hide_older") : t("cpdlc.history_show_older", { count: olderCount })}
          </button>
        )}
      </footer>
    </div>
  );
}
