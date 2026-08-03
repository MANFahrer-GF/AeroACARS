// v1.3.5 (#Livemap-4a) — categorized event feed for the Live-Karte screen
// (README handoff_livemap_4a §4.2). Replaces the plain embedded
// `ActivityLogPanel` this screen used before: that was a flat, undifferentiated
// string stream (no category, no position, no dedup) — this hook turns it into
// three distinct kinds of events, groups repeated system messages into one row
// with a counter instead of nine identical lines, and (for anything carrying a
// position) makes a row clickable to fly the map there.
//
// FLUG (flight-phase events — Pushback, Abheben, Fahrwerk, …) is NOT wired up
// yet: today's backend does not log phase transitions as discrete events at
// all (verified — there is no phase-transition-to-activity-log instrumentation
// anywhere in src-tauri). Building that is a separate, real backend feature,
// out of scope for this pass per explicit instruction ("das bleibt so wie das
// ist"). The FLUG filter still renders — it will just show nothing until that
// instrumentation exists. Nothing here fakes data to fill the category.

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { useCpdlcMessages, type ThreadEntry } from "../hooks/useCpdlcMessages";

interface ActivityEntry {
  timestamp: string;
  level: "info" | "warn" | "error";
  message: string;
  detail?: string;
}

export type EventCategory = "flug" | "acars" | "system";

export interface MapEvent {
  key: string;
  at: string;
  category: EventCategory;
  title: string;
  sub?: string;
  /** [lon, lat] — only FLUG events will ever carry this once phase-transition
   *  logging exists; ACARS/SYSTEM rows have no coordinate today. */
  position?: [number, number];
}

// Common emoji ranges + the couple of dingbats/arrows this app's own strings
// use (e.g. the version banner's ❤️). README §4.2/§9: "Kein Emoji in
// Logzeilen." — stripped generically rather than special-cased on one string,
// so it also catches anything else that slips an emoji into a log message.
function stripEmoji(s: string): string {
  return s.replace(/[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}❤️]/gu, "").replace(/\s{2,}/g, " ").trim();
}

/** Field feedback (2026-08-03): the previous version grouped repeated
 *  system messages into one row with an "N since ... / last ..." summary
 *  — collapsing e.g. per-engine start/shutdown entries into a bare count
 *  and losing each entry's own detail. Every entry now gets its own row
 *  with its own real timestamp and detail, no merging. */
function mapSystemEntries(entries: ActivityEntry[]): MapEvent[] {
  return entries.map((e, i) => ({
    key: `sys-${e.timestamp}-${i}`,
    at: e.timestamp,
    category: "system" as const,
    title: stripEmoji(e.message),
    sub: e.detail ? stripEmoji(e.detail) : undefined,
  }));
}

/** One-line label for a raw ACARS/telex message. Deliberately a light
 *  heuristic, not a field-by-field breakdown — the full parser for that
 *  already lives in datalinkParse.ts for the PDC/CPDLC screen; this list
 *  only needs README §4.2's own example vocabulary (OUT/OFF/POS REPORT,
 *  "PDC erhalten"). */
function acarsTitle(m: ThreadEntry): string {
  const text = m.text.toUpperCase();
  const dir = m.direction === "sent" ? "gesendet" : "erhalten";
  if (m.kind === "telex" && m.direction === "received" && /\bCLR?D? TO\b|\bCLEARED TO\b/.test(text)) {
    return "PDC erhalten";
  }
  if (/^OUT\b/.test(text)) return `OUT REPORT ${dir}`;
  if (/^OFF\b/.test(text)) return `OFF REPORT ${dir}`;
  if (/^ON\b/.test(text)) return `ON REPORT ${dir}`;
  if (/^IN\b/.test(text)) return `IN REPORT ${dir}`;
  if (/\bPOS\b/.test(text)) return `POS REPORT ${dir}`;
  return m.kind === "cpdlc" ? `CPDLC ${dir}` : `ACARS ${dir}`;
}

/** Builds the unified, categorized event feed from the two data sources that
 *  actually exist today. `active` gates both polls — no point hitting either
 *  endpoint while the pilot is looking at a different tab. */
export function useMapEvents(active: boolean): { events: MapEvent[] } {
  const [activityEntries, setActivityEntries] = useState<ActivityEntry[]>([]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const poll = () =>
      void invoke<ActivityEntry[]>("activity_log_get")
        .then((list) => {
          if (!cancelled) setActivityEntries(list);
        })
        .catch(() => undefined);
    void poll();
    const id = window.setInterval(poll, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [active]);

  const { messages: thread } = useCpdlcMessages(active);

  const events = useMemo(() => {
    const sys = mapSystemEntries(activityEntries);
    const acars: MapEvent[] = thread.map((m, i) => ({
      key: `acars-${m.at}-${i}`,
      at: m.at,
      category: "acars" as const,
      title: acarsTitle(m),
      sub: stripEmoji(m.text).slice(0, 90),
    }));
    return [...sys, ...acars].sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0));
  }, [activityEntries, thread]);

  return { events };
}

function fmtUtcHms(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${d.toISOString().slice(11, 19)}z`;
}

interface EventListProps {
  events: MapEvent[];
  callsign: string | null;
  onJumpTo: (position: [number, number]) => void;
}

/** README §4.2 — the right panel's event feed. Own component (not inlined in
 *  LiveMapView.tsx) because it owns real state (the new-entry fade-in
 *  baseline) independent of the map engine.
 *
 *  Field feedback (2026-08-03): the Alle/Flug/ACARS/System filter bar
 *  "passt nicht" — removed. Every entry shows, unfiltered, always. */
export function LiveMapEventList({ events, callsign, onJumpTo }: EventListProps) {
  const { t } = useTranslation();

  // Same "fade in only what's genuinely new" pattern as the PDC/CPDLC
  // history (DatalinkHistory.tsx) — the whole list must NOT fade in every
  // time the pilot switches back to this tab, only entries that arrived
  // while they were already looking at it.
  const seenKeysRef = useRef<Set<string> | null>(null);
  if (seenKeysRef.current === null) {
    seenKeysRef.current = new Set(events.map((e) => e.key));
  }
  const freshKeys = new Set(events.filter((e) => !seenKeysRef.current!.has(e.key)).map((e) => e.key));
  useEffect(() => {
    for (const e of events) seenKeysRef.current!.add(e.key);
  });

  return (
    <div className="aa-livemap-events aa-livemap-overlay">
      <header className="aa-livemap-events__head">
        <span className="aa-livemap-events__title">
          {t("livemap.events_title", { callsign: callsign ?? "—" })}
        </span>
      </header>

      <div className="aa-livemap-events__log" role="log" aria-live="polite">
        {events.length === 0 && <p className="aa-livemap-events__empty">{t("livemap.events_empty")}</p>}
        {events.map((e) => {
          const fresh = freshKeys.has(e.key);
          const content = (
            <>
              <span className={`aa-livemap-events__edge aa-livemap-events__edge--${e.category}`} aria-hidden="true" />
              <div className="aa-livemap-events__body">
                <div className="aa-livemap-events__row1">
                  <span className="aa-livemap-events__time">{fmtUtcHms(e.at)}</span>
                  <span className="aa-livemap-events__title-text">{e.title}</span>
                </div>
                {e.sub && <div className="aa-livemap-events__sub">{e.sub}</div>}
              </div>
            </>
          );
          const className = `aa-livemap-events__row ${fresh ? "aa-livemap-events__row--fresh" : ""}`;
          return e.position ? (
            <button
              key={e.key}
              type="button"
              className={className}
              aria-label={`${fmtUtcHms(e.at)} ${e.title}, Position auf Karte zeigen`}
              onClick={() => onJumpTo(e.position!)}
            >
              {content}
            </button>
          ) : (
            <div key={e.key} className={className} style={{ cursor: "default" }}>
              {content}
            </div>
          );
        })}
      </div>
    </div>
  );
}
