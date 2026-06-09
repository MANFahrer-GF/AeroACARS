import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { SimSnapshot } from "../types";

/**
 * Diff snapshot — capture two states and show what changed.
 *
 * Workflow: pilot/dev clicks "Snapshot A", flips a cockpit switch,
 * clicks "Snapshot B", and the panel shows every field that moved
 * between them. Saves a ton of guess-work when figuring out which
 * SimVar/LVar belongs to which control — flip one thing, see one
 * row in the diff.
 *
 * The diff covers:
 *   * the live SimSnapshot (everything the standard telemetry
 *     pipeline reads)
 *   * the live inspector watchlist (anything the user added)
 *
 * Pure frontend feature — no backend wiring needed beyond the
 * existing inspector_list command from Phase B.
 */

type WatchValue =
  | { type: "number"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "string"; value: string };

interface InspectorWatch {
  id: number;
  name: string;
  unit: string;
  kind: string;
  error: string | null;
  value: WatchValue | null;
}

interface Capture {
  takenAt: Date;
  // Loose-typed bag of every value we want to diff. Fields can be
  // primitives or null; deep objects get flattened by the helpers.
  fields: Record<string, FieldValue>;
}

type FieldValue = string | number | boolean | null;

interface DiffRow {
  key: string;
  before: FieldValue;
  after: FieldValue;
}

interface Props {
  snapshot: SimSnapshot | null;
}

export function SimDiffSnapshot({ snapshot }: Props) {
  const { t, i18n } = useTranslation();
  const [a, setA] = useState<Capture | null>(null);
  const [b, setB] = useState<Capture | null>(null);

  async function takeCapture(): Promise<Capture | null> {
    if (!snapshot) return null;
    let watches: InspectorWatch[] = [];
    try {
      watches = await invoke<InspectorWatch[]>("inspector_list");
    } catch {
      // Inspector unavailable — diff still works for SimSnapshot.
    }
    return {
      takenAt: new Date(),
      fields: { ...flattenSnapshot(snapshot), ...flattenWatches(watches) },
    };
  }

  async function handleTakeA() {
    const c = await takeCapture();
    if (c) setA(c);
  }
  async function handleTakeB() {
    const c = await takeCapture();
    if (c) setB(c);
  }
  function handleReset() {
    setA(null);
    setB(null);
  }

  const diff = a && b ? computeDiff(a.fields, b.fields) : null;

  return (
    <>
      <h3 className="sim-panel__section">{t("diff.title")}</h3>
      <p className="sim-panel__hint">{t("diff.hint")}</p>

      <div className="diff-controls">
        <button
          type="button"
          className="button"
          onClick={() => void handleTakeA()}
          disabled={!snapshot}
        >
          {a ? t("diff.retake_a") : t("diff.take_a")}
        </button>
        <button
          type="button"
          className="button"
          onClick={() => void handleTakeB()}
          disabled={!snapshot || !a}
        >
          {b ? t("diff.retake_b") : t("diff.take_b")}
        </button>
        <button
          type="button"
          className="button button--ghost"
          onClick={handleReset}
          disabled={!a && !b}
        >
          {t("diff.reset")}
        </button>
      </div>

      {a && (
        <p className="sim-panel__hint">
          {t("diff.a_taken_at", { time: a.takenAt.toLocaleTimeString(i18n.language) })}
          {b && (
            <>
              {" · "}
              {t("diff.b_taken_at", {
                time: b.takenAt.toLocaleTimeString(i18n.language),
              })}
            </>
          )}
        </p>
      )}

      {diff && diff.length === 0 && (
        <p className="sim-panel__hint">{t("diff.no_changes")}</p>
      )}

      {diff && diff.length > 0 && (
        <table className="diff-table">
          <thead>
            <tr>
              <th>{t("diff.col_field")}</th>
              <th>{t("diff.col_before")}</th>
              <th>{t("diff.col_after")}</th>
            </tr>
          </thead>
          <tbody>
            {diff.map((row) => (
              <tr key={row.key}>
                <td className="diff-table__field">{row.key}</td>
                <td className="diff-table__before">{formatField(row.before)}</td>
                <td className="diff-table__after">{formatField(row.after)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

/**
 * Pull the diff-relevant fields out of a SimSnapshot. Two categories
 * are deliberately skipped:
 *
 *   * Position floats (lat/lon/altitude/speeds) — drift every tick.
 *   * Fuel / weight / ZFW — drift continuously while engines or APU
 *     are running (~0.04 kg/s for an A320 APU). The pilot sees those
 *     in the Mass & Fuel section already; including them here just
 *     drowns out the actual switch effect we're trying to find.
 *
 * Anything that actually represents a switch / discrete state (lights,
 * AP modes, gear, flaps, brakes, transponder, COM/NAV) stays in.
 */
function flattenSnapshot(s: SimSnapshot): Record<string, FieldValue> {
  return {
    "snap.on_ground": s.on_ground,
    "snap.parking_brake": s.parking_brake,
    "snap.stall_warning": s.stall_warning,
    "snap.overspeed_warning": s.overspeed_warning,
    "snap.engines_running": s.engines_running,
    "snap.gear_position": round(s.gear_position, 2),
    "snap.flaps_position": round(s.flaps_position, 2),
    "snap.transponder_code": s.transponder_code ?? null,
    "snap.com1_mhz": s.com1_mhz ?? null,
    "snap.com2_mhz": s.com2_mhz ?? null,
    "snap.nav1_mhz": s.nav1_mhz ?? null,
    "snap.nav2_mhz": s.nav2_mhz ?? null,
    "snap.light_landing": s.light_landing,
    "snap.light_beacon": s.light_beacon,
    "snap.light_strobe": s.light_strobe,
    "snap.light_taxi": s.light_taxi,
    "snap.light_nav": s.light_nav,
    "snap.light_logo": s.light_logo,
    "snap.autopilot_master": s.autopilot_master,
    "snap.autopilot_heading": s.autopilot_heading,
    "snap.autopilot_altitude": s.autopilot_altitude,
    "snap.autopilot_nav": s.autopilot_nav,
    "snap.autopilot_approach": s.autopilot_approach,
  };
}

function flattenWatches(watches: InspectorWatch[]): Record<string, FieldValue> {
  const out: Record<string, FieldValue> = {};
  for (const w of watches) {
    if (!w.value) continue;
    const key = `watch.${w.name}`;
    switch (w.value.type) {
      case "number":
        out[key] = round(w.value.value, 4);
        break;
      case "bool":
        out[key] = w.value.value;
        break;
      case "string":
        out[key] = w.value.value;
        break;
    }
  }
  return out;
}

function round(n: number, digits: number): number {
  const f = Math.pow(10, digits);
  return Math.round(n * f) / f;
}

function computeDiff(
  before: Record<string, FieldValue>,
  after: Record<string, FieldValue>,
): DiffRow[] {
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
  const rows: DiffRow[] = [];
  for (const key of keys) {
    const a = before[key] ?? null;
    const b = after[key] ?? null;
    if (a !== b) rows.push({ key, before: a, after: b });
  }
  rows.sort((x, y) => x.key.localeCompare(y.key));
  return rows;
}

function formatField(v: FieldValue): React.ReactNode {
  if (v === null || v === undefined)
    return <span className="sim-panel__muted">—</span>;
  if (typeof v === "boolean") return v ? "true" : "false";
  return String(v);
}
