// Über-Seite (Beta 1, Thomas): „Hilfe anfordern“ führt zur eigenen Seite,
// und „Alle Releases auf GitHub“ ist ein Knopf wie seine Nachbarn — ein
// <a class="button"> blieb ein nackter Link (App.css gestaltet nur <button>).
import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
  isTauri: true,
}));

import { AboutPanel } from "./AboutPanel";

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

describe("Über-Seite", () => {
  it("Hilfe-Knopf führt zur Hilfe-Seite, Releases sind ein Knopf und öffnen extern", async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "app_info" ? Promise.resolve({ version: "3.0.0", commit: null, credit: "x" }) : Promise.resolve(null),
    );
    const onHilfe = vi.fn();
    render(<AboutPanel onShowReleaseNotes={() => {}} onHilfe={onHilfe} />);
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole("button", { name: deCommon.hilfe.knopf }));
    expect(onHilfe).toHaveBeenCalledTimes(1);
    const releases = screen.getByRole("button", { name: deCommon.release_notes.about_all_releases });
    expect(releases.tagName).toBe("BUTTON");
    expect(releases.className).toBe("button button--ghost");
    expect(screen.queryByRole("link", { name: deCommon.release_notes.about_all_releases })).toBeNull();
    fireEvent.click(releases);
    expect(invokeMock).toHaveBeenCalledWith("plugin:opener|open_url", { url: "https://github.com/MANFahrer-GF/AeroACARS/releases" });
  });
});

describe("Über-Seite: Paket und Landebewertung (paket_info)", () => {
  const rendern = async (info: Record<string, unknown> | null) => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "app_info"
        ? Promise.resolve({ version: "3.0.0", commit: null, credit: "x" })
        : cmd === "paket_info" && info
          ? Promise.resolve(info)
          : Promise.reject(new Error("aus")),
    );
    render(<AboutPanel onShowReleaseNotes={() => {}} onHilfe={() => {}} />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  };
  const basis = { herkunft: "paket", id: "53-ab", flug_pin: null, naechster_start: null, letzte_pruefung: null, flug_offen: false };
  it("zeigt Generation und Fassung in Klartext", async () => {
    await rendern({ ...basis, generation: 53, bewertung_fassung: 27 });
    expect(screen.getByTestId("paket-zeile").textContent).toBe("Paket: Generation 53 · Landebewertung: Fassung 27");
  });
  it("Generation 0 heißt eingebauter Stand, mit Fassung", async () => {
    await rendern({ ...basis, herkunft: "eingebaut", generation: 0, bewertung_fassung: 27 });
    expect(screen.getByTestId("paket-zeile").textContent).toBe("Paket: eingebauter Stand · Landebewertung: Fassung 27");
  });
  it("holt die Angabe nicht mehr über die Hilfe-Vorschau", async () => {
    await rendern({ ...basis, generation: 53, bewertung_fassung: 27 });
    expect(invokeMock.mock.calls.some((c) => c[0] === "hilfe_vorschau")).toBe(false);
  });
  it("ohne Angabe keine Übersicht", async () => {
    await rendern(null);
    expect(screen.queryByTestId("paket-uebersicht")).toBeNull();
  });
});
