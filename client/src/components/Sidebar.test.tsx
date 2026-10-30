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
import { render, screen, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import { Sidebar, reiterAusSeite } from "./Sidebar";

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
    tab: "flug",
    setTab: vi.fn(),
    collapsed: false,
    onToggleCollapsed: vi.fn(),
    cpdlcEnabled: true,
    chatAn: true,
    chatUngelesen: 0,
    onChatOpen: vi.fn(),
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
  it("zeigt alle Bereiche", () => {
    renderNav();
    for (const label of [
      deCommon.tabs.flug,
      deCommon.tabs.map,
      deCommon.tabs.cpdlc,
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

  // Die Datenschutzseite sagt zu, dass der Chat abschaltbar ist. Aus muss
  // aus heissen — auch in der Navigation, nicht nur beim Empfang.
  it("blendet den Chat aus, wenn er abgeschaltet ist", () => {
    const an = renderNav({ chatAn: true });
    expect(within(an.container).getByRole("button", { name: new RegExp(deCommon.tabs.chat) })).toBeTruthy();
    const aus = renderNav({ chatAn: false });
    expect(within(aus.container).queryByRole("button", { name: new RegExp(deCommon.tabs.chat) })).toBeNull();
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

  // Beta 1 (Thomas): „Hilfe anfordern ist mir zu versteckt“ — eigener
  // Reiter direkt über „Über“, mit F1 im Titel.
  it("führt „Hilfe“ als eigenen Reiter direkt vor „Über“", () => {
    const setTab = vi.fn();
    const { container } = renderNav({ setTab, tab: "hilfe" });
    const knopf = screen.getByRole("button", { name: new RegExp(`^${deCommon.tabs.hilfe}$`) });
    expect(knopf.getAttribute("title")).toBe(deCommon.hilfe.kuerzel);
    expect(knopf.getAttribute("aria-current")).toBe("page");
    const liste = [...container.querySelectorAll(".nav__list .navitem")].map((b) => b.textContent);
    expect(liste.indexOf(deCommon.tabs.hilfe)).toBe(liste.indexOf(deCommon.tabs.about) - 1);
    expect(liste.indexOf(deCommon.tabs.hilfe)).toBe(liste.indexOf(deCommon.tabs.settings) + 1);
    knopf.click();
    expect(setTab).toHaveBeenCalledWith("hilfe");
  });

  // 3.0: Briefing und Cockpit sind EIN Reiter „Flug“ (FlugView).
  it("führt „Flug“ statt „Briefing“ und „Cockpit“ — als ersten Eintrag", () => {
    const setTab = vi.fn();
    const { container } = renderNav({ setTab, tab: "flug" });
    const liste = [...container.querySelectorAll(".nav__list .navitem")].map((b) => b.textContent);
    expect(liste[0]).toBe(deCommon.tabs.flug);
    expect(liste.some((l) => /^(Briefing|Cockpit)$/.test(l ?? ""))).toBe(false);
    const knopf = screen.getByRole("button", { name: new RegExp(`^${deCommon.tabs.flug}$`) });
    expect(knopf.getAttribute("aria-current")).toBe("page");
    knopf.click();
    expect(setTab).toHaveBeenCalledWith("flug");
  });

  it("zeigt den Aktiv-Flug-Punkt am Reiter „Flug“", () => {
    const ohne = renderNav({ hasActiveFlight: false });
    expect(ohne.container.querySelector(".navitem__dot")).toBeNull();
    const mit = renderNav({ hasActiveFlight: true });
    const punkt = mit.container.querySelector(".navitem__dot");
    expect(punkt?.closest(".navitem")?.textContent).toBe(deCommon.tabs.flug);
  });

  it("hat einen beschrifteten Einklapp-Knopf", () => {
    renderNav({ collapsed: false });
    expect(screen.getByRole("button", { name: deCommon.nav.collapse })).toBeTruthy();
    renderNav({ collapsed: true });
    expect(screen.getByRole("button", { name: deCommon.nav.expand })).toBeTruthy();
  });
});

// StreamDeck (`deck-seite`, DECK_SEITEN in panel_server.rs) und das Plugin
// senden noch die alten Seiten — sie müssen auf „Flug“ landen.
describe("reiterAusSeite — alte Seiten werden umgeleitet", () => {
  it("leitet cockpit und briefing auf flug", () => {
    expect(reiterAusSeite("cockpit")).toBe("flug");
    expect(reiterAusSeite("briefing")).toBe("flug");
  });
  it("lässt die übrigen Deck-Seiten durch", () => {
    for (const s of ["landing", "kabine", "map", "cpdlc", "flug"]) {
      expect(reiterAusSeite(s)).toBe(s);
    }
  });
  it("verwirft Unbekanntes statt eine leere Seite zu öffnen", () => {
    expect(reiterAusSeite("gibtsnicht")).toBeNull();
    expect(reiterAusSeite(undefined)).toBeNull();
    expect(reiterAusSeite(42)).toBeNull();
  });
});
