// v2.1.25 — Band „AeroACARS 3 ist da“: nur mit Version aus dem 3.x-Kanal,
// nie im Flug, „Später“ hält 24 Stunden, Fehler bleiben sichtbar.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";

const invoke = vi.fn();
vi.mock("../lib/ipc", () => ({
  isTauri: true,
  invoke: (...a: unknown[]) => invoke(...a),
  formatIpcError: (e: unknown) => String(e),
}));

import { BrueckeV3Band } from "./BrueckeV3Band";

const V3 = { version: "3.0.0", sperre: null };

beforeEach(() => {
  invoke.mockReset();
  localStorage.clear();
});
afterEach(() => vi.useRealTimers());

async function zeigen(phase: Parameters<typeof BrueckeV3Band>[0]["activePhase"] = null) {
  const r = render(<BrueckeV3Band activePhase={phase} />);
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
    const { container, rerender } = await zeigen("cruise");
    expect(container.innerHTML).toBe("");
    rerender(<BrueckeV3Band activePhase={"preflight" as never} />);
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("startet den Umstieg und zeigt einen Fehler", async () => {
    invoke.mockResolvedValueOnce(V3).mockRejectedValueOnce("kein Netz");
    await zeigen();
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button")[0]);
    });
    expect(invoke).toHaveBeenLastCalledWith("bruecke_v3_installieren");
    expect(screen.getByRole("status").textContent).toContain("kein Netz");
  });

  it("sperrt den Umstieg bei wartenden Flügen und sagt warum", async () => {
    invoke.mockResolvedValueOnce({ version: "3.0.0", sperre: "warteschlange" });
    await zeigen();
    expect((screen.getAllByRole("button")[0] as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("Es warten noch Flüge");
  });

  it("übernimmt eine Sperre, die erst beim Installieren auffällt", async () => {
    invoke.mockResolvedValueOnce(V3).mockRejectedValueOnce("gesperrt:flug");
    await zeigen();
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button")[0]);
    });
    expect((screen.getAllByRole("button")[0] as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("status").textContent).toContain("Erst den offenen Flug beenden");
  });

  it("sperrt die Knöpfe, solange der Umstieg läuft", async () => {
    invoke.mockResolvedValueOnce(V3).mockReturnValueOnce(new Promise(() => {}));
    await zeigen();
    fireEvent.click(screen.getAllByRole("button")[0]);
    for (const b of screen.getAllByRole("button")) expect((b as HTMLButtonElement).disabled).toBe(true);
  });

  it("„Später“ blendet aus und gilt 24 Stunden", async () => {
    invoke.mockResolvedValue(V3);
    const { container, unmount } = await zeigen();
    fireEvent.click(screen.getAllByRole("button")[1]);
    expect(container.innerHTML).toBe("");
    unmount();
    expect((await zeigen()).container.innerHTML).toBe("");
    // Nach 24 Stunden wieder da.
    localStorage.setItem("aa-bruecke-v3-spaeter", String(Date.now() - 24 * 3600 * 1000 - 1));
    await zeigen();
    expect(screen.getByRole("status")).toBeTruthy();
  });
});
