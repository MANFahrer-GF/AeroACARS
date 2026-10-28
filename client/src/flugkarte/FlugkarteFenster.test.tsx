// @vitest-environment jsdom
// Flugkarte in AeroACARS (Etappe 3, 28.10.2026): das Fenster um den Baustein.
// Geprüft wird, was der Pilot sieht, wenn es keine Akte gibt, wenn der Zugang
// zum Live-Server fehlt, und dass die Akte mit Übersetzer und Sprache an den
// Baustein geht. Der Baustein selbst ist eine Kopie aus aeroacars-live und dort
// getestet (flugkarteBaustein.test.ts); hier steht er als Attrappe.
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

const tauriInvoke = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => tauriInvoke(...a),
  listen: () => Promise.resolve(() => {}),
  isTauri: true,
}));
const anzeigen = vi.fn((_root: HTMLElement, _akte: unknown, _opt: unknown) => () => {});
vi.mock("./flugkarte", () => ({ flugkarteAnzeigen: (...a: [HTMLElement, unknown, unknown]) => anzeigen(...a) }));
vi.mock("./stil-dunkel.json?url", () => ({ default: "/stil-dunkel.json" }));
vi.mock("./fenster.css", () => ({}));

import i18n from "../i18n";
import FlugkarteFenster from "./FlugkarteFenster";

beforeAll(async () => { await i18n.changeLanguage("de"); });
afterEach(() => { cleanup(); tauriInvoke.mockReset(); anzeigen.mockClear(); });

describe("FlugkarteFenster (AeroACARS)", () => {
  it("fragt die Akte des eigenen PIREPs ab", async () => {
    tauriInvoke.mockResolvedValue(null);
    render(<FlugkarteFenster pirepId="278xq2bgoYV5qZjL" onClose={() => {}} />);
    await waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith("flugakte_laden", { pirepId: "278xq2bgoYV5qZjL" }));
  });

  it("ohne Protokoll auf dem Server: verständlicher Hinweis, keine Karte", async () => {
    tauriInvoke.mockResolvedValue(null);
    render(<FlugkarteFenster pirepId="A" onClose={() => {}} />);
    expect(await screen.findByText(/gibt es kein Flugprotokoll/)).toBeTruthy();
    expect(anzeigen).not.toHaveBeenCalled();
  });

  it("ohne Live-Server-Zugang: sagt, dass AeroACARS verbunden sein muss", async () => {
    tauriInvoke.mockRejectedValue({ code: "flugakte_kein_zugang", message: "kein Live-Server-Zugang" });
    render(<FlugkarteFenster pirepId="A" onClose={() => {}} />);
    expect(await screen.findByText(/kommt vom Live-Server/)).toBeTruthy();
  });

  it("anderer Fehler: Meldung mit Ursache", async () => {
    tauriInvoke.mockRejectedValue({ code: "flugakte_laden", message: "HTTP 500" });
    render(<FlugkarteFenster pirepId="A" onClose={() => {}} />);
    expect(await screen.findByText(/konnte nicht geladen werden \(HTTP 500\)/)).toBeTruthy();
  });

  it("mit Akte: Baustein bekommt Akte, eingebauten Kartenstil, Übersetzer und Sprache", async () => {
    tauriInvoke.mockResolvedValue(JSON.stringify({ version: 3, kopf: { start: "2026-10-27T18:27:06Z" }, track: [], events: [] }));
    render(<FlugkarteFenster pirepId="A" onClose={() => {}} />);
    await waitFor(() => expect(anzeigen).toHaveBeenCalledTimes(1));
    const [, akte, opt] = anzeigen.mock.calls[0] as [HTMLElement, { version: number }, { stilUrl: string; sprache: string; datum?: string; t: (k: string) => string | null }];
    expect(akte.version).toBe(3);
    expect(opt.stilUrl).toBe("/stil-dunkel.json");
    expect(opt.sprache).toBe("de");
    expect(opt.datum).toBe("27.10.2026");
    expect(opt.t("flugkarte.ui.fit_all")).toBe("Ganzer Flug");
    expect(opt.t("flugkarte.gibt.es.nicht")).toBeNull();
  });
});
