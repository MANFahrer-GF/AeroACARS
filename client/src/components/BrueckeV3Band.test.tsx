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
    invoke.mockResolvedValueOnce("3.0.0-beta.2.3");
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
    invoke.mockResolvedValue("3.0.0");
    const { container, rerender } = await zeigen("cruise");
    expect(container.innerHTML).toBe("");
    rerender(<BrueckeV3Band activePhase={"preflight" as never} />);
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("startet den Umstieg und zeigt einen Fehler", async () => {
    invoke.mockResolvedValueOnce("3.0.0").mockRejectedValueOnce("kein Netz");
    await zeigen();
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button")[0]);
    });
    expect(invoke).toHaveBeenLastCalledWith("bruecke_v3_installieren");
    expect(screen.getByRole("status").textContent).toContain("kein Netz");
  });

  it("„Später“ blendet aus und gilt 24 Stunden", async () => {
    invoke.mockResolvedValue("3.0.0");
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
