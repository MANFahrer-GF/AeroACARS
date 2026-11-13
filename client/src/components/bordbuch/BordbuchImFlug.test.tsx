import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";
import type { Punkt } from "../../lib/bordbuch";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de", resources: { de: { common: deCommon } }, defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

const h = vi.hoisted(() => ({ live: null as unknown, aufrufe: [] as string[] }));
vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string) => {
    h.aufrufe.push(cmd);
    return Promise.resolve(h.live);
  },
  listen: () => Promise.resolve(() => undefined),
}));

import { BordbuchImFlug, BORDBUCH_IM_FLUG_OFFEN_KEY } from "./BordbuchImFlug";

const pk = (regel: string, schalter: string, status: Punkt["status"]): Punkt => ({
  regel, schalter, abschnitt: "vor_dem_rollen", art: "pflicht", status, auto_status: status,
  zeit: status === "erledigt" ? "2026-11-13T10:00:00Z" : null, hoehe_ft: 16, stellung: null,
  grund: null, markiert_at: null,
} as unknown as Punkt);

const live = (extra: Record<string, unknown> = {}) => ({
  klasse: "airliner", nacht_start: false, eingeschaltet: ["beacon", "nav"], aus_grund: null,
  punkte: [pk("beacon_anlassen", "beacon", "erledigt"), pk("nav_lichter", "nav", "diesmal_ohne")],
  hinweis: null, profil: [], rolltempo_grenze_kt: 30, hinweise_im_flug: true, ...extra,
});

beforeEach(() => {
  localStorage.clear();
  h.aufrufe = [];
});
afterEach(() => cleanup());

describe("Bordbuch im Reiter „Flug“", () => {
  it("ist standardmäßig offen: Stand im Kopf, Checkliste darunter", async () => {
    h.live = live();
    render(<BordbuchImFlug aktiv />);
    const kasten = (await screen.findByTestId("flug-bordbuch")) as HTMLDetailsElement;
    expect(kasten.open).toBe(true);
    expect(screen.getByText("1 von 2 Punkten erledigt")).toBeTruthy();
    expect(kasten.querySelector(".bb-checkliste")).not.toBeNull();
    expect(h.aufrufe).toContain("bordbuch_live");
  });

  it("lässt sich zuklappen, merkt sich das, und zeigt den Stand auch zugeklappt", async () => {
    h.live = live();
    const { unmount } = render(<BordbuchImFlug aktiv />);
    const kasten = (await screen.findByTestId("flug-bordbuch")) as HTMLDetailsElement;
    kasten.open = false;
    fireEvent(kasten, new Event("toggle"));
    expect(localStorage.getItem(BORDBUCH_IM_FLUG_OFFEN_KEY)).toBe("0");
    expect(kasten.querySelector(".bb-checkliste")).toBeNull();
    unmount();

    render(<BordbuchImFlug aktiv />);
    const wieder = (await screen.findByTestId("flug-bordbuch")) as HTMLDetailsElement;
    expect(wieder.open).toBe(false);
    expect(screen.getByText("1 von 2 Punkten erledigt")).toBeTruthy();
    expect(wieder.querySelector(".bb-checkliste")).toBeNull();
  });

  it("zeigt nichts, wenn das Bordbuch für den Flug aus ist", async () => {
    h.live = live({ aus_grund: "ga" });
    const { container } = render(<BordbuchImFlug aktiv />);
    await act(async () => {}); // Antwort verarbeiten lassen, sonst ist der Kasten nur „noch nicht da“
    expect(h.aufrufe).toContain("bordbuch_live");
    expect(container.innerHTML).toBe("");
  });

  it("zeigt keinen leeren Kasten, solange kein Punkt sichtbar ist", async () => {
    h.live = live({ eingeschaltet: [] });
    const { container } = render(<BordbuchImFlug aktiv />);
    await act(async () => {});
    expect(h.aufrufe).toContain("bordbuch_live");
    expect(container.innerHTML).toBe("");
  });

  it("Gegenprobe: dieselbe Antwort ohne Abschaltgrund zeigt den Kasten nach demselben Warten", async () => {
    h.live = live();
    render(<BordbuchImFlug aktiv />);
    await act(async () => {});
    expect(screen.getByTestId("flug-bordbuch")).toBeTruthy();
  });

  it("fragt nicht ab und zeigt nichts, solange der Flug gerade wieder aufgenommen wird", async () => {
    h.live = live();
    const { container } = render(<BordbuchImFlug aktiv={false} />);
    await Promise.resolve();
    expect(h.aufrufe).toEqual([]);
    expect(container.innerHTML).toBe("");
  });
});
