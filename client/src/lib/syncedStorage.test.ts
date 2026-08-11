// v1.5.6 (#lan-bruecke-1zu1): Der Vertrag des gespiegelten Speichers.
//
// Feldbefund: Über die LAN-Brücke ist das Tablet ein anderer Browser mit
// eigenem Speicher — SimBrief-Felder leer, alle Nachrichten "ungelesen".
// Diese Tests halten fest, was die Spiegelung leisten muss und wo sie
// bewusst NICHT greift.
import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import {
  SYNCED_KEYS,
  syncedSet,
  syncedRemove,
  syncedGet,
  hydrateSyncedStorage,
} from "./syncedStorage";

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue({});
  localStorage.clear();
  sessionStorage.clear();
});

describe("syncedStorage", () => {
  it("mirrors the values that must match across devices", () => {
    // Genau die drei Befunde von Thomas + der Squawk-Merker.
    expect(SYNCED_KEYS).toContain("simbrief_username");
    expect(SYNCED_KEYS).toContain("simbrief_user_id");
    expect(SYNCED_KEYS).toContain("aeroacars.readNewsIds");
    expect(SYNCED_KEYS).toContain("session:aeroacars.transponder.squawk_memo");
  });

  it("writes locally AND to the host", () => {
    syncedSet("simbrief_username", "thomas");
    expect(localStorage.getItem("simbrief_username")).toBe("thomas");
    expect(invokeMock).toHaveBeenCalledWith("ui_state_set", {
      key: "simbrief_username",
      value: "thomas",
    });
  });

  it("propagates deletions", () => {
    localStorage.setItem("simbrief_user_id", "12345");
    syncedRemove("simbrief_user_id");
    expect(localStorage.getItem("simbrief_user_id")).toBeNull();
    expect(invokeMock).toHaveBeenCalledWith("ui_state_set", {
      key: "simbrief_user_id",
      value: null,
    });
  });

  it("keeps session keys in sessionStorage, not localStorage", () => {
    // Der Transponder-Merker darf den Programmstart NICHT überleben —
    // geteilt ja, haltbar nein.
    const k = "session:aeroacars.transponder.squawk_memo";
    syncedSet(k, "7000");
    expect(sessionStorage.getItem(k)).toBe("7000");
    expect(localStorage.getItem(k)).toBeNull();
    expect(syncedGet(k)).toBe("7000");
  });

  it("does not call the host for unmirrored view preferences", () => {
    // Kartenhintergrund & Co. dürfen pro Gerät verschieden bleiben.
    syncedSet("aaLivemapBasemap", "satellite");
    expect(localStorage.getItem("aaLivemapBasemap")).toBe("satellite");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("seeds local values up, then adopts the host's answer", async () => {
    localStorage.setItem("simbrief_username", "lokal");
    invokeMock.mockResolvedValue({
      simbrief_username: "vom-host",
      "aeroacars.readNewsIds": "[1,2,3]",
    });

    const ok = await hydrateSyncedStorage();

    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("ui_state_seed", {
      values: { simbrief_username: "lokal" },
    });
    // Host gewinnt — und Schlüssel, die er nicht kennt, werden lokal geleert.
    expect(localStorage.getItem("simbrief_username")).toBe("vom-host");
    expect(localStorage.getItem("aeroacars.readNewsIds")).toBe("[1,2,3]");
    expect(localStorage.getItem("simbrief_user_id")).toBeNull();
  });

  it("leaves the local state untouched when the host is unreachable", async () => {
    localStorage.setItem("simbrief_username", "lokal");
    invokeMock.mockRejectedValue(new Error("offline"));

    const ok = await hydrateSyncedStorage();

    expect(ok).toBe(false);
    expect(localStorage.getItem("simbrief_username")).toBe("lokal");
  });
});
