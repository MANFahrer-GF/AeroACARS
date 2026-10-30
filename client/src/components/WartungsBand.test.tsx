// v3 (P-9.12): Wartungsband aus status.json von live — beim Start und alle
// 10 Minuten; ohne Netz nichts.

import { describe, it, expect, vi, afterEach } from "vitest";
import { act, render, screen } from "@testing-library/react";
import { WartungsBand } from "./WartungsBand";
import { STATUS_TAKT_MS, STATUS_URL, statusLesen } from "../lib/statusHinweis";

const JETZT = Date.parse("2026-10-30T12:00:00Z");

function status(teil: Record<string, unknown>) {
  return { version: 1, erzeugt: "2026-10-30T11:59:00Z", gesamt: "ok", dienste: [], wartung: [], stoerungen: [], verlauf: {}, ...teil };
}

const wartung = (von: string, bis: string, extra: Record<string, unknown> = {}) => ({
  id: 1, von, bis, dienste: ["mqtt"], notfall: false, aktiv: false,
  text: { de: "Umzug der Datenbank", en: "Database move", it: "Trasloco del database" }, ...extra,
});

function fetchMit(...antworten: unknown[]) {
  const f = vi.fn();
  for (const a of antworten) {
    if (a instanceof Error) f.mockRejectedValueOnce(a);
    else f.mockResolvedValueOnce({ ok: true, json: async () => a });
  }
  vi.stubGlobal("fetch", f);
  return f;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("statusLesen", () => {
  it("laufende Wartung vor angekuendigter, vergangene faellt weg", () => {
    const h = statusLesen(status({ wartung: [
      wartung("2026-10-31T20:00:00Z", "2026-10-31T22:00:00Z"),
      wartung("2026-10-30T11:00:00Z", "2026-10-30T13:00:00Z", { text: { de: "laeuft" } }),
      wartung("2026-10-29T11:00:00Z", "2026-10-29T13:00:00Z", { text: { de: "vorbei" } }),
    ] }), JETZT);
    expect(h?.wartung?.aktiv).toBe(true);
    expect(h?.wartung?.text.de).toBe("laeuft");
  });

  it("Gegenprobe: nur vergangene Wartung und unbekanntes Format ergeben nichts", () => {
    const h = statusLesen(status({ wartung: [wartung("2026-10-29T11:00:00Z", "2026-10-29T13:00:00Z")] }), JETZT);
    expect(h?.wartung).toBeNull();
    expect(statusLesen({ version: 2, wartung: [wartung("2026-10-30T11:00:00Z", "2026-10-30T13:00:00Z")] }, JETZT)).toBeNull();
    expect(statusLesen("kaputt", JETZT)).toBeNull();
  });
});

describe("WartungsBand", () => {
  it("zeigt die angekuendigte Wartung mit dem Text des Betreibers", async () => {
    vi.useFakeTimers({ now: JETZT, toFake: ["Date", "setInterval", "clearInterval"] });
    const f = fetchMit(status({ wartung: [wartung("2026-10-31T20:00:00Z", "2026-10-31T22:00:00Z")] }));
    render(<WartungsBand />);
    expect(await screen.findByText("Wartung geplant")).toBeInTheDocument();
    expect(screen.getByText("Umzug der Datenbank")).toBeInTheDocument();
    expect(screen.getByText(/31\.10\..*20:00.*22:00 UTC/)).toBeInTheDocument();
    expect(f).toHaveBeenCalledWith(STATUS_URL, expect.objectContaining({ cache: "no-store" }));
  });

  it("ohne Netz nichts — und der naechste Abruf nach 10 Minuten zeigt die laufende Wartung", async () => {
    vi.useFakeTimers({ now: JETZT, toFake: ["Date", "setInterval", "clearInterval"] });
    const f = fetchMit(new TypeError("offline"), status({ wartung: [wartung("2026-10-30T11:00:00Z", "2026-10-30T13:00:00Z")] }));
    const { container } = render(<WartungsBand />);
    await act(async () => {});
    expect(f).toHaveBeenCalledTimes(1);
    expect(container).toBeEmptyDOMElement();
    // Gegenprobe zum Takt: kurz davor noch kein zweiter Abruf.
    await act(async () => { vi.advanceTimersByTime(STATUS_TAKT_MS - 1); });
    expect(f).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(1); });
    expect(f).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("Wartung läuft")).toBeInTheDocument();
  });

  it("Stoerungen dezent als Hinweis, ohne Stoerung und Wartung nichts", async () => {
    fetchMit(status({ dienste: [
      { id: "mqtt", name: { de: "MQTT-Annahme", en: "MQTT intake", it: "Ricezione MQTT" }, status: "stoerung" },
      { id: "paket", name: { de: "Paket-Auslieferung", en: "Package delivery", it: "Distribuzione pacchetti" }, status: "ok" },
    ] }));
    const { container, unmount } = render(<WartungsBand />);
    expect(await screen.findByText("Störung")).toBeInTheDocument();
    expect(screen.getByText("MQTT-Annahme")).toBeInTheDocument();
    expect(container.querySelector(".ui-notice--info")).not.toBeNull();
    unmount();
    fetchMit(status({}));
    const leer = render(<WartungsBand />);
    await act(async () => {});
    expect(leer.container).toBeEmptyDOMElement();
  });
});
