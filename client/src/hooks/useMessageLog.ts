// v1.3.0 (#Hoppie-PDC-CPDLC) — shared behaviour for both message logs.
//
// PDC and CPDLC show different traffic but must READ the same way: what
// just happened is in view without scrolling, and settled history folds
// to one line above it rather than pushing the live exchange off-screen.
// The real DCDU shows a single message at a time for the same reason.
//
// Lives in one place so the two logs can't drift apart.

import { useCallback, useEffect, useRef, useState } from "react";

/** How many of the most recent entries stay fully expanded. */
export const EXPANDED_TAIL = 3;

/** Treat the view as "at the bottom" within this many pixels. Generous,
 *  because a message that just grew reply keys can push the position out
 *  by more than a hair. */
const AT_BOTTOM_SLACK = 120;

export function useMessageLog(count: number) {
  const logRef = useRef<HTMLUListElement>(null);
  /** Entries the pilot unfolded by hand. */
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  /** Follow new traffic? Cleared when the pilot scrolls up to read back,
   *  so we never yank the view away from something they're reading. */
  const stick = useRef(true);
  const lastHeight = useRef(0);

  const scrollToEnd = useCallback(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);

  // Re-assert the bottom whenever the content HEIGHT changes, not just
  // when a message is added. An uplink arrives, then renders its reply
  // keys a beat later and grows — scrolling only on arrival left those
  // keys just below the fold, which is exactly the thing the pilot has
  // to reach. Runs after every render and compares heights, which costs
  // nothing and needs no ResizeObserver plumbing.
  useEffect(() => {
    const el = logRef.current;
    if (!el) return;
    if (el.scrollHeight === lastHeight.current) return;
    lastHeight.current = el.scrollHeight;
    if (stick.current) scrollToEnd();
  });

  // A new entry always re-engages following: fresh traffic is the point.
  useEffect(() => {
    stick.current = true;
    scrollToEnd();
  }, [count, scrollToEnd]);

  const onScroll = useCallback(() => {
    const el = logRef.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < AT_BOTTOM_SLACK;
  }, []);

  const toggle = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  /**
   * Whether entry `index` of `count` should render folded.
   *
   * `live` means the exchange is still going — awaiting our reply or
   * ATC's. Those never fold, however old, because they are exactly what
   * the pilot must not lose sight of.
   */
  const isCollapsed = useCallback(
    (index: number, key: string, live: boolean) => {
      if (live) return false;
      if (index >= count - EXPANDED_TAIL) return false;
      return !expanded.has(key);
    },
    [count, expanded],
  );

  return { logRef, onScroll, toggle, isCollapsed };
}
