import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import type {
  PmdgStatus,
  SimConnectionState,
  SimSnapshot,
} from "../types";

/**
 * Settings → Debug: PMDG SDK status card.
 *
 * Three states:
 *
 *  1. **Inactive** (no PMDG variant detected) — grey badge, hint
 *     that the panel becomes useful once a PMDG aircraft is loaded.
 *  2. **Active** (variant detected, data flowing) — green badge,
 *     shows variant + last-packet age. This is the happy path.
 *  3. **SDK disabled** (variant detected, subscribed, no packets
 *     for >5s, AND the sim is fully connected — see `simState` prop)
 *     — amber warning with the exact instructions to enable the SDK.
 *
 * v0.3.0: Die Warnung wird NUR angezeigt wenn `simState === "connected"`
 * — sonst sieht der Pilot beim App-Start (Sim noch im Loading) eine
 * irreführende "SDK nicht aktiviert"-Warnung obwohl einfach noch keine
 * Daten fließen können.
 *
 * Polls `pmdg_status` every 2s — cheap, the Tauri command just
 * reads a mutex on the adapter side.
 *
 * Phase H.4 / v0.2.0 — Boeing Premium Telemetry.
 */
interface Props {
  simState: SimConnectionState;
  /** Latest sim snapshot — used to detect "aircraft fully loaded"
   *  state. PMDG NG3 takes 20-60s to initialize after MSFS shows the
   *  cockpit; firing the SDK warning earlier than that is misleading. */
  simSnapshot: SimSnapshot | null;
}

export function PmdgPremiumPanel({ simState, simSnapshot }: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<PmdgStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const next = await invoke<PmdgStatus>("pmdg_status");
        if (!cancelled) setStatus(next);
      } catch {
        // IPC errors are transient on dev rebuilds; ignore.
      }
    }
    void poll();
    const id = window.setInterval(poll, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  if (!status) return null;

  // Decide the visual state.
  const variantLabel =
    status.variant === "ng3"
      ? "PMDG 737 NG3"
      : status.variant === "x777"
        ? "PMDG 777X"
        : null;

  // v0.3.0: SDK-Warnung erst wenn ALLE Vorbedingungen erfüllt sind:
  //   1. Sim ist tatsächlich Connected (nicht nur Connecting)
  //   2. Aircraft ist im MSFS fertig geladen (Sim liefert sinnvolle
  //      Snapshot-Daten — fuel + aircraft_title gesetzt). PMDG NG3
  //      braucht beim Cold-Start 20-60s bis das ClientData fließt;
  //      vorher würde die Warnung fälschlich fired werden.
  //   3. Backend's looks_like_sdk_disabled (variant + subscribed +
  //      !ever_received + stale > 5s)
  //   4. ZUSÄTZLICH eine UI-seitige Geduld von 20s nach Aircraft-Load
  //      damit ein langsam initialisierendes PMDG nicht False-Positiv
  //      gibt.
  const simConnected = simState === "connected";
  // "Aircraft fully loaded" Heuristik: MSFS-Snapshot hat ein Aircraft-
  // Title UND Fuel/Weight-Werte (= Aircraft-Modell ist initialisiert,
  // nicht nur Welt-Loading-Screen).
  const aircraftLoaded =
    !!simSnapshot &&
    !!simSnapshot.aircraft_title &&
    simSnapshot.fuel_total_kg > 0;
  const staleSecs = status.stale_secs ?? Number.MAX_SAFE_INTEGER;
  const showSdkWarning =
    status.looks_like_sdk_disabled &&
    simConnected &&
    aircraftLoaded &&
    staleSecs >= 20;
  const stateClass = showSdkWarning
    ? "pmdg-panel--warn"
    : status.ever_received
      ? "pmdg-panel--active"
      : "pmdg-panel--inactive";

  // "vor 0s" looks broken to the eye even though it's actually
  // healthy — PMDG fires PERIOD_ON_SET + FLAG_CHANGED on every
  // cockpit-value change, and a living simulation has many
  // changes per second (fuel needles, engine readouts, etc.).
  // So "0s old" = "data flowing constantly" = the desired state.
  // Display as "📡 live" to make that obvious. Pilot feedback
  // 2026-05-03: "da steht immer vor 0s".
  const ageLabel =
    status.stale_secs === null
      ? "—"
      : status.stale_secs <= 1
        ? "📡 live"
        : status.stale_secs < 60
          ? `vor ${status.stale_secs}s`
          : `vor ${Math.floor(status.stale_secs / 60)} min`;

  return (
    <section className={`pmdg-panel ${stateClass}`}>
      <header className="pmdg-panel__header">
        <span className="pmdg-panel__title">
          {t("pmdg_panel.title")}
        </span>
        {variantLabel && (
          <span className="pmdg-panel__variant">{variantLabel}</span>
        )}
      </header>

      {/* No PMDG aircraft loaded */}
      {!variantLabel && (
        <p className="pmdg-panel__hint">
          {t("pmdg_panel.inactive_hint")}
        </p>
      )}

      {/* PMDG loaded but SDK probably not enabled */}
      {variantLabel && showSdkWarning && (
        <div className="pmdg-panel__warning">
          <p className="pmdg-panel__warning-title">
            ⚠️ {t("pmdg_panel.sdk_disabled_title")}
          </p>
          <p>{t("pmdg_panel.sdk_disabled_explanation")}</p>
          <ol className="pmdg-panel__steps">
            <li>{t("pmdg_panel.step_close_msfs")}</li>
            <li>
              {t("pmdg_panel.step_open_options_ini")}{" "}
              <code className="pmdg-panel__code">
                {status.variant === "ng3"
                  ? "pmdg-aircraft-738\\work\\737NG3_Options.ini"
                  : "pmdg-aircraft-77er\\work\\777X_Options.ini"}
              </code>
            </li>
            <li>
              {t("pmdg_panel.step_add_lines")}
              <pre className="pmdg-panel__code-block">
{`[SDK]
EnableDataBroadcast=1`}
              </pre>
            </li>
            <li>{t("pmdg_panel.step_save_restart")}</li>
          </ol>
        </div>
      )}

      {/* Active — data flowing */}
      {variantLabel && status.ever_received && !showSdkWarning && (
        <div className="pmdg-panel__active">
          <div className="pmdg-panel__metrics">
            <div className="pmdg-panel__metric">
              <span className="pmdg-panel__metric-label">
                {t("pmdg_panel.last_packet")}
              </span>
              <span className="pmdg-panel__metric-value">{ageLabel}</span>
            </div>
            <div className="pmdg-panel__metric">
              <span className="pmdg-panel__metric-label">
                {t("pmdg_panel.subscription")}
              </span>
              <span className="pmdg-panel__metric-value">
                {status.subscribed ? "✅" : "—"}
              </span>
            </div>
          </div>
          <p className="pmdg-panel__hint">
            {t("pmdg_panel.active_hint")}
          </p>
        </div>
      )}

      {/* Der frühere "waiting"-Hint (subscribed but no data) wurde
          v0.3.0 in drei spezifischere Hints aufgesplittet: waiting_for_sim,
          waiting_for_aircraft, waiting_init — siehe unten. */}

      {/* v0.3.0: Sim noch nicht verbunden — keine SDK-Bewertung möglich.
          Statt der irreführenden "SDK nicht aktiviert"-Warnung zeigen
          wir hier einen klaren "warte auf Sim"-Hint. */}
      {variantLabel && !simConnected && (
        <p className="pmdg-panel__hint">
          {t("pmdg_panel.waiting_for_sim")}
        </p>
      )}

      {/* v0.3.0: Sim verbunden, aber Aircraft noch nicht voll geladen
          (MSFS Welt-/Aircraft-Loading-Screen). PMDG braucht 20-60s
          bis es ClientData liefert — solange zeigen wir keinen
          False-Positive sondern einen "Aircraft lädt"-Hint. */}
      {variantLabel && simConnected && !aircraftLoaded && (
        <p className="pmdg-panel__hint">
          {t("pmdg_panel.waiting_for_aircraft")}
        </p>
      )}

      {/* Aircraft geladen, subscribed, aber noch keine Daten —
          und Geduld noch nicht aufgebraucht (< 20s seit Subscribe).
          Eigene Zeile, sonst wäre's "kein Hint sichtbar" während der
          Init-Phase. */}
      {variantLabel
        && simConnected
        && aircraftLoaded
        && !status.ever_received
        && !showSdkWarning && (
          <p className="pmdg-panel__hint">
            {t("pmdg_panel.waiting_init", {
              secs: Math.max(0, 20 - staleSecs),
            })}
          </p>
        )}
    </section>
  );
}
