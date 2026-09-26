import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";
import type { Eintrag, Punkt } from "../../lib/bordbuch";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

const punkt = (regel: Punkt["regel"], schalter: Punkt["schalter"], abschnitt: Punkt["abschnitt"], status: Punkt["status"], extra: Partial<Punkt> = {}): Punkt => ({
  regel, schalter, abschnitt, art: "pflicht", status, auto_status: status,
  zeit: "2026-09-26T09:41:00Z", hoehe_ft: 364, stellung: null, grund: null, markiert_at: null, ...extra,
});

const EINTRAG: Eintrag = {
  schema: 1, pirep_id: "DLH1156", erstellt_at: "2026-09-26T09:30:00Z", updated_at: "2026-09-26T12:00:00Z",
  client_version: "1.9.0", flug: { callsign: "DLH1156", dep: "EDDF", arr: "LEPA", muster: "A20N", titel: "Fenix A320" },
  klasse: "airliner", klasse_quelle: "profil", regelwerk: "ifr", nacht_start: false, nacht_landung: false,
  zeitquelle: "sim", eingeschaltet: ["beacon", "strobes", "transponder", "apu", "spoiler"], aus_grund: null,
  punkte: [
    punkt("beacon_anlassen", "beacon", "vor_dem_rollen", "erledigt"),
    punkt("strobes_start", "strobes", "start", "diesmal_ohne"),
    punkt("transponder_start", "transponder", "start", "erledigt", { stellung: "TA-RA" }),
    punkt("tcas_start", "transponder", "start", "nicht_messbar", { grund: "wert_fehlt" }),
    punkt("apu_reiseflug", "apu", "reiseflug", "diesmal_ohne", { art: "bestaetigung" }),
    punkt("spoiler_landung", "spoiler", "anflug", "erledigt", { stellung: "ARMED" }),
  ],
  rollen_max_abflug_kt: 22, rollen_max_ankunft_kt: 18, rolltempo_grenze_kt: 30, profil: [],
};

const h = vi.hoisted(() => ({ calls: [] as Array<{ cmd: string; args?: Record<string, unknown> }>, eintrag: null as unknown }));

vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.calls.push({ cmd, args });
    if (cmd === "bordbuch_liste") return Promise.resolve([h.eintrag]);
    if (cmd === "bordbuch_live") return Promise.resolve(null);
    if (cmd === "bordbuch_eintrag") return Promise.resolve(h.eintrag);
    if (cmd === "bordbuch_markieren") {
      const e = structuredClone(h.eintrag) as Eintrag;
      const p = e.punkte.find((x) => x.regel === args?.regel)!;
      p.status = args?.nachAtc ? "nach_atc" : "diesmal_ohne";
      h.eintrag = e;
      return Promise.resolve(e);
    }
    return Promise.resolve(null);
  },
  listen: () => Promise.resolve(() => undefined),
}));

import { BordbuchView } from "./BordbuchView";

beforeEach(() => {
  h.calls = [];
  h.eintrag = structuredClone(EINTRAG);
  try { localStorage.removeItem("aeroacars.bordbuch.variante"); } catch { /* egal */ }
});
afterEach(() => cleanup());

describe("Bordbuch-Seite", () => {
  it("Liste → Checkliste: Bilanz, Stellungen, nicht messbar mit Flugzeugname, keine offene Bestätigung", async () => {
    render(<BordbuchView />);
    fireEvent.click(await screen.findByText("DLH1156"));
    // Pflicht: Beacon, Strobes, Transponder, Spoiler zählen (TCAS nicht messbar) → 3 von 4.
    expect(await screen.findByText("3 von 4 Punkten erledigt")).toBeTruthy();
    expect(screen.getByText("TA-RA")).toBeTruthy();
    expect(screen.getByText("ARMED")).toBeTruthy();
    expect(screen.getByText(/Fenix A320 meldet den Transponder-Modus nicht an AeroACARS/)).toBeTruthy();
    // APU ist Bestätigung und nicht erledigt → erscheint gar nicht.
    expect(screen.queryByText("APU")).toBeNull();
    // Kein Tadel-Vokabular.
    expect(document.body.textContent).not.toMatch(/Fehler|Verstoß|vergessen/);
  });

  it("grauen Punkt antippen → ‚Nach ATC-Anweisung' → zählt als erledigt", async () => {
    render(<BordbuchView />);
    fireEvent.click(await screen.findByText("DLH1156"));
    await screen.findByText("3 von 4 Punkten erledigt");
    fireEvent.click(screen.getByText("STROBES"));
    fireEvent.click(screen.getByText("Nach ATC-Anweisung"));
    await waitFor(() => expect(screen.getByText("4 von 4 Punkten erledigt")).toBeTruthy());
    const m = h.calls.find((c) => c.cmd === "bordbuch_markieren");
    expect(m?.args).toEqual({ pirepId: "DLH1156", regel: "strobes_start", nachAtc: true });
  });

  it("erledigte Punkte lassen sich nicht antippen", async () => {
    render(<BordbuchView />);
    fireEvent.click(await screen.findByText("DLH1156"));
    await screen.findByText("3 von 4 Punkten erledigt");
    const knopf = screen.getByText("BEACON").closest("button")!;
    expect(knopf.hasAttribute("disabled")).toBe(true);
  });

  it("Flugprofil: Legende und Hinweis, auch ohne Höhenprofil kein Absturz", async () => {
    render(<BordbuchView />);
    fireEvent.click(await screen.findByText("DLH1156"));
    fireEvent.click(await screen.findByText("Flugprofil"));
    expect(await screen.findByText(/Tippe einen Punkt an|noch kein Höhenprofil/)).toBeTruthy();
  });
});
