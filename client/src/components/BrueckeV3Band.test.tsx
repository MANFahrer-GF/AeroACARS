// v2.1.25/26 — Band „AeroACARS 3 ist da“ und fester Knopf: nur mit Version
// aus dem 3.x-Kanal, Band nie im Flug, nicht wegklickbar, Sperre wird nach
// dem Flug neu abgefragt, Fehler bleiben sichtbar.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { FlightPhase } from "../types";

const invoke = vi.fn();
vi.mock("../lib/ipc", () => ({
  isTauri: true,
  invoke: (...a: unknown[]) => invoke(...a),
  formatIpcError: (e: unknown) => String(e),
}));

import { BrueckeV3Band, BrueckeV3Knopf, useBrueckeV3 } from "./BrueckeV3Band";

const V3 = { version: "3.0.0", sperre: null };

function Band({ phase }: { phase: FlightPhase | null }) {
  return <BrueckeV3Band bruecke={useBrueckeV3(phase)} activePhase={phase} />;
}
function Knopf({ phase }: { phase: FlightPhase | null }) {
  return <BrueckeV3Knopf bruecke={useBrueckeV3(phase)} />;
}

beforeEach(() => {
  invoke.mockReset();
  localStorage.clear();
});
afterEach(() => vi.useRealTimers());

async function zeigen(phase: FlightPhase | null = null) {
  const r = render(<Band phase={phase} />);
  await act(async () => {});
  return r;
}

describe("BrueckeV3Band", () => {
  it("zeigt den Umstieg, wenn der Kanal eine Version liefert", async () => {
    invoke.mockResolvedValueOnce({ version: "3.0.0-beta.2.3", sperre: null });
    await zeigen();
    expect(invoke).toHaveBeenCalledWith("bruecke_v3_pruefen");
    expect(screen.getByRole("status").textContent).toContain("3.0.0-beta.2.3");
  });

  it("zeigt nichts ohne Version", async () => {
    invoke.mockResolvedValueOnce(null);
    const { container } = await zeigen();
    expect(container.innerHTML).toBe("");
  });

  it("zeigt nichts im Flug, aber am Stand", async () => {
    invoke.mockResolvedValue(V3);
    const { container, rerender } = await zeigen("cruise" as FlightPhase);
    expect(container.innerHTML).toBe("");
    await act(async () => rerender(<Band phase={"preflight" as FlightPhase} />));
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("lässt sich nicht wegklicken: nur der Umstiegsknopf, auch nach Neustart", async () => {
    invoke.mockResolvedValue(V3);
    const { unmount } = await zeigen();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("button").textContent).toBe("Jetzt umsteigen");
    // Ein „Später“ aus 2.1.25 im Speicher blendet nichts mehr aus.
    localStorage.setItem("aa-bruecke-v3-spaeter", String(Date.now()));
    unmount();
    await zeigen();
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("startet den Umstieg und zeigt einen Fehler", async () => {
    invoke.mockResolvedValueOnce(V3).mockRejectedValueOnce("kein Netz");
    await zeigen();
    await act(async () => {
      fireEvent.click(screen.getByRole("button"));
    });
    expect(invoke).toHaveBeenLastCalledWith("bruecke_v3_installieren");
    expect(screen.getByRole("status").textContent).toContain("kein Netz");
  });

  it("sperrt den Umstieg bei wartenden Flügen und sagt warum", async () => {
    invoke.mockResolvedValueOnce({ version: "3.0.0", sperre: "warteschlange" });
    await zeigen();
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("Es warten noch Flüge");
  });

  it("übernimmt eine Sperre, die erst beim Installieren auffällt", async () => {
    invoke.mockResolvedValueOnce(V3).mockRejectedValueOnce("gesperrt:flug");
    await zeigen();
    await act(async () => {
      fireEvent.click(screen.getByRole("button"));
    });
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("Erst den offenen Flug beenden");
  });

  it("sperrt den Knopf, solange der Umstieg läuft", async () => {
    invoke.mockResolvedValueOnce(V3).mockReturnValueOnce(new Promise(() => {}));
    await zeigen();
    fireEvent.click(screen.getByRole("button"));
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });

  it("gibt den Knopf nach dem Flug frei (neue Abfrage beim Phasenwechsel)", async () => {
    invoke.mockResolvedValueOnce({ version: "3.0.0", sperre: "flug" }).mockResolvedValue(V3);
    const { rerender } = await zeigen("preflight" as FlightPhase);
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => rerender(<Band phase={null} />));
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(false);
  });

  it("fragt eine Sperre jede Minute neu ab", async () => {
    vi.useFakeTimers();
    invoke.mockResolvedValueOnce({ version: "3.0.0", sperre: "warteschlange" }).mockResolvedValue(V3);
    await zeigen();
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => {
      vi.advanceTimersByTime(60 * 1000);
    });
    expect(invoke).toHaveBeenCalledTimes(2);
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(false);
  });

  it("fragt im Flug nicht im Minutentakt ab", async () => {
    vi.useFakeTimers();
    invoke.mockResolvedValue({ version: "3.0.0", sperre: "flug" });
    await zeigen("cruise" as FlightPhase);
    await act(async () => {
      vi.advanceTimersByTime(30 * 60 * 1000);
    });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("BrueckeV3Knopf", () => {
  it("steht in der Seitenleiste und startet den Umstieg", async () => {
    invoke.mockResolvedValueOnce(V3).mockReturnValueOnce(new Promise(() => {}));
    render(<Knopf phase={null} />);
    await act(async () => {});
    const knopf = screen.getByRole("button") as HTMLButtonElement;
    expect(knopf.textContent).toContain("AeroACARS 3");
    expect(knopf.title).toContain("3.0.0");
    fireEvent.click(knopf);
    expect(invoke).toHaveBeenLastCalledWith("bruecke_v3_installieren");
  });

  it("bleibt im Flug sichtbar, aber gesperrt mit Grund", async () => {
    invoke.mockResolvedValue({ version: "3.0.0", sperre: "flug" });
    render(<Knopf phase={"cruise" as FlightPhase} />);
    await act(async () => {});
    const knopf = screen.getByRole("button") as HTMLButtonElement;
    expect(knopf.disabled).toBe(true);
    expect(knopf.title).toContain("Erst den offenen Flug beenden");
  });

  it("fehlt ohne Version", async () => {
    invoke.mockResolvedValue(null);
    const { container } = render(<Knopf phase={null} />);
    await act(async () => {});
    expect(container.innerHTML).toBe("");
  });
});
