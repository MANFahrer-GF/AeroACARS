import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { applyTheme, getInitialTheme, type Theme } from "./theme";
import { LoginPage } from "./components/LoginPage";
import { CockpitView } from "./components/CockpitView";
import { BriefingView } from "./components/BriefingView";
import { SettingsPanel } from "./components/SettingsPanel";
import { ReleaseNotesModal } from "./components/ReleaseNotesModal";
import { ActivityLogPanel } from "./components/ActivityLogPanel";
import { AboutPanel } from "./components/AboutPanel";
import { LandingPanel } from "./components/LandingPanel";
import RunwayDiagramPreview from "./dev/RunwayDiagramPreview";
import { UpdateButton } from "./components/UpdateButton";
import { UpdateBanner } from "./components/UpdateBanner";
import { ErrorReportingFirstRunBanner } from "./components/ErrorReportingFirstRunBanner";
import { useDiscordRpcPush } from "./hooks/useDiscordRpcPush";
import { LiveRecordingIndicator } from "./components/LiveRecordingIndicator";
import { useSimSession } from "./hooks/useSimSession";
import { useUpdateChecker } from "./hooks/useUpdateChecker";
import type { ActiveFlightInfo, LoginResult, Profile, UiError } from "./types";

type SessionStatus =
  | { kind: "loading" }
  /** v0.12.1 (Stream B LE6): `restoreError` is set when a session-restore
   *  was blocked by the pilot-status gate — the login form surfaces it. */
  | { kind: "loggedOut"; restoreError?: UiError }
  | { kind: "loggedIn"; session: LoginResult };

type Tab = "cockpit" | "briefing" | "landing" | "log" | "settings" | "about" | "devpreview";

const DEBUG_STORAGE_KEY = "aeroacars.debug";
const AUTO_FILE_STORAGE_KEY = "aeroacars.autoFile";
const AUTO_START_STORAGE_KEY = "aeroacars.autoStart";
const AUTO_DELETE_LOGS_STORAGE_KEY = "aeroacars.autoDeleteFlightLogs";
/** Days threshold for the auto-purge sweep. Mirrors the wording of the
 *  Settings hint — keep both in sync if you ever change it. */
const AUTO_DELETE_LOGS_DAYS = 30;

/** Tracks the version for which the "What's new" modal was last
 *  shown. App start compares this against the running version and
 *  pops the modal once when they differ. Stays sticky through
 *  re-launches so we don't re-show on every startup. */
const RELEASE_NOTES_LAST_SEEN_KEY = "aeroacars.releaseNotes.lastSeenVersion";

const MINIMIZE_TO_TRAY_KEY = "aeroacars.minimizeToTray";
const APPROACH_ADVISORIES_KEY = "aeroacars.approachAdvisories";

/** v0.5.38: Stable-Approach-Banner im Cockpit-Tab. Default ON ("1"
 *  bedeutet aktiv; nur explizites "0" oder leerer Storage-Wert beim
 *  ersten Run setzt OFF — wir wollen Default ON für Safety). */
function loadApproachAdvisories(): boolean {
  const v = localStorage.getItem(APPROACH_ADVISORIES_KEY);
  return v === null ? true : v === "1";
}

function saveApproachAdvisories(value: boolean): void {
  localStorage.setItem(APPROACH_ADVISORIES_KEY, value ? "1" : "0");
}

/** Default OFF on both platforms — most pilots expect "X = quit"
 *  even on Mac. The setting hint explains why someone might want it
 *  on (long flights / heartbeat keep-alive). Persisted "1" → on,
 *  anything else (incl. unset) → off. */
function loadMinimizeToTray(): boolean {
  return localStorage.getItem(MINIMIZE_TO_TRAY_KEY) === "1";
}

function saveMinimizeToTray(value: boolean): void {
  localStorage.setItem(MINIMIZE_TO_TRAY_KEY, value ? "1" : "0");
}

function loadLastSeenReleaseNotesVersion(): string | null {
  return localStorage.getItem(RELEASE_NOTES_LAST_SEEN_KEY);
}

function saveLastSeenReleaseNotesVersion(version: string): void {
  localStorage.setItem(RELEASE_NOTES_LAST_SEEN_KEY, version);
}

function loadDebugMode(): boolean {
  return localStorage.getItem(DEBUG_STORAGE_KEY) === "1";
}

function saveDebugMode(value: boolean) {
  localStorage.setItem(DEBUG_STORAGE_KEY, value ? "1" : "0");
}

/** Auto-file the PIREP when the FSM reaches Arrived. Default ON —
 *  removes one click from the happy path. Disabling forces the
 *  pilot to hit "Flug beenden" manually, useful when they want to
 *  inspect mass / fuel / activity log before submitting. */
function loadAutoFile(): boolean {
  const v = localStorage.getItem(AUTO_FILE_STORAGE_KEY);
  // Default true: only persisted "0" disables.
  return v !== "0";
}

function saveAutoFile(value: boolean) {
  localStorage.setItem(AUTO_FILE_STORAGE_KEY, value ? "1" : "0");
}

/** Auto-start a flight when the aircraft is parked at the departure
 *  airport of one of the user's bids. Default OFF — opt-in feature.
 *  Backend watcher polls every 3 s while enabled. */
function loadAutoStart(): boolean {
  return localStorage.getItem(AUTO_START_STORAGE_KEY) === "1";
}

function saveAutoStart(value: boolean) {
  localStorage.setItem(AUTO_START_STORAGE_KEY, value ? "1" : "0");
}

/** Sweep stale per-flight JSONL recorder files at app start. Default
 *  ON — keeps the app data dir from accumulating gigabytes over years
 *  of flying. Pilots who want every flight retained forever can flip
 *  the toggle off in Settings → Speicher. Only persisted "0" disables. */
function loadAutoDeleteFlightLogs(): boolean {
  return localStorage.getItem(AUTO_DELETE_LOGS_STORAGE_KEY) !== "0";
}

function saveAutoDeleteFlightLogs(value: boolean) {
  localStorage.setItem(AUTO_DELETE_LOGS_STORAGE_KEY, value ? "1" : "0");
}

/**
 * Map a SimKind string to the brand label shown on the top-right
 * status pill. Pilots want to see WHICH sim is connected, not the
 * generic word "Simulator". Falls back to "SIM" when nothing is
 * selected so the pill never goes blank.
 */
function simKindLabel(kind: string | undefined): string {
  switch (kind) {
    case "msfs2024":
    case "msfs2020":
      return "MSFS";
    case "xplane11":
    case "xplane12":
      return "X-PLANE";
    case "off":
      return "SIM OFF";
    default:
      return "SIM";
  }
}

function App() {
  const { t } = useTranslation();
  const [theme, setTheme] = useState<Theme>(() => getInitialTheme());
  const [status, setStatus] = useState<SessionStatus>({ kind: "loading" });
  const [tab, setTab] = useState<Tab>("briefing");
  const [debugMode, setDebugMode] = useState<boolean>(() => loadDebugMode());
  const [autoFile, setAutoFile] = useState<boolean>(() => loadAutoFile());
  const [autoStart, setAutoStart] = useState<boolean>(() => loadAutoStart());
  const [autoDeleteFlightLogs, setAutoDeleteFlightLogs] = useState<boolean>(
    () => loadAutoDeleteFlightLogs(),
  );
  const [minimizeToTray, setMinimizeToTray] = useState<boolean>(
    () => loadMinimizeToTray(),
  );
  const [approachAdvisoriesEnabled, setApproachAdvisoriesEnabled] = useState<boolean>(
    () => loadApproachAdvisories(),
  );

  // Sync the minimize-to-tray flag to the Rust backend whenever it
  // changes, plus on first mount. Backend default is `false`;
  // localStorage is the source of truth across restarts. The actual
  // close-handler in Rust just reads this atomic flag.
  useEffect(() => {
    void invoke("set_minimize_to_tray", { enabled: minimizeToTray }).catch(() => {});
  }, [minimizeToTray]);

  // v0.12.6: Auto-File-Flag in den Rust-Backend-State spiegeln — bei
  // Mount UND bei jedem Toggle. Der Position-Streamer im Backend filet
  // den PIREP beim Latch auf `Arrived` selbst, sobald dieses Flag an
  // ist (fenster-unabhängig). localStorage ist die Source of Truth;
  // das Backend defaultet auf `true` bis dieser Sync läuft.
  useEffect(() => {
    void invoke("set_auto_file_enabled", { enabled: autoFile }).catch(() => {});
  }, [autoFile]);
  /** Version we should pop the release-notes modal for. Set on first
   *  mount when the running version differs from the lastSeen
   *  localStorage entry, AND when the user manually triggers it via
   *  the About panel. Cleared by ReleaseNotesModal.onClose. */
  const [releaseNotesVersion, setReleaseNotesVersion] = useState<string | null>(
    null,
  );
  const { status: simStatus, snapshot: simSnapshot } = useSimSession();

  // One-shot: if the app version has changed since the user last saw
  // the release-notes modal, fire it on next mount. New installs
  // (no lastSeen) silently set lastSeen=current to avoid greeting
  // first-timers with a "what changed since v0.0.0" message.
  useEffect(() => {
    void (async () => {
      try {
        const info = await invoke<{ version: string } | null>("app_info");
        if (!info?.version) return;
        const lastSeen = loadLastSeenReleaseNotesVersion();
        if (lastSeen === null) {
          // First launch ever on this device — don't pop, just remember.
          saveLastSeenReleaseNotesVersion(info.version);
          return;
        }
        if (lastSeen !== info.version) {
          setReleaseNotesVersion(info.version);
        }
      } catch {
        // app_info missing or errored — silently skip
      }
    })();
  }, []);

  // Auto-purge stale flight log files once per app launch when the
  // toggle is on. Fires on mount only — re-toggling at runtime doesn't
  // re-sweep (next launch will). 30-day threshold matches the Settings
  // hint copy.
  useEffect(() => {
    if (!loadAutoDeleteFlightLogs()) return;
    void invoke("flight_logs_purge_older_than", {
      olderThanDays: AUTO_DELETE_LOGS_DAYS,
    }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync the persisted auto-start flag to the Rust backend on every
  // mount/change. Backend default is OFF; ab v0.3.0 ist der Backend-
  // Wert (persistiert in app_config_dir/auto_start.json) die Source
  // of truth — localStorage wäre im Tauri-Dev-Mode / nach Force-Kill
  // unzuverlässig. Beim ersten Mount fragen wir den Backend-Wert ab
  // und überschreiben den lokalen State falls der abweicht.
  useEffect(() => {
    let cancelled = false;
    void invoke<boolean>("auto_start_get_enabled")
      .then((backendValue) => {
        if (cancelled) return;
        if (backendValue !== autoStart) {
          // Backend-State zählt — Frontend nachziehen UND localStorage
          // aktualisieren damit's beim nächsten Frontend-Reload sofort
          // konsistent ist.
          setAutoStart(backendValue);
          saveAutoStart(backendValue);
        }
      })
      .catch(() => {
        // IPC-Fehler beim ersten Mount nicht tragisch — fallback auf
        // den localStorage-Wert. Wird beim nächsten Toggle korrigiert.
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync den Backend-Wert bei jeder echten Toggle-Änderung. Backend
  // persistiert selbst (siehe write_auto_start_persisted), localStorage
  // ist nur schneller Frontend-Cache fürs nächste Mount.
  //
  // v0.12.1 (Stream D): den ERSTEN Mount überspringen. Die Backend-Datei
  // `auto_start.json` ist die Source of Truth — Effect A oben liest sie.
  // Würde dieser Effect den (evtl. aus leerem localStorage stammenden)
  // Frontend-Wert beim Mount nach unten pushen, überschriebe er den
  // korrekt persistierten Backend-Stand. Genau das brach Auto-Start nach
  // macOS-Auto-Updates: das WebView-localStorage war zurückgesetzt →
  // `false` wurde gepusht und der gespeicherte `true`-Stand zerstört.
  // Nur bei einer echten Nutzer-Änderung syncen.
  const autoStartSyncDidMount = useRef(false);
  useEffect(() => {
    if (!autoStartSyncDidMount.current) {
      autoStartSyncDidMount.current = true;
      return;
    }
    void invoke("auto_start_set_enabled", { enabled: autoStart }).catch(() => {});
  }, [autoStart]);
  const simState = simStatus?.state ?? "disconnected";
  const [activeFlight, setActiveFlight] = useState<ActiveFlightInfo | null>(
    null,
  );

  // v0.5.48: Zentraler Update-Checker. Beide UI-Komponenten — der
  // Header-Button und das große Banner — konsumieren denselben State,
  // damit nicht doppelt gegen GitHub gepollt wird.
  const updateChecker = useUpdateChecker();

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const result = await invoke<LoginResult | null>("phpvms_load_session");
        if (cancelled) return;
        setStatus(
          result ? { kind: "loggedIn", session: result } : { kind: "loggedOut" },
        );
      } catch (e) {
        if (cancelled) return;
        // v0.12.1 (Stream B LE6): a session-restore blocked by the
        // pilot-status gate comes back as a UiError with a `pilot_*`
        // code — surface it on the login form. Other errors (network
        // etc.) stay silent, as before.
        const code =
          e && typeof e === "object" && "code" in e
            ? String((e as { code: unknown }).code)
            : "";
        const restoreError = code.startsWith("pilot_")
          ? (e as UiError)
          : undefined;
        setStatus({ kind: "loggedOut", restoreError });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Centralised active-flight polling. Lives at the top so both the
  // v0.7.8: Beim Login-Mount localStorage → Backend syncen damit
  // SimBrief-Settings nach App-Restart sofort verfuegbar sind, ohne
  // dass der Pilot Settings oeffnen muss. Spec §4.2 P2-Korrektur.
  useEffect(() => {
    if (status.kind !== "loggedIn") return;
    const username = localStorage.getItem("simbrief_username") ?? null;
    const userId = localStorage.getItem("simbrief_user_id") ?? null;
    if (username || userId) {
      void invoke("set_simbrief_settings", {
        username: username && username.trim() ? username.trim() : null,
        userId: userId && userId.trim() ? userId.trim() : null,
      }).catch(() => null);
    }
  }, [status.kind]);

  // Cockpit and the Briefing tab see the same state without duplicate
  // IPC calls. Cockpit auto-becomes the default tab once a flight
  // shows up; Briefing is the default while idle.
  useEffect(() => {
    if (status.kind !== "loggedIn") return;
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    async function poll() {
      try {
        const flight = await invoke<ActiveFlightInfo | null>("flight_status");
        if (cancelled) return;
        setActiveFlight(flight);
      } catch {
        // ignore
      }
    }
    void poll();
    timer = setInterval(poll, 2000);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [status.kind]);

  // Auto-switch to the cockpit tab the first time an active flight
  // appears (resume on startup, or just-started flight). The user can
  // still manually switch back to briefing afterwards — we only force
  // the switch on the rising edge.
  const [hadActiveFlight, setHadActiveFlight] = useState(false);
  useEffect(() => {
    if (activeFlight && !hadActiveFlight) {
      setTab("cockpit");
      setHadActiveFlight(true);
    }
    if (!activeFlight && hadActiveFlight) {
      setHadActiveFlight(false);
      // Flight just ended (filed / cancelled / discarded). PhpVMS
      // updates the pilot's `curr_airport` server-side as part of an
      // accepted PIREP, but our cached LoginResult never sees it
      // unless we re-fetch. Without this, the dashboard "Aktueller
      // Airport" stays at the old value until the next app restart.
      void invoke<Profile | null>("phpvms_refresh_profile")
        .then((fresh) => {
          if (!fresh) return;
          setStatus((prev) =>
            prev.kind === "loggedIn"
              ? {
                  kind: "loggedIn",
                  session: { ...prev.session, profile: fresh },
                }
              : prev,
          );
        })
        .catch(() => {});
    }
  }, [activeFlight, hadActiveFlight]);

  // v0.9.0 (#Discord-RPC): bei jedem activeFlight/simStatus-Update die
  // Presence ans Rust-Backend pushen. Backend dedupliziert (= no-op wenn
  // Settings.enabled=false oder nichts geaendert), also billig bei jedem
  // Render. Bei null/null wird die Activity sauber gecleart.
  useDiscordRpcPush({ activeFlight, simStatus });

  async function handleLogout() {
    try {
      await invoke("phpvms_logout");
    } catch {
      // Drop in-memory session even if the keyring call fails.
    }
    setStatus({ kind: "loggedOut" });
    setTab("briefing");
  }

  function handleDebugModeChange(next: boolean) {
    setDebugMode(next);
    saveDebugMode(next);
  }

  function handleAutoFileChange(next: boolean) {
    setAutoFile(next);
    saveAutoFile(next);
  }

  function handleAutoStartChange(next: boolean) {
    setAutoStart(next);
    saveAutoStart(next);
    // The useEffect on autoStart will sync the toggle to the Rust
    // watcher. setState alone won't fire it (React batches), so we
    // also pre-emptively call here for snappier UX.
    void invoke("auto_start_set_enabled", { enabled: next }).catch(() => {});
  }

  function handleAutoDeleteFlightLogsChange(next: boolean) {
    setAutoDeleteFlightLogs(next);
    saveAutoDeleteFlightLogs(next);
  }

  const phpvmsConnected = status.kind === "loggedIn";
  const simConnected = simState === "connected";
  const simConnecting = simState === "connecting";
  const showTabs = status.kind === "loggedIn";

  return (
    <main className="app">
      {/* v0.5.48 — großes Update-Banner ÜBER dem Header. Wird vom
          Hook + UpdateBanner-Komponente nur eingeblendet wenn (a)
          Update >= 3 Tage gesehen, (b) kein aktiver Flug, (c) nicht
          gerade snoozed. Sonst rendert die Komponente selbst null. */}
      <UpdateBanner
        checker={updateChecker}
        activePhase={activeFlight?.phase ?? null}
      />
      {/* v0.9.0 (#GlitchTip) — First-Run-Consent fuer anonyme Fehler-Telemetrie.
          Selbst-versteckend nach der ersten Entscheidung. Zeigt sich
          NUR beim allerersten Start (= leerer localStorage-Key). */}
      <ErrorReportingFirstRunBanner />
      <header className="app__header">
        <div className="app__brand">
          <h1>{t("app.name")}</h1>
          <p className="tagline">{t("app.tagline")}</p>
        </div>
        <div className="app__status-pills">
          <UpdateButton checker={updateChecker} />
          <span
            className={`status-pill status-pill--${
              phpvmsConnected ? "online" : "offline"
            }`}
            title={
              phpvmsConnected
                ? t("status.phpvms_connected")
                : t("status.phpvms_disconnected")
            }
          >
            <span className="status-pill__dot" />
            {t("status.phpvms")}
          </span>
          <span
            className={`status-pill status-pill--${
              simConnected ? "online" : simConnecting ? "connecting" : "offline"
            }`}
            title={
              simConnected
                ? t("status.simulator_connected")
                : simConnecting
                  ? t("status.simulator_connecting")
                  : t("status.simulator_disconnected")
            }
          >
            <span className="status-pill__dot" />
            {simKindLabel(simStatus?.kind)}
          </span>
          {activeFlight && (
            <LiveRecordingIndicator
              lastPositionAt={activeFlight.last_position_at}
              queuedCount={activeFlight.queued_position_count}
              positionCount={activeFlight.position_count}
              connectionState={
                // v0.7.17 (B-007): "blocked" durchreichen so dass der
                // LiveRecordingIndicator den Pilot warnen kann.
                activeFlight.connection_state === "blocked"
                  ? "blocked"
                  : activeFlight.connection_state === "failing"
                    ? "failing"
                    : "live"
              }
            />
          )}
        </div>
      </header>

      {showTabs && (
        <nav className="tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "cockpit"}
            className={`tab ${tab === "cockpit" ? "tab--active" : ""}`}
            onClick={() => setTab("cockpit")}
          >
            {t("tabs.cockpit")}
            {activeFlight && <span className="tab__badge" aria-hidden="true" />}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "briefing"}
            className={`tab ${tab === "briefing" ? "tab--active" : ""}`}
            onClick={() => setTab("briefing")}
          >
            {t("tabs.briefing")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "landing"}
            className={`tab ${tab === "landing" ? "tab--active" : ""}`}
            onClick={() => setTab("landing")}
          >
            {t("tabs.landing")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "log"}
            className={`tab ${tab === "log" ? "tab--active" : ""}`}
            onClick={() => setTab("log")}
          >
            {t("tabs.log")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "settings"}
            className={`tab ${tab === "settings" ? "tab--active" : ""}`}
            onClick={() => setTab("settings")}
          >
            {t("tabs.settings")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "about"}
            className={`tab ${tab === "about" ? "tab--active" : ""}`}
            onClick={() => setTab("about")}
          >
            {t("tabs.about")}
          </button>
          {import.meta.env.DEV && (
            <button
              type="button"
              role="tab"
              aria-selected={tab === "devpreview"}
              className={`tab ${tab === "devpreview" ? "tab--active" : ""}`}
              onClick={() => setTab("devpreview")}
              title="Dev-only: RunwayDiagram-Preview mit Mock-Daten"
            >
              🧪 Preview
            </button>
          )}
        </nav>
      )}

      {status.kind === "loading" && (
        <section className="phase">
          <p>{t("status.checking_session")}</p>
        </section>
      )}

      {status.kind === "loggedOut" && (
        <LoginPage
          initialError={status.restoreError}
          onSuccess={(s) => setStatus({ kind: "loggedIn", session: s })}
        />
      )}

      {status.kind === "loggedIn" && tab === "cockpit" && (
        <CockpitView
          session={status.session}
          activeFlight={activeFlight}
          setActiveFlight={setActiveFlight}
          simSnapshot={simSnapshot}
          onSwitchToBriefing={() => setTab("briefing")}
          approachAdvisoriesEnabled={approachAdvisoriesEnabled}
        />
      )}

      {status.kind === "loggedIn" && tab === "briefing" && (
        <BriefingView
          session={status.session}
          activeFlight={activeFlight}
          setActiveFlight={setActiveFlight}
          onLogout={handleLogout}
          simState={simState}
          simSnapshot={simSnapshot}
          onActiveFlightUpdated={() => {
            // v0.7.7: Bid-Tab-Refresh hat den OFP des aktiven Flugs
            // erfolgreich erneuert (changed=true). Status sofort
            // refetchen damit Cockpit + Loadsheet die neuen Plan-
            // Werte sehen — ohne den 2s-flight_status-Poll abzuwarten.
            // Spec docs/spec/ofp-refresh-during-boarding.md §6.5b.
            void invoke<ActiveFlightInfo | null>("flight_status")
              .then(setActiveFlight)
              .catch(() => null);
          }}
          onProfileRefreshed={(fresh) => {
            // Bubbled up from BidsList' Aktualisieren button.
            // Update the cached session so PilotHeader picks up
            // a new curr_airport / rank / airline without a
            // logout/login cycle. v0.1.30 — fixes the "Standort
            // bleibt veraltet"-Beschwerde.
            setStatus((prev) =>
              prev.kind === "loggedIn"
                ? {
                    kind: "loggedIn",
                    session: { ...prev.session, profile: fresh },
                  }
                : prev,
            );
          }}
        />
      )}

      {status.kind === "loggedIn" && tab === "landing" && <LandingPanel />}

      {status.kind === "loggedIn" && tab === "log" && <ActivityLogPanel />}

      {status.kind === "loggedIn" && tab === "settings" && (
        <SettingsPanel
          debugMode={debugMode}
          onDebugModeChange={handleDebugModeChange}
          autoFile={autoFile}
          onAutoFileChange={handleAutoFileChange}
          autoStart={autoStart}
          onAutoStartChange={handleAutoStartChange}
          autoDeleteFlightLogs={autoDeleteFlightLogs}
          onAutoDeleteFlightLogsChange={handleAutoDeleteFlightLogsChange}
          minimizeToTray={minimizeToTray}
          onMinimizeToTrayChange={(next) => {
            setMinimizeToTray(next);
            saveMinimizeToTray(next);
          }}
          approachAdvisoriesEnabled={approachAdvisoriesEnabled}
          onApproachAdvisoriesEnabledChange={(next) => {
            setApproachAdvisoriesEnabled(next);
            saveApproachAdvisories(next);
          }}
          theme={theme}
          onThemeChange={setTheme}
          simStatus={simStatus}
          activeFlight={activeFlight}
        />
      )}

      {status.kind === "loggedIn" && tab === "about" && (
        <AboutPanel onShowReleaseNotes={(v) => setReleaseNotesVersion(v)} />
      )}

      {import.meta.env.DEV &&
        status.kind === "loggedIn" &&
        tab === "devpreview" && <RunwayDiagramPreview />}

      {releaseNotesVersion && (
        <ReleaseNotesModal
          version={releaseNotesVersion}
          onClose={() => {
            saveLastSeenReleaseNotesVersion(releaseNotesVersion);
            setReleaseNotesVersion(null);
          }}
        />
      )}
    </main>
  );
}

export default App;
