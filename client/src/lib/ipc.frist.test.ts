// Frist für Abfragen über die LAN-Brücke (Feldbefund 28.10.2026).
//
// Der Browser hält je Adresse nur ~6 Verbindungen. Eine Abfrage, die nie
// fertig wird, hält ihre Verbindung fest; reichen ein paar davon, wartet
// alles andere dahinter und das Tablet wirkt eingefroren. Nach der Frist
// muss die Abfrage abbrechen, ihre Verbindung freigeben und NICHT einzeln
// nachgeschickt werden (das stellte dieselbe Last wieder in die Schlange).

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

const fetchMock = vi.fn();
(globalThis as unknown as { fetch: unknown }).fetch = fetchMock;
localStorage.setItem("aeroacars.remote.token", "test-token");

const { invoke, _buendelungZuruecksetzen, BRUECKE_FRIST_MS } = await import("./ipc");

// Antwortet nie — bis das Signal abbricht, wie ein echter fetch.
function haengt(_url: string, init: { signal?: AbortSignal }) {
  return new Promise((_res, rej) => {
    init.signal?.addEventListener("abort", () => rej(new DOMException("aborted", "AbortError")));
  });
}

beforeEach(() => {
  fetchMock.mockReset();
  _buendelungZuruecksetzen();
  vi.useFakeTimers();
});
afterEach(() => vi.useRealTimers());

describe("LAN-Brücke — Frist", () => {
  it("bricht eine haengende Einzelabfrage nach der Frist ab", async () => {
    fetchMock.mockImplementation(haengt);
    const p = invoke("logbook_stats").catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(BRUECKE_FRIST_MS);
    expect(await p).toMatchObject({ code: "bridge_timeout" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("bricht ein haengendes Buendel ab und schickt es nicht einzeln nach", async () => {
    fetchMock.mockImplementation(haengt);
    const ps = [invoke("airport_get", { icao: "EDDB" }), invoke("logbook_stats")].map((p) =>
      p.catch((e: unknown) => e),
    );
    await vi.advanceTimersByTimeAsync(BRUECKE_FRIST_MS);
    for (const e of await Promise.all(ps)) expect(e).toMatchObject({ code: "bridge_timeout" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe("/api/cmd-batch");
  });

  it("laesst eine Abfrage innerhalb der Frist in Ruhe", async () => {
    fetchMock.mockImplementation(() =>
      new Promise((res) => setTimeout(() => res({ status: 200, ok: true, text: async () => "7" }), BRUECKE_FRIST_MS - 1000)),
    );
    const p = invoke("logbook_stats");
    await vi.advanceTimersByTimeAsync(BRUECKE_FRIST_MS);
    expect(await p).toBe(7);
  });
});
