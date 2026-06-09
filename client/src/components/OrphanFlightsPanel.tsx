// v0.7.18 (B-011) — Orphan-Flight-Cleanup-UI
//
// Spec: docs/spec/v0.7.18-orphan-flight-cleanup.md §B-011
//
// Zeigt verwaiste PIREPs (state=IN_PROGRESS auf phpVMS, lokal ohne
// aktiven Flight) und erlaubt dem Pilot pro Eintrag:
//   - Cancel: cancel_pirep + delete_bid + lokale Queue-Aufraeumung
//   - Forget: nur lokale Aufraeumung, kein API-Call (fuer 404-Faelle)

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { OrphanFlight } from "../types";
import { useConfirm } from "./ConfirmDialog";

function formatAge(minutes: number | null): string {
  if (minutes == null) return "—";
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `${h} h ${m} min`;
}

function fmtIcao(s: string | null): string {
  return s && s.trim() !== "" ? s : "—";
}

export function OrphanFlightsPanel() {
  const { t } = useTranslation();
  const { confirm, dialog } = useConfirm();
  const [orphans, setOrphans] = useState<OrphanFlight[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<OrphanFlight[]>("flight_list_orphans");
      setOrphans(list);
    } catch (err: unknown) {
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      setError(msg);
      setOrphans(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleCancel(orphan: OrphanFlight) {
    const label =
      (orphan.flight_number?.trim() || orphan.pirep_id.slice(0, 8)) +
      ` (${fmtIcao(orphan.dpt_airport)} → ${fmtIcao(orphan.arr_airport)})`;
    const acLabel = orphan.aircraft_registration
      ? `${orphan.aircraft_icao ?? "?"} ${orphan.aircraft_registration}`
      : orphan.aircraft_icao ?? t("orphan.aircraft_unknown");

    const ok = await confirm({
      title: t("orphan.confirm_cancel_title", { label }),
      message: t("orphan.confirm_cancel_body", { label, aircraft: acLabel }),
      confirmLabel: t("orphan.confirm_cancel_yes"),
      cancelLabel: t("orphan.confirm_cancel_cancel"),
      destructive: true,
    });
    if (!ok) return;

    setBusyId(orphan.pirep_id);
    setError(null);
    try {
      await invoke("flight_cancel_orphan", {
        pirepId: orphan.pirep_id,
        bidId: orphan.bid_id,
        flightId: orphan.flight_id,
      });
      await refresh();
    } catch (err: unknown) {
      const code =
        typeof err === "object" && err !== null && "code" in err
          ? String((err as { code: string }).code)
          : null;
      const msg =
        typeof err === "object" && err !== null && "message" in err
          ? String((err as { message: string }).message)
          : String(err);
      // 404 → biete „lokal vergessen" an
      if (code === "not_found") {
        const forgetOk = await confirm({
          title: t("orphan.forget_404_title"),
          message: t("orphan.forget_404_body", { label }),
          confirmLabel: t("orphan.forget_yes"),
          cancelLabel: t("orphan.forget_cancel"),
        });
        if (forgetOk) {
          try {
            await invoke("flight_forget_remote", { pirepId: orphan.pirep_id });
            await refresh();
          } catch (e2: unknown) {
            const m2 =
              typeof e2 === "object" && e2 !== null && "message" in e2
                ? String((e2 as { message: string }).message)
                : String(e2);
            setError(m2);
          }
        }
      } else if (code === "blocked") {
        setError(t("orphan.error_blocked"));
      } else {
        setError(msg);
      }
    } finally {
      setBusyId(null);
    }
  }

  return (
    <section className="orphan-flights">
      {dialog}
      <header className="orphan-flights__header">
        <h3>🧹 {t("orphan.title")}</h3>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading || busyId !== null}
        >
          {loading ? t("orphan.refreshing") : t("orphan.refresh")}
        </button>
      </header>

      <p className="orphan-flights__intro">{t("orphan.intro")}</p>

      {error && (
        <div className="orphan-flights__error" role="alert">
          ⚠ {error}
        </div>
      )}

      {orphans == null && !loading && !error && (
        <div className="orphan-flights__empty">—</div>
      )}

      {orphans != null && orphans.length === 0 && !loading && (
        <div className="orphan-flights__empty orphan-flights__empty--ok">
          ✓ {t("orphan.none_found")}
        </div>
      )}

      {orphans != null && orphans.length > 0 && (
        <ul className="orphan-flights__list">
          {orphans.map((o) => {
            const label =
              o.flight_number?.trim() || o.pirep_id.slice(0, 8);
            return (
              <li key={o.pirep_id} className="orphan-flights__row">
                <div className="orphan-flights__row-main">
                  <div className="orphan-flights__row-title">
                    ⚠ <strong>{label}</strong>
                    {o.aircraft_icao && (
                      <span> — {o.aircraft_icao}</span>
                    )}
                    {(o.dpt_airport || o.arr_airport) && (
                      <span>
                        {" — "}
                        {fmtIcao(o.dpt_airport)} → {fmtIcao(o.arr_airport)}
                      </span>
                    )}
                  </div>
                  <dl className="orphan-flights__row-details">
                    <div>
                      <dt>{t("orphan.row_status")}</dt>
                      <dd>
                        <code>IN_PROGRESS</code>
                      </dd>
                    </div>
                    {o.aircraft_registration && (
                      <div>
                        <dt>{t("orphan.row_aircraft")}</dt>
                        <dd>{o.aircraft_registration}</dd>
                      </div>
                    )}
                    <div>
                      <dt>{t("orphan.row_age")}</dt>
                      <dd>{formatAge(o.age_minutes)}</dd>
                    </div>
                    {o.bid_id != null && (
                      <div>
                        <dt>{t("orphan.row_bid")}</dt>
                        <dd>#{o.bid_id}</dd>
                      </div>
                    )}
                    <div>
                      <dt>{t("orphan.row_pirep_id")}</dt>
                      <dd>
                        <code>{o.pirep_id}</code>
                      </dd>
                    </div>
                  </dl>
                </div>
                <div className="orphan-flights__row-actions">
                  <button
                    type="button"
                    className="orphan-flights__cancel-btn"
                    onClick={() => void handleCancel(o)}
                    disabled={busyId !== null}
                  >
                    {busyId === o.pirep_id
                      ? t("orphan.cancelling")
                      : t("orphan.cancel")}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
