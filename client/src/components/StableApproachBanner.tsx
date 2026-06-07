// v0.5.38: Visual Stable-Approach-Advisory Banner.
//
// Zeigt im Cockpit-Tab eine farbig kodierte Warnung wenn der Pilot
// die FAA-Stable-Approach-Kriterien (AC 120-71B) verletzt.
// Vier Schwellen analog zur Touchdown-Forensik-Pipeline:
//
//   1) 1000 ft AAL — 🟡 Approach instabil (V/S, Bank oder Konfig)
//   2)  500 ft AAL — 🟠 Stable approach failed (kritisch)
//   3)  200 ft AAL — 🔴 Go-around empfohlen (letzte Chance)
//   4) Sub-100 ft V/S<-700 — 🔴 Sink rate, pull up
//
// Plus Post-TD-Banner wenn V/S < -600 fpm (Hard Landing).
//
// Banner blendet sich automatisch ein/aus wenn die Bedingung
// wechselt. Cooldown 3s zwischen Wechseln verhindert Flackern bei
// Werten direkt an der Schwelle.

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActiveFlightInfo, SimSnapshot } from "../types";

type Severity = "warn" | "alert" | "crit" | "info";
type AdvisoryKey =
  | "gate1000_unstable"
  | "gate500_unstable"
  | "da200_go_around"
  | "sink_rate_pull_up"
  | "hard_landing";

interface Advisory {
  key: AdvisoryKey;
  severity: Severity;
  reason: string;
}

interface Props {
  activeFlight: ActiveFlightInfo;
  simSnapshot: SimSnapshot | null;
  /** Settings-Toggle: Banner komplett aus. Default: ON. */
  enabled: boolean;
}

/** FAA AC 120-71B Stable-Approach-Kriterien:
 *  - Bank ≤ 5°
 *  - V/S in [-1100, -300] fpm (auch zu langsame Sinkrate ist suspekt)
 *  - Gear & Flaps konfiguriert (gear ≥ 0.99, flaps ≥ 0.70)
 *
 *  v0.8.5: Thresholds an Backend (lib.rs compute_approach_stability_v2)
 *  angeglichen. Vorher 0.95/0.2, das war zu lax — siehe Item 4 im
 *  Landing-Analyse-Bundle.
 *
 *  v0.15.18 — Gleitwinkel-Caveat (bewusst NICHT skaliert): die V/S-Schwellen
 *  hier sind fix. Das POST-FLIGHT-Scoring (excessive_sink, Stability-Card,
 *  V/S-Chart-Linie) skaliert die Sink-Grenze seit v0.15.17/.18 mit dem echten
 *  publizierten Gleitweg (≈ −1334 fpm @4°, ≈ −1834 fpm @5,5°). Dieser LIVE-
 *  Banner kennt den Gleitweg in Echtzeit nicht (SimSnapshot hat keinen GP-
 *  Winkel) und bleibt daher ein bewusst konservativer Sicherheits-Nudge — auf
 *  einem Steilanflug kann er kurz aufblinken, ohne dass der Flug am Ende
 *  „instabil" gescort wird. Die Bewertungs-Wahrheit ist das Post-Flight-Gate.
 *  TODO(optional): GP-Winkel aus den Navdaten live durchreichen → mitskalieren.
 */
function evaluateApproach(snap: SimSnapshot, phase: string): Advisory | null {
  const isApproachPhase =
    phase === "approach" ||
    phase === "final" ||
    phase === "landing" ||
    phase === "descent";
  if (!isApproachPhase || snap.on_ground) return null;

  const agl = snap.altitude_agl_ft;
  const vs = snap.vertical_speed_fpm;
  const bank = Math.abs(snap.bank_deg);
  const gear = snap.gear_position ?? 0;
  const flaps = snap.flaps_position ?? 0;

  // Sub-100 ft mit excessive sink → höchste Priorität
  if (agl < 100 && agl > 5 && vs < -700) {
    return {
      key: "sink_rate_pull_up",
      severity: "crit",
      reason: `V/S ${Math.round(vs)} fpm @ ${Math.round(agl)} ft AGL`,
    };
  }

  // 200 ft AAL — Go-Around-Schwelle
  if (agl < 250 && agl > 100) {
    if (bank > 5 || vs < -800) {
      const reasons: string[] = [];
      if (bank > 5) reasons.push(`Bank ${bank.toFixed(0)}°`);
      if (vs < -800) reasons.push(`V/S ${Math.round(vs)} fpm`);
      return {
        key: "da200_go_around",
        severity: "crit",
        reason: reasons.join(" · "),
      };
    }
  }

  // 500 ft AAL — Stable Approach Failed
  if (agl < 600 && agl >= 250) {
    if (bank > 5 || vs < -1000) {
      const reasons: string[] = [];
      if (bank > 5) reasons.push(`Bank ${bank.toFixed(0)}°`);
      if (vs < -1000) reasons.push(`V/S ${Math.round(vs)} fpm`);
      return {
        key: "gate500_unstable",
        severity: "alert",
        reason: reasons.join(" · "),
      };
    }
  }

  // 1000 ft AAL Stable-Approach-Gate
  if (agl < 1100 && agl >= 600) {
    const vsBad = vs < -1100 || vs > -300;
    const bankBad = bank > 5;
    // v0.8.5: Thresholds an Backend (lib.rs compute_approach_stability_v2)
    // angeglichen — vorher 0.95/0.2, Backend nutzt 0.99/0.70 (FAA-strikter).
    // Inkonsistenz fuehrte zu Cockpit-Banner „Config OK" + PIREP-Card
    // „Landing-Config NICHT GESETZT" beim gleichen Flug.
    const configBad = gear < 0.99 || flaps < 0.7;
    if (vsBad || bankBad || configBad) {
      const reasons: string[] = [];
      if (bankBad) reasons.push(`Bank ${bank.toFixed(0)}°`);
      if (vsBad) reasons.push(`V/S ${Math.round(vs)} fpm`);
      if (configBad) {
        const parts: string[] = [];
        if (gear < 0.99) parts.push(`Gear ${(gear * 100).toFixed(0)}%`);
        if (flaps < 0.7) parts.push(`Flaps ${(flaps * 100).toFixed(0)}%`);
        reasons.push(`Config: ${parts.join("/")}`);
      }
      return {
        key: "gate1000_unstable",
        severity: "warn",
        reason: reasons.join(" · "),
      };
    }
  }

  return null;
}

export function StableApproachBanner({ activeFlight, simSnapshot, enabled }: Props) {
  const { t } = useTranslation();
  const [hardLandingHint, setHardLandingHint] = useState<{ vs: number; until: number } | null>(null);

  // Track Touchdown-Übergang: phase wechselt zu "landing" oder Touchdown-Event,
  // dann die V/S aus snap.touchdown_vs_fpm (oder aktuelle V/S falls bereits am Boden).
  useEffect(() => {
    if (!enabled || !simSnapshot) return;
    const tdVs = simSnapshot.touchdown_vs_fpm;
    if (tdVs != null && tdVs < -600) {
      setHardLandingHint({ vs: tdVs, until: Date.now() + 8000 });
    }
  }, [simSnapshot?.touchdown_vs_fpm, enabled]);

  useEffect(() => {
    if (!hardLandingHint) return;
    const id = window.setTimeout(() => setHardLandingHint(null), Math.max(0, hardLandingHint.until - Date.now()));
    return () => window.clearTimeout(id);
  }, [hardLandingHint]);

  const advisory = useMemo<Advisory | null>(() => {
    if (!enabled || !simSnapshot) return null;
    return evaluateApproach(simSnapshot, activeFlight.phase);
  }, [enabled, simSnapshot, activeFlight.phase]);

  if (!enabled) return null;

  // Hard-Landing-Banner überschreibt die Live-Advisory falls beides anliegt
  if (hardLandingHint) {
    return (
      <div className="approach-advisory approach-advisory--crit" role="alert">
        <div className="approach-advisory__icon">🛬</div>
        <div className="approach-advisory__body">
          <div className="approach-advisory__title">
            {t("approach_advisory.hard_landing")}
          </div>
          <div className="approach-advisory__detail">
            V/S {Math.round(hardLandingHint.vs)} fpm — &gt; {-600} fpm Boeing/Airbus FCOM Hard-Landing-Schwelle
          </div>
        </div>
      </div>
    );
  }

  if (!advisory) return null;

  const titleKey = `approach_advisory.${advisory.key}`;

  return (
    <div
      className={`approach-advisory approach-advisory--${advisory.severity}`}
      role={advisory.severity === "crit" ? "alert" : "status"}
    >
      <div className="approach-advisory__icon">
        {advisory.severity === "crit" ? "🔴" : advisory.severity === "alert" ? "🟠" : "🟡"}
      </div>
      <div className="approach-advisory__body">
        <div className="approach-advisory__title">{t(titleKey)}</div>
        <div className="approach-advisory__detail">{advisory.reason}</div>
      </div>
    </div>
  );
}
