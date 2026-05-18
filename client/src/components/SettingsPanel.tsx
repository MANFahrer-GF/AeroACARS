import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { setLanguage, SUPPORTED_LANGUAGES, LANGUAGE_LABELS, type SupportedLanguage } from "../i18n";
import type { ActiveFlightInfo, SimKind, SimStatus } from "../types";
import type { Theme } from "../theme";
import { SimDebugPanel } from "./SimDebugPanel";
import { PmdgPremiumPanel } from "./PmdgPremiumPanel";
import { XPlanePremiumPanel } from "./XPlanePremiumPanel";
import { OrphanFlightsPanel } from "./OrphanFlightsPanel";
import { useConfirm } from "./ConfirmDialog";
import { getConsent, setConsent } from "../lib/sentry";
import { DiscordRpcPanel } from "./DiscordRpcPanel";

const ALL_KINDS: SimKind[] = [
  "msfs2024",
  "msfs2020",
  "xplane11",
  "xplane12",
  "off",
];

interface Props {
  debugMode: boolean;
  onDebugModeChange: (next: boolean) => void;
  /** Auto-file the PIREP once the FSM reaches Arrived. Persisted
   *  via the App.tsx storage helpers. */
  autoFile: boolean;
  onAutoFileChange: (next: boolean) => void;
  /** Auto-start a flight when the aircraft is parked at one of the
   *  bid's departure airports. Persisted via App.tsx storage helpers. */
  autoStart: boolean;
  onAutoStartChange: (next: boolean) => void;
  /** Auto-purge per-flight JSONL log files older than 30 days on
   *  every app start. Persisted via App.tsx storage helpers; the
   *  actual sweep call is fired once at mount inside App.tsx. */
  autoDeleteFlightLogs: boolean;
  onAutoDeleteFlightLogsChange: (next: boolean) => void;
  /** When true, clicking the close button hides the window into the
   *  system tray (Win) / menubar (Mac) instead of quitting. Default
   *  off; toggle persisted in localStorage and synced to the Rust
   *  backend so the close-handler reads it directly. */
  minimizeToTray: boolean;
  onMinimizeToTrayChange: (next: boolean) => void;
  /** v0.5.38: Stable-Approach-Banner im Cockpit-Tab. Default ON. */
  approachAdvisoriesEnabled: boolean;
  onApproachAdvisoriesEnabledChange: (next: boolean) => void;
  theme: Theme;
  onThemeChange: (next: Theme) => void;
  /** Latest sim telemetry — surfaced in the debug section when the
   *  user has enabled debug mode. Polled centrally by `useSimSession`. */
  simStatus: SimStatus | null;
  /** Active flight, used to surface heartbeat / position-post timing
   *  in the debug panel. Null when no flight is in progress. */
  activeFlight: ActiveFlightInfo | null;
}

export function SettingsPanel({
  debugMode,
  onDebugModeChange,
  autoFile,
  onAutoFileChange,
  autoStart,
  onAutoStartChange,
  autoDeleteFlightLogs,
  onAutoDeleteFlightLogsChange,
  minimizeToTray,
  onMinimizeToTrayChange,
  approachAdvisoriesEnabled,
  onApproachAdvisoriesEnabledChange,
  theme,
  onThemeChange,
  simStatus,
  activeFlight,
}: Props) {
  const { t, i18n } = useTranslation();
  const [kind, setKind] = useState<SimKind | null>(null);
  const [busy, setBusy] = useState(false);

  // v0.7.8: SimBrief Integration Settings — Username + User-ID.
  // Persistence: localStorage Frontend-side, Backend-State wird per
  // set_simbrief_settings befuellt. App.tsx pusht beim Login-Mount
  // (Spec §4.2). Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md.
  const [simbriefUsername, setSimbriefUsername] = useState<string>(
    () => localStorage.getItem("simbrief_username") ?? "",
  );
  const [simbriefUserId, setSimbriefUserId] = useState<string>(
    () => localStorage.getItem("simbrief_user_id") ?? "",
  );

  // v0.7.17 (F-001): Fenix A32x Beta is no longer an opt-in toggle —
  // the backend auto-detects Fenix profiles and applies the LVAR
  // overrides unconditionally. localStorage key `fenix_beta_enabled`
  // becomes a no-op and gets cleaned up here so leftover state from
  // v0.7.16 doesn't sit around.
  useEffect(() => {
    localStorage.removeItem("fenix_beta_enabled");
  }, []);
  const [verifying, setVerifying] = useState(false);
  const [verifyStatus, setVerifyStatus] = useState<{
    tone: "ok" | "err";
    text: string;
  } | null>(null);

  // v0.7.14: Discord-Webhook-UI entfernt. Discord-Posts macht ab v0.7.14
  // der Recorder auf live.kant.ovh zentral — VA-Owner setzt die URL einmal
  // im Webapp-Admin (https://live.kant.ovh/admin/ → Settings → Discord),
  // Pilots tun nichts. Audit C1.

  // v0.7.8: Auto-clear verify-status nach 8s.
  useEffect(() => {
    if (!verifyStatus) return;
    const id = window.setTimeout(() => setVerifyStatus(null), 8000);
    return () => window.clearTimeout(id);
  }, [verifyStatus]);

  // v0.7.8: Persistiere SimBrief-Settings bei onBlur in localStorage +
  // Backend. KEIN Test-Fetch hier (= Pilot druckt Pruefen-Button
  // explizit, Spec §4.4 Punkt 1).
  function persistSimbriefSettings(username: string, userId: string) {
    const u = username.trim();
    const i = userId.trim();
    if (u) localStorage.setItem("simbrief_username", u);
    else localStorage.removeItem("simbrief_username");
    if (i) localStorage.setItem("simbrief_user_id", i);
    else localStorage.removeItem("simbrief_user_id");
    void invoke("set_simbrief_settings", {
      username: u || null,
      userId: i || null,
    }).catch(() => null);
  }

  async function handleVerifySimbrief() {
    if (verifying) return;
    setVerifying(true);
    setVerifyStatus(null);
    try {
      const result = await invoke<{
        ok: boolean;
        origin?: string;
        destination?: string;
        callsign?: string;
        error_code?: string;
      }>("verify_simbrief_identifier", {
        username: simbriefUsername.trim() || null,
        userId: simbriefUserId.trim() || null,
      });
      if (result.ok) {
        setVerifyStatus({
          tone: "ok",
          text: t("settings.simbrief.verify_ok", {
            origin: result.origin ?? "—",
            destination: result.destination ?? "—",
            callsign: result.callsign ?? "—",
          }),
        });
      } else {
        const errCode = result.error_code ?? "unknown";
        setVerifyStatus({
          tone: "err",
          text: t(`settings.simbrief.verify_err_${errCode}`),
        });
      }
    } catch (err: unknown) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      setVerifyStatus({ tone: "err", text: msg });
    } finally {
      setVerifying(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const k = await invoke<string>("sim_get_kind");
        if (!cancelled) setKind(k as SimKind);
      } catch {
        if (!cancelled) setKind("off");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleKindChange(next: SimKind) {
    if (busy) return;
    setBusy(true);
    setKind(next);
    try {
      await invoke("sim_set_kind", { kind: next });
    } catch {
      // ignore
    } finally {
      setBusy(false);
    }
  }

  const language = i18n.resolvedLanguage ?? "en";

  // v0.11.0-dev: Tab-Navigation für die Settings, weil das alles auf
  // einer endlos langen Liste „zu durcheinander" war (Pilot-Feedback).
  // Drei Gruppen:
  //   - required: SimBrief + Simulator + Filing → was AeroACARS zwingend
  //     braucht damit Flight-Tracking funktioniert
  //   - extras: Sprache/Theme + Verhalten + Discord-RPC + Speicher +
  //     Fehler-Reporting → optionale Komfort/Privacy-Einstellungen
  //   - tech: Entwickler-Debug + Orphan-Flights-Cleanup → selten
  //     gebraucht, primär für Troubleshooting
  // Tab-Wahl wird in localStorage gemerkt, damit der Pilot beim nächsten
  // Settings-Öffnen wieder dort landet wo er war.
  type SettingsTab = "simulator" | "required" | "extras" | "plugins" | "tech";
  const [activeTab, setActiveTab] = useState<SettingsTab>(() => {
    try {
      const saved = localStorage.getItem("aeroacars.settings.activeTab");
      if (
        saved === "simulator" ||
        saved === "required" ||
        saved === "extras" ||
        saved === "plugins" ||
        saved === "tech"
      ) {
        return saved;
      }
    } catch {
      /* noop */
    }
    // Default für Erst-Nutzer: Simulator-Tab, weil das die allerwichtigste
    // erste Einstellung ist (ohne Sim funktioniert nichts).
    return "simulator";
  });
  const switchTab = (next: SettingsTab) => {
    setActiveTab(next);
    try {
      localStorage.setItem("aeroacars.settings.activeTab", next);
    } catch {
      /* noop */
    }
  };

  const tabHintKey = `settings.tabs.${activeTab}_hint` as const;

  return (
    <section className="settings">
      <header className="settings__header">
        <h2>{t("settings.title")}</h2>
        <p className="settings__hint">{t("settings.description")}</p>
      </header>

      {/* Tab-Bar — drei Pills, aktive in Akzent-Farbe */}
      <div
        role="tablist"
        aria-label={t("settings.title") ?? "Settings"}
        style={{
          display: "flex",
          gap: 6,
          marginBottom: 8,
          padding: 4,
          background: "rgba(255,255,255,0.04)",
          border: "1px solid rgba(255,255,255,0.08)",
          borderRadius: 10,
        }}
      >
        {(["simulator", "required", "extras", "plugins", "tech"] as SettingsTab[]).map((tab) => {
          const isActive = activeTab === tab;
          return (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => switchTab(tab)}
              style={{
                flex: 1,
                padding: "8px 14px",
                background: isActive
                  ? "rgba(59,130,246,0.18)"
                  : "transparent",
                border: isActive
                  ? "1px solid rgba(59,130,246,0.45)"
                  : "1px solid transparent",
                borderRadius: 7,
                color: isActive
                  ? "rgba(255,255,255,0.96)"
                  : "rgba(255,255,255,0.65)",
                fontSize: "0.88rem",
                fontWeight: isActive ? 600 : 500,
                cursor: "pointer",
                transition: "all 0.15s ease",
              }}
            >
              {t(`settings.tabs.${tab}`)}
            </button>
          );
        })}
      </div>
      <p
        className="settings__hint"
        style={{
          marginTop: 0,
          marginBottom: 14,
          fontSize: "0.82rem",
          opacity: 0.7,
        }}
      >
        {t(tabHintKey)}
      </p>

      {/* ─── Tab: Simulator ────────────────────────────────────────
          v0.11.0-dev: eigener Tab nur für die Sim-Auswahl. Das ist die
          allererste Einstellung die ein neuer Pilot braucht — wenn
          falsch oder nicht gesetzt, hilft AeroACARS nicht. Default-
          Tab für Erst-Nutzer, damit der Sim sofort ins Auge sticht.
      */}
      {activeTab === "simulator" && (
        <div className="settings__section">
          <h3>{t("settings.simulator_section")}</h3>
          <p className="settings__row-hint">{t("settings.simulator_hint")}</p>
          <label className="settings__field">
            <span className="settings__field-label">
              {t("settings.simulator_label")}
            </span>
            <select
              value={kind ?? "off"}
              onChange={(e) => handleKindChange(e.target.value as SimKind)}
              disabled={busy || kind === null}
            >
              {ALL_KINDS.map((k) => (
                <option key={k} value={k}>
                  {t(`sim.kinds.${k}`)}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}

      {/* ─── Tab: Benötigt ────────────────────────────────────────
          SimBrief + Flug-Aufzeichnungs-Verhalten. Simulator-Auswahl
          ist in einen eigenen Tab umgezogen (v0.11.0-dev).
      */}
      {activeTab === "required" && (
        <>
          {/* v0.7.8: SimBrief Integration — fuer Direct-OFP-Refresh
              ohne phpVMS-Bid-Pointer (W5-Workaround). */}
          <div className="settings__section">
            <h3>{t("settings.simbrief.title")}</h3>
            <p className="settings__row-hint">{t("settings.simbrief.intro")}</p>

            <label className="settings__field">
              <span className="settings__field-label">
                {t("settings.simbrief.username_label")}
              </span>
              <input
                type="text"
                value={simbriefUsername}
                onChange={(e) => setSimbriefUsername(e.target.value)}
                onBlur={() => persistSimbriefSettings(simbriefUsername, simbriefUserId)}
                placeholder="z.B. thomaskant"
                autoComplete="off"
                spellCheck={false}
              />
              <small>{t("settings.simbrief.username_hint")}</small>
            </label>

            <label className="settings__field">
              <span className="settings__field-label">
                {t("settings.simbrief.userid_label")}
              </span>
              <input
                type="text"
                inputMode="numeric"
                value={simbriefUserId}
                onChange={(e) =>
                  setSimbriefUserId(e.target.value.replace(/[^0-9]/g, ""))
                }
                onBlur={() => persistSimbriefSettings(simbriefUsername, simbriefUserId)}
                placeholder="z.B. 612345"
                autoComplete="off"
                spellCheck={false}
              />
              <small>{t("settings.simbrief.userid_hint")}</small>
            </label>

            <div className="settings__field" style={{ flexDirection: "row", gap: 10, alignItems: "center" }}>
              <button
                type="button"
                onClick={handleVerifySimbrief}
                disabled={
                  verifying ||
                  (!simbriefUsername.trim() && !simbriefUserId.trim())
                }
              >
                {verifying ? "…" : t("settings.simbrief.verify_button")}
              </button>
              {verifyStatus && (
                <span
                  style={{
                    fontSize: "0.85rem",
                    color: verifyStatus.tone === "ok" ? "#4ade80" : "#f87171",
                  }}
                >
                  {verifyStatus.tone === "ok" ? "✓ " : "⚠ "}
                  {verifyStatus.text}
                </span>
              )}
            </div>
          </div>

          <div className="settings__section">
            <h3>{t("settings.filing_section")}</h3>
            <label className="settings__checkbox">
              <input
                type="checkbox"
                checked={autoFile}
                onChange={(e) => onAutoFileChange(e.target.checked)}
              />
              <span>
                <strong>{t("settings.auto_file_label")}</strong>
                <span className="settings__row-hint">
                  {t("settings.auto_file_hint")}
                </span>
              </span>
            </label>
            <label className="settings__checkbox">
              <input
                type="checkbox"
                checked={autoStart}
                onChange={(e) => onAutoStartChange(e.target.checked)}
              />
              <span>
                <strong>Auto-Start aufzeichnen</strong>
                <span className="settings__row-hint">
                  Startet einen Flug automatisch, sobald das Flugzeug am
                  Departure-Airport eines deiner Bids steht (≤ 5 km, On-Ground,
                  Engines aus). Watcher tickt alle 3 s.
                </span>
              </span>
            </label>
            <label className="settings__checkbox">
              <input
                type="checkbox"
                checked={approachAdvisoriesEnabled}
                onChange={(e) => onApproachAdvisoriesEnabledChange(e.target.checked)}
              />
              <span>
                <strong>{t("approach_advisory.settings_label")}</strong>
                <span className="settings__row-hint">
                  {t("approach_advisory.settings_hint")}
                </span>
              </span>
            </label>
          </div>
        </>
      )}

      {/* ─── Tab: Komfort ─────────────────────────────────────────
          Sprache/Theme, Verhalten, Speicher, Discord-RPC, Fehler-
          Reporting — optionale Qualität-Verbesserungen.
      */}
      {activeTab === "extras" && (
        <>
          <div className="settings__section">
            <h3>{t("settings.appearance_section")}</h3>

            <label className="settings__field">
              <span className="settings__field-label">
                {t("settings.language_label")}
              </span>
              <select
                value={language}
                onChange={(e) => setLanguage(e.target.value as SupportedLanguage)}
              >
                {SUPPORTED_LANGUAGES.map((lng) => (
                  <option key={lng} value={lng}>{LANGUAGE_LABELS[lng]}</option>
                ))}
              </select>
            </label>

            <label className="settings__field">
              <span className="settings__field-label">
                {t("settings.theme_label")}
              </span>
              <select
                value={theme}
                onChange={(e) => onThemeChange(e.target.value as Theme)}
              >
                <option value="dark">{t("settings.theme_dark")}</option>
                <option value="light">{t("settings.theme_light")}</option>
              </select>
            </label>
          </div>

          <div className="settings__section">
            <h3>{t("behaviour.section_title")}</h3>
            <label className="settings__checkbox">
              <input
                type="checkbox"
                checked={minimizeToTray}
                onChange={(e) => onMinimizeToTrayChange(e.target.checked)}
              />
              <span>
                <strong>{t("behaviour.minimize_to_tray_label")}</strong>
                <span
                  className="settings__row-hint"
                  dangerouslySetInnerHTML={{
                    __html: t("behaviour.minimize_to_tray_hint"),
                  }}
                />
              </span>
            </label>
          </div>

          <div className="settings__section">
            <h3>{t("settings.storage_section")}</h3>
            <FlightLogsManager
              autoDelete={autoDeleteFlightLogs}
              onAutoDeleteChange={onAutoDeleteFlightLogsChange}
            />
          </div>

          {/* v0.9.0 (#GlitchTip): Anonyme Fehler-Telemetrie.
              Opt-In, Default = aus. Pflicht laut DSGVO Art. 6 (1) a. */}
          <ErrorReportingPanel />

          {/* v0.9.0 (#Discord-RPC): Rich-Presence im Discord-Profil.
              Opt-In, Default = aus. */}
          <DiscordRpcPanel />
        </>
      )}

      {/* ─── Tab: Plugins ─────────────────────────────────────────
          Flugzeug-spezifische Add-ons (PMDG SDK, AeroACARS-X-Plane-
          Plugin). v0.11.0-dev: in eigenen Tab gezogen — vorher
          waren die unter „Developer → Debug" versteckt, obwohl sie
          für PMDG-/X-Plane-Piloten ein normaler Settings-Bereich
          sind. Beide Panels rendern jetzt unabhängig vom Debug-Mode
          (die Sichtbarkeit der Karten selbst hängt nur am Sim-
          State und ob das passende Plugin/Aircraft erkannt wird).
      */}
      {activeTab === "plugins" && (
        <>
          <PmdgPremiumPanel
            simState={simStatus?.state ?? "disconnected"}
            simSnapshot={simStatus?.snapshot ?? null}
          />
          <XPlanePremiumPanel
            simState={simStatus?.state ?? "disconnected"}
          />
        </>
      )}

      {/* ─── Tab: Technik ─────────────────────────────────────────
          Debug-Mode + Sim-Debug + phpVMS-Heartbeat und Orphan-Flight-
          Cleanup — selten gebraucht, primär für Dev/Troubleshooting.
          PMDG-/X-Plane-Premium-Panels sind nicht mehr hier (siehe
          „Plugins"-Tab).
      */}
      {activeTab === "tech" && (
        <>
          <div className="settings__section">
            <h3>{t("settings.developer_section")}</h3>
            <label className="settings__checkbox">
              <input
                type="checkbox"
                checked={debugMode}
                onChange={(e) => onDebugModeChange(e.target.checked)}
              />
              <span>
                <strong>{t("settings.debug_mode_label")}</strong>
                <span className="settings__row-hint">
                  {t("settings.debug_mode_hint")}
                </span>
              </span>
            </label>

            {debugMode && (
              <div className="settings__debug-panel">
                <SimDebugPanel status={simStatus} />
                <PhpvmsHeartbeatDebug activeFlight={activeFlight} />
              </div>
            )}
          </div>

          {/* v0.7.18 (B-011) — Orphan-Flight-Cleanup. Notnagel-
              Funktion, deshalb im Tech-Tab. */}
          <OrphanFlightsPanel />
        </>
      )}
    </section>
  );
}

/**
 * v0.9.0 (#GlitchTip) — Anonyme Fehler-Telemetrie an unseren self-hosted
 * GlitchTip-Server. Opt-In, Default = aus. Wenn an: nur anonymisierte
 * Crash/Error-Events ohne PII, Auth-Header, Position oder Routen-
 * Details. Tag-Allowlist + Redaction-Hook strippen alles vor dem Send.
 *
 * Spec: docs/spec/v0.9.0-glitchtip-self-hosted.md (LE4)
 *     + docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (DSGVO).
 *
 * Persistence: localStorage `aeroacars.errorReporting.enabled`
 * + Mirror in den Rust-Backend per `error_reporting_set_consent`.
 * UI-Hint zeigt sofort an dass der Toggle sofort wirkt (vor jedem
 * Sentry-Event wird die Consent-Atomic geprueft).
 */
function ErrorReportingPanel() {
  const { t } = useTranslation();
  const [enabled, setEnabledState] = useState<boolean>(() => getConsent());

  const handleToggle = (next: boolean) => {
    setEnabledState(next);
    setConsent(next);
    // Mirror in Rust-Backend (Atomic-Gate). Wenn der Command nicht
    // existiert (alter Build), ignorieren — Default bleibt OFF.
    void invoke("error_reporting_set_consent", { enabled: next }).catch(
      () => undefined,
    );
  };

  return (
    <div className="settings__section">
      <h3>{t("error_reporting.section_title")}</h3>
      <p className="settings__row-hint">{t("error_reporting.intro")}</p>
      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => handleToggle(e.target.checked)}
        />
        <span>
          <strong>{t("error_reporting.toggle_label")}</strong>
          <span
            className="settings__row-hint"
            dangerouslySetInnerHTML={{
              __html: t("error_reporting.toggle_hint"),
            }}
          />
        </span>
      </label>
    </div>
  );
}

/**
 * Live status card for the phpVMS API connection — keeps the same
 * visual language as the SimDebugPanel above so the Settings → Debug
 * area reads as a single coherent block. Re-renders once a second to
 * keep the relative ages live.
 *
 * State-color mapping mirrors the simulator card:
 *   - no active flight        → disconnected (grey)
 *   - heartbeat ≤ 45 s ago    → connected (green pulse)
 *   - heartbeat 45-90 s       → connecting (yellow pulse, "stale")
 *   - heartbeat > 90 s / never→ connecting (yellow, but with a stale label)
 * "Connecting" is reused for both warn cases because phpVMS doesn't
 * have an "error" CSS variant in the sim-panel system; a yellow pulse
 * is the right level of "this is fishy, look at me" without false-
 * alarming red on a single dropped packet.
 */
function PhpvmsHeartbeatDebug({ activeFlight }: { activeFlight: ActiveFlightInfo | null }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const ageSec = (iso: string | null): number | null => {
    if (!iso) return null;
    return Math.max(0, Math.floor((now - new Date(iso).getTime()) / 1000));
  };
  const fmtAge = (sec: number | null): string => {
    if (sec === null) return t("phpvms_status.age_unknown");
    if (sec < 60) return t("phpvms_status.age_seconds", { seconds: sec });
    const m = Math.floor(sec / 60);
    const r = sec % 60;
    return r === 0
      ? t("phpvms_status.age_minutes", { minutes: m })
      : t("phpvms_status.age_minutes_seconds", { minutes: m, seconds: r });
  };

  if (!activeFlight) {
    return (
      <section className="sim-panel sim-panel--disconnected">
        <header className="sim-panel__header">
          <div className="sim-panel__header-left">
            <h2>{t("phpvms_status.title")}</h2>
            <span className="sim-panel__kind">{t("phpvms_status.badge_heartbeat")}</span>
          </div>
          <span className="sim-panel__state">
            <span className="sim-panel__dot" /> {t("phpvms_status.state_inactive")}
          </span>
        </header>
        <p className="sim-panel__hint">{t("phpvms_status.no_active_flight")}</p>
      </section>
    );
  }

  const heartbeatAge = ageSec(activeFlight.last_heartbeat_at);
  const positionAge = ageSec(activeFlight.last_position_at);
  // Heartbeat fires every 30s by design — anything beyond ~45s means a
  // posted call failed or the streamer hasn't gotten its first one in yet.
  const isStale = heartbeatAge === null || heartbeatAge > 45;
  const state = isStale ? "connecting" : "connected";
  const stateLabel = isStale
    ? t("phpvms_status.state_waiting")
    : t("phpvms_status.state_active");
  // Truncate the PIREP id for the badge — full id is ~16 chars, that's
  // too wide for a header pill. The first 6 chars are enough to
  // disambiguate in practice.
  const pirepBadge = activeFlight.pirep_id.length > 8
    ? `${activeFlight.pirep_id.slice(0, 6)}…`
    : activeFlight.pirep_id;

  return (
    <section className={`sim-panel sim-panel--${state}`}>
      <header className="sim-panel__header">
        <div className="sim-panel__header-left">
          <h2>{t("phpvms_status.title")}</h2>
          <span className="sim-panel__kind">
            {t("phpvms_status.badge_pirep_prefix")} {pirepBadge}
          </span>
        </div>
        <span className={`sim-panel__state sim-panel__state--${state}`}>
          <span className="sim-panel__dot" /> {stateLabel}
        </span>
      </header>
      <dl className="sim-panel__compact">
        <dt>{t("phpvms_status.row_last_position")}</dt>
        <dd>
          {fmtAge(positionAge)}
          {positionAge !== null && positionAge > 60 && (
            <span className="sim-panel__compact-muted"> · {t("phpvms_status.muted_stale")}</span>
          )}
        </dd>
        <dt>{t("phpvms_status.row_last_heartbeat")}</dt>
        <dd>
          {fmtAge(heartbeatAge)}
          {heartbeatAge === null && (
            <span className="sim-panel__compact-muted"> · {t("phpvms_status.muted_not_yet_sent")}</span>
          )}
        </dd>
        {activeFlight.queued_position_count > 0 && (
          <>
            <dt>{t("phpvms_status.row_position_queue")}</dt>
            <dd>
              {activeFlight.queued_position_count}{" "}
              {/* v0.6.2 — Label hängt am echten Connection-State, nicht
                 nur am Backlog. „queued (offline)" für jeden Backlog war
                 missverständlich (= Pilot dachte Connection weg). */}
              <span className="sim-panel__compact-muted">
                {activeFlight.connection_state === "failing"
                  ? t("phpvms_status.muted_pending_offline")
                  : t("phpvms_status.muted_pending_sync")}
              </span>
            </dd>
          </>
        )}
      </dl>
    </section>
  );
}

/**
 * Settings → Speicher: lets the pilot manage on-disk per-flight JSONL
 * recorder files. Two controls:
 *  - Toggle for the auto-purge sweep that the App.tsx mount triggers
 *    (default ON, threshold 30 days).
 *  - "Alle löschen jetzt" button (with confirm) for the manual nuke.
 *
 * The backing files are at `<app_data_dir>/flight_logs/<pirep_id>.jsonl`
 * — see README → Troubleshooting for the exact OS-specific paths.
 * Stats are loaded once on mount and re-fetched after a successful
 * delete so the user sees the file count drop immediately.
 */
function FlightLogsManager({
  autoDelete,
  onAutoDeleteChange,
}: {
  autoDelete: boolean;
  onAutoDeleteChange: (next: boolean) => void;
}) {
  const { t } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [stats, setStats] = useState<{ count: number; total_bytes: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Inline success-feedback shown right under the delete button.
  // Replaces the v0.3.1 `window.alert()` which is silently dropped on
  // macOS WKWebView (same bug class as `confirm()`). Cleared on next
  // refresh, so the user sees it briefly and then it goes away.
  const [doneMsg, setDoneMsg] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const s = await invoke<{ count: number; total_bytes: number }>("flight_logs_stats");
      setStats(s);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const handleDeleteAll = async () => {
    const ok = await confirm({
      message: t("settings.delete_all_logs_confirm"),
      destructive: true,
    });
    if (!ok) return;
    setBusy(true);
    setDoneMsg(null);
    try {
      const res = await invoke<{ deleted: number }>("flight_logs_delete_all");
      await refresh();
      // v0.11.0-dev (QS-Audit): direkt i18next-Plural-Resolution nutzen.
      // i18next-V4 mappt `count` automatisch auf `_one`/`_other` —
      // der vorherige defaultValue-Hack mit manuellem Branching war ein
      // Code-Smell und triggerte den Audit-„missing-in-EN"-False-Positive
      // weil der Base-Key `delete_all_logs_done` nie existieren musste.
      setDoneMsg(
        t("settings.delete_all_logs_done", { count: res.deleted }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const fmtSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  };

  const usageText = (() => {
    if (stats === null) return t("settings.storage_usage_loading");
    if (stats.count === 0) return t("settings.storage_usage_empty");
    const key = stats.count === 1
      ? "settings.storage_usage_count_one"
      : "settings.storage_usage_count_other";
    return t(key, { count: stats.count, size: fmtSize(stats.total_bytes) });
  })();

  return (
    <>
      {confirmDialog}
      <label className="settings__checkbox">
        <input
          type="checkbox"
          checked={autoDelete}
          onChange={(e) => onAutoDeleteChange(e.target.checked)}
        />
        <span>
          <strong>{t("settings.auto_delete_logs_label")}</strong>
          <span
            className="settings__row-hint"
            dangerouslySetInnerHTML={{ __html: t("settings.auto_delete_logs_hint") }}
          />
        </span>
      </label>

      <div className="storage-card">
        <div className="storage-card__row">
          <span className="storage-card__label">{t("settings.storage_usage_label")}</span>
          <span className="storage-card__value">{usageText}</span>
        </div>
        <div className="storage-card__actions">
          <button
            type="button"
            className="storage-card__btn storage-card__btn--danger"
            onClick={handleDeleteAll}
            disabled={busy || stats === null || stats.count === 0}
          >
            {busy ? t("settings.delete_all_logs_busy") : t("settings.delete_all_logs_button")}
          </button>
          <button
            type="button"
            className="storage-card__btn"
            onClick={() => void refresh()}
            disabled={busy}
            aria-label={t("settings.refresh_button_label")}
            title={t("settings.refresh_button_title")}
          >
            ↻
          </button>
        </div>
        {error && <p className="storage-card__error">{error}</p>}
        {doneMsg && <p className="storage-card__done">{doneMsg}</p>}
      </div>
    </>
  );
}
