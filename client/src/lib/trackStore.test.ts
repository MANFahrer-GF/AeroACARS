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

  it("nimmt am Boden feiner auf als in der Luft (Taxi-Auflösung)", async () => {
    const { recordTrackPoint, getTrack } = await import("./trackStore");
    // Eine kleine Bewegung (~0,0005° ≈ 55 m): in der LUFT verworfen
    // (< 0,002°), am BODEN aufgenommen (> 0,00015°).
    recordTrackPoint("AIR", 10.0, 50.0, false);
    recordTrackPoint("AIR", 10.0005, 50.0005, false); // Luft → verworfen
    expect(getTrack("AIR")).toEqual([[10.0, 50.0]]);

    recordTrackPoint("GND", 10.0, 50.0, true);
    recordTrackPoint("GND", 10.0005, 50.0005, true); // Boden → aufgenommen
    expect(getTrack("GND")).toEqual([
      [10.0, 50.0],
      [10.0005, 50.0005],
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

  // v0.15.x: setTrack spiegelt den vom Backend gepollten Track in den Store —
  // überschreibt (kein Anhängen), validiert endliche Koordinaten, kappt auf die
  // letzten MAX_POINTS und persistiert nach localStorage (Neustart-Recovery).
  it("setTrack überschreibt, filtert ungültige Punkte und ist via getTrack lesbar", async () => {
    const { setTrack, getTrack } = await import("./trackStore");
    setTrack("S1", [
      [10.0, 50.0],
      [Number.NaN, 51.0], // ungültig → gefiltert
      [11.0, Number.POSITIVE_INFINITY], // ungültig → gefiltert
      [12.0, 52.0],
    ]);
    expect(getTrack("S1")).toEqual([
      [10.0, 50.0],
      [12.0, 52.0],
    ]);
    // Erneuter Aufruf ÜBERSCHREIBT (kein Anhängen).
    setTrack("S1", [[20.0, 60.0]]);
    expect(getTrack("S1")).toEqual([[20.0, 60.0]]);
  });

  it("setTrack kappt auf die letzten MAX_POINTS (5000)", async () => {
    const { setTrack, getTrack } = await import("./trackStore");
    const big: [number, number][] = Array.from(
      { length: 5003 },
      (_, i) => [i * 0.01, 0] as [number, number],
    );
    setTrack("S2", big);
    const out = getTrack("S2");
    expect(out).toHaveLength(5000);
    // Die ÄLTESTEN 3 sind weg → erster Punkt ist der mit Index 3.
    expect(out[0]).toEqual([3 * 0.01, 0]);
    expect(out[out.length - 1]).toEqual([5002 * 0.01, 0]);
  });

  it("setTrack persistiert nach localStorage (übersteht App-Neustart)", async () => {
    const m1 = await import("./trackStore");
    m1.setTrack("S3", [
      [7.0, 47.0],
      [7.5, 47.5],
    ]);
    // „Neustart": Modul-Registry zurücksetzen → frischer in-memory-Store,
    // localStorage bleibt erhalten. getTrack muss daraus hydratisieren.
    vi.resetModules();
    const m2 = await import("./trackStore");
    expect(m2.getTrack("S3")).toEqual([
      [7.0, 47.0],
      [7.5, 47.5],
    ]);
  });
});
