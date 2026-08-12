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
    // `logbook_stats` ist seit dem QS-Befund NICHT mehr zwischengespeichert
    // (das Logbuch soll nach der Landung sofort stimmen) — deshalb hier ein
    // echter Stammdaten-Befehl.
    vi.useFakeTimers();
    tauriInvoke.mockResolvedValue("alt");
    expect(await invoke("airport_get", { icao: "LPPT" })).toBe("alt");

    vi.advanceTimersByTime(25_000); // über die Haltbarkeit, unter der Grenze
    tauriInvoke.mockResolvedValue("neu");

    // Sofortige Antwort = noch der alte Wert, ohne Warten.
    expect(await invoke("airport_get", { icao: "LPPT" })).toBe("alt");
    expect(tauriInvoke).toHaveBeenCalledTimes(2); // die Auffrischung lief an

    await vi.runAllTimersAsync();
    expect(await invoke("airport_get", { icao: "LPPT" })).toBe("neu");
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
    await invoke("logbook_pirep", { id: "abc" });
    clearIpcCache();
    tauriInvoke.mockResolvedValue("zweiter Pilot");
    expect(await invoke("logbook_pirep", { id: "abc" })).toBe("zweiter Pilot");
  });

  it("hält die gefährlichen Befehle draußen (QS-Befund v1.5.7)", async () => {
    // Jeder dieser Einträge hat einen konkreten Schaden angerichtet, als
    // er zwischengespeichert wurde — sie dürfen nie zurückkommen:
    //   divert_nearest_airports → nimmt die Position NICHT als Argument,
    //     der Schlüssel war sitzungsweit gleich → Ausweichliste des
    //     VORIGEN Flugs landet als Zielflughafen im PIREP
    //   metar_get → der Aktualisieren-Knopf zeigte altes Wetter
    //   landing_list → gelöschte Landung kam zurück
    //   phpvms_get_bids → machte den v1.4.7-Fix rückgängig
    //   va_live_flights / logbook_* / news_fetch → bewusste Poll-Takte
    for (const cmd of [
      "divert_nearest_airports",
      "metar_get",
      "landing_list",
      "phpvms_get_bids",
      "va_live_flights",
      "logbook_stats",
      "logbook_pireps",
      "news_fetch",
    ]) {
      tauriInvoke.mockReset();
      tauriInvoke.mockResolvedValue("frisch");
      await invoke(cmd, { a: 1 });
      await invoke(cmd, { a: 1 });
      expect(tauriInvoke, `${cmd} MUSS jedes Mal frisch geholt werden`).toHaveBeenCalledTimes(2);
    }
  });

  it("liefert jenseits der harten Altersgrenze nichts Altes mehr", async () => {
    // QS-Befund: Ohne Obergrenze konnte ein Wert beliebig alt werden —
    // die Auffrischung schrieb nur in den Speicher, eine Ansicht, die beim
    // Öffnen einmal lädt, sah den alten Stand für immer.
    vi.useFakeTimers();
    tauriInvoke.mockResolvedValue("alt");
    expect(await invoke("airport_get", { icao: "EDDF" })).toBe("alt");

    vi.advanceTimersByTime(3 * 60_000); // weit über der Obergrenze
    tauriInvoke.mockResolvedValue("neu");

    // Kein Ausliefern des alten Werts mehr — es wird gewartet.
    expect(await invoke("airport_get", { icao: "EDDF" })).toBe("neu");
  });

  it("verwirft verspätete Antworten aus einer alten Sitzung (QS-Befund)", async () => {
    // Der Wettlauf: Eine Auffrischung startet VOR dem Abmelden und kommt
    // DANACH zurück. Ohne Generationszähler landete sie im frisch
    // geleerten Speicher — der nächste Pilot sah die Daten des vorigen.
    let aufloesen: (v: unknown) => void = () => {};
    const mockAufgerufen = new Promise<void>((bereit) => {
      tauriInvoke.mockImplementationOnce(
        () =>
          new Promise((res) => {
            aufloesen = res;
            bereit();
          }),
      );
    });

    const unterwegs = invoke("airport_get", { icao: "EDDF" });
    await mockAufgerufen; // sicherstellen, dass die Anfrage wirklich läuft
    clearIpcCache(); // Pilot meldet sich ab
    aufloesen("PILOT-A"); // alte Antwort trifft erst jetzt ein
    await unterwegs;

    tauriInvoke.mockResolvedValue("PILOT-B");
    expect(await invoke("airport_get", { icao: "EDDF" })).toBe("PILOT-B");
  });

  it("hält Live-Verfügbarkeit und fehlertolerante Befehle draußen", async () => {
    // QS-Befund: `fleet_list_at_airport` filtert auf PARKED/aktiv — das
    // ist Live-Verfügbarkeit, kein Stammdatum (ein belegtes Flugzeug
    // würde weiter angeboten). `airport_ground_*` wandeln Netzfehler in
    // leere Erfolge — die wären sonst zwischengespeichert worden.
    for (const cmd of [
      "fleet_list_at_airport",
      "airport_ground_get",
      "airport_ground_index",
    ]) {
      tauriInvoke.mockReset();
      tauriInvoke.mockResolvedValue("frisch");
      await invoke(cmd, { icao: "EDDF" });
      await invoke(cmd, { icao: "EDDF" });
      expect(tauriInvoke, `${cmd} MUSS jedes Mal frisch geholt werden`).toHaveBeenCalledTimes(2);
    }
  });

  it("leert sich beim Abmelden selbst — auch ohne Aufruf von außen", async () => {
    // QS-Runde 4: Der Schutz hing an einer einzelnen Zeile in der
    // Abmelde-Funktion, deren Löschen kein Test bemerkte. Jetzt hängt er
    // am Befehl — wer sich abmeldet, leert den Speicher zwangsläufig.
    tauriInvoke.mockResolvedValue("PILOT-A");
    await invoke("airport_get", { icao: "EDDF" });

    tauriInvoke.mockResolvedValue(null);
    await invoke("phpvms_logout");

    tauriInvoke.mockResolvedValue("PILOT-B");
    expect(await invoke("airport_get", { icao: "EDDF" })).toBe("PILOT-B");
  });

  it("leert auch, wenn das Abmelden fehlschlägt", async () => {
    tauriInvoke.mockResolvedValue("PILOT-A");
    await invoke("airport_get", { icao: "EDDF" });

    tauriInvoke.mockRejectedValueOnce(new Error("Keyring weg"));
    await expect(invoke("phpvms_logout")).rejects.toThrow();

    tauriInvoke.mockResolvedValue("PILOT-B");
    expect(await invoke("airport_get", { icao: "EDDF" })).toBe("PILOT-B");
  });
});
