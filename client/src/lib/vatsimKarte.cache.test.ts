import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fetchVatsimData, _vatsimCacheLeeren } from "./vatsimKarte";

/**
 * v1.6.11 — Zwischenspeicher fuer den VATSIM-Datafeed.
 *
 * Hintergrund (Feldbefund 18.08.2026): der Datafeed ist **1,8 MB** gross, und
 * die Karte wird bei JEDEM Reiterwechsel neu aufgebaut, weil App.tsx die Reiter
 * als `{tab === "…" && <Komponente/>}` rendert. Ohne Zwischenspeicher zog jeder
 * Wechsel die 1,8 MB erneut — das war die vom Piloten gemeldete Traegheit.
 *
 * Die beiden Schwester-Funktionen im selben Modul (`loadBoundaries`,
 * `loadVatSpy`) hatten so einen Speicher von Anfang an. Ausgerechnet die
 * einzige noch benutzte hatte keinen.
 */

const ANTWORT = {
  pilots: [
    {
      callsign: "DLH400",
      latitude: 50.0,
      longitude: 8.5,
      altitude: 35000,
      groundspeed: 450,
      heading: 270,
      flight_plan: { departure: "EDDF", arrival: "KJFK", aircraft_short: "A359" },
    },
  ],
  controllers: [],
  atis: [],
};

function fetchMock() {
  return vi.fn(async () => ({
    ok: true,
    json: async () => structuredClone(ANTWORT),
  })) as unknown as typeof fetch;
}

describe("fetchVatsimData — Zwischenspeicher", () => {
  beforeEach(() => {
    _vatsimCacheLeeren();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    _vatsimCacheLeeren();
  });

  it("holt beim ersten Aufruf und liefert die Daten", async () => {
    const f = fetchMock();
    vi.stubGlobal("fetch", f);

    const daten = await fetchVatsimData();

    expect(f).toHaveBeenCalledTimes(1);
    expect(daten.pilots).toHaveLength(1);
    expect(daten.pilots[0].callsign).toBe("DLH400");
  });

  it("holt beim zweiten Aufruf NICHT erneut — das ist der ganze Punkt", async () => {
    const f = fetchMock();
    vi.stubGlobal("fetch", f);

    await fetchVatsimData();
    await fetchVatsimData();
    await fetchVatsimData();

    expect(f).toHaveBeenCalledTimes(1);
  });

  it("holt nach Ablauf der Frist wieder — der Speicher darf nicht einfrieren", async () => {
    const f = fetchMock();
    vi.stubGlobal("fetch", f);

    await fetchVatsimData();
    expect(f).toHaveBeenCalledTimes(1);

    // Gegenprobe zur Regel oben: knapp DAVOR noch kein neuer Abruf …
    vi.setSystemTime(Date.now() + 14_000);
    await fetchVatsimData();
    expect(f).toHaveBeenCalledTimes(1);

    // … knapp DANACH schon.
    vi.setSystemTime(Date.now() + 2_000);
    await fetchVatsimData();
    expect(f).toHaveBeenCalledTimes(2);
  });

  it("buendelt gleichzeitige Aufrufer auf EINEN Abruf", async () => {
    // Genau der Fall beim Reiterwechsel: der alte Effekt raeumt ab, der neue
    // startet — beide wollen die Daten im selben Augenblick.
    let aufloesen: (() => void) | null = null;
    const f = vi.fn(async () => {
      await new Promise<void>((r) => {
        aufloesen = r;
      });
      return { ok: true, json: async () => structuredClone(ANTWORT) };
    }) as unknown as typeof fetch;
    vi.stubGlobal("fetch", f);

    const a = fetchVatsimData();
    const b = fetchVatsimData();
    const c = fetchVatsimData();

    // Warten bis der Mock tatsaechlich im `await` haengt.
    await vi.waitFor(() => expect(aufloesen).not.toBeNull());
    aufloesen!();

    const [ra, rb, rc] = await Promise.all([a, b, c]);
    expect(f).toHaveBeenCalledTimes(1);
    expect(ra.pilots[0].callsign).toBe("DLH400");
    expect(rb).toBe(ra);
    expect(rc).toBe(ra);
  });

  it("gibt einem abbrechenden Aufrufer seinen Abbruch, ohne die anderen mitzureissen", async () => {
    // Wer den Reiter wechselt, bricht ab. Der geteilte Abruf muss fuer die
    // uebrigen Aufrufer weiterlaufen — sonst haette das Buendeln oben die
    // Nebenwirkung, dass ein Abbrecher allen die Daten wegnimmt.
    let aufloesen: (() => void) | null = null;
    const f = vi.fn(async () => {
      await new Promise<void>((r) => {
        aufloesen = r;
      });
      return { ok: true, json: async () => structuredClone(ANTWORT) };
    }) as unknown as typeof fetch;
    vi.stubGlobal("fetch", f);

    const steuerung = new AbortController();
    const abbrecher = fetchVatsimData(steuerung.signal);
    const bleibt = fetchVatsimData();

    await vi.waitFor(() => expect(aufloesen).not.toBeNull());
    steuerung.abort();

    await expect(abbrecher).rejects.toMatchObject({ name: "AbortError" });

    aufloesen!();
    await expect(bleibt).resolves.toMatchObject({
      pilots: [expect.objectContaining({ callsign: "DLH400" })],
    });
    expect(f).toHaveBeenCalledTimes(1);
  });
});
