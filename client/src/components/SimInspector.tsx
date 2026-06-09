import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";

/**
 * Live SimVar / LVar inspector — Settings → Debug.
 *
 * Lets the developer add an arbitrary SimConnect variable to a
 * watchlist and see its current value update once per second. Used
 * for two things:
 *
 *   1. Discovering which LVar a cockpit switch maps to. Add a
 *      candidate LVar, flip the switch, watch the value change.
 *   2. Validating coverage of an aircraft addon. Many aircraft
 *      ship their own LVars (FBW's `L:A32NX_*`, Fenix's `L:S_OH_*`).
 *      We can subscribe and verify they're being read correctly
 *      without recompiling.
 *
 * Architecture: every add/remove triggers a re-registration of
 * SimConnect data definition #3 in the Rust adapter. Values are
 * polled every second through `inspector_list`. The watchlist
 * survives reconnect — the adapter sets the dirty flag and
 * re-registers on the new connection.
 */

type WatchKind = "number" | "bool" | "string";

type WatchValue =
  | { type: "number"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "string"; value: string };

interface Watch {
  id: number;
  name: string;
  unit: string;
  kind: WatchKind;
  error: string | null;
  value: WatchValue | null;
}

const POLL_INTERVAL_MS = 1000;

export function SimInspector() {
  const { t } = useTranslation();
  const [watches, setWatches] = useState<Watch[]>([]);
  const [name, setName] = useState("");
  const [unit, setUnit] = useState("Number");
  const [kind, setKind] = useState<WatchKind>("number");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Periodic poll of inspector_list. We don't bother with a Tauri
  // event channel here — once-a-second polling matches the SimConnect
  // SECOND cadence anyway and keeps the wiring trivial.
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const list = await invoke<Watch[]>("inspector_list");
        if (!cancelled) setWatches(list);
      } catch {
        // ignore — adapter likely not running yet
      }
    };
    void tick();
    const id = setInterval(tick, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  async function handleAdd() {
    if (busy) return;
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await invoke<number>("inspector_add", {
        args: { name: trimmed, unit: unit.trim() || "Number", kind },
      });
      setName("");
      // The next poll picks up the new entry — no need to refresh
      // immediately, keeps the UI quiet.
    } catch (e) {
      const msg =
        typeof e === "object" && e !== null && "message" in e
          ? String((e as { message: string }).message)
          : String(e);
      setError(msg);
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove(id: number) {
    try {
      await invoke("inspector_remove", { id });
    } catch {
      // ignore — entry will get reaped on next poll if it's gone
    }
  }

  return (
    <>
      <h3 className="sim-panel__section">{t("inspector.title")}</h3>
      <p className="sim-panel__hint">{t("inspector.hint")}</p>

      <form
        className="inspector-add"
        onSubmit={(e) => {
          e.preventDefault();
          void handleAdd();
        }}
      >
        <input
          className="inspector-add__name"
          type="text"
          placeholder={t("inspector.placeholder_name")}
          value={name}
          onChange={(e) => setName(e.target.value)}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        <select
          className="inspector-add__kind"
          value={kind}
          onChange={(e) => setKind(e.target.value as WatchKind)}
        >
          <option value="number">{t("inspector.kind_number")}</option>
          <option value="bool">{t("inspector.kind_bool")}</option>
          <option value="string">{t("inspector.kind_string")}</option>
        </select>
        <input
          className="inspector-add__unit"
          type="text"
          placeholder={t("inspector.placeholder_unit")}
          value={unit}
          onChange={(e) => setUnit(e.target.value)}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        <button
          type="submit"
          className="button button--primary"
          disabled={busy || !name.trim()}
        >
          {t("inspector.add")}
        </button>
      </form>

      {error && (
        <p className="sim-panel__error" role="alert">
          {error}
        </p>
      )}

      {watches.length === 0 ? (
        <p className="sim-panel__hint">{t("inspector.empty")}</p>
      ) : (
        <table className="inspector-table">
          <thead>
            <tr>
              <th>{t("inspector.col_name")}</th>
              <th>{t("inspector.col_unit")}</th>
              <th>{t("inspector.col_value")}</th>
              <th aria-label={t("inspector.col_actions")} />
            </tr>
          </thead>
          <tbody>
            {watches.map((w) => (
              <tr key={w.id}>
                <td className="inspector-table__name">{w.name}</td>
                <td className="inspector-table__unit">{w.unit}</td>
                <td className="inspector-table__value">{formatValue(w)}</td>
                <td className="inspector-table__actions">
                  <button
                    type="button"
                    className="inspector-remove"
                    onClick={() => void handleRemove(w.id)}
                    aria-label={t("inspector.remove")}
                  >
                    ×
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

function formatValue(w: Watch): React.ReactNode {
  if (w.error) {
    return <span className="inspector-table__error">{w.error}</span>;
  }
  if (!w.value) {
    return <span className="sim-panel__muted">…</span>;
  }
  switch (w.value.type) {
    case "bool":
      return w.value.value ? "true" : "false";
    case "number":
      // Avoid the "0.000000000000000003" output by capping precision.
      return Number.isInteger(w.value.value)
        ? String(w.value.value)
        : w.value.value.toFixed(4);
    case "string":
      return w.value.value;
  }
}
