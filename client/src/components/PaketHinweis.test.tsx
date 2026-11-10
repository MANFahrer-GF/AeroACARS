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

  // 3.0.1 (Thomas 10.11.): Bis 3.0.0 gab es im Flug weder Band noch
  // Nachfrage. Jetzt fragt das Band auch im Flug und zeigt die neue
  // Generation „ab dem nächsten Flug“, ohne Neustart-Knopf.
  it("im Flug: fragt nach und zeigt das Band ohne Neustart", async () => {
    antworten.paket_info = info(null, true);
    const r = await rendern(true);
    expect(r.container.textContent).toBe("");
    antworten.paket_info = info(54, true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ERSTE_PRUEFUNG_MS);
    });
    expect(invokeMock).toHaveBeenCalledWith("paket_jetzt_pruefen");
    expect(screen.getByText("Neues Paket bereit: Generation 54")).toBeTruthy();
    expect(screen.getByText(deCommon.paket.neu_starten_nach_flug)).toBeTruthy();
    expect(screen.queryByText("Jetzt neu starten")).toBeNull();
    expect(screen.getByText("Später")).toBeTruthy();
  });

  it("Kern meldet offenen Flug: Band ohne Neustart, auch wenn die Oberfläche keinen Flug kennt", async () => {
    antworten.paket_info = info(54, true);
    await rendern();
    expect(screen.getByText(deCommon.paket.neu_starten_nach_flug)).toBeTruthy();
    expect(screen.queryByText("Jetzt neu starten")).toBeNull();
  });

  it("nach dem Flug: Neustart-Knopf wieder da", async () => {
    antworten.paket_info = info(54, true);
    const r = await rendern(true);
    expect(screen.queryByText("Jetzt neu starten")).toBeNull();
    antworten.paket_info = info(54, false);
    await act(async () => {
      r.rerender(<PaketHinweis imFlug={false} />);
    });
    expect(screen.getByText("Jetzt neu starten")).toBeTruthy();
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
