// Prüfstatus beim Live-Server im Landungs-Tab (Befund DLH 880, 15.09.2026).
//
// Sven M's DLH 880 hing im Integritäts-Gate, im Client war davon nichts zu
// sehen — weder das Festhalten noch die spätere Freigabe. Geprüft wird hier,
// was der Pilot tatsächlich sieht (gerenderte Detailansicht und Marke), und
// dass der Stand selbstständig nachgeladen wird, solange ein Flug in der
// Prüfung steht.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, renderHook } from "@testing-library/react";

const tauriInvoke = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
  listen: () => Promise.resolve(() => {}),
  isTauri: true,
}));
vi.mock("maplibre-gl", () => ({
  default: { Map: class {}, Marker: class {}, LngLatBounds: class {}, NavigationControl: class {} },
}));
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));

import {
  PruefstatusMarke,
  TAKT_OFFEN_MS,
  TAKT_RUHIG_MS,
  grundSchluessel,
  pruefArt,
  usePirepPruefstatus,
  type PirepPruefstatus,
} from "./PirepPruefstatus";
import { LandingDetail } from "./LandingPanel";
import { LogbookView } from "./LogbookView";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";

function status(teil: Partial<PirepPruefstatus>): PirepPruefstatus {
  return {
    pirep_id: "278xq2bgoYV5qZjL",
    known: true,
    score_trust_level: "trusted",
    requires_review: false,
    review_state: "open",
    review_decision: null,
    reason_codes: [],
    reviewed_at: null,
    ...teil,
  };
}

const IN_PRUEFUNG = status({
  score_trust_level: "review",
  requires_review: true,
  reason_codes: ["no_touchdown_recorded"],
});

describe("pruefArt", () => {
  it("ordnet die Serverzustände den Anzeigen zu", () => {
    expect(pruefArt(undefined)).toBe("unbekannt");
    expect(pruefArt(status({ known: false }))).toBe("unbekannt");
    expect(pruefArt(status({}))).toBe("ok");
    expect(pruefArt(IN_PRUEFUNG)).toBe("in_pruefung");
    expect(pruefArt(status({ requires_review: true, review_state: "acknowledged" }))).toBe("in_pruefung");
  });

  it("eine Entscheidung des Admins geht vor der offenen Prüfung", () => {
    // DLH 880 nach der Freigabe: requires_review steht noch auf 1, entschieden ist aber.
    const dlh880 = { ...IN_PRUEFUNG, review_state: "dismissed_false_positive", review_decision: "dismissed_false_positive" };
    expect(pruefArt(dlh880)).toBe("fehlalarm");
    expect(pruefArt({ ...IN_PRUEFUNG, review_decision: "accepted_with_warning" })).toBe("freigegeben_hinweis");
    expect(pruefArt({ ...IN_PRUEFUNG, review_state: "rejected", review_decision: "rejected" })).toBe("abgelehnt");
  });

  it("unbekannte Gründe bekommen den allgemeinen Text", () => {
    expect(grundSchluessel("no_touchdown_recorded")).toBe("landing.pruefstatus.grund.no_touchdown_recorded");
    expect(grundSchluessel("etwas_neues")).toBe("landing.pruefstatus.grund.sonstiges");
  });
});

function zeigeDetail(pruefstatus: PirepPruefstatus | undefined, isPreview = false) {
  const rec = MOCK_LANDING_OPTIONS[0]!.build();
  return render(
    <LandingDetail
      record={rec as never}
      allRecords={[rec] as never}
      onBack={() => {}}
      isPreview={isPreview}
      pruefstatus={pruefstatus}
    />,
  );
}

describe("Anzeige in der Detailansicht", () => {
  it("zeigt einen festgehaltenen Flug mit Grund", () => {
    const text = zeigeDetail(IN_PRUEFUNG).container.textContent ?? "";
    expect(text).toContain("Flug wird geprüft");
    expect(text).toContain("keine Landung vor");
  });

  it("zeigt die Freigabe nach einem Fehlalarm", () => {
    const text =
      zeigeDetail({ ...IN_PRUEFUNG, review_state: "dismissed_false_positive", review_decision: "dismissed_false_positive" })
        .container.textContent ?? "";
    expect(text).toContain("Fehlalarm");
    expect(text).not.toContain("Flug wird geprüft");
  });

  it("zeigt nichts, wenn der Server den Flug nicht kennt oder es die Vorschau ist", () => {
    expect(zeigeDetail(undefined).queryByTestId("pruefstatus-kasten")).toBeNull();
    expect(zeigeDetail(status({ known: false })).queryByTestId("pruefstatus-kasten")).toBeNull();
    expect(zeigeDetail(IN_PRUEFUNG, true).queryByTestId("pruefstatus-kasten")).toBeNull();
  });
});

describe("Marke in der Übersicht", () => {
  it("nur, wenn es etwas zu sagen gibt", () => {
    expect(render(<PruefstatusMarke status={status({})} />).queryByTestId("pruefstatus-marke")).toBeNull();
    expect(render(<PruefstatusMarke status={undefined} />).queryByTestId("pruefstatus-marke")).toBeNull();
    const m = render(<PruefstatusMarke status={IN_PRUEFUNG} />).getByTestId("pruefstatus-marke");
    expect(m.textContent).toBe("in Prüfung");
  });
});

describe("usePirepPruefstatus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriInvoke.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("lädt den Stand und fragt bei offener Prüfung jede Minute nach, bis entschieden ist", async () => {
    tauriInvoke.mockResolvedValueOnce([IN_PRUEFUNG]);
    const { result } = renderHook(() => usePirepPruefstatus(["278xq2bgoYV5qZjL"]));
    await act(async () => {});
    expect(tauriInvoke).toHaveBeenCalledWith("pirep_pruefstatus", { pirepIds: ["278xq2bgoYV5qZjL"] });
    expect(pruefArt(result.current["278xq2bgoYV5qZjL"])).toBe("in_pruefung");

    // Server hat inzwischen neu bewertet → nach einer Minute sichtbar.
    tauriInvoke.mockResolvedValueOnce([status({})]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(TAKT_OFFEN_MS);
    });
    expect(tauriInvoke).toHaveBeenCalledTimes(2);
    expect(pruefArt(result.current["278xq2bgoYV5qZjL"])).toBe("ok");

    // Ohne offene Prüfung nur noch im ruhigen Takt.
    tauriInvoke.mockResolvedValue([status({})]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(TAKT_OFFEN_MS);
    });
    expect(tauriInvoke).toHaveBeenCalledTimes(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(TAKT_RUHIG_MS);
    });
    expect(tauriInvoke).toHaveBeenCalledTimes(3);
  });

  it("ein neues Array mit denselben IDs löst keinen neuen Abruf aus", async () => {
    tauriInvoke.mockResolvedValue([status({})]);
    const { rerender } = renderHook(({ ids }) => usePirepPruefstatus(ids), {
      initialProps: { ids: ["A", "B"] },
    });
    await act(async () => {});
    rerender({ ids: ["A", "B"] });
    await act(async () => {});
    expect(tauriInvoke).toHaveBeenCalledTimes(1);
  });

  it("ein Fehler lässt den letzten Stand stehen", async () => {
    tauriInvoke.mockResolvedValueOnce([IN_PRUEFUNG]);
    const { result } = renderHook(() => usePirepPruefstatus(["278xq2bgoYV5qZjL"]));
    await act(async () => {});
    tauriInvoke.mockRejectedValueOnce(new Error("offline"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(TAKT_OFFEN_MS);
    });
    expect(pruefArt(result.current["278xq2bgoYV5qZjL"])).toBe("in_pruefung");
  });
});

describe("Logbuch", () => {
  beforeEach(() => tauriInvoke.mockReset());

  it("zeigt den Prüfstatus auch für Flüge ohne Landungsdatensatz", async () => {
    // Codex-Abnahme 15.09.2026: Ein verworfener Aufsetz-Kandidat (DLH 880)
    // erzeugt keinen Landungsdatensatz — der Landungs-Tab kennt den Flug
    // dann nicht. Das Logbuch listet alle eingereichten PIREPs.
    tauriInvoke.mockImplementation((cmd: string) => {
      if (cmd === "logbook_stats") return Promise.resolve({});
      if (cmd === "logbook_pireps")
        return Promise.resolve({ total: 1, items: [{ id: "278xq2bgoYV5qZjL", dep_icao: "EDDF", arr_icao: "EETN", status: "PENDING" }] });
      if (cmd === "pirep_pruefstatus") return Promise.resolve([IN_PRUEFUNG]);
      return Promise.resolve(undefined);
    });
    const r = render(<LogbookView />);
    await act(async () => {});
    await act(async () => {});
    expect(tauriInvoke).toHaveBeenCalledWith("pirep_pruefstatus", { pirepIds: ["278xq2bgoYV5qZjL"] });
    expect(r.getByTestId("pruefstatus-marke").textContent).toBe("in Prüfung");
  });
});
