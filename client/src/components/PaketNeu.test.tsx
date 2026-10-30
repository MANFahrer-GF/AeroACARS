// „Neu in Generation N“ nach dem Neustart (30.10.2026, Thomas).
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

import { PaketNeu, GESEHEN_KEY } from "./PaketNeu";

const TEXT = deCommon.paket.neu_text;

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

beforeEach(() => {
  localStorage.clear();
  i18next.addResource("de", "common", "paket.neu_text", TEXT);
  invokeMock.mockReset();
  invokeMock.mockResolvedValue({ generation: 57 });
});

const rendern = async (imFlug = false) => {
  let r!: ReturnType<typeof render>;
  await act(async () => {
    r = render(<PaketNeu imFlug={imFlug} />);
  });
  return r;
};

describe("PaketNeu", () => {
  it("die Paket-Texte haben einen Text für Piloten", () => {
    expect(TEXT.trim().length).toBeGreaterThan(20);
  });

  it("zeigt den Text der Generation mit ihrer Nummer", async () => {
    await rendern();
    expect(screen.getByText("Neu in Generation 57")).toBeTruthy();
    expect(screen.getByText(TEXT)).toBeTruthy();
  });

  it("„Verstanden“ blendet aus und merkt sich den Text — derselbe Text kommt nicht wieder", async () => {
    const r = await rendern();
    fireEvent.click(screen.getByText("Verstanden"));
    expect(r.container.textContent).toBe("");
    expect(localStorage.getItem(GESEHEN_KEY)).toBe(TEXT);
    r.unmount();
    const neu = await rendern();
    expect(neu.container.textContent).toBe("");
  });

  it("ein neuer Text in einer späteren Generation erscheint wieder", async () => {
    localStorage.setItem(GESEHEN_KEY, TEXT);
    i18next.addResource("de", "common", "paket.neu_text", "Etwas anderes ist neu.");
    await rendern();
    expect(screen.getByText("Etwas anderes ist neu.")).toBeTruthy();
  });

  it("im Flug und bei leerem Text: kein Band", async () => {
    expect((await rendern(true)).container.textContent).toBe("");
    i18next.addResource("de", "common", "paket.neu_text", "  ");
    expect((await rendern()).container.textContent).toBe("");
  });

  it("nach „Verstanden“ auf Deutsch kommt dieselbe Neuigkeit auf Englisch nicht wieder", async () => {
    i18next.addResourceBundle("en", "common", { paket: { neu_text: "Same news in English." } }, true, true);
    const r = await rendern();
    fireEvent.click(screen.getByText("Verstanden"));
    r.unmount();
    await act(async () => {
      await i18next.changeLanguage("en");
    });
    try {
      expect((await rendern()).container.textContent).toBe("");
    } finally {
      await act(async () => {
        await i18next.changeLanguage("de");
      });
    }
  });

  it("eingebauter Stand (Generation 0): Titel ohne Nummer statt „Generation 0“", async () => {
    invokeMock.mockResolvedValue({ generation: 0 });
    await rendern();
    expect(screen.getByText("Neu in diesem Paket")).toBeTruthy();
  });

  it("ohne paket_info (älterer Kern): Titel ohne Nummer", async () => {
    invokeMock.mockRejectedValue("unbekannt");
    await rendern();
    expect(screen.getByText("Neu in diesem Paket")).toBeTruthy();
  });
});
