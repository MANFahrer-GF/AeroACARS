import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("./ipc", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { appVersion } from "./appVersion";

afterEach(() => {
  invoke.mockReset();
});

describe("Version des laufenden Clients (19.11.2026)", () => {
  it("fragt den Client über app_info (Tauri wie LAN-Brücke)", async () => {
    invoke.mockResolvedValue({ name: "AeroACARS", version: "3.1.3" });
    expect(await appVersion()).toBe("3.1.3");
    expect(invoke).toHaveBeenCalledWith("app_info");
  });

  it("ohne Antwort oder mit leerem Wert: 0.0.0", async () => {
    invoke.mockRejectedValue(new Error("nicht angemeldet"));
    expect(await appVersion()).toBe("0.0.0");
    invoke.mockResolvedValue({ version: " " });
    expect(await appVersion()).toBe("0.0.0");
    invoke.mockResolvedValue(null);
    expect(await appVersion()).toBe("0.0.0");
  });
});
