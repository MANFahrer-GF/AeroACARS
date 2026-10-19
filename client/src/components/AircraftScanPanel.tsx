// Aircraft-Scan — "Flugzeug zur Analyse einreichen" (Settings → Plugins).
//
// Gegenstueck zum Web-Tool auf https://live.kant.ovh/aircraft/: der Client
// findet MSFS-Community-Ordner (UserCfg.opt) UND X-Plane-Aircraft-Ordner
// (x-plane_install_*.txt) selbst, listet die Flugzeug-Pakete, zeigt VOR dem
// Senden die exakte Dateiliste (DSGVO-Transparenz — nur cfg/json/xml/js/wasm
// bzw. acf/lua/xpl, nie Texturen/Modelle/Sounds) und schickt den gefilterten
// Auszug an live.kant.ovh. Die Einreichung erscheint dort unter "Meine
// Einreichungen".

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { formatIpcError, invoke } from "../lib/ipc";

interface FoundAircraft {
  index: number;
  folder: string;
  title: string;
  creator: string | null;
  source_dir: string;
}

interface CollectedFile {
  path: string;
  size: number;
}

interface CollectResult {
  files: CollectedFile[];
  total_bytes: number;
  skipped_large: string[];
}

interface SubmitResult {
  ok: boolean;
  id: string | null;
  status: string | null;
  zip_bytes: number;
  icao: string | null;
  lvar_count: number | null;
  external_process_suspected: boolean | null;
  warnings: string[];
}

type Step =
  | { kind: "idle" }
  | { kind: "listing" }
  | { kind: "list"; aircraft: FoundAircraft[] }
  | { kind: "collecting"; plane: FoundAircraft }
  | { kind: "confirm"; plane: FoundAircraft; collected: CollectResult }
  | { kind: "sending"; plane: FoundAircraft }
  | { kind: "done"; plane: FoundAircraft; result: SubmitResult };

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function AircraftScanPanel() {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const [manualDir, setManualDir] = useState("");
  const [error, setError] = useState<string | null>(null);

  const pickFolder = async () => {
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") {
        setManualDir(selected);
      }
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  // Zwei bewusst GETRENNTE, immer gleich beschriftete Aktionen statt eines
  // einzigen Buttons, der je nach manualDir sein Verhalten (und seinen Text)
  // aendert — das hat Piloten verwirrt ("kann ich nicht mehr umstellen auf
  // den kompletten Ordner", Thomas K. 05.07.2026). "Flugzeuge suchen"
  // ignoriert manualDir IMMER (volle Auto-Erkennung); "Diesen Ordner
  // scannen" nimmt IMMER den aktuell eingetragenen/gewaehlten Pfad. Beide
  // Buttons bleiben immer sichtbar und tun immer dasselbe — kein Umschalten
  // noetig, um zwischen beiden Modi zu wechseln.
  const runSearch = async (dir: string | null) => {
    setError(null);
    setStep({ kind: "listing" });
    try {
      const aircraft = await invoke<FoundAircraft[]>("ascan_list_aircraft", {
        manualDir: dir,
      });
      if (aircraft.length === 0) {
        setError(t("ascan.none_found"));
        setStep({ kind: "idle" });
        return;
      }
      setStep({ kind: "list", aircraft });
    } catch (e) {
      setError(formatIpcError(e));
      setStep({ kind: "idle" });
    }
  };

  const searchAuto = () => runSearch(null);
  const scanManualFolder = () => runSearch(manualDir.trim());

  const pick = async (plane: FoundAircraft) => {
    setError(null);
    setStep({ kind: "collecting", plane });
    try {
      const collected = await invoke<CollectResult>("ascan_collect", { index: plane.index });
      setStep({ kind: "confirm", plane, collected });
    } catch (e) {
      setError(formatIpcError(e));
      setStep({ kind: "idle" });
    }
  };

  const submit = async (plane: FoundAircraft) => {
    setError(null);
    setStep({ kind: "sending", plane });
    try {
      const result = await invoke<SubmitResult>("ascan_submit", { index: plane.index });
      setStep({ kind: "done", plane, result });
    } catch (e) {
      setError(formatIpcError(e));
      setStep({ kind: "idle" });
    }
  };

  return (
    <div className="settings__section" data-testid="aircraft-scan-panel">
      <h3>{t("ascan.title")}</h3>
      <p className="settings__row-hint">{t("ascan.hint")}</p>

      {error && (
        <p className="settings__row-hint" style={{ color: "var(--danger, #f87171)" }}>
          {error}
        </p>
      )}

      {(step.kind === "idle" || step.kind === "listing") && (
        <>
          <button type="button" onClick={searchAuto} disabled={step.kind === "listing"}>
            {step.kind === "listing" ? t("ascan.searching") : t("ascan.find_button")}
          </button>
          <label className="settings__field" style={{ marginTop: 10 }}>
            <span>{t("ascan.manual_dir_label")}</span>
            <div style={{ display: "flex", gap: 8 }}>
              <input
                type="text"
                value={manualDir}
                placeholder={t("ascan.manual_dir_placeholder")}
                onChange={(e) => setManualDir(e.target.value)}
                style={{ flex: 1 }}
              />
              <button type="button" onClick={pickFolder}>
                {t("ascan.pick_folder_button")}
              </button>
              <button
                type="button"
                onClick={scanManualFolder}
                disabled={step.kind === "listing" || !manualDir.trim()}
              >
                {t("ascan.scan_folder_button")}
              </button>
            </div>
          </label>
        </>
      )}

      {step.kind === "list" && (
        <>
          <p className="settings__row-hint">{t("ascan.pick_hint", { count: step.aircraft.length })}</p>
          <ul style={{ listStyle: "none", padding: 0, margin: 0, maxHeight: 260, overflow: "auto" }}>
            {step.aircraft.map((a) => (
              <li key={`${a.source_dir}/${a.folder}`} style={{ marginBottom: 6 }}>
                <button type="button" onClick={() => pick(a)} style={{ width: "100%", textAlign: "left" }}>
                  <strong>{a.title}</strong>
                  <span style={{ opacity: 0.65 }}> — {a.creator ?? "?"} · {a.folder}</span>
                </button>
              </li>
            ))}
          </ul>
          <button type="button" onClick={() => setStep({ kind: "idle" })} style={{ marginTop: 8 }}>
            {t("ascan.back")}
          </button>
        </>
      )}

      {step.kind === "collecting" && <p>{t("ascan.collecting", { title: step.plane.title })}</p>}

      {step.kind === "confirm" && (
        <>
          <p>
            <strong>{step.plane.title}</strong> — {step.collected.files.length}{" "}
            {t("ascan.files")} · {fmtBytes(step.collected.total_bytes)}
          </p>
          <p className="settings__row-hint">{t("ascan.transparency")}</p>
          {step.collected.skipped_large.length > 0 && (
            <p className="settings__row-hint">
              {t("ascan.skipped_large")}: {step.collected.skipped_large.join(", ")}
            </p>
          )}
          <div
            style={{
              maxHeight: 220,
              overflow: "auto",
              fontFamily: "ui-monospace, monospace",
              fontSize: "0.78rem",
              border: "1px solid var(--border, #333)",
              borderRadius: 6,
              padding: "6px 10px",
              marginBottom: 10,
            }}
          >
            {step.collected.files.map((f) => (
              <div key={f.path} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                <span style={{ wordBreak: "break-all" }}>{f.path}</span>
                <span style={{ opacity: 0.6, whiteSpace: "nowrap" }}>{fmtBytes(f.size)}</span>
              </div>
            ))}
          </div>
          <button type="button" onClick={() => submit(step.plane)}>
            {t("ascan.send_button")}
          </button>{" "}
          <button type="button" onClick={() => setStep({ kind: "idle" })}>
            {t("ascan.cancel")}
          </button>
        </>
      )}

      {step.kind === "sending" && <p>{t("ascan.sending", { title: step.plane.title })}</p>}

      {step.kind === "done" && (
        <>
          <p>
            ✓ {t("ascan.done", { title: step.plane.title })}{" "}
            {step.result.icao ? `(${step.result.icao})` : ""} —{" "}
            {t("ascan.done_stats", {
              lvars: step.result.lvar_count ?? 0,
              size: fmtBytes(step.result.zip_bytes),
            })}
          </p>
          {step.result.external_process_suspected && (
            <p className="settings__row-hint">{t("ascan.stub_warning")}</p>
          )}
          {step.result.warnings.map((w) => (
            <p className="settings__row-hint" key={w}>⚠ {w}</p>
          ))}
          <p className="settings__row-hint">
            {t("ascan.see_web")}{" "}
            <a href="https://live.kant.ovh/aircraft/" target="_blank" rel="noreferrer">
              live.kant.ovh/aircraft
            </a>
          </p>
          <button type="button" onClick={() => setStep({ kind: "idle" })}>
            {t("ascan.again")}
          </button>
        </>
      )}
    </div>
  );
}
