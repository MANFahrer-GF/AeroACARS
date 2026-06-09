import { useEffect, useState } from "react";
import { invoke } from "../lib/ipc";
import { useTranslation } from "react-i18next";
import { useConfirm } from "./ConfirmDialog";

interface ActivityEntry {
  timestamp: string;
  level: "info" | "warn" | "error";
  message: string;
  detail?: string;
}

/** Format an ISO timestamp as HH:MM in the local timezone — same format as
 * the smartcars activity feed: short, scannable, never the date. */
function fmtTime(iso: string): string {
  const d = new Date(iso);
  const hh = d.getHours().toString().padStart(2, "0");
  const mm = d.getMinutes().toString().padStart(2, "0");
  return `${hh}:${mm}`;
}

export function ActivityLogPanel() {
  const { t } = useTranslation();
  const { confirm, dialog: confirmDialog } = useConfirm();
  const [entries, setEntries] = useState<ActivityEntry[]>([]);

  // Pull the log every 2 seconds. The buffer is capped server-side at
  // ACTIVITY_LOG_CAPACITY (1000) so the payload stays small.
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const list = await invoke<ActivityEntry[]>("activity_log_get");
        if (!cancelled) setEntries(list);
      } catch {
        // ignore — IPC errors are transient on dev rebuilds
      }
    }
    void poll();
    const id = setInterval(poll, 2000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  async function handleClear() {
    if (
      !(await confirm({
        message: t("activity_log.confirm_clear"),
        destructive: true,
      }))
    )
      return;
    try {
      await invoke("activity_log_clear");
      setEntries([]);
    } catch {
      // ignore
    }
  }

  return (
    <section className="activity-log">
      {confirmDialog}
      <header className="activity-log__header">
        <h2 className="activity-log__title">{t("activity_log.title")}</h2>
        <span className="activity-log__count">
          {t("activity_log.count", { count: entries.length })}
        </span>
        <button
          type="button"
          className="activity-log__clear"
          onClick={() => void handleClear()}
          disabled={entries.length === 0}
        >
          {t("activity_log.clear")}
        </button>
      </header>

      {entries.length === 0 ? (
        <p className="activity-log__empty">{t("activity_log.empty")}</p>
      ) : (
        <ol className="activity-log__list">
          {/* Newest at the top — same as smartcars. */}
          {[...entries].reverse().map((e, i) => (
            <li
              key={`${e.timestamp}-${i}`}
              className={`activity-log__entry activity-log__entry--${e.level}`}
            >
              <span className="activity-log__time">{fmtTime(e.timestamp)}</span>
              <div className="activity-log__body">
                <span className="activity-log__message">{e.message}</span>
                {e.detail && (
                  <span className="activity-log__detail">{e.detail}</span>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
