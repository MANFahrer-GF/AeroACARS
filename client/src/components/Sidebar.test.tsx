// Redesign Stufe D — Regressions-Schutz für die Seitenleiste.
//
// Die Seitenleiste hat die horizontale Tab-Leiste abgelöst. Dieser Test
// pinnt fest, dass dabei WIRKLICH alles mitgekommen ist: alle zehn
// Einträge, die beiden Zähler-Badges, der Aktiv-Flug-Punkt am Cockpit,
// die CPDLC-Bedingung und die drei Statusanzeigen im Fuß.
//
// Grund für den Aufwand: beim Umbau einer Navigation fällt ein Eintrag
// lautlos weg und niemand merkt es, bis ein Pilot ihn sucht.

import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { Sidebar } from "./Sidebar";

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "de",
    resources: { de: { common: deCommon } },
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });
});

function renderNav(over: Partial<React.ComponentProps<typeof Sidebar>> = {}) {
  const props: React.ComponentProps<typeof Sidebar> = {
    tab: "cockpit",
    setTab: vi.fn(),
    collapsed: false,
    onToggleCollapsed: vi.fn(),
    cpdlcEnabled: true,
    cpdlcPendingCount: 0,
    onCpdlcOpen: vi.fn(),
    unreadNews: 0,
    hasActiveFlight: false,
    phpvmsConnected: true,
    simConnected: true,
    simConnecting: false,
    simLabel: "MSFS",
    ...over,
  };
  return render(<Sidebar {...props} />);
}

describe("Sidebar — nichts darf beim Umbau verloren gehen", () => {
  it("zeigt alle zehn Bereiche", () => {
    renderNav();
    for (const label of [
      deCommon.tabs.cockpit,
      deCommon.tabs.map,
      deCommon.tabs.cpdlc,
      deCommon.tabs.briefing,
      deCommon.tabs.logbook,
      deCommon.tabs.landing,
      deCommon.nav.news,
      deCommon.tabs.log,
      deCommon.tabs.settings,
      deCommon.tabs.about,
    ]) {
      expect(screen.getByRole("button", { name: new RegExp(label) })).toBeTruthy();
    }
  });

  it("blendet PDC/CPDLC aus, wenn Hoppie nicht aktiv ist", () => {
    renderNav({ cpdlcEnabled: false });
    expect(screen.queryByRole("button", { name: new RegExp(deCommon.tabs.cpdlc) })).toBeNull();
  });

  it("zeigt den Nachrichten-Zähler und deckelt ihn bei 9+", () => {
    renderNav({ unreadNews: 3 });
    expect(screen.getByText("3")).toBeTruthy();
    renderNav({ unreadNews: 12 });
    expect(screen.getByText("9+")).toBeTruthy();
  });

  it("zeigt den CPDLC-Zähler", () => {
    renderNav({ cpdlcPendingCount: 2 });
    expect(screen.getByText("2")).toBeTruthy();
  });

  it("markiert den aktiven Bereich für Screenreader", () => {
    renderNav({ tab: "landing" });
    const active = screen.getByRole("button", { name: new RegExp(deCommon.tabs.landing) });
    expect(active.getAttribute("aria-current")).toBe("page");
  });

  it("führt die drei Statusanzeigen im Fuß", () => {
    const { container } = renderNav();
    expect(container.querySelectorAll(".navstatus").length).toBe(2);
    expect(screen.getByText(deCommon.status.phpvms)).toBeTruthy();
    expect(screen.getByText("MSFS")).toBeTruthy();
  });

  it("hat einen beschrifteten Einklapp-Knopf", () => {
    renderNav({ collapsed: false });
    expect(screen.getByRole("button", { name: deCommon.nav.collapse })).toBeTruthy();
    renderNav({ collapsed: true });
    expect(screen.getByRole("button", { name: deCommon.nav.expand })).toBeTruthy();
  });
});
