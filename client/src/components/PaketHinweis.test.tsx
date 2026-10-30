// Meldung „neue Paket-Generation bereit“ (30.10.2026, Thomas).
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
  isTauri: true,
}));

import { PaketHinweis, ERSTE_PRUEFUNG_MS, PRUEF_ABSTAND_MS } from "./PaketHinweis";

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

const info = (naechster: number | null, flug_offen = false) => ({
  herkunft: "paket", id: "53-ab", generation: 53, bewertung_fassung: 27, flug_pin: null,
  naechster_start: naechster === null ? null : { generation: naechster, bewertung_fassung: 27 },
  letzte_pruefung: null, flug_offen,
});

let antworten: Record<string, unknown>;
beforeEach(() => {
  vi.useFakeTimers();
  antworten = { paket_info: info(null), paket_jetzt_pruefen: { ergebnis: "kein_neueres" }, paket_neu_starten: undefined };
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (befehl: string) => antworten[befehl]);
});
afterEach(() => vi.useRealTimers());

const rendern = async (imFlug = false) => {
  let r!: ReturnType<typeof render>;
  await act(async () => {
    r = render(<PaketHinweis imFlug={imFlug} />);
  });
  return r;
};

describe("PaketHinweis", () => {
  it("ohne wartendes Paket: kein Band", async () => {
    const r = await rendern();
    expect(r.container.textContent).toBe("");
  });

  it("wartendes Paket: Band mit Generation und Neustart", async () => {
    antworten.paket_info = info(54);
    await rendern();
    expect(screen.getByText("Neues Paket bereit: Generation 54")).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByText("Jetzt neu starten"));
    });
    expect(invokeMock).toHaveBeenCalledWith("paket_neu_starten");
  });

  it("fragt nach dem Start und dann alle 30 Minuten bei live nach", async () => {
    await rendern();
    expect(invokeMock).not.toHaveBeenCalledWith("paket_jetzt_pruefen");
    antworten.paket_info = info(55);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ERSTE_PRUEFUNG_MS);
    });
    expect(invokeMock).toHaveBeenCalledWith("paket_jetzt_pruefen");
    expect(screen.getByText("Neues Paket bereit: Generation 55")).toBeTruthy();
    const vorher = invokeMock.mock.calls.filter((c) => c[0] === "paket_jetzt_pruefen").length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PRUEF_ABSTAND_MS);
    });
    expect(invokeMock.mock.calls.filter((c) => c[0] === "paket_jetzt_pruefen").length).toBe(vorher + 1);
  });

  it("im Flug: kein Band und keine Nachfrage", async () => {
    antworten.paket_info = info(54);
    const r = await rendern(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ERSTE_PRUEFUNG_MS + PRUEF_ABSTAND_MS);
    });
    expect(r.container.textContent).toBe("");
    expect(invokeMock).not.toHaveBeenCalledWith("paket_jetzt_pruefen");
  });

  it("Kern meldet offenen Flug: kein Band", async () => {
    antworten.paket_info = info(54, true);
    const r = await rendern();
    expect(r.container.textContent).toBe("");
  });

  it("„Später“ blendet diese Generation aus, eine neuere erscheint wieder", async () => {
    antworten.paket_info = info(54);
    const r = await rendern();
    fireEvent.click(screen.getByText("Später"));
    expect(r.container.textContent).toBe("");
    antworten.paket_info = info(56);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ERSTE_PRUEFUNG_MS);
    });
    expect(screen.getByText("Neues Paket bereit: Generation 56")).toBeTruthy();
  });
});
