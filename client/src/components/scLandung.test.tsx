// Shared Cockpit (B4): Beim Partner zeigen Landeanzeige (Live-Vorschau) und
// Landungsansicht die Landung des Gastgebers von live, nicht die eigene
// Messung; die Abgabe-Meldung sagt „Partnerflug“. Ohne Crew (404), bei
// Fehlern oder auf einem Programm ohne `sc-landung-1` bleibt alles exakt wie
// bisher (DOM-Vergleich gegen den Lauf ohne Abfrage).
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { ActiveFlightInfo } from "../types";
import type { LandingRecord } from "../lib/landungsDatensatz";
import THY from "../lib/landungsFaelle.thy39.json";
import {
  SC_HOECHSTENS, SC_HOECHSTENS_NOTE, SC_NACHFRAGE_MS, SC_NACHFRAGE_NOTE_MS, scUebernehmen, scVergessen,
  type ScLandung,
} from "../lib/scLandung";

const lage = vi.hoisted(() => ({
  faehigkeiten: ["sc-landung-1"] as string[],
  sc: [] as Array<unknown>,
  aufrufe: [] as Array<unknown>,
}));

vi.mock("../lib/ipc", async (orig) => ({
  ...(await orig<typeof import("../lib/ipc")>()),
  invoke: (cmd: string, args?: unknown) => {
    if (cmd === "paket_info") return Promise.resolve({ faehigkeiten: lage.faehigkeiten });
    if (cmd === "sc_landung_gastgeber") {
      lage.aufrufe.push(args);
      const n = lage.sc.length > 1 ? lage.sc.shift() : lage.sc[0];
      return n instanceof Error ? Promise.reject(n) : Promise.resolve(n);
    }
    return Promise.reject(new Error(`kein ${cmd} im Test`));
  },
  listen: () => Promise.resolve(() => {}),
}));

const { LandingDetail } = await import("./LandingPanel");
const { CockpitView } = await import("./CockpitView");
const { StableApproachBanner } = await import("./StableApproachBanner");

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

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  lage.faehigkeiten = ["sc-landung-1"];
  lage.sc = [];
  lage.aufrufe = [];
  scVergessen();
});

const REC = THY as unknown as LandingRecord;
const GELANDET = {
  status: "gelandet", touchdown_id: 4711, fuehrender_pirep_id: "H1", gastgeber_name: "Anna B.",
  rolle_gastgeber: "PF", landing_rate_fpm: -200.03, landing_score: 83, g_force: 1.14, bounce_count: 0, score_text: "acceptable",
};
const AUSSTEHEND = { status: "ausstehend", fuehrender_pirep_id: "H1", gastgeber_name: "Anna B.", rolle_gastgeber: "PF" };
const KEINE_CREW = { status: "keine_crew" };
/** live beim Aufsetzen: Landung ohne Note, Sinkrate noch nicht angeglichen. */
const OHNE_NOTE = { ...GELANDET, landing_score: null, landing_rate_fpm: -310 };
/** Touch-and-Go: erster Aufsetzer, ältere touchdown_id. */
const TOUCH_AND_GO = { ...GELANDET, touchdown_id: 4710, landing_score: null, landing_rate_fpm: -150 };

const ruhe = () => act(async () => { await new Promise((r) => setTimeout(r, 50)); });

async function detailHtml(isPreview = false): Promise<string> {
  const { container, unmount } = render(
    <LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview={isPreview} />,
  );
  await ruhe();
  // React-`useId`-Kennungen zählen je Lauf weiter; sonst muss alles gleich sein.
  const html = container.innerHTML.replace(/_r_[0-9a-z]+_/g, "_r_");
  unmount();
  return html;
}

describe("Landungsansicht beim Partner", () => {
  it("zeigt die Landung des Gastgebers statt der eigenen Bewertung", async () => {
    lage.sc = [GELANDET];
    const { container } = render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview={false} />);
    await ruhe();
    const karte = screen.getByTestId("sc-landung");
    expect(karte.textContent).toContain("Landung von Anna B. (Shared Cockpit)");
    expect(karte.textContent).toContain("-200 fpm");
    expect(karte.textContent).toContain("83 · gut");
    expect(karte.textContent).toContain("1.14 G");
    // Die eigene Messung (THY 39: -46 fpm) steht nirgends, auch nicht im Druck-Knopf.
    expect(container.querySelectorAll(".landing-section")).toHaveLength(1);
    expect(container.textContent).not.toContain("-46");
    expect(screen.queryByText(/PDF/)).toBeNull();
    expect(lage.aufrufe).toEqual([{ pirepId: REC.pirep_id }]);
  });

  it("Rate des Gastgebers null oder fehlend: „--“, nie 0 und nie die eigene Messung", async () => {
    const { landing_rate_fpm: _weg, ...ohneFeld } = GELANDET;
    void _weg;
    for (const antwort of [{ ...GELANDET, landing_rate_fpm: null }, ohneFeld]) {
      lage.sc = [antwort];
      const { container, unmount } = render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview={false} />);
      await ruhe();
      const karte = screen.getByTestId("sc-landung").textContent ?? "";
      expect(karte).toContain("Landung von Anna B. (Shared Cockpit)");
      expect(karte).toContain("--");
      expect(karte).toContain("83 · gut");
      expect(karte).not.toMatch(/\b0 fpm/);
      expect(container.textContent).not.toContain("-46");
      unmount();
      scVergessen();
    }
  });

  it("beim erneuten Öffnen steht die Gastgeber-Landung im ersten Bild, die eigene blitzt nicht auf", async () => {
    lage.sc = [GELANDET];
    const erst = render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview={false} />);
    await ruhe();
    erst.unmount();
    lage.sc = [new Error("langsam")];
    const { container } = render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview={false} />);
    // ohne Warten: erstes Bild
    expect(container.querySelectorAll(".landing-section")).toHaveLength(1);
    expect(screen.getByTestId("sc-landung").textContent).toContain("-200 fpm");
  });

  it("in der Live-Vorschau nach dem Aufsetzen: Hinweis, bis der Gastgeber gelandet ist", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [AUSSTEHEND, AUSSTEHEND, GELANDET];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    expect(screen.getByTestId("sc-landung").textContent).toContain("Landung des Gastgebers folgt.");
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS); });
    expect(lage.aufrufe).toHaveLength(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS); });
    expect(screen.getByTestId("sc-landung").textContent).toContain("-200 fpm");
    // gelandet beendet das Nachfragen
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS * 5); });
    expect(lage.aufrufe).toHaveLength(3);
  });

  it("gelandet ohne Note: fragt langsamer weiter, bis Note und angeglichene Sinkrate da sind", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [OHNE_NOTE, OHNE_NOTE, GELANDET];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    expect(screen.getByTestId("sc-landung").textContent).toContain("-310 fpm");
    expect(screen.getByTestId("sc-landung").textContent).not.toContain("83");
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS); });
    expect(lage.aufrufe).toHaveLength(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_NOTE_MS); });
    expect(lage.aufrufe).toHaveLength(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_NOTE_MS); });
    expect(lage.aufrufe).toHaveLength(3);
    const karte = screen.getByTestId("sc-landung").textContent;
    expect(karte).toContain("-200 fpm");
    expect(karte).toContain("83 · gut");
    // mit Note Schluss
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_NOTE_MS * 5); });
    expect(lage.aufrufe).toHaveLength(3);
  });

  it("Touch-and-Go: die neue touchdown_id ersetzt den ersten Aufsetzer", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [TOUCH_AND_GO, GELANDET];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    expect(screen.getByTestId("sc-landung").textContent).toContain("-150 fpm");
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_NOTE_MS); });
    expect(screen.getByTestId("sc-landung").textContent).toContain("-200 fpm");
  });

  it("ohne Note höchstens SC_HOECHSTENS_NOTE weitere Abfragen", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [OHNE_NOTE];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_NOTE_MS * (SC_HOECHSTENS_NOTE + 10)); });
    expect(lage.aufrufe).toHaveLength(1 + SC_HOECHSTENS_NOTE);
  });

  it("fragt bei „ausstehend“ höchstens SC_HOECHSTENS-mal", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [AUSSTEHEND];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS * (SC_HOECHSTENS + 10)); });
    expect(lage.aufrufe).toHaveLength(SC_HOECHSTENS);
  });

  it("ohne Crew (404), bei Fehlern und ohne Fähigkeit exakt wie bisher", async () => {
    lage.faehigkeiten = [];
    const vorher = await detailHtml();
    expect(lage.aufrufe).toHaveLength(0);
    expect(vorher).toContain("landing-export");
    lage.faehigkeiten = ["sc-landung-1"];
    lage.sc = [KEINE_CREW];
    expect(await detailHtml()).toBe(vorher);
    expect(lage.aufrufe).toHaveLength(1);
    lage.aufrufe = [];
    lage.sc = [new Error("sc_landung_fehler")];
    expect(await detailHtml()).toBe(vorher);
    expect(lage.aufrufe).toHaveLength(1);
  });

  it("Fehler: wenige Versuche, dann still", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    lage.sc = [new Error("sc_landung_fehler")];
    render(<LandingDetail record={REC} allRecords={[REC]} onBack={() => {}} isPreview />);
    await ruhe();
    await act(async () => { await vi.advanceTimersByTimeAsync(SC_NACHFRAGE_MS * 10); });
    expect(lage.aufrufe).toHaveLength(3);
    expect(screen.queryByTestId("sc-landung")).toBeNull();
  });
});

function flug(o: Partial<ActiveFlightInfo>): ActiveFlightInfo {
  return {
    pirep_id: "P1", phase: "taxi_in", was_just_resumed: true, takeoff_at: "x", landing_at: "y",
    approach_glideslope_angle: null, landing_rate_fpm: null, landing_score_finalized: false, ...o,
  } as ActiveFlightInfo;
}

function cockpit(activeFlight: ActiveFlightInfo | null, abschluss: unknown = null) {
  return (
    <CockpitView
      session={{} as never}
      activeFlight={activeFlight}
      setActiveFlight={() => {}}
      simSnapshot={null}
      vorFlug={null}
      briefingImFlug={null}
      approachAdvisoriesEnabled
      abschlussVonAussen={abschluss as never}
    />
  );
}

const FILED = { kind: "filed", callsign: "GSG 1", dpt: "EDDF", arr: "EDDM" };

describe("Abgabe-Hinweis beim Partner", () => {
  it("sagt „Partnerflug“ statt „eingereicht“, ein Soloflug danach wieder das Übliche", async () => {
    lage.sc = [AUSSTEHEND];
    const { rerender } = render(cockpit(flug({ pirep_id: "P1" })));
    await ruhe();
    rerender(cockpit(null, FILED));
    await ruhe();
    expect(screen.getByRole("status").textContent).toContain(
      "Dein Flug wird als Partnerflug gutgeschrieben, sobald der Flug des Gastgebers angenommen ist.",
    );
    expect(screen.queryByText(deCommon.cockpit.pirep_filed_title)).toBeNull();

    // Gegenprobe: nächster Flug ohne Crew
    lage.sc = [KEINE_CREW];
    rerender(cockpit(flug({ pirep_id: "P2" })));
    await ruhe();
    rerender(cockpit(null, { ...FILED }));
    await ruhe();
    expect(screen.getByText(deCommon.cockpit.pirep_filed_title)).toBeTruthy();
    expect(screen.queryByText(deCommon.cockpit.sc_partner_title)).toBeNull();
  });

  it("das Cockpit wird nach dem Flug neu eingehängt (Reiterwechsel): Partner-Text bleibt", async () => {
    lage.sc = [AUSSTEHEND];
    const erst = render(cockpit(flug({ pirep_id: "P5" })));
    await ruhe();
    erst.unmount();
    render(cockpit(null, FILED));
    await ruhe();
    expect(screen.getByText(deCommon.cockpit.sc_partner_title)).toBeTruthy();
  });

  it("ohne Antwort von live bleibt die übliche Meldung", async () => {
    lage.sc = [new Error("sc_landung_fehler")];
    const { rerender } = render(cockpit(flug({ pirep_id: "P3" })));
    await ruhe();
    rerender(cockpit(null, FILED));
    await ruhe();
    expect(screen.getByText(deCommon.cockpit.pirep_filed_title)).toBeTruthy();
  });
});

describe("Hartlande-Hinweis beim Partner", () => {
  it("zeigt die eigene Rate beim Partner nicht, sonst wie bisher", async () => {
    const f = flug({ landing_rate_fpm: -700, landing_score_finalized: true, was_just_resumed: false });
    const { rerender } = render(<StableApproachBanner activeFlight={f} simSnapshot={null} enabled ohneHartlandung />);
    await ruhe();
    expect(screen.queryByRole("alert")).toBeNull();
    rerender(<StableApproachBanner activeFlight={{ ...f }} simSnapshot={null} enabled />);
    expect((await screen.findByRole("alert")).textContent).toContain("-700");
  });
});

describe("scUebernehmen (gleiche Regel wie sc_ersetzt in Rust)", () => {
  const g = (o: Partial<typeof GELANDET>) => ({ ...GELANDET, ...o }) as ScLandung;
  it("neuere Landung ersetzt, Note geht nie verloren, gelandet bleibt", () => {
    expect(scUebernehmen(g({ landing_score: null }), g({}))).toEqual(g({}));
    expect(scUebernehmen(g({ touchdown_id: 1, landing_score: null }), g({ touchdown_id: 2, landing_score: null })))
      .toEqual(g({ touchdown_id: 2, landing_score: null }));
    expect(scUebernehmen(g({}), g({ landing_score: null }))).toEqual(g({}));
    expect(scUebernehmen(g({}), AUSSTEHEND as ScLandung)).toEqual(g({}));
    expect(scUebernehmen(g({}), KEINE_CREW as ScLandung)).toEqual(g({}));
    expect(scUebernehmen(null, AUSSTEHEND as ScLandung)).toEqual(AUSSTEHEND);
    expect(scUebernehmen(AUSSTEHEND as ScLandung, g({ landing_score: null }))).toEqual(g({ landing_score: null }));
  });
});
