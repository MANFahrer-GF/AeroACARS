// v3 (P-7.24): „Upload ausstehend seit …“ mit Grund — geprüft wird, was der
// Pilot sieht (Marke in der Zeile, Kasten in der Detailansicht).
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render, renderHook } from "@testing-library/react";

const tauriInvoke = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
  listen: () => Promise.resolve(() => {}),
  isTauri: true,
}));

import { UploadKasten, UploadMarke, useUploadsAusstehend, type UploadAusstehend } from "./UploadAusstehend";

const eingabe: UploadAusstehend = { pirep_id: "P1", art: "eingabe", seit_ms: Date.UTC(2026, 9, 30, 10, 0), versuche: 3, grund: "HTTP 503" };
const protokoll: UploadAusstehend = { pirep_id: "P1", art: "protokoll", seit_ms: Date.UTC(2026, 9, 30, 11, 0), versuche: 0, grund: null };

afterEach(() => tauriInvoke.mockReset());

describe("UploadAusstehend", () => {
  it("Marke zeigt „ausstehend seit“, der Tooltip den Grund", () => {
    const { getByTestId } = render(<UploadMarke eintraege={[eingabe, protokoll]} />);
    const marke = getByTestId("upload-marke");
    expect(marke.textContent).toMatch(/Upload ausstehend seit/);
    expect(marke.getAttribute("title")).toBe("Letzter Fehlschlag: HTTP 503");
  });

  it("ohne Eintrag keine Marke und kein Kasten (Gegenprobe)", () => {
    const a = render(<UploadMarke eintraege={undefined} />);
    expect(a.queryByTestId("upload-marke")).toBeNull();
    const b = render(<UploadKasten eintraege={[]} />);
    expect(b.queryByTestId("upload-kasten")).toBeNull();
  });

  it("Kasten nennt je Teil Art, seit und Grund", () => {
    const { getByTestId } = render(<UploadKasten eintraege={[eingabe, protokoll]} />);
    const text = getByTestId("upload-kasten").textContent ?? "";
    expect(text).toMatch(/Landungsdaten · Upload ausstehend seit .* · Letzter Fehlschlag: HTTP 503/);
    expect(text).toMatch(/Flugprotokoll · Upload ausstehend seit .* · Noch nicht gesendet/);
  });

  it("Konflikt (409) wird als Konflikt gezeigt, nicht als „ausstehend“", () => {
    const konflikt: UploadAusstehend = { ...eingabe, versuche: 0, grund: "HTTP 409", konflikt: true };
    const marke = render(<UploadMarke eintraege={[konflikt, protokoll]} />).getByTestId("upload-marke");
    expect(marke.textContent).toBe("Upload-Konflikt");
    expect(marke.getAttribute("title")).toMatch(/^Konflikt: Der Live-Server hat für diese Landung schon andere Daten/);
    const text = render(<UploadKasten eintraege={[konflikt, protokoll]} />).getAllByTestId("upload-kasten")[0].textContent ?? "";
    expect(text).toMatch(/Landungsdaten · Konflikt: /);
    expect(text).not.toMatch(/Landungsdaten · Upload ausstehend/);
    expect(text).toMatch(/Flugprotokoll · Upload ausstehend seit/);
  });

  it("Hook fragt uploads_ausstehend und gruppiert je PIREP", async () => {
    tauriInvoke.mockResolvedValue([eingabe, protokoll, { ...eingabe, pirep_id: "P2" }]);
    const { result } = renderHook(() => useUploadsAusstehend());
    await act(async () => {});
    expect(tauriInvoke).toHaveBeenCalledWith("uploads_ausstehend");
    expect(result.current["P1"]).toHaveLength(2);
    expect(result.current["P2"]).toHaveLength(1);
  });
});
