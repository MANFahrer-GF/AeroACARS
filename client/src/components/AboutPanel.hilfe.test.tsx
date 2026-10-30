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

describe("Über-Seite: Paket-Generation", () => {
  const rendern = async (kennung: string | null) => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "app_info"
        ? Promise.resolve({ version: "3.0.0", commit: null, credit: "x" })
        : cmd === "hilfe_vorschau" && kennung
          ? Promise.resolve({ meta: { paket_generation: kennung } })
          : Promise.reject(new Error("aus")),
    );
    render(<AboutPanel onShowReleaseNotes={() => {}} onHilfe={() => {}} />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
  };
  it("zeigt die Generation aus der Kennung", async () => {
    await rendern("12-0123456789ab");
    expect(screen.getByTestId("about-paket").textContent).toBe("Laufzeit-Paket: Generation 12");
  });
  it("Generation 0 heißt eingebauter Stand", async () => {
    await rendern("0-0123456789ab");
    expect(screen.getByTestId("about-paket").textContent).toBe("Laufzeit-Paket: eingebauter Stand");
  });
  it("ohne Angabe keine Zeile", async () => {
    await rendern(null);
    expect(screen.queryByTestId("about-paket")).toBeNull();
  });
});
