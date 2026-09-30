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

function bauen() {
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
  };
  render(<SettingsPanel {...(props as never)} />);
  return screen.getByRole("combobox", { name: /Aktiver Simulator/ }) as HTMLSelectElement;
}

describe("Simulator-Auswahl", () => {
  it("zeigt „Automatisch“ und den erkannten Simulator", async () => {
    auswahl = { automatisch: true, kind: "xplane12" };
    const select = bauen();
    await waitFor(() => expect(select.value).toBe("auto"));
    expect(screen.getByText(/Automatisch eingestellt: X-Plane 12/)).toBeTruthy();
  });

  it("Handwahl bleibt Handwahl — ohne Hinweis auf die Automatik", async () => {
    auswahl = { automatisch: false, kind: "msfs2024" };
    const select = bauen();
    await waitFor(() => expect(select.value).toBe("msfs2024"));
    expect(screen.queryByText(/Automatisch eingestellt/)).toBeNull();
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
