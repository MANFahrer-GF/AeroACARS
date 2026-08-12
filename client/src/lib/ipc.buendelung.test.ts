// Bündelung der Abfragen über die LAN-Brücke.
//
// Eine Ansicht fragt beim Aufbau fünf bis zwölf Werte ab. Am PC ist das
// native Brücke; im LAN-Browser ist jede Abfrage ein eigener Rundlauf durchs
// WLAN — das war der Rest der Trägheit beim Tabwechsel, den Vorladen und
// Zwischenspeicher (v1.5.7) nicht erwischen: beim ersten Aufbau gibt es
// nichts vorzuladen und nichts im Speicher.
//
// Was hier festgehalten wird, ist nicht die Ersparnis an sich, sondern dass
// die Bündelung nichts kaputt macht:
//   - jeder Aufruf bekommt SEIN Ergebnis, in der richtigen Zuordnung,
//   - ein Fehler reißt die anderen nicht mit,
//   - kennt die Gegenstelle die Sammelroute nicht, läuft alles einzeln
//     weiter (ältere Brücke).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const fetchMock = vi.fn();
(globalThis as unknown as { fetch: unknown }).fetch = fetchMock;
localStorage.setItem("aeroacars.remote.token", "test-token");

const { invoke, _buendelungZuruecksetzen } = await import("./ipc");

function antwort(status: number, koerper: unknown) {
  return {
    status,
    ok: status === 200,
    json: async () => koerper,
    text: async () => JSON.stringify(koerper),
  };
}

beforeEach(() => {
  fetchMock.mockReset();
  _buendelungZuruecksetzen();
});

describe("LAN-Brücke — Abfragen bündeln", () => {
  // Zwei Zahlen, die zusammenpassen müssen: der Browser packt höchstens
  // BUENDEL_MAX Befehle in eine Anfrage, die Brücke nimmt höchstens
  // BATCH_MAX an. Läuft der Browser der Brücke davon, antwortet sie mit
  // "batch_too_large" — und zwar für das GANZE Bündel, also fällt eine
  // ganze Ansicht aus. Beim Ändern muss man beide Stellen anfassen; dieser
  // Test sorgt dafür, dass man es merkt.
  it("hält dieselbe Obergrenze wie die Brücke", () => {
    // Pfade relativ zum Projektverzeichnis: im Browser-Umfeld von Vitest
    // ist `import.meta.url` keine Datei-URL.
    const rust = readFileSync(resolve("src-tauri/src/remote/router.rs"), "utf8");
    const treffer = /const BATCH_MAX: usize = (\d+);/.exec(rust);
    expect(treffer, "BATCH_MAX steht nicht mehr im Router").toBeTruthy();
    const ts = readFileSync(resolve("src/lib/ipc.ts"), "utf8");
    const eigen = /const BUENDEL_MAX = (\d+);/.exec(ts);
    expect(eigen, "BUENDEL_MAX steht nicht mehr in ipc.ts").toBeTruthy();
    expect(Number(eigen![1])).toBeLessThanOrEqual(Number(treffer![1]));
  });

  it("fasst Aufrufe desselben Arbeitsschritts zu einer Anfrage zusammen", async () => {
    fetchMock.mockResolvedValue(antwort(200, [
      { status: 200, value: { name: "Berlin" } },
      { status: 200, value: [1, 2, 3] },
      { status: 200, value: "ok" },
    ]));

    const [a, b, c] = await Promise.all([
      invoke("airport_get", { icao: "EDDB" }),
      invoke("landing_list"),
      invoke("logbook_stats"),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][0]).toBe("/api/cmd-batch");
    // Jeder bekommt SEIN Ergebnis — Zuordnung über die Reihenfolge.
    expect(a).toEqual({ name: "Berlin" });
    expect(b).toEqual([1, 2, 3]);
    expect(c).toBe("ok");
  });

  it("lässt einen fehlgeschlagenen Befehl die anderen nicht mitreißen", async () => {
    fetchMock.mockResolvedValue(antwort(200, [
      { status: 200, value: "da" },
      { status: 422, error: { code: "no_flight", message: "Kein Flug aktiv" } },
      { status: 200, value: "auch da" },
    ]));

    const ergebnisse = await Promise.allSettled([
      invoke("airport_get"),
      invoke("flight_current"),
      invoke("logbook_stats"),
    ]);

    expect(ergebnisse[0]).toMatchObject({ status: "fulfilled", value: "da" });
    expect(ergebnisse[1].status).toBe("rejected");
    expect((ergebnisse[1] as PromiseRejectedResult).reason).toMatchObject({ code: "no_flight" });
    expect(ergebnisse[2]).toMatchObject({ status: "fulfilled", value: "auch da" });
  });

  it("schickt einen einzelnen Aufruf weiterhin einzeln", async () => {
    fetchMock.mockResolvedValue(antwort(200, "wert"));
    await invoke("logbook_stats");
    expect(fetchMock.mock.calls[0][0]).toBe("/api/cmd/logbook_stats");
  });

  it("fällt auf Einzelanfragen zurück, wenn die Brücke die Sammelroute nicht kennt", async () => {
    fetchMock.mockImplementation((url: string) =>
      Promise.resolve(url === "/api/cmd-batch" ? antwort(404, {}) : antwort(200, "einzeln")));

    const werte = await Promise.all([invoke("a"), invoke("b")]);
    expect(werte).toEqual(["einzeln", "einzeln"]);

    // Und ab jetzt gar nicht mehr über die Sammelroute.
    fetchMock.mockClear();
    await Promise.all([invoke("c"), invoke("d")]);
    expect(fetchMock.mock.calls.some((c) => c[0] === "/api/cmd-batch")).toBe(false);
  });

  it("hält die Obergrenze ein und schickt den Rest hinterher", async () => {
    fetchMock.mockImplementation((_url: string, init: RequestInit) => {
      const anzahl = (JSON.parse(String(init.body)) as unknown[]).length;
      return Promise.resolve(antwort(200, Array.from({ length: anzahl }, () => ({ status: 200, value: 1 }))));
    });

    await Promise.all(Array.from({ length: 30 }, (_, i) => invoke(`cmd_${i}`)));

    const groessen = fetchMock.mock.calls
      .filter((c) => c[0] === "/api/cmd-batch")
      .map((c) => (JSON.parse(String((c[1] as RequestInit).body)) as unknown[]).length);
    expect(Math.max(...groessen)).toBeLessThanOrEqual(24);
    expect(groessen.reduce((a, b) => a + b, 0)).toBe(30);
  });
});
