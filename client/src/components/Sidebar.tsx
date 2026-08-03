/**
 * Seitenleiste — ersetzt die horizontale Tab-Leiste (Redesign Stufe D).
 *
 * Gründe für den Wechsel:
 *   - Die alte `.tabs` war `display:flex` OHNE `flex-wrap` und OHNE
 *     `overflow-x`. Bei zehn Einträgen und 80 Zeichen deutscher
 *     Beschriftung passte das im 900-px-Minimum gerade so — ohne Reserve
 *     und ohne definiertes Verhalten beim Überlauf.
 *   - Die App war auf 1200 px gedeckelt; darüber entstanden tote Ränder.
 *     Mit der Seitenleiste darf der Inhalt mitwachsen.
 *   - Die drei Statusanzeigen (phpVMS, Simulator, Aufzeichnung) lagen in
 *     der Kopfzeile und konkurrierten dort mit dem Flugtitel.
 *
 * Einklappbar auf eine reine Symbolleiste (216 px ↔ 60 px), Zustand wird
 * gemerkt. Sämtliche Texte, Bedingungen und Zähler sind unverändert aus
 * App.tsx übernommen.
 */

import { type ReactNode } from "react";
import { useTranslation } from "react-i18next";

export type Tab =
  | "cockpit"
  | "map"
  | "cpdlc"
  | "briefing"
  | "logbook"
  | "landing"
  | "news"
  | "log"
  | "settings"
  | "about"
  | "devpreview";

const STORAGE_KEY = "aeroacars.nav.collapsed";

export function getInitialCollapsed(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

/* ---------------------------------------------------------------- Symbole */
/* Bewusst als Inline-SVG statt Emoji: Emoji rendern unter Windows und macOS
   unterschiedlich, lassen sich nicht einfärben und werden vom Screenreader
   vorgelesen. Strichstärke 1.6, currentColor. */

const I = {
  cockpit: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" /><path d="M2 12h20M12 2a15 15 0 0 1 0 20M12 2a15 15 0 0 0 0 20" />
    </svg>
  ),
  map: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="m9 4-6 3v13l6-3 6 3 6-3V4l-6 3-6-3z" /><path d="M9 4v13M15 7v13" />
    </svg>
  ),
  cpdlc: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 5h16v11H8l-4 4V5z" /><path d="M8 9h8M8 12.5h5" />
    </svg>
  ),
  briefing: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 3h9l4 4v14H6z" /><path d="M14 3v5h5M9 12h7M9 16h5" />
    </svg>
  ),
  logbook: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v18H6.5A1.5 1.5 0 0 1 5 19.5v-15z" /><path d="M5 17h14M9 7h6" />
    </svg>
  ),
  landing: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 20h18" /><path d="M5 16.5 19.5 12a1.6 1.6 0 0 0-1-3L15 10 9 5 6.5 5.8 10 11l-4 1.3-2.5-1.8-1.3.5L5 16.5z" />
    </svg>
  ),
  news: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 6h13v12H3z" /><path d="M16 10h3l2 3v5h-5M6 10h7M6 13.5h5" />
    </svg>
  ),
  log: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 4h16v16H4z" /><path d="M7.5 9h9M7.5 12h9M7.5 15h5" />
    </svg>
  ),
  settings: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.7 1.7 0 0 0 9 19.4a1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </svg>
  ),
  about: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" /><path d="M12 11v5M12 8h.01" />
    </svg>
  ),
  dev: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 3h6M10 3v6.5L5.5 18A2 2 0 0 0 7.2 21h9.6a2 2 0 0 0 1.7-3L14 9.5V3" />
    </svg>
  ),
  collapse: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 5h16v14H4z" /><path d="M10 5v14" />
    </svg>
  ),
  plane: (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z" />
    </svg>
  ),
};

/* ------------------------------------------------------------------ Zeile */

function Item({
  icon,
  label,
  active,
  badge,
  badgeLabel,
  dot,
  title,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active: boolean;
  badge?: ReactNode;
  badgeLabel?: string;
  dot?: boolean;
  title?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`navitem ${active ? "navitem--active" : ""}`}
      aria-current={active ? "page" : undefined}
      title={title ?? label}
      onClick={onClick}
    >
      <span className="navitem__icon" aria-hidden="true">
        {icon}
      </span>
      <span className="navitem__label">{label}</span>
      {badge != null && (
        <span className="navitem__badge" aria-label={badgeLabel}>
          {badge}
        </span>
      )}
      {dot && <span className="navitem__dot" aria-hidden="true" />}
    </button>
  );
}

/* --------------------------------------------------------------- Sidebar */

export function Sidebar({
  tab,
  setTab,
  collapsed,
  onToggleCollapsed,
  cpdlcEnabled,
  cpdlcPendingCount,
  onCpdlcOpen,
  unreadNews,
  hasActiveFlight,
  phpvmsConnected,
  simConnected,
  simConnecting,
  simLabel,
  recording,
  updateButton,
}: {
  tab: Tab;
  setTab: (t: Tab) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  cpdlcEnabled: boolean;
  cpdlcPendingCount: number;
  onCpdlcOpen: () => void;
  unreadNews: number;
  hasActiveFlight: boolean;
  phpvmsConnected: boolean;
  simConnected: boolean;
  simConnecting: boolean;
  simLabel: string;
  recording?: ReactNode;
  updateButton?: ReactNode;
}) {
  const { t } = useTranslation();

  return (
    <nav className="nav" aria-label={t("app.name")}>
      <div className="nav__brand">
        <span className="nav__mark" aria-hidden="true">
          {I.plane}
        </span>
        <span className="nav__wordmark">
          <span className="nav__name">{t("app.name")}</span>
          <span className="nav__tagline">{t("app.tagline")}</span>
        </span>
      </div>

      <div className="nav__list">
        {/* Briefing (Gebuchte Flüge) first — that's where a session starts:
            pick/confirm the flight before anything else. Matches the
            default tab (App.tsx's useState<Tab>("briefing")) already. */}
        <Item icon={I.briefing} label={t("tabs.briefing")} active={tab === "briefing"} onClick={() => setTab("briefing")} />
        <Item
          icon={I.cockpit}
          label={t("tabs.cockpit")}
          active={tab === "cockpit"}
          dot={hasActiveFlight}
          onClick={() => setTab("cockpit")}
        />
        <Item icon={I.map} label={t("tabs.map")} active={tab === "map"} onClick={() => setTab("map")} />
        {cpdlcEnabled && (
          <Item
            icon={I.cpdlc}
            label={t("tabs.cpdlc")}
            active={tab === "cpdlc"}
            badge={
              cpdlcPendingCount > 0 ? (cpdlcPendingCount > 9 ? "9+" : cpdlcPendingCount) : undefined
            }
            badgeLabel={t("cpdlc.banner_text", { count: cpdlcPendingCount })}
            onClick={onCpdlcOpen}
          />
        )}
        <Item icon={I.logbook} label={t("tabs.logbook")} active={tab === "logbook"} onClick={() => setTab("logbook")} />
        <Item icon={I.landing} label={t("tabs.landing")} active={tab === "landing"} onClick={() => setTab("landing")} />
        <Item
          icon={I.news}
          label={t("nav.news")}
          active={tab === "news"}
          badge={unreadNews > 0 ? (unreadNews > 9 ? "9+" : unreadNews) : undefined}
          badgeLabel={t("news.new_badge")}
          onClick={() => setTab("news")}
        />
        <Item icon={I.log} label={t("tabs.log")} active={tab === "log"} onClick={() => setTab("log")} />
        <Item icon={I.settings} label={t("tabs.settings")} active={tab === "settings"} onClick={() => setTab("settings")} />
        <Item icon={I.about} label={t("tabs.about")} active={tab === "about"} onClick={() => setTab("about")} />
        {import.meta.env.DEV && (
          <Item
            icon={I.dev}
            label="Preview"
            active={tab === "devpreview"}
            title="Dev-only: RunwayDiagram-Preview mit Mock-Daten"
            onClick={() => setTab("devpreview")}
          />
        )}
      </div>

      <div className="nav__foot">
        {updateButton}
        <span
          className={`navstatus navstatus--${phpvmsConnected ? "online" : "offline"}`}
          title={t("status.tooltip", {
            service: t("status.phpvms"),
            state: phpvmsConnected ? t("status.phpvms_connected") : t("status.phpvms_disconnected"),
          })}
        >
          <span className="navstatus__dot" />
          <span className="navstatus__text">{t("status.phpvms")}</span>
        </span>
        <span
          className={`navstatus navstatus--${
            simConnected ? "online" : simConnecting ? "connecting" : "offline"
          }`}
          title={t("status.tooltip", {
            service: simLabel,
            state: simConnected
              ? t("status.simulator_connected")
              : simConnecting
                ? t("status.simulator_connecting")
                : t("status.simulator_disconnected"),
          })}
        >
          <span className="navstatus__dot" />
          <span className="navstatus__text">{simLabel}</span>
        </span>
        {recording}
      </div>

      <div className="nav__tools">
        <button
          type="button"
          className="navtool"
          onClick={onToggleCollapsed}
          aria-label={collapsed ? t("nav.expand") : t("nav.collapse")}
          title={collapsed ? t("nav.expand") : t("nav.collapse")}
          aria-pressed={collapsed}
        >
          {I.collapse}
        </button>
      </div>
    </nav>
  );
}
