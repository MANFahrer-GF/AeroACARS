// Der Chat-Schalter in den Einstellungen.
//
// Warum ein eigener Test: auf der Datenschutzseite steht seit dem
// 12.08.2026 der Satz „Sie können den Chat in den Einstellungen des
// Clients abschalten." Als der Satz geschrieben wurde, gab es den
// Schalter nicht — nur einen gespeicherten Wert ohne Bedienelement.
// Eine Zusage auf einer Rechtsseite darf nicht wieder lautlos
// verschwinden, deshalb ist sie hier festgenagelt.

import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
// Der Einstellungs-Bereich fragt beim Aufbau den Client ab (Sim-Status,
// Hoppie, Speicher). Fuer diesen Test zaehlt nur der Chat-Abschnitt —
// also antwortet die Bruecke schlicht mit nichts, statt ins Leere zu
// laufen und den Lauf mit unbehandelten Fehlern zu faerben.
vi.mock("../lib/ipc", () => ({
  invoke: vi.fn(async () => null),
  isTauri: () => false,
  formatIpcError: (e: unknown) => String(e),
}));

import { SettingsPanel } from "./SettingsPanel";

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "de",
    resources: { de: { common: deCommon } },
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
});

function baueEinstellungen(over: Record<string, unknown> = {}) {
  // Der Chat-Schalter wohnt im Reiter „Extras" (Komfort/Privatsphäre).
  localStorage.setItem("aeroacars.settings.activeTab", "extras");
  const props = {
    debugMode: false,
    onDebugModeChange: vi.fn(),
    autoFile: false,
    onAutoFileChange: vi.fn(),
    autoStart: false,
    onAutoStartChange: vi.fn(),
    minimizeToTray: false,
    onMinimizeToTrayChange: vi.fn(),
    chatAn: true,
    onChatAnChange: vi.fn(),
    chatTon: true,
    onChatTonChange: vi.fn(),
    ...over,
  };
  return { props, ...render(<SettingsPanel {...(props as never)} />) };
}

function schalter(name: RegExp): HTMLInputElement {
  return screen.getByRole("checkbox", { name }) as HTMLInputElement;
}

describe("Einstellungen — Pilotenchat", () => {
  it("hat einen Schalter für den Chat, so wie es die Datenschutzseite zusagt", () => {
    const { props } = baueEinstellungen();
    const s = schalter(/Pilotenchat verwenden/);
    expect(s.checked).toBe(true);
    fireEvent.click(s);
    expect(props.onChatAnChange).toHaveBeenCalledWith(false);
  });

  it("hat einen eigenen Schalter für den Ton", () => {
    const { props } = baueEinstellungen();
    fireEvent.click(schalter(/Ton bei neuen Nachrichten/));
    expect(props.onChatTonChange).toHaveBeenCalledWith(false);
  });

  it("sperrt den Ton-Schalter, wenn der Chat ganz aus ist", () => {
    baueEinstellungen({ chatAn: false });
    expect(schalter(/Ton bei neuen Nachrichten/).disabled).toBe(true);
  });
});
