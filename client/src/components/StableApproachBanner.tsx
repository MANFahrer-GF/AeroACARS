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
import { sollSinkrateFpm } from "../lib/anflugSollband";
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
 *  v0.15.19 — Gleitwinkel-aware (vorher fix): die V/S-Sink-Schwellen skalieren
 *  jetzt mit dem echten publizierten Gleitweg (`gs_factor = tan(g)/tan(3°)`),
 *  identisch zum Post-Flight-Scorer (`compute_approach_stability_v2`). Der
 *  Winkel kommt live aus den Navdaten via
 *  `ActiveFlightInfo.approach_glideslope_angle` (beim Flugstart gecacht). So
 *  blinkt der Banner auf einem korrekt geflogenen Steilanflug (EGLC 5,5°,
 *  ENTC 4°) nicht mehr fälschlich „instabil". Bei 3°/unbekanntem Winkel ist
 *  `gsFactor=1` → bit-identisch zu vorher. Bank/Config UND die −600-Hard-
 *  Landing-Schwelle (strukturell, am Boden) bleiben bewusst fix — die sind
 *  gleitwinkel-unabhängig.
 */
export function evaluateApproach(
  snap: SimSnapshot,
  phase: string,
  gsFactor = 1,
  glideslopeDeg?: number | null,
): Advisory | null {
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

  // Die Schwellen hängen an der GESCHWINDIGKEIT, nicht nur am Winkel.
  //
  // Bis 20.09.2026 standen hier feste fpm-Werte, nur mit `gsFactor`
  // skaliert. Das ist die halbe Geometrie: Auf einem Gleitpfad ist die
  // Sinkrate `GS × 5,31 × tan(Winkel)/tan(3°)` — sie wächst mit der
  // Geschwindigkeit. Bei 3° erzeugt schon jede Groundspeed über rund
  // 132 kt mehr als 700 fpm, also bekam JEDER Jet auf perfektem Pfad die
  // rote „PULL UP"-Meldung. Thomas, DLH 373 nach EDDM: konstant −720 fpm
  // bei 139 kt, `stable_at_gate=true`, Abweichung 46 fpm — und trotzdem
  // ab 100 ft durchgehend Alarm.
  //
  // Dieselbe Verwechslung hatte vier Tage zuvor Thorbens PC-12 getroffen
  // (siehe `anflugSollband.ts`). Die Lösung lag also schon im Projekt und
  // wurde hier nur nicht angewandt: dieselbe Funktion, eine Wahrheit.
  //
  // Gewarnt wird jetzt bei einem AUFSCHLAG auf das Soll statt bei einer
  // absoluten Zahl. Der Sockel (80 fpm) fängt Messrauschen ab, ohne die
  // Schwelle bei langsamen Mustern unbrauchbar zu machen: Mit 150 blieb
  // eine PC-12 (95 kt, Soll 504 fpm) bei −723 fpm stumm — 43 % über dem
  // Pfad und kurz vor dem Boden.
  const soll = sollSinkrateFpm(snap.groundspeed_kt, glideslopeDeg);
  // Unter 30 kt über Grund (`MIN_GS_KT` in `anflugSollband.ts`) gibt es
  // kein Soll: Rollen, Stillstand, Sim-Aussetzer. NUR dann gelten die
  // alten Festwerte.
  //
  // Hier stand zuerst „ein alter Client liefert `groundspeed_kt` nicht" —
  // den Fall gibt es nicht: Das Feld ist weder in `sim-core` noch in
  // `types.ts` optional, und beide Hälften werden als EINE Datei
  // ausgeliefert. Eine erfundene Begründung hält jemanden davon ab, den
  // Zweig zu hinterfragen (externe Abnahme, 21.09.2026).
  //
  // NACH UNTEN gedeckelt: Die geschwindigkeitsabhaengige Schwelle darf
  // nie STRENGER werden als die alte Festzahl. Sonst kehrt sich der
  // Fehler bei langsamen Mustern um — ein Hubschrauber oder Buschflieger
  // mit 40–60 kt ueber Grund bekaeme bei 500 fpm eine rote Meldung, die
  // es nie gab (Schwelle waere dort −345 bis −478 fpm; externe Abnahme,
  // 21.09.2026). `Math.min` nimmt bei negativen Zahlen die STEILERE,
  // also die nachsichtigere Grenze.
  const deckel = (wert: number, alt: number) => Math.min(wert, alt * gsFactor);
  const sink100 = soll != null ? deckel(soll * 1.25 - 80, -700) : -700 * gsFactor;
  const sink200 = soll != null ? deckel(soll * 1.35 - 80, -800) : -800 * gsFactor;
  const sink500 = soll != null ? deckel(soll * 1.5 - 80, -1000) : -1000 * gsFactor;
  const sink1000Lo =
    soll != null ? deckel(soll * 1.65 - 80, -1100) : -1100 * gsFactor;
  // Die OBERE Grenze (zu flach) — ebenfalls gedeckelt, nur andersherum.
  //
  // Hier warnt ein GRÖSSERER Wert seltener, also ist `Math.max` die
  // nachsichtige Richtung. Ohne den Deckel war diese Grenze oberhalb von
  // 141 kt strenger als die alte Festzahl (160 kt → −340 statt −300;
  // 250 kt → −531), und ein Muster mit −320 fpm zwischen 600 und 1100 ft
  // hätte eine Meldung bekommen, die es vorher nicht gab. Die
  // Release-Notes sagen ausdrücklich das Gegenteil zu — eine Zusage, die
  // der Code halten muss (externe Nachprüfung, 21.09.2026).
  const sink1000Hi =
    soll != null ? Math.max(soll * 0.4, -300 * gsFactor) : -300 * gsFactor;

  // Sub-100 ft mit excessive sink → höchste Priorität
  if (agl < 100 && agl > 5 && vs < sink100) {
    return {
      key: "sink_rate_pull_up",
      severity: "crit",
      reason: `V/S ${Math.round(vs)} fpm @ ${Math.round(agl)} ft AGL`,
    };
  }

  // 200 ft AAL — Go-Around-Schwelle
  if (agl < 250 && agl > 100) {
    if (bank > 5 || vs < sink200) {
      const reasons: string[] = [];
      if (bank > 5) reasons.push(`Bank ${bank.toFixed(0)}°`);
      if (vs < sink200) reasons.push(`V/S ${Math.round(vs)} fpm`);
      return {
        key: "da200_go_around",
        severity: "crit",
        reason: reasons.join(" · "),
      };
    }
  }

  // 500 ft AAL — Stable Approach Failed
  if (agl < 600 && agl >= 250) {
    if (bank > 5 || vs < sink500) {
      const reasons: string[] = [];
      if (bank > 5) reasons.push(`Bank ${bank.toFixed(0)}°`);
      if (vs < sink500) reasons.push(`V/S ${Math.round(vs)} fpm`);
      return {
        key: "gate500_unstable",
        severity: "alert",
        reason: reasons.join(" · "),
      };
    }
  }

  // 1000 ft AAL Stable-Approach-Gate
  if (agl < 1100 && agl >= 600) {
    const vsBad = vs < sink1000Lo || vs > sink1000Hi;
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

/** v0.15.19: gs_factor = tan(g)/tan(3°), plausibilitäts-geclampt auf 2–7,5°
 *  (identisch zum Backend `compute_approach_stability_v2`). Unbekannt/außerhalb
 *  → 1 (keine Skalierung = unverändertes 3°-Verhalten). */
export function glideslopeScaleFactor(angleDeg: number | null | undefined): number {
  return angleDeg != null && angleDeg >= 2 && angleDeg <= 7.5
    ? Math.tan((angleDeg * Math.PI) / 180) / Math.tan((3 * Math.PI) / 180)
    : 1;
}

export function StableApproachBanner({ activeFlight, simSnapshot, enabled }: Props) {
  const { t } = useTranslation();
  const [hardLandingHint, setHardLandingHint] = useState<{ vs: number; until: number } | null>(null);

  // v1.3.5 fix (Feldbefund 31.07.2026): this used to fire straight off
  // `simSnapshot.touchdown_vs_fpm` — the raw MSFS SimVar, latched the
  // instant the wheels touch. That value is known-unreliable (the whole
  // reason AeroACARS recomputes its own vertical speed from multiple
  // samples): a real flight showed -770 fpm here while the refined,
  // actually-scored value for the SAME touchdown was -455 fpm. The pilot
  // saw the wrong number in the banner and the right one, minutes later,
  // in the Logbook — a jarring mismatch, not two views of the same fact.
  //
  // Now waits for `activeFlight.landing_score_finalized` (mirrors the
  // backend's own `FlightStats.landing_score_finalized` — flips true once
  // the post-touchdown refinement, ~9-12s, is done) and shows the
  // canonical `landing_rate_fpm` — the SAME number the Logbook/PIREP use.
  // No timer guess: the banner only ever appears once the number is the
  // real, final one, however long that refinement actually took.
  useEffect(() => {
    if (!enabled) return;
    if (!activeFlight.landing_score_finalized) return;
    const vs = activeFlight.landing_rate_fpm;
    if (vs != null && vs < -600) {
      setHardLandingHint({ vs, until: Date.now() + 20000 });
    }
  }, [activeFlight.landing_score_finalized, activeFlight.landing_rate_fpm, enabled]);

  useEffect(() => {
    if (!hardLandingHint) return;
    const id = window.setTimeout(() => setHardLandingHint(null), Math.max(0, hardLandingHint.until - Date.now()));
    return () => window.clearTimeout(id);
  }, [hardLandingHint]);

  // v0.15.19: Gleitwinkel-Skalierungsfaktor (identisch zum Backend-Scorer),
  // aus dem live aufgelösten Approach-Gleitweg. Unbekannt/außerhalb 2–7,5° → 1.
  const gsFactor = useMemo(
    () => glideslopeScaleFactor(activeFlight.approach_glideslope_angle),
    [activeFlight.approach_glideslope_angle],
  );

  const advisory = useMemo<Advisory | null>(() => {
    if (!enabled || !simSnapshot) return null;
    return evaluateApproach(
      simSnapshot,
      activeFlight.phase,
      gsFactor,
      activeFlight.approach_glideslope_angle,
    );
  }, [
    enabled,
    simSnapshot,
    activeFlight.phase,
    gsFactor,
    activeFlight.approach_glideslope_angle,
  ]);

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
          {/* QS 2026-08-04: stand vorher fest verdrahtet auf Deutsch (also
              auch für englische/italienische Piloten) — UND der Vergleich
              war verdreht: das Banner erscheint nur bei vs < -600, gezeigt
              wurde aber "-770 fpm — > -600 fpm", was rechnerisch falsch
              ist. Jetzt übersetzt und richtig herum formuliert. */}
          <div className="approach-advisory__detail">
            {t("approach_advisory.hard_landing_detail", {
              vs: Math.round(hardLandingHint.vs),
              threshold: -600,
            })}
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
