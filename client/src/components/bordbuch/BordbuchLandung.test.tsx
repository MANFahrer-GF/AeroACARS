import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, renderHook, screen, cleanup, waitFor, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";
import type { Eintrag, Punkt } from "../../lib/bordbuch";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

const punkt = (regel: Punkt["regel"], abschnitt: Punkt["abschnitt"], status: Punkt["status"], extra: Partial<Punkt> = {}): Punkt => ({
  regel, schalter: "rolltempo", abschnitt, art: "pflicht", status, auto_status: status,
  zeit: "2026-09-26T20:02:00Z", hoehe_ft: null, stellung: null, grund: null, markiert_at: null, ...extra,
});

const eintrag = (pirep: string, ankunftKt: number): Eintrag => ({
  schema: 1, pirep_id: pirep, erstellt_at: "2026-09-26T17:00:00Z", updated_at: "2026-09-26T20:10:00Z",
  client_version: "1.9.2", flug: { callsign: "QAF 424", dep: "OTHH", arr: "EDDF", muster: "B77W", titel: null },
  klasse: "airliner", klasse_quelle: "profil", regelwerk: "ifr", nacht_start: false, nacht_landung: false,
  zeitquelle: "sim", eingeschaltet: ["rolltempo"], aus_grund: null,
  punkte: [
    punkt("rolltempo_abflug", "rollen", "diesmal_ohne", { stellung: "max 33 kt", beleg: { max_kt: 33.4, laengste_ueber_grenze_s: 12.1 } }),
    // Echter Fall QAF 424: 21,7 kt stand als „max 22 kt" da und wurde als Grenze gelesen.
    punkt("rolltempo_ankunft", "nach_der_landung", "erledigt", { stellung: `max ${Math.round(ankunftKt)} kt`, beleg: { max_kt: ankunftKt, laengste_ueber_grenze_s: 0 } }),
  ],
  rollen_max_abflug_kt: 33.4, rollen_max_ankunft_kt: ankunftKt, rolltempo_grenze_kt: 30, profil: [],
});

const h = vi.hoisted(() => ({
  antworten: new Map<string, { resolve: (e: unknown) => void; promise: Promise<unknown> }>(),
}));

function offen(pirep: string) {
  let resolve!: (e: unknown) => void;
  const promise = new Promise<unknown>((r) => (resolve = r));
  h.antworten.set(pirep, { resolve, promise });
}

vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "bordbuch_eintrag") return h.antworten.get(args?.pirepId as string)!.promise;
    return Promise.resolve(null);
  },
  listen: () => Promise.resolve(() => undefined),
}));

import { BordbuchBericht, BordbuchLandungsAbschnitt, useBordbuchEintrag } from "./BordbuchLandung";

function Seite({ pirep }: { pirep: string }) {
  const b = useBordbuchEintrag(pirep);
  return <BordbuchLandungsAbschnitt eintrag={b.eintrag} onMarkieren={b.markieren} />;
}

afterEach(() => {
  cleanup();
  h.antworten.clear();
});

describe("Bordbuch im Landungs-Tab", () => {
  it("zeigt gefahrene Spitze und Grenze nebeneinander", async () => {
    offen("A");
    render(<Seite pirep="A" />);
    await act(async () => h.antworten.get("A")!.resolve(eintrag("A", 21.7)));
    expect(await screen.findByText(/schnellstes Rollen 22 kt · Grenze 30 kt/)).toBeTruthy();
    expect(screen.getByText(/schnellstes Rollen 33 kt · 12 s über der Grenze 30 kt/)).toBeTruthy();
    expect(screen.queryByText("max 22 kt")).toBeNull();
  });

  it("eine späte Antwort der vorigen Landung überschreibt die aktuelle nicht", async () => {
    offen("A");
    offen("B");
    const { rerender } = render(<Seite pirep="A" />);
    rerender(<Seite pirep="B" />);
    await act(async () => h.antworten.get("B")!.resolve(eintrag("B", 17.1)));
    await act(async () => h.antworten.get("A")!.resolve(eintrag("A", 21.7)));
    await waitFor(() => expect(screen.getByText(/schnellstes Rollen 17 kt/)).toBeTruthy());
    expect(screen.queryByText(/schnellstes Rollen 22 kt/)).toBeNull();
  });

  it("ohne Eintrag (ältere Flüge) bleibt der Abschnitt weg", async () => {
    offen("A");
    const { container } = render(<Seite pirep="A" />);
    await act(async () => h.antworten.get("A")!.resolve(null));
    expect(container.querySelector(".bb-landung")).toBeNull();
  });

  it("nach ATC-Markierung bleiben Spitze und Grenze stehen", () => {
    const e = eintrag("A", 21.7);
    e.punkte[0] = { ...e.punkte[0]!, status: "nach_atc" };
    render(<BordbuchBericht eintrag={e} />);
    expect(screen.getByText(/nach ATC-Anweisung · schnellstes Rollen 33 kt · 12 s über der Grenze 30 kt/)).toBeTruthy();
  });

  it("der PDF-Export wartet auf das Bordbuch", async () => {
    offen("A");
    const { result } = renderHook(() => useBordbuchEintrag("A"));
    expect(result.current.bereit).toBe(false);
    await act(async () => h.antworten.get("A")!.resolve(eintrag("A", 21.7)));
    expect(result.current.bereit).toBe(true);
    expect(result.current.eintrag?.pirep_id).toBe("A");
  });

  it("der Druck sagt nicht „Tippe den Punkt an“", () => {
    render(<BordbuchBericht eintrag={eintrag("A", 21.7)} />);
    expect(screen.queryByText(/Tippe den Punkt an/)).toBeNull();
    expect(screen.getByText(/Grau heißt diesmal ohne/)).toBeTruthy();
  });
});
