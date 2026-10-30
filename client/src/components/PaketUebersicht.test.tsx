// Paket-Übersicht (30.10.2026, Thomas): Generation + Fassung, „neues Paket
// wartet“ mit „Jetzt neu starten“ (nur ohne Flug) und „Auf neues Paket prüfen“.
import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
  isTauri: true,
}));

import { PaketUebersicht } from "./PaketUebersicht";

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

const basis = {
  herkunft: "paket",
  id: "53-ab",
  generation: 53,
  bewertung_fassung: 27,
  flug_pin: null,
  naechster_start: null,
  letzte_pruefung: null,
  flug_offen: false,
};

const rendern = async (info: Record<string, unknown>, weitere: Record<string, () => Promise<unknown>> = {}) => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) =>
    cmd === "paket_info" ? Promise.resolve(info) : (weitere[cmd]?.() ?? Promise.reject(new Error(cmd))),
  );
  render(<PaketUebersicht />);
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};

describe("Paket-Übersicht", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("neues Paket wartet: Hinweis und „Jetzt neu starten“ ruft den Kern", async () => {
    await rendern({ ...basis, naechster_start: { generation: 54, bewertung_fassung: 27 } }, { paket_neu_starten: () => Promise.resolve(null) });
    expect(screen.getByTestId("paket-neu").textContent).toContain("Neues Paket 54 geladen, wird beim nächsten Start aktiv.");
    fireEvent.click(screen.getByRole("button", { name: deCommon.paket.neu_starten }));
    expect(invokeMock).toHaveBeenCalledWith("paket_neu_starten");
  });

  it("Neustart: Doppelklick ruft einmal, „zu früh“ wird erklärt", async () => {
    let fertig: (v: unknown) => void = () => {};
    await rendern({ ...basis, naechster_start: { generation: 54, bewertung_fassung: 27 } }, {
      paket_neu_starten: () => new Promise((_, nein) => (fertig = () => nein({ code: "zu_frueh", message: "x" }))),
    });
    const knopf = screen.getByRole("button", { name: deCommon.paket.neu_starten });
    fireEvent.click(knopf);
    fireEvent.click(knopf);
    expect(invokeMock.mock.calls.filter((c) => c[0] === "paket_neu_starten").length).toBe(1);
    await act(async () => {
      fertig(null);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByRole("alert").textContent).toBe(deCommon.paket.neustart_zu_frueh);
  });

  it("Gegenprobe: im Flug kein Neustart-Knopf, ohne wartendes Paket kein Hinweis", async () => {
    await rendern({ ...basis, flug_offen: true, naechster_start: { generation: 54, bewertung_fassung: 27 } });
    expect(screen.queryByRole("button", { name: deCommon.paket.neu_starten })).toBeNull();
    expect(screen.getByTestId("paket-neu").textContent).toContain(deCommon.paket.neu_starten_nach_flug);
  });

  it("ohne wartendes Paket keine Zeile „Neues Paket“", async () => {
    await rendern(basis);
    expect(screen.queryByTestId("paket-neu")).toBeNull();
  });

  it("„Auf neues Paket prüfen“ zeigt Fehler, „geladen“ und „kein neueres“", async () => {
    let antwort: unknown = { zeit: "t", ergebnis: "fehler", generation: null, fehler: "HTTP 401" };
    await rendern(basis, { paket_jetzt_pruefen: () => Promise.resolve(antwort) });
    const knopf = screen.getByRole("button", { name: deCommon.paket.pruefen });
    await act(async () => {
      fireEvent.click(knopf);
    });
    expect(screen.getByTestId("paket-antwort").textContent).toBe("Prüfung fehlgeschlagen: HTTP 401");
    antwort = { zeit: "t", ergebnis: "neu", generation: 54, fehler: null };
    await act(async () => {
      fireEvent.click(knopf);
    });
    expect(screen.getByTestId("paket-antwort").textContent).toBe("Generation 54 geladen.");
    antwort = { zeit: "t", ergebnis: "kein_neueres", generation: null, fehler: null };
    await act(async () => {
      fireEvent.click(knopf);
    });
    expect(screen.getByTestId("paket-antwort").textContent).toBe("Kein neueres Paket.");
    // Nach dem Prüfen liest die Übersicht den Stand neu (neues Paket wartet).
    expect(invokeMock.mock.calls.filter((c) => c[0] === "paket_info").length).toBe(4);
  });

  it("zeigt einen fehlgeschlagenen Startabruf (z. B. 401) ohne Klick", async () => {
    await rendern({ ...basis, letzte_pruefung: { zeit: "t", ergebnis: "fehler", generation: null, fehler: "HTTP 401" } });
    expect(screen.getByTestId("paket-letzte-fehler").textContent).toBe("Letzte Prüfung fehlgeschlagen: HTTP 401");
  });
});
