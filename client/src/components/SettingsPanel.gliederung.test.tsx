// Gliederung der Einstellungen (17.10.2026): „Komfort“ war mit elf
// Abschnitten zu lang und wurde auf Komfort, Cockpit und Technik verteilt.
// Festgehalten wird, dass jeder Abschnitt genau einen Reiter hat und beim
// Umbau keiner verloren ging (Feedback: bei Redesign nichts entfernen).

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
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

function baueEinstellungen(reiter: string, over: Record<string, unknown> = {}) {
  localStorage.setItem("aeroacars.settings.activeTab", reiter);
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

afterEach(cleanup);

function ueberschriften(reiter: string): string[] {
  baueEinstellungen(reiter);
  const h = screen.queryAllByRole("heading", { level: 3 }).map((e) => e.textContent ?? "");
  cleanup();
  return h;
}

const t = (k: string) => k.split(".").reduce<unknown>((o, s) => (o as Record<string, unknown>)[s], deCommon) as string;

describe("Einstellungen — Gliederung", () => {
  const erwartet: Record<string, string[]> = {
    extras: [t("settings.appearance_section"), "Pilotenchat", t("behaviour.section_title"), t("discord_rpc.section_title")],
    cockpit: [
      t("hoppie_settings.section_title"),
      t("kabine.einstellungen.titel"),
      t("msfs_hud.section_title"),
      t("xplane_band.section_title"),
    ],
    tech: [t("settings.storage_section"), t("error_reporting.section_title"), t("settings.developer_section")],
  };

  for (const [reiter, titel] of Object.entries(erwartet)) {
    it(`Reiter ${reiter} zeigt seine Abschnitte`, () => {
      const h = ueberschriften(reiter);
      for (const x of titel) expect(h, `${x} fehlt in ${reiter}`).toContain(x);
    });
  }

  it("jeder Abschnitt steht in genau einem Reiter", () => {
    const alle = Object.keys(erwartet).flatMap((r) => ueberschriften(r).map((h) => `${r}:${h}`));
    for (const titel of Object.values(erwartet).flat()) {
      const fund = alle.filter((x) => x.endsWith(`:${titel}`));
      expect(fund, titel).toHaveLength(1);
    }
  });

  it("der Reiter Cockpit steht in der Leiste", () => {
    baueEinstellungen("simulator");
    expect(screen.getByRole("tab", { name: t("settings.tabs.cockpit") })).toBeTruthy();
  });
});
