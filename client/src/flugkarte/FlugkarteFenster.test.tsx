// @vitest-environment jsdom
// Flugkarte in AeroACARS (Etappe 3, 28.10.2026): das Fenster um den Baustein.
// Geprüft wird, was der Pilot sieht, wenn es keine Akte gibt, wenn der Zugang
// zum Live-Server fehlt, und dass die Akte mit Übersetzer und Sprache an den
// Baustein geht. Der Baustein selbst ist eine Kopie aus aeroacars-live und dort
// getestet (flugkarteBaustein.test.ts); hier steht er als Attrappe.
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

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
import { eintrag } from "../components/bilder/logik/testhilfen";

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

  describe("Bilder", () => {
    const akte = JSON.stringify({ version: 3, kopf: { start: "2026-10-27T18:27:06Z" }, track: [], events: [] });
    const mitBildern = (eintraege: unknown[]) => tauriInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "flugakte_laden") return akte;
      if (cmd === "bilder_liste") return { eintraege, belegt_bytes: 0, quellen: [], altbestand: [], funde: [] };
      if (cmd === "bild_lesen") return new Uint8Array([1, 2, 3]);
      return undefined;
    });
    const optionen = () => anzeigen.mock.calls[0][2] as { bilder: Promise<Array<{ id: string; lat: number; lon: number; name: string; hoeheFt?: number | null }>>; beiBild: (id: string) => void };

    it("nur Bilder dieses Flugs mit Position werden Punkte; Esc schliesst erst das Grossbild, dann das Fenster", async () => {
      URL.createObjectURL = vi.fn(() => "blob:x");
      mitBildern([
        eintrag({ id: "hier", meta: { pirep_id: "A", zeit_utc: "2026-10-27T18:30:00Z", breite: 50, laenge: 8 } }),
        eintrag({ id: "ohne_ort", meta: { pirep_id: "A", zeit_utc: "2026-10-27T18:20:00Z", breite: null, laenge: null } }),
        eintrag({ id: "anderer", meta: { pirep_id: "B", breite: 51, laenge: 9 } }),
      ]);
      const zu = vi.fn();
      render(<FlugkarteFenster pirepId="A" onClose={zu} />);
      await waitFor(() => expect(anzeigen).toHaveBeenCalledTimes(1));
      expect(await optionen().bilder).toEqual([{ id: "hier", lat: 50, lon: 8, hoeheFt: 12000, name: "Bild 18:30Z" }]);
      act(() => optionen().beiBild("hier"));
      const gross = screen.getByRole("dialog", { name: "Bild in Großansicht" });
      expect(document.querySelector(".flugkarte-fenster")!.contains(gross)).toBe(true);
      expect(document.querySelector(".bt-gross__zahl")?.textContent).toBe("2 von 2");
      fireEvent.keyDown(document.activeElement!, { key: "Escape" });
      expect(screen.queryByRole("dialog", { name: "Bild in Großansicht" })).toBeNull();
      expect(zu).not.toHaveBeenCalled();
      fireEvent.keyDown(document.activeElement!, { key: "Escape" });
      expect(zu).toHaveBeenCalledTimes(1);
    });

    it("schlaegt die Bilderliste fehl, kommt die Karte trotzdem", async () => {
      tauriInvoke.mockImplementation(async (cmd: string) => { if (cmd === "bilder_liste") throw new Error("kein bilder-1"); return akte; });
      render(<FlugkarteFenster pirepId="ZZ" onClose={() => {}} />);
      await waitFor(() => expect(anzeigen).toHaveBeenCalledTimes(1));
      expect(await optionen().bilder).toEqual([]);
    });

    it("die Karte wartet nicht auf eine langsame Bilderliste", async () => {
      let liste: (v: unknown) => void = () => undefined;
      tauriInvoke.mockImplementation((cmd: string) => (cmd === "bilder_liste" ? new Promise((ok) => { liste = ok; }) : Promise.resolve(akte)));
      render(<FlugkarteFenster pirepId="YY" onClose={() => {}} />);
      await waitFor(() => expect(anzeigen).toHaveBeenCalledTimes(1));
      liste({ eintraege: [eintrag({ id: "spaet", meta: { pirep_id: "YY", breite: 50, laenge: 8 } })], belegt_bytes: 0, quellen: [], altbestand: [], funde: [] });
      expect((await optionen().bilder).map((b) => b.id)).toEqual(["spaet"]);
    });
  });
});
