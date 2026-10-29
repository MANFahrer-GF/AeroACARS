// 2.1.3 — Michel (iPad über die LAN-Brücke, CL650): der Server lehnte das
// Paket mit 413 ab, das Feld zeigte aber nur "[object Object]". Ein Fehler
// aus invoke() kommt als {code, message}-Objekt; String(e) macht daraus
// nichts Lesbares. Das Feld muss die Meldung selbst zeigen.

import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", async (importOriginal) => {
  const echt = await importOriginal<typeof import("../lib/ipc")>();
  return {
    ...echt,
    invoke: (...args: unknown[]) => invokeMock(...args),
  };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { AircraftScanPanel } from "./AircraftScanPanel";

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

describe("AircraftScanPanel Fehleranzeige", () => {
  it("zeigt die Meldung eines {code, message}-Fehlers statt [object Object]", async () => {
    invokeMock.mockRejectedValue({
      code: "unknown",
      message: "Server lehnte ab (413 Payload Too Large): Request body is too large",
    });
    render(<AircraftScanPanel />);
    await userEvent.click(screen.getByRole("button", { name: /Flugzeuge suchen/ }));
    expect(await screen.findByText(/Server lehnte ab \(413/)).toBeTruthy();
    expect(screen.queryByText(/\[object Object\]/)).toBeNull();
  });
});
