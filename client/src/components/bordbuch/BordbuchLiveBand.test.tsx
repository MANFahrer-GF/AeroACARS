import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({ lng: "de", resources: { de: { common: deCommon } }, defaultNS: "common", interpolation: { escapeValue: false } });
  }
});

const h = vi.hoisted(() => ({ live: null as unknown }));
vi.mock("../../lib/ipc", () => ({
  invoke: () => Promise.resolve(h.live),
  listen: () => Promise.resolve(() => undefined),
}));

import { BordbuchLiveBand } from "./BordbuchLiveBand";

const punkt = { regel: "beacon_anlassen", schalter: "beacon", abschnitt: "vor_dem_rollen", art: "pflicht", status: "erledigt", auto_status: "erledigt", zeit: "2026-11-10T09:41:00Z", hoehe_ft: 300, stellung: "ON", grund: null, markiert_at: null };
const LIVE = { klasse: "airliner", nacht_start: false, eingeschaltet: ["beacon"], aus_grund: null, punkte: [punkt], hinweis: null, profil: [], rolltempo_grenze_kt: 30, hinweise_im_flug: true };
const FLUG = { kennung: "DLH 767", dep: "VABB", arr: "EDDM", phase: "Reiseflug", flugzeug: "A359 · D-AIXA" };

afterEach(() => { cleanup(); h.live = null; });

describe("Laufender Flug im Logbuch", () => {
  it("ohne Flug und ohne Bordbuch steht nichts da", async () => {
    const r = render(<BordbuchLiveBand flug={null} />);
    await Promise.resolve();
    expect(r.container.textContent).toBe("");
  });

  it("zeigt Flug und Stand; ‚Live ansehen' klappt die Checkliste ohne Markieren auf", async () => {
    h.live = LIVE;
    render(<BordbuchLiveBand flug={FLUG} />);
    expect(await screen.findByText("Bordbuch bisher 1 von 1 erledigt")).toBeTruthy();
    expect(screen.getByText("DLH 767")).toBeTruthy();
    expect(screen.queryByText("BEACON")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Live ansehen" }));
    expect(await screen.findByText("BEACON")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Auf Karte" })).toBeNull();
    expect(screen.getByRole("button", { name: "Live zuklappen" })).toBeTruthy();
  });

  it("ausgeschaltetes Bordbuch (GA): Hinweis statt Knopf", async () => {
    h.live = { ...LIVE, aus_grund: "ga" };
    render(<BordbuchLiveBand flug={FLUG} />);
    await screen.findByText("DLH 767");
    await screen.findByText((_, e) => e?.className === "bb-aus-hinweis");
    expect(screen.queryByRole("button", { name: "Live ansehen" })).toBeNull();
  });
});
