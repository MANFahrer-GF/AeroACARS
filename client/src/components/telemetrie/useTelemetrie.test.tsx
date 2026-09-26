/**
 * v1.8.2: Der Hook schliesst Luecken im Strom, indem er den Verlauf des
 * Sim-PCs nachlaedt — geprueft ueber den echten Hook mit einer
 * Test-Datenquelle, nicht nur ueber die Hilfsfunktion.
 */
import { describe, expect, it } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useTelemetrie, type Datenquelle } from "./useTelemetrie";
import type { Frame, Katalog } from "./typen";

const katalog: Katalog = {
  zahlen: [{ id: "ias", gruppe: "flug", einheit: "kt", stellen: 0, quelle: "sim", art: "zahl" }],
  texte: [],
};
const f = (t: number): Frame => ({ t, z: [t / 100], s: [] });
const bereich = (von: number, bis: number) => {
  const r: Frame[] = [];
  for (let t = von; t <= bis; t += 100) r.push(f(t));
  return r;
};

describe("useTelemetrie — Luecken", () => {
  it("laedt nach einer Unterbrechung den Verlauf nach", async () => {
    let senden: ((fr: Frame) => void) | null = null;
    let starts = 0;
    const quelle: Datenquelle = {
      start: async () => {
        starts++;
        // Erster Start: Verlauf bis 900 ms. Nachladen: bis 3000 ms.
        return { katalog, verlauf: starts === 1 ? bereich(0, 900) : bereich(0, 3000) };
      },
      abonnieren: async (cb) => {
        senden = cb;
        return () => {
          senden = null;
        };
      },
      halten: () => undefined,
      stop: () => undefined,
    };
    const { result } = renderHook(() => useTelemetrie(quelle));
    await waitFor(() => expect(result.current.zustand).toBe("bereit"));
    expect(result.current.frames.at(-1)!.t).toBe(900);

    // Strom setzt nach 2 s Pause wieder ein → Luecke 1000..2900.
    act(() => senden!(f(3000)));
    await waitFor(() => expect(starts).toBe(2));
    await waitFor(() => expect(result.current.frames.length).toBe(31));
    const zeiten = result.current.frames.map((x) => x.t);
    expect(zeiten).toEqual(bereich(0, 3000).map((x) => x.t));
  });

  it("ohne Luecke wird nichts nachgeladen", async () => {
    let senden: ((fr: Frame) => void) | null = null;
    let starts = 0;
    const quelle: Datenquelle = {
      start: async () => {
        starts++;
        return { katalog, verlauf: bereich(0, 900) };
      },
      abonnieren: async (cb) => {
        senden = cb;
        return () => undefined;
      },
      halten: () => undefined,
      stop: () => undefined,
    };
    const { result } = renderHook(() => useTelemetrie(quelle));
    await waitFor(() => expect(result.current.zustand).toBe("bereit"));
    act(() => senden!(f(1000)));
    act(() => senden!(f(1100)));
    expect(starts).toBe(1);
  });

  it("Ereignisse aus der Luecke erscheinen nach dem Nachladen mit richtiger Zeit", async () => {
    const kat: Katalog = {
      zahlen: [{ id: "fahrwerk", gruppe: "fahrwerk", einheit: "%", stellen: 0, quelle: "sim", art: "zahl" }],
      texte: [],
    };
    // Fahrwerk faehrt bei 1500 ms aus — mitten in der Luecke.
    const g = (t: number): Frame => ({ t, z: [t >= 1500 ? 100 : 0], s: [] });
    const reihe = (von: number, bis: number) => {
      const r: Frame[] = [];
      for (let t = von; t <= bis; t += 100) r.push(g(t));
      return r;
    };
    let senden: ((fr: Frame) => void) | null = null;
    let starts = 0;
    const quelle: Datenquelle = {
      start: async () => {
        starts++;
        return { katalog: kat, verlauf: starts === 1 ? reihe(0, 900) : reihe(0, 3000) };
      },
      abonnieren: async (cb) => {
        senden = cb;
        return () => undefined;
      },
      halten: () => undefined,
      stop: () => undefined,
    };
    const { result } = renderHook(() => useTelemetrie(quelle));
    await waitFor(() => expect(result.current.zustand).toBe("bereit"));
    act(() => senden!(g(3100)));
    await waitFor(() => expect(result.current.frames.at(0)!.t).toBe(0));
    await waitFor(() =>
      expect(result.current.ereignisse.map((e) => [e.art, e.t])).toEqual([["fahrwerk_unten", 1500]]),
    );
  });
});

