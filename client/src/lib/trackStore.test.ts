// Sichert die geflogene-Spur-Aufzeichnung ab — inkl. der v0.15.7-Persistenz,
// die einen App-Neustart mitten im Flug übersteht (vorher ging die Linie weg).

import { describe, it, expect, beforeEach, vi } from "vitest";

// Diese Node/jsdom-Umgebung stellt kein globales localStorage bereit. Im echten
// Tauri-Webview existiert es — hier ein einfacher In-Memory-Stub mit gleicher
// Semantik. Bewusst PRO Test frisch (Isolation), überlebt aber vi.resetModules
// INNERHALB eines Tests (für den „App-Neustart"-Fall unten).
beforeEach(() => {
  const mem = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => (mem.has(k) ? mem.get(k)! : null),
    setItem: (k: string, v: string) => {
      mem.set(k, String(v));
    },
    removeItem: (k: string) => {
      mem.delete(k);
    },
    clear: () => mem.clear(),
    key: (i: number) => [...mem.keys()][i] ?? null,
    get length() {
      return mem.size;
    },
  });
  vi.resetModules();
});

describe("trackStore", () => {
  it("nimmt Punkte auf und dünnt zu nah beieinander liegende aus", async () => {
    const { recordTrackPoint, getTrack } = await import("./trackStore");
    recordTrackPoint("P1", 10.0, 50.0);
    recordTrackPoint("P1", 10.0005, 50.0005); // < MIN_DELTA_DEG → verworfen
    recordTrackPoint("P1", 10.01, 50.01); // > MIN_DELTA_DEG → aufgenommen
    expect(getTrack("P1")).toEqual([
      [10.0, 50.0],
      [10.01, 50.01],
    ]);
  });

  it("übersteht einen App-Neustart via localStorage", async () => {
    const m1 = await import("./trackStore");
    m1.recordTrackPoint("P2", 8.0, 48.0);
    m1.recordTrackPoint("P2", 8.05, 48.05);
    expect(m1.getTrack("P2")).toHaveLength(2);

    // „Neustart": Modul-Registry zurücksetzen → frischer in-memory-Store,
    // localStorage bleibt erhalten. getTrack muss daraus hydratisieren.
    vi.resetModules();
    const m2 = await import("./trackStore");
    expect(m2.getTrack("P2")).toEqual([
      [8.0, 48.0],
      [8.05, 48.05],
    ]);
  });

  it("ignoriert ungültige Werte und liefert leer ohne PIREP", async () => {
    const { recordTrackPoint, getTrack } = await import("./trackStore");
    recordTrackPoint("P3", null, 5);
    recordTrackPoint("P3", NaN, NaN);
    expect(getTrack("P3")).toEqual([]);
    expect(getTrack(null)).toEqual([]);
    expect(getTrack("unbekannt")).toEqual([]);
  });
});
