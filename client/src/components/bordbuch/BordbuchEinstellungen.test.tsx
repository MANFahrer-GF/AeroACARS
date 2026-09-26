import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
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
vi.mock("../../lib/ipc", () => ({
  invoke: () => Promise.resolve({
    schema: 1, updated_at: null, hinweise_im_flug: true, ga_an: false, vfr_an: false,
    regeln: {}, rolltempo_kt: 30, rolltempo_ga_kt: 20, klassen_override: {},
  }),
}));
import { BordbuchEinstellungen } from "./BordbuchEinstellungen";
afterEach(() => cleanup());

describe("Bordbuch-Einstellungen", () => {
  it("die kt-Grenzen stehen direkt unter „Rolltempo im Rahmen“, nicht unter APU", async () => {
    render(<BordbuchEinstellungen />);
    const rollen = await screen.findByText("Rolltempo im Rahmen");
    const grenze = screen.getByText("Grenze für das Rolltempo:");
    const parkbremse = screen.getByText("Parkbremse vor dem Rollen gelöst");
    const apu = screen.getByText("APU im Reiseflug aus (nur mit APU)");
    const nachher = (a: Element, b: Element) =>
      !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(nachher(rollen, grenze)).toBe(true);
    expect(nachher(grenze, parkbremse)).toBe(true);
    expect(nachher(grenze, apu)).toBe(true);
  });
});
