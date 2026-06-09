import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo } from "../types";
import { formatRefreshError } from "../lib/refreshErrorFormatter";

interface Props {
  info: ActiveFlightInfo;
}

/**
 * Live-Loadsheet (v0.3.0 — überarbeitet, v0.5.46 — OFP-Refresh inline).
 *
 * **Sichtbarkeit:** nur in Phase = `preflight` oder `boarding`.
 * Sobald Pushback / TaxiOut beginnt → komplett weg, weil dann der
 * Cruise/Approach-Pfad relevant ist und das Loadsheet "fertig" ist.
 *
 * **Optik:** identisch zum InfoStrip darüber — gleiche `.info-strip`-
 * Klasse, gleiche `Group`/`Cell`-Struktur, gleiche Schriftgrößen
 * und Monospace-Werte. Pilot soll's als nahtlose Erweiterung der
 * MASSE/FLUG/TRIP-Zeilen sehen, nicht als eigene Box mit anderem
 * Stil.
 *
 * **Toggle:** kleiner Aufklapp-Pfeil rechts in der Group-Label-Zeile.
 * Default offen während Boarding.
 *
 * **Δ-Anzeige:** kompakt inline (z.B. `TOW 64.544 kg (+227)`) statt
 * einer eigenen Spalte. Farbcode wie sonst: <5% grün, 5-10% gelb,
 * >10% rot. Bei Overweight (IST > MAX): rote Δ + ⚠.
 *
 * **v0.5.46 — OFP-Refresh-Button inline:** wenn der IST/SOLL-Vergleich
 * ein "OFP-Outdated"-Muster zeigt (großer Block-Delta + ZFW-Match,
 * typisch wenn der Pilot in PasStudio neu geplant hat aber AeroACARS
 * noch den alten OFP hält), zeigen wir einen Refresh-Button direkt
 * im Loadsheet-Card. Adrian (GSG) hatte sich beschwert dass die
 * Loadsheet-Werte nach PasStudio-Update nicht aktuell sind — der
 * Mechanismus existiert schon (Header-Button), aber der kontextuelle
 * Hinweis direkt am Vergleich ist schlüssiger.
 */
export function LoadsheetMonitor({ info }: Props) {
  const { t } = useTranslation();
  // Hook-Reihenfolge: alle Hooks vor early returns, sonst stolpert
  // React beim Re-Render wenn die Phase wechselt und der Component
  // mal mit/ohne useState aufgerufen wird.
  const [expanded, setExpanded] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshDone, setRefreshDone] = useState(false);
  const [refreshErr, setRefreshErr] = useState<string | null>(null);

  // Sichtbar nur in den Boarding-Phasen — sobald TaxiOut beginnt,
  // ist das Loadsheet "abgeschlossen" und wir blenden's komplett aus.
  const visible =
    info.phase === "preflight" || info.phase === "boarding";
  if (!visible) return null;

  // Wenn weder Plan noch Live-Werte da sind, nichts rendern.
  const hasAnyPlan =
    info.planned_block_fuel_kg != null ||
    info.planned_zfw_kg != null ||
    info.planned_tow_kg != null;
  const hasAnyLive =
    info.sim_fuel_kg != null ||
    info.sim_zfw_kg != null ||
    info.sim_tow_kg != null;
  if (!hasAnyPlan && !hasAnyLive) return null;

  // Status-Hint
  const fuelDelta =
    info.sim_fuel_kg != null && info.planned_block_fuel_kg != null
      ? info.sim_fuel_kg - info.planned_block_fuel_kg
      : null;
  const zfwDelta =
    info.sim_zfw_kg != null && info.planned_zfw_kg != null
      ? info.sim_zfw_kg - info.planned_zfw_kg
      : null;

  // v0.5.46 — OFP-Outdated-Heuristik (Adrian-Fix):
  // Klassisches Muster wenn Pilot in PasStudio neu geplant hat:
  //   - Fuel-Delta groß (>= 400 kg ODER >= 5 % vom Plan)
  //   - ZFW-Delta klein (< 200 kg) — Pax/Cargo passen, nur Fuel weicht ab
  //   - Plan-Block-Wert ist da (sonst kein OFP zum Refreshen)
  // → Refresh-Hint zeigen + Button anbieten. Greift sowohl bei
  //   Über- als auch bei Unter-Tankung gegenüber dem alten Plan.
  const fuelDeltaPct =
    fuelDelta != null && info.planned_block_fuel_kg && info.planned_block_fuel_kg > 0
      ? Math.abs(fuelDelta / info.planned_block_fuel_kg) * 100
      : null;
  const fuelLooksOutdated =
    fuelDelta != null &&
    info.planned_block_fuel_kg != null &&
    (Math.abs(fuelDelta) >= 400 || (fuelDeltaPct != null && fuelDeltaPct >= 5));
  const zfwLooksMatching =
    zfwDelta == null || Math.abs(zfwDelta) < 200;
  const ofpLooksOutdated = fuelLooksOutdated && zfwLooksMatching;

  const hint = computeHint(fuelDelta, zfwDelta, t);
  // Wenn OFP-Outdated erkannt wurde, übersteuern wir den normalen
  // Hint mit dem expliziten Refresh-Hint — der ist actionable.
  const effectiveHint = ofpLooksOutdated
    ? t("cockpit.loadsheet.hint_ofp_outdated")
    : hint;

  async function handleRefreshOfp() {
    if (refreshing) return;
    setRefreshing(true);
    setRefreshErr(null);
    setRefreshDone(false);
    try {
      await invoke("flight_refresh_simbrief");
      setRefreshDone(true);
      // Auto-clear "✓ Done"-Hinweis nach 4 s, damit der UI-State
      // nicht für immer "frisch" suggeriert.
      setTimeout(() => setRefreshDone(false), 4000);
    } catch (err: unknown) {
      // v0.7.8 v1.5.2: shared Helper formattiert Mismatch-JSON +
      // bekannte Error-Codes in lesbare Notices.
      // v1.5.3: context="cockpit" damit phase_locked + no_simbrief_link
      // lesbar werden — Loadsheet ist Cockpit-Context.
      const formatted = formatRefreshError(
        err as { code?: string; message?: string } | null,
        t,
        "cockpit",
      );
      setRefreshErr(formatted?.text ?? String(err));
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <section className="info-strip">
      {/* Header-Zeile mit Toggle-Button rechts */}
      <div className="info-strip__group loadsheet__header-row">
        <h4 className="info-strip__group-label">
          {t("cockpit.loadsheet.label")}
        </h4>
        <button
          type="button"
          className="loadsheet__toggle"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
          title={
            expanded
              ? t("cockpit.loadsheet.collapse")
              : t("cockpit.loadsheet.expand")
          }
        >
          {expanded ? "▾" : "▸"}
        </button>
      </div>

      {/* Werte-Zeile — gleicher Stil wie MASSE-Strip oben */}
      {expanded && (
        <>
          <div className="info-strip__group">
            <h4 className="info-strip__group-label">
              {t("cockpit.loadsheet.ist_label")}
            </h4>
            <div className="info-strip__cells">
              <Cell
                label={t("cockpit.loadsheet.block")}
                ist={info.sim_fuel_kg}
                soll={info.planned_block_fuel_kg}
                max={null}
              />
              <Cell
                label="ZFW"
                ist={info.sim_zfw_kg}
                soll={info.planned_zfw_kg}
                max={info.planned_max_zfw_kg}
              />
              <Cell
                label="TOW"
                ist={info.sim_tow_kg}
                soll={info.planned_tow_kg}
                max={info.planned_max_tow_kg}
              />
            </div>
          </div>
          {(effectiveHint || ofpLooksOutdated) && (
            <div className="info-strip__group">
              <h4 className="info-strip__group-label">&nbsp;</h4>
              <div className="loadsheet__hint-inline">
                {effectiveHint}
                {/* v0.5.46 — Inline Refresh-Button bei OFP-Outdated.
                    Bewusst direkt neben dem Hint, damit die Aktion
                    am Ort des Problems sichtbar ist. */}
                {ofpLooksOutdated && (
                  <>
                    {" "}
                    <button
                      type="button"
                      className="loadsheet__refresh-btn"
                      onClick={handleRefreshOfp}
                      disabled={refreshing}
                      title={t("cockpit.loadsheet.refresh_btn_hint")}
                    >
                      {refreshing
                        ? t("cockpit.loadsheet.refresh_btn_busy")
                        : t("cockpit.loadsheet.refresh_btn")}
                    </button>
                    {refreshDone && (
                      <span className="loadsheet__refresh-done">
                        {" "}
                        {t("cockpit.loadsheet.refresh_btn_done")}
                      </span>
                    )}
                    {refreshErr && (
                      <span
                        className="loadsheet__refresh-err"
                        title={refreshErr}
                      >
                        {" "}⚠ {refreshErr}
                      </span>
                    )}
                  </>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}

/**
 * Eine Loadsheet-Cell im InfoStrip-Stil. Format: `LABEL 6.334 kg (+0)`
 * mit Δ inline und farbcodiert. Bei MAX-Wert + Overweight: ⚠-Indikator.
 */
function Cell({
  label,
  ist,
  soll,
  max,
}: {
  label: string;
  ist: number | null;
  soll: number | null;
  max: number | null;
}) {
  // Wenn weder IST noch SOLL da sind, Cell überspringen.
  if (ist == null && soll == null) return null;

  const delta = ist != null && soll != null ? ist - soll : null;
  const deltaPct =
    delta != null && soll != null && soll !== 0
      ? Math.abs(delta / soll) * 100
      : null;

  // Δ-Farbcode: <5% grün, 5-10% gelb, >10% rot. Wird auf den
  // Delta-Suffix angewendet, nicht auf den Hauptwert.
  let deltaClass = "loadsheet__delta--ok";
  if (deltaPct != null) {
    if (deltaPct >= 10) deltaClass = "loadsheet__delta--alert";
    else if (deltaPct >= 5) deltaClass = "loadsheet__delta--warn";
  }

  // Overweight: IST > MAX → ⚠ + alert-color
  const overweight = ist != null && max != null && ist > max;
  if (overweight) deltaClass = "loadsheet__delta--alert";

  const istLabel =
    ist != null ? `${Math.round(ist).toLocaleString("de-DE")} kg` : "—";

  return (
    <div className="info-strip__cell">
      <span className="info-strip__cell-label">{label}</span>
      <span className="info-strip__cell-value">{istLabel}</span>
      {delta != null && (
        <span className={`loadsheet__delta-inline ${deltaClass}`}>
          {overweight ? "⚠ " : ""}
          {delta >= 0 ? "+" : ""}
          {Math.round(delta).toLocaleString("de-DE")}
        </span>
      )}
    </div>
  );
}

function computeHint(
  fuelDelta: number | null,
  zfwDelta: number | null,
  t: (k: string, opts?: Record<string, unknown>) => string,
): string | null {
  if (fuelDelta == null && zfwDelta == null) return null;

  const fuelOk = fuelDelta == null || Math.abs(fuelDelta) < 200;
  const zfwOk = zfwDelta == null || Math.abs(zfwDelta) < 200;
  if (fuelOk && zfwOk) {
    return t("cockpit.loadsheet.hint_ready");
  }

  if (fuelDelta != null && fuelDelta < -300) {
    return t("cockpit.loadsheet.hint_fueling", {
      missing: Math.abs(Math.round(fuelDelta)).toLocaleString("de-DE"),
    });
  }
  if (zfwDelta != null && zfwDelta < -300) {
    return t("cockpit.loadsheet.hint_boarding", {
      missing: Math.abs(Math.round(zfwDelta)).toLocaleString("de-DE"),
    });
  }
  if (fuelDelta != null && fuelDelta > 500) {
    return t("cockpit.loadsheet.hint_overfueled", {
      extra: Math.round(fuelDelta).toLocaleString("de-DE"),
    });
  }

  return null;
}
