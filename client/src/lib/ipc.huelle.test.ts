// v3 (Etappe 5, P-8.10): Systembefehle gehen aus der Oberfläche nur über die
// Hülle (`shell_request`). Die Capability `ui` weist sie sonst ab — ein
// direkter Aufruf wäre also nicht unsicher, sondern kaputt. Diese Tests
// halten die Umleitung an der einen Stelle fest, an der alle Aufrufe vorbeigehen.
import { describe, it, expect, vi, beforeEach } from "vitest";
import befehle from "../../src-tauri/befehle.json";

const tauriInvoke = vi.hoisted(() => {
  (globalThis as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  if (typeof window !== "undefined") {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  }
  return vi.fn();
});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
}));

import { invoke, aufrufZiel, SYSTEMBEFEHLE, clearIpcCache } from "./ipc";

beforeEach(() => {
  tauriInvoke.mockReset();
  clearIpcCache();
});

describe("Systembefehle über die Hülle", () => {
  it("kennt genau die Stufe system aus befehle.json", () => {
    const system = Object.entries(befehle.befehle).filter(([, s]) => s === "system").map(([b]) => b);
    expect([...SYSTEMBEFEHLE].sort()).toEqual(system.sort());
    expect(SYSTEMBEFEHLE.has("phpvms_login")).toBe(true);
    expect(SYSTEMBEFEHLE.has("shell_request")).toBe(false);
  });

  it("leitet jeden Systembefehl um, lesende und fachliche nicht", () => {
    for (const [befehl, stufe] of Object.entries(befehle.befehle)) {
      const [ziel, werte] = aufrufZiel(befehl, { a: 1 }, false);
      if (stufe === "system") expect([ziel, werte]).toEqual(["shell_request", { befehl, werte: { a: 1 } }]);
      else expect([ziel, werte]).toEqual([befehl, { a: 1 }]);
    }
  });

  it("im Hülle-Fenster geht alles direkt", () => {
    expect(aufrufZiel("phpvms_logout", undefined, true)).toEqual(["phpvms_logout", undefined]);
  });

  it("schickt den Aufruf als shell_request und gibt das Ergebnis des Befehls zurück", async () => {
    tauriInvoke.mockResolvedValueOnce({ deleted: 3 });
    const r = await invoke("flight_logs_delete_all");
    expect(tauriInvoke).toHaveBeenCalledWith("shell_request", { befehl: "flight_logs_delete_all", werte: {} });
    expect(r).toEqual({ deleted: 3 });
  });

  it("Abmelden über die Hülle leert weiterhin den Zwischenspeicher", async () => {
    tauriInvoke.mockResolvedValue({ icao: "EDDF" });
    await invoke("airport_get", { icao: "EDDF" });
    await invoke("airport_get", { icao: "EDDF" });
    expect(tauriInvoke).toHaveBeenCalledTimes(1); // zweiter Aufruf aus dem Speicher
    await invoke("phpvms_logout");
    expect(tauriInvoke).toHaveBeenLastCalledWith("shell_request", { befehl: "phpvms_logout", werte: {} });
    await invoke("airport_get", { icao: "EDDF" });
    expect(tauriInvoke).toHaveBeenCalledTimes(3); // nach dem Abmelden neu geholt
  });

  it("ein Fehler des Befehls (z. B. Flug läuft) kommt unverändert an", async () => {
    tauriInvoke.mockRejectedValueOnce({ code: "flight_active", message: "Flug läuft" });
    await expect(invoke("phpvms_logout")).rejects.toEqual({ code: "flight_active", message: "Flug läuft" });
  });
});
