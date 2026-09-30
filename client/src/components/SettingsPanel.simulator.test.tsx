// Simulator-Auswahl mit „Automatisch“ (v1.9.14).
//
// Festgehalten wird, was der Pilot sieht und was beim Umstellen an den
// Client geht — nicht nur, dass es die Option gibt.

import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const aufrufe: Array<{ cmd: string; args?: unknown }> = [];
let auswahl = { automatisch: true, kind: "xplane12" };

vi.mock("../lib/ipc", () => ({
  invoke: vi.fn(async (cmd: string, args?: unknown) => {
    aufrufe.push({ cmd, args });
    if (cmd === "sim_get_auswahl") return auswahl;
    return null;
  }),
  listen: vi.fn(async () => () => {}),
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

beforeEach(() => {
  aufrufe.length = 0;
});

function bauen(simStatus: unknown = null) {
  localStorage.setItem("aeroacars.settings.activeTab", "simulator");
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
    simStatus,
  };
  render(<SettingsPanel {...(props as never)} />);
  return screen.getByRole("combobox", { name: /Aktiver Simulator/ }) as HTMLSelectElement;
}

describe("Simulator-Auswahl", () => {
  // Der Verbindungsstand steht gross unter der Auswahl — mit dem ECHTEN
  // Zustand, nicht nur „eingestellt“ (Feldbefund Thomas 30.09.2026).
  it("verbunden: Simulator und Flugzeug gross im Statusblock", async () => {
    auswahl = { automatisch: true, kind: "msfs2024" };
    const select = bauen({
      state: "connected",
      kind: "msfs2024",
      snapshot: { aircraft_title: "A380-800 RR Basic" },
      last_error: null,
      available: true,
    });
    await waitFor(() => expect(select.value).toBe("auto"));
    const block = screen.getByRole("status");
    expect(block.className).toContain("settings__sim-status--connected");
    expect(block.querySelector("strong")?.textContent).toBe("Verbunden mit MSFS 2024");
    expect(block.textContent).toContain("Flugzeug: A380-800 RR Basic");
    expect(block.textContent).toContain("Automatisch erkannt");
  });

  it("Automatik ohne laufenden Simulator sagt das deutlich", async () => {
    auswahl = { automatisch: true, kind: "msfs2024" };
    bauen({ state: "disconnected", kind: "msfs2024", snapshot: null, last_error: null, available: true });
    const block = await screen.findByRole("status");
    expect(block.querySelector("strong")?.textContent).toBe("Kein Simulator gefunden");
  });

  it("Automatik auf dem Mac: kein „bitte X-Plane wählen“, sondern „kein Simulator“", async () => {
    auswahl = { automatisch: true, kind: "msfs2024" };
    bauen({ state: "disconnected", kind: "msfs2024", snapshot: null, last_error: null, available: false });
    const block = await screen.findByRole("status");
    expect(block.querySelector("strong")?.textContent).toBe("Kein Simulator gefunden");
  });

  it("Handwahl MSFS auf dem Mac: nicht verfügbar", async () => {
    auswahl = { automatisch: false, kind: "msfs2024" };
    bauen({ state: "disconnected", kind: "msfs2024", snapshot: null, last_error: null, available: false });
    const block = await screen.findByRole("status");
    expect(block.querySelector("strong")?.textContent).toBe("MSFS 2024 ist auf diesem Rechner nicht verfügbar");
  });

  it("Handwahl: fest eingestellt, nicht verbunden", async () => {
    auswahl = { automatisch: false, kind: "xplane12" };
    const select = bauen({ state: "disconnected", kind: "xplane12", snapshot: null, last_error: null, available: true });
    await waitFor(() => expect(select.value).toBe("xplane12"));
    const block = screen.getByRole("status");
    expect(block.querySelector("strong")?.textContent).toBe("X-Plane 12 nicht verbunden");
    expect(block.textContent).toContain("Fest eingestellt");
    expect(block.textContent).not.toContain("Automatisch erkannt");
  });

  it("schickt beim Umstellen „auto“ bzw. den gewählten Simulator", async () => {
    auswahl = { automatisch: false, kind: "msfs2024" };
    const select = bauen();
    await waitFor(() => expect(select.value).toBe("msfs2024"));
    fireEvent.change(select, { target: { value: "auto" } });
    await waitFor(() =>
      expect(aufrufe).toContainEqual({ cmd: "sim_set_kind", args: { kind: "auto" } }),
    );
    await waitFor(() => expect(select.value).toBe("auto"));
    fireEvent.change(select, { target: { value: "xplane11" } });
    await waitFor(() =>
      expect(aufrufe).toContainEqual({ cmd: "sim_set_kind", args: { kind: "xplane11" } }),
    );
    expect(select.value).toBe("xplane11");
  });
});
