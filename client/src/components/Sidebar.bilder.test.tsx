// Reiter „Bilder“ nur mit gemeldeter Faehigkeit `bilder-1` (P-11.33): sonst kein Eintrag, und ein von
// aussen gesetzter Reiter faellt auf „Flug“ zurueck.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
const h = vi.hoisted(() => ({ tauri: true }));
vi.mock("../lib/ipc", () => ({ get isTauri() { return h.tauri; } }));
import { Sidebar, erlaubterReiter, reiterAusSeite } from "./Sidebar";

const basis = {
  tab: "flug" as const, setTab: () => undefined, collapsed: false, onToggleCollapsed: () => undefined, cpdlcEnabled: false, cpdlcPendingCount: 0,
  onCpdlcOpen: () => undefined, chatAn: false, chatUngelesen: 0, onChatOpen: () => undefined, unreadNews: 0, hasActiveFlight: false,
  phpvmsConnected: true, simConnected: true, simConnecting: false, simLabel: "X-Plane 12",
};

describe("Reiter Bilder", () => {
  it("ohne Faehigkeit kein Reiter, mit ihr einer", () => {
    const { rerender } = render(<Sidebar {...basis} />);
    expect(screen.queryByRole("button", { name: "Bilder" })).toBeNull();
    rerender(<Sidebar {...basis} bilderAn={true} />);
    expect(screen.getByRole("button", { name: "Bilder" })).toBeInTheDocument();
  });
  it("im LAN-Browser gibt es den Reiter genauso, Thomas 07.11.", () => {
    h.tauri = false;
    render(<Sidebar {...basis} bilderAn={true} />);
    expect(screen.getByRole("button", { name: "Bilder" })).toBeInTheDocument();
    h.tauri = true;
  });
  it("tab=bilder faellt ohne Faehigkeit auf Flug zurueck, mit ihr bleibt er", () => {
    expect(erlaubterReiter("bilder", false)).toBe("flug");
    expect(erlaubterReiter("bilder", true)).toBe("bilder");
    expect(erlaubterReiter("map", false)).toBe("map");
  });
  it("der Reiter ist von aussen (StreamDeck-Seite) erreichbar, wird aber ebenso zurueckgesetzt", () => {
    expect(reiterAusSeite("bilder")).toBe("bilder");
  });

  it("alter Reiter Bordbuch landet im Logbuch statt auf einer leeren Seite", () => {
    expect(reiterAusSeite("bordbuch")).toBe("logbook");
  });
});
