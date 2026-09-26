import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de", resources: { de: { common: deCommon } }, defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

const h = vi.hoisted(() => ({ live: null as unknown }));
vi.mock("../../lib/ipc", () => ({
  invoke: () => Promise.resolve(h.live),
}));

import { BordbuchHinweis } from "./BordbuchHinweis";

afterEach(() => cleanup());

const live = (hinweis: string | null, an = true) => ({
  klasse: "airliner", nacht_start: false, eingeschaltet: ["strobes"], aus_grund: null,
  punkte: [], hinweis: hinweis ? { regel: hinweis, seit: "2026-09-26T10:00:00Z" } : null,
  profil: [], rolltempo_grenze_kt: 30, hinweise_im_flug: an,
});

describe("Bordbuch-Hinweis im Cockpit", () => {
  it("zeigt die Frage, solange das Backend sie liefert", async () => {
    h.live = live("strobes_start");
    render(<BordbuchHinweis aktiv />);
    expect(await screen.findByText("Strobes?")).toBeTruthy();
  });

  it("ohne Hinweis nichts — auch nicht, wenn inaktiv", async () => {
    h.live = live(null);
    const { container } = render(<BordbuchHinweis aktiv />);
    await act(async () => { await Promise.resolve(); });
    expect(container.textContent).toBe("");
    h.live = live("strobes_start");
    const r2 = render(<BordbuchHinweis aktiv={false} />);
    await act(async () => { await Promise.resolve(); });
    expect(r2.container.textContent).toBe("");
  });

  it("Bordbuch aus (GA/VFR) → keine Frage", async () => {
    h.live = { ...live("strobes_start"), aus_grund: "ga" };
    const { container } = render(<BordbuchHinweis aktiv />);
    await act(async () => { await Promise.resolve(); });
    expect(container.textContent).toBe("");
  });
});
