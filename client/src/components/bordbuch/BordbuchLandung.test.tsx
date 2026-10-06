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
  markieren: null as Promise<unknown> | null,
  ereignis: null as (() => void) | null,
}));

function offen(pirep: string) {
  let resolve!: (e: unknown) => void;
  const promise = new Promise<unknown>((r) => (resolve = r));
  h.antworten.set(pirep, { resolve, promise });
}

vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "bordbuch_eintrag") return h.antworten.get(args?.pirepId as string)!.promise;
    if (cmd === "bordbuch_markieren") return h.markieren ?? Promise.resolve(null);
    return Promise.resolve(null);
  },
  listen: (_name: string, cb: () => void) => {
    h.ereignis = cb;
    return Promise.resolve(() => undefined);
  },
}));

import { BordbuchLandungsAbschnitt, useBordbuchEintrag } from "./BordbuchLandung";
import { DruckKontext } from "../../lib/druck";
import type { Eintrag } from "../../lib/bordbuch";

/** Der Abschnitt so, wie der PDF-Bericht ihn druckt (DruckKontext). */
function BordbuchBericht({ eintrag }: { eintrag: Eintrag }) {
  return (
    <DruckKontext.Provider value={true}>
      <BordbuchLandungsAbschnitt eintrag={eintrag} onMarkieren={async () => {}} />
    </DruckKontext.Provider>
  );
}

function Seite({ pirep }: { pirep: string }) {
  const b = useBordbuchEintrag(pirep);
  return <BordbuchLandungsAbschnitt eintrag={b.eintrag} onMarkieren={b.markieren} />;
}

afterEach(() => {
  cleanup();
  h.antworten.clear();
  h.markieren = null;
  h.ereignis = null;
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

  it("eine ATC-Markierung geht nicht verloren, wenn parallel geladen wird", async () => {
    offen("A");
    const { result } = renderHook(() => useBordbuchEintrag("A"));
    await act(async () => h.antworten.get("A")!.resolve(eintrag("A", 21.7)));
    // Markierung startet; bevor ihre Antwort da ist, beginnt ein Laden,
    // das noch den alten Stand liest.
    const markiert = eintrag("A", 21.7);
    markiert.punkte[0] = { ...markiert.punkte[0]!, status: "nach_atc" };
    let markierenFertig!: (e: unknown) => void;
    h.markieren = new Promise((r) => (markierenFertig = r));
    let lauf!: Promise<void>;
    act(() => {
      lauf = result.current.markieren("rolltempo_abflug", true);
    });
    offen("A");
    const altesLaden = h.antworten.get("A")!;
    act(() => {
      h.ereignis?.();
    });
    await act(async () => altesLaden.resolve(eintrag("A", 21.7)));
    // Das Nachladen nach der verworfenen Markierung liest den neuen Stand.
    offen("A");
    await act(async () => {
      markierenFertig(markiert);
      await Promise.resolve();
    });
    await act(async () => {
      h.antworten.get("A")!.resolve(markiert);
      await lauf;
    });
    expect(result.current.eintrag?.punkte[0]?.status).toBe("nach_atc");
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

  it("„nicht gesehen“ zeigt darunter, was gemessen wurde", () => {
    const e = eintrag("A", 21.7);
    e.punkte.push(
      { ...e.punkte[1]!, regel: "transponder_start", schalter: "transponder", abschnitt: "start", status: "diesmal_ohne", stellung: "STBY", beleg: {} },
      { ...e.punkte[1]!, regel: "strobes_start", schalter: "strobes", abschnitt: "start", status: "diesmal_ohne", stellung: null, beleg: {} },
    );
    e.eingeschaltet = ["rolltempo", "transponder", "strobes"];
    render(<BordbuchBericht eintrag={e} />);
    expect(screen.getAllByText("nicht gesehen").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("Transponder stand beim Start auf STBY")).toBeTruthy();
    expect(screen.getByText("Strobes waren beim Startlauf aus")).toBeTruthy();
    expect(screen.queryByText(/diesmal ohne/)).toBeNull();
  });

  it("der Druck sagt nicht „Tippe den Punkt an“", () => {
    render(<BordbuchBericht eintrag={eintrag("A", 21.7)} />);
    expect(screen.queryByText(/Tippe den Punkt an/)).toBeNull();
    expect(screen.getByText(/Grau heißt: nicht gesehen/)).toBeTruthy();
  });

  // QS 06.10.2026: Seit der Bericht die Bildschirm-Abschnitte druckt, stand
  // dort „Tippe den Punkt an" samt ATC-Knöpfen. Gegenprobe zum Test oben:
  // auf dem Bildschirm bleibt beides.
  it("auf Papier keine ATC-Knöpfe, auf dem Bildschirm schon", () => {
    const e = eintrag("A", 21.7);
    const { container, unmount } = render(<BordbuchBericht eintrag={e} />);
    expect(container.querySelector(".bb-bericht")).not.toBeNull();
    const tippbar = (c: HTMLElement) => c.querySelectorAll(".bb-checkliste button:not([disabled])").length;
    expect(tippbar(container)).toBe(0);
    unmount();
    const bildschirm = render(<BordbuchLandungsAbschnitt eintrag={e} onMarkieren={async () => {}} />);
    expect(bildschirm.container.querySelector(".bb-bericht")).toBeNull();
    expect(tippbar(bildschirm.container)).toBeGreaterThan(0);
    expect(screen.getByText(/Tippe den Punkt an/)).toBeTruthy();
  });
});

describe("„nicht gesehen“-Zeile für jede Regel", () => {
  it("jede Regel hat eine Erklärung (außer Rolltempo, das rechnet selbst)", async () => {
    const de = (await import("../../locales/de/common.json")).default as { bordbuch: { regel: Record<string, string>; gesehen: Record<string, string> } };
    for (const regel of Object.keys(de.bordbuch.regel)) {
      if (regel.startsWith("rolltempo")) continue;
      expect(de.bordbuch.gesehen[regel], regel).toBeTruthy();
    }
  });
});
