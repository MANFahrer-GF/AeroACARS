import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo } from "../types";
import { formatRefreshError } from "../lib/refreshErrorFormatter";
import { Badge } from "./ui";

interface Props {
  info: ActiveFlightInfo;
}

/**
 * Live-Loadsheet (v0.3.0, v0.5.46 OFP-Refresh inline, Stage E redesign
 * — grid4 card with an Actual/Plan/Δ table).
 *
 * **Sichtbarkeit:** nur in Phase = `preflight` oder `boarding`.
 * Sobald Pushback / TaxiOut beginnt → komplett weg, weil dann der
 * Cruise/Approach-Pfad relevant ist und das Loadsheet "fertig" ist.
 *
 * **Optik (Stage E):** eigene Karte im selben `.grid4` wie Weights/Air
 * Data/Trip, mit einer echten Actual/Plan/Δ-Tabelle statt der früheren
 * kompakten Inline-Δ-Zelle — dieselben Werte, dieselbe Farbcodierung
 * (<5 % grün, 5-10 % gelb, >10 % rot; Overweight = rot + ⚠), nur
 * tabellarisch statt inline.
 *
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
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("cockpit.loadsheet.label")}</span>
        <Badge tone="neutral" style={{ marginLeft: 6 }}>
          {t(
            info.phase === "boarding"
              ? "cockpit.loadsheet.boarding"
              : "cockpit.loadsheet.preflight",
          )}
        </Badge>
      </div>

      <div className="card__body">
          <table className="ls" aria-label={t("cockpit.loadsheet.ist_label")}>
            <thead>
              <tr>
                <th></th>
                <th>{t("cockpit.loadsheet.ist")}</th>
                <th>{t("cockpit.loadsheet.soll")}</th>
                <th>Δ</th>
              </tr>
            </thead>
            <tbody>
              <LsRow
                label={t("cockpit.loadsheet.block")}
                ist={info.sim_fuel_kg}
                soll={info.planned_block_fuel_kg}
                max={null}
              />
              <LsRow
                label="ZFW"
                ist={info.sim_zfw_kg}
                soll={info.planned_zfw_kg}
                max={info.planned_max_zfw_kg}
              />
              <LsRow
                label="TOW"
                ist={info.sim_tow_kg}
                soll={info.planned_tow_kg}
                max={info.planned_max_tow_kg}
              />
            </tbody>
          </table>

          {/* Field feedback (2026-08-03): the combined "Max ZFW / TOW" label
              was too long for this narrow card at this font size — it wrapped
              mid-phrase across three ragged lines ("Max" / "ZFW /" / "TOW"),
              which read as broken/implausible even though the numbers
              themselves were correct. Split into two ordinary rows (short
              labels, same pattern as Block/ZFW/TOW above) — each fits on one
              line like every other row in this card. */}
          {info.planned_max_zfw_kg != null && (
            <div className="row" style={{ paddingTop: 9, marginTop: 5, borderTop: "1px solid var(--line)" }}>
              <span className="row__label">Max ZFW</span>
              <span className="row__value">{fmtKg(info.planned_max_zfw_kg)}</span>
            </div>
          )}
          {info.planned_max_tow_kg != null && (
            <div className="row" style={info.planned_max_zfw_kg == null ? { paddingTop: 9, marginTop: 5, borderTop: "1px solid var(--line)" } : undefined}>
              <span className="row__label">Max TOW</span>
              <span className="row__value">{fmtKg(info.planned_max_tow_kg)}</span>
            </div>
          )}

          {(effectiveHint || ofpLooksOutdated) && (
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
                    <span className="loadsheet__refresh-err" title={refreshErr}>
                      {" "}⚠ {refreshErr}
                    </span>
                  )}
                </>
              )}
            </div>
          )}
        </div>
    </div>
  );
}

/** One Actual/Plan/Δ row. Overweight (IST > MAX) gets the alert color
 *  on the Δ cell plus a ⚠ prefix, same as the pre-redesign inline Cell. */
function LsRow({
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
  // Wenn weder IST noch SOLL da sind, Zeile überspringen.
  if (ist == null && soll == null) return null;

  const delta = ist != null && soll != null ? ist - soll : null;
  const deltaPct =
    delta != null && soll != null && soll !== 0
      ? Math.abs(delta / soll) * 100
      : null;

  // Δ-Farbcode: <5% grün, 5-10% gelb, >10% rot.
  let deltaClass = "loadsheet__delta--ok";
  if (deltaPct != null) {
    if (deltaPct >= 10) deltaClass = "loadsheet__delta--alert";
    else if (deltaPct >= 5) deltaClass = "loadsheet__delta--warn";
  }

  // Overweight: IST > MAX → ⚠ + alert-color
  const overweight = ist != null && max != null && ist > max;
  if (overweight) deltaClass = "loadsheet__delta--alert";

  return (
    <tr>
      <td>{label}</td>
      <td>{fmtKg(ist)}</td>
      <td>{fmtKg(soll)}</td>
      <td className={deltaClass}>
        {delta == null
          ? "—"
          : `${overweight ? "⚠ " : ""}${delta >= 0 ? "+" : ""}${Math.round(delta).toLocaleString("de-DE")}`}
      </td>
    </tr>
  );
}

function fmtKg(kg: number | null): string {
  return kg != null ? `${Math.round(kg).toLocaleString("de-DE")} kg` : "—";
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
