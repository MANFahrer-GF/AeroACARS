// v1.5.7 (#lan-traegheit) — Feldbefund Thomas: "auf der LAN-Brücke dauert die
// Umschaltung lange (träge)".
//
// Ursache waren zwei Dinge: der Programmcode einer Ansicht kam beim ersten
// Öffnen erst übers WLAN (dafür das Vorladen in App.tsx), und jede Ansicht
// fragt beim Öffnen 5–12 Werte EINZELN ab — über die Brücke also 5–12
// HTTP-Runden, bei jedem Wechsel neu.
//
// Diese Tests sichern den Zwischenspeicher ab. Der gefährlichste Fehler wäre
// nicht Langsamkeit, sondern ein GECACHTER SCHREIBBEFEHL — deshalb steht der
// entsprechende Test zuerst.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// `vi.hoisted` läuft VOR den Imports — nötig, weil ipc.ts beim Laden
// EINMALIG entscheidet, ob es im Tauri- oder im Browser-Zweig läuft.
// Der Tauri-Zweig ist der einfacher testbare; die Zwischenspeicher-Logik
// sitzt davor und ist für beide Wege identisch.
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

import { invoke, clearIpcCache } from "./ipc";

beforeEach(() => {
  tauriInvoke.mockReset();
  clearIpcCache();
  vi.useRealTimers();
});
afterEach(() => vi.useRealTimers());

describe("IPC-Zwischenspeicher", () => {
  it("speichert SCHREIBENDE Befehle niemals zwischen", async () => {
    // Der Kern-Sicherheitstest: ein zwischengespeicherter Flugstart oder
    // Verbindungsaufbau wäre ein echter Schaden, keine Optimierung.
    for (const cmd of [
      "flight_start",
      "flight_end",
      "hoppie_connect",
      "hoppie_send_cpdlc",
      "ui_state_set",
      "remote_server_stop",
    ]) {
      tauriInvoke.mockReset();
      tauriInvoke.mockResolvedValue("ok");
      await invoke(cmd, { a: 1 });
      await invoke(cmd, { a: 1 });
      expect(tauriInvoke, `${cmd} darf nie aus dem Speicher kommen`).toHaveBeenCalledTimes(2);
    }
  });

  it("beantwortet einen wiederholten Lesebefehl ohne neue Anfrage", async () => {
    tauriInvoke.mockResolvedValue({ icao: "EDDF" });
    const a = await invoke("airport_get", { icao: "EDDF" });
    const b = await invoke("airport_get", { icao: "EDDF" });
    expect(a).toEqual(b);
    expect(tauriInvoke).toHaveBeenCalledTimes(1);
  });

  it("unterscheidet verschiedene Argumente", async () => {
    tauriInvoke.mockResolvedValue({});
    await invoke("airport_get", { icao: "EDDF" });
    await invoke("airport_get", { icao: "LPPT" });
    expect(tauriInvoke).toHaveBeenCalledTimes(2);
  });

  it("liefert nach Ablauf sofort den alten Wert und frischt still nach", async () => {
    // Das ist der eigentliche Trick gegen die Trägheit: die Ansicht steht
    // augenblicklich da, die Zahlen aktualisieren sich einen Wimpernschlag
    // später — statt dass der Pilot auf das WLAN wartet.
    vi.useFakeTimers();
    tauriInvoke.mockResolvedValue("alt");
    expect(await invoke("logbook_stats")).toBe("alt");

    vi.advanceTimersByTime(25_000); // über die Haltbarkeit hinaus
    tauriInvoke.mockResolvedValue("neu");

    // Sofortige Antwort = noch der alte Wert, ohne Warten.
    expect(await invoke("logbook_stats")).toBe("alt");
    expect(tauriInvoke).toHaveBeenCalledTimes(2); // die Auffrischung lief an

    await vi.runAllTimersAsync();
    expect(await invoke("logbook_stats")).toBe("neu");
  });

  it("speichert Fehler nicht", async () => {
    // Ein einmaliger Netzfehler darf sich nicht 20 Sekunden festsetzen.
    tauriInvoke.mockRejectedValueOnce(new Error("weg"));
    await expect(invoke("news_fetch")).rejects.toThrow("weg");

    tauriInvoke.mockResolvedValue(["Meldung"]);
    expect(await invoke("news_fetch")).toEqual(["Meldung"]);
  });

  it("lässt sich leeren (Abmelden, VA-Wechsel)", async () => {
    tauriInvoke.mockResolvedValue("erster Pilot");
    await invoke("logbook_stats");
    clearIpcCache();
    tauriInvoke.mockResolvedValue("zweiter Pilot");
    expect(await invoke("logbook_stats")).toBe("zweiter Pilot");
  });
});
