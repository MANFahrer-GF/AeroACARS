import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import type { Eintrag, Punkt } from "../lib/bordbuch";

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
    punkt("tcas_start", "transponder", "start", "nicht_messbar", { grund: "wert_fehlt", zeit: null }),
    punkt("apu_reiseflug", "apu", "reiseflug", "diesmal_ohne", { art: "bestaetigung" }),
    punkt("spoiler_landung", "spoiler", "anflug", "erledigt", { stellung: "ARMED" }),
  ],
  rollen_max_abflug_kt: 22, rollen_max_ankunft_kt: 18, rolltempo_grenze_kt: 30, profil: [],
};

type FlaecheProps = {
  zusatz?: { zusatzEreignisse?: Array<{ schluessel: string; txt: string }> };
  onBereit?: (api: { zeigeEreignis: (s: string) => boolean }) => void;
  onOhneAkte?: (g: { art: "fehlt" }) => void;
};

const h = vi.hoisted(() => ({
  calls: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
  eintrag: null as unknown,
  landung: {} as unknown,
  flaeche: null as null | FlaecheProps,
  gesprungen: [] as string[],
  ohneAkte: false,
}));

vi.mock("../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.calls.push({ cmd, args });
    if (cmd === "bordbuch_eintrag") return Promise.resolve(h.eintrag);
    if (cmd === "landing_get") return Promise.resolve(h.landung);
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
  isTauri: true,
}));
vi.mock("maplibre-gl", () => {
  class FakeMap { on() { return this; } remove() {} }
  return { default: { Map: FakeMap, Marker: class {}, LngLatBounds: class {} } };
});
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));
vi.mock("./PirepPruefstatus", () => ({ PruefstatusKasten: () => null, PruefstatusMarke: () => null, pruefArt: () => "unbekannt" }));
vi.mock("./UploadAusstehend", () => ({ UploadKasten: () => null }));
vi.mock("../flugkarte/FlugkarteFenster", () => ({ default: () => <div data-testid="vollbild" /> }));
vi.mock("../flugkarte/FlugkarteFlaeche", async () => {
  const { useEffect } = await import("react");
  return {
    default: (p: FlaecheProps) => {
      h.flaeche = p;
      useEffect(() => {
        if (h.ohneAkte) p.onOhneAkte?.({ art: "fehlt" });
        else p.onBereit?.({ zeigeEreignis: (s) => { h.gesprungen.push(s); return true; } });
        // eslint-disable-next-line react-hooks/exhaustive-deps
      }, []);
      return <div data-testid="flaeche" />;
    },
  };
});

import { LogbuchFlug } from "./LogbuchFlug";

const DETAIL = { id: "DLH1156", callsign: "DLH1156", dep_icao: "EDDF", arr_icao: "LEPA", duration_min: 130, distance_nm: 800, landing_rate_fpm: -180, status: "accepted", route: [], log: [{ t: 60000, message: "Phase: Pushback" }] };

function zeigen(onLandung?: (id: string) => void) {
  return render(<LogbuchFlug detail={DETAIL} pruefstatus={undefined} uploads={undefined} onZurueck={() => undefined} onLandungAnsehen={onLandung} />);
}

beforeEach(() => {
  h.calls = []; h.eintrag = structuredClone(EINTRAG); h.landung = {}; h.flaeche = null; h.gesprungen = []; h.ohneAkte = false;
});
afterEach(() => cleanup());

const reiter = (name: RegExp) => screen.getByRole("tab", { name });

describe("Geöffneter Flug im Logbuch", () => {
  it("startet auf der Karte; der Reiter Bordbuch trägt die Bilanz", async () => {
    zeigen();
    expect(await screen.findByTestId("flaeche")).toBeTruthy();
    expect(reiter(/^Karte$/).getAttribute("aria-selected")).toBe("true");
    expect(reiter(/^Bordbuch 3 von 4$/)).toBeTruthy();
  });

  it("gibt der Karte die Bordbuch-Punkte mit Zeit mit, ohne die unsichtbaren und die ohne Zeit", async () => {
    zeigen();
    await screen.findByTestId("flaeche");
    const namen = h.flaeche!.zusatz!.zusatzEreignisse!.map((e) => e.schluessel);
    expect(namen).toEqual(["beacon_anlassen", "strobes_start", "transponder_start", "spoiler_landung"]);
  });

  it("Checkliste: Stellungen, nicht messbar mit Flugzeugname, keine offene Bestätigung, kein Tadel", async () => {
    zeigen();
    fireEvent.click(await screen.findByRole("tab", { name: /^Bordbuch/ }));
    expect(await screen.findByText(/3 von 4 Punkten erledigt/)).toBeTruthy();
    expect(screen.getByText("TA-RA")).toBeTruthy();
    expect(screen.getByText("ARMED")).toBeTruthy();
    expect(screen.getByText(/Fenix A320 meldet den Transponder-Modus nicht an AeroACARS/)).toBeTruthy();
    expect(screen.queryByText("APU")).toBeNull();
    expect(document.body.textContent).not.toMatch(/Fehler|Verstoß|vergessen/);
  });

  it("„Auf Karte“ wechselt zur Karte und springt zum Punkt", async () => {
    zeigen();
    await screen.findByTestId("flaeche");
    fireEvent.click(await screen.findByRole("tab", { name: /^Bordbuch/ }));
    const links = await screen.findAllByRole("button", { name: "Auf Karte" });
    expect(links).toHaveLength(3); // erledigt mit Zeit, nicht der graue Punkt
    fireEvent.click(links[0]);
    expect(reiter(/^Karte$/).getAttribute("aria-selected")).toBe("true");
    expect(h.gesprungen).toEqual(["beacon_anlassen"]);
  });

  it("grauen Punkt antippen → ‚Nach ATC-Anweisung' → zählt, Bilanz im Reiter zieht nach", async () => {
    zeigen();
    fireEvent.click(await screen.findByRole("tab", { name: /^Bordbuch/ }));
    await screen.findByText(/3 von 4 Punkten erledigt/);
    fireEvent.click(screen.getByText("STROBES"));
    fireEvent.click(screen.getByText("Nach ATC-Anweisung"));
    await waitFor(() => expect(reiter(/^Bordbuch 4 von 4$/)).toBeTruthy());
    expect(h.calls.find((c) => c.cmd === "bordbuch_markieren")?.args).toEqual({ pirepId: "DLH1156", regel: "strobes_start", nachAtc: true });
  });

  it("erledigte Punkte lassen sich nicht antippen", async () => {
    zeigen();
    fireEvent.click(await screen.findByRole("tab", { name: /^Bordbuch/ }));
    await screen.findByText(/3 von 4 Punkten erledigt/);
    expect(screen.getByText("BEACON").closest("button")!.hasAttribute("disabled")).toBe(true);
  });

  it("ohne Aufzeichnung: Hinweis und Linie, kein ‚Auf Karte', kein Vollbild", async () => {
    h.ohneAkte = true;
    zeigen();
    expect(await screen.findByText("Für diesen Flug liegt keine Aufzeichnung vor")).toBeTruthy();
    expect(screen.queryByTestId("flaeche")).toBeNull();
    expect(screen.queryByRole("button", { name: "Vollbild" })).toBeNull();
    fireEvent.click(await screen.findByRole("tab", { name: /^Bordbuch/ }));
    await screen.findByText(/3 von 4 Punkten erledigt/);
    expect(screen.queryByRole("button", { name: "Auf Karte" })).toBeNull();
  });

  it("ohne Bordbuch: eigener Hinweis, Reiter ohne Zahl", async () => {
    h.eintrag = null;
    zeigen();
    await screen.findByTestId("flaeche");
    fireEvent.click(reiter(/^Bordbuch$/));
    expect(await screen.findByText("Für diesen Flug gibt es kein Bordbuch.")).toBeTruthy();
    expect(h.flaeche!.zusatz).toBeUndefined();
  });

  it("„Landung ansehen“ springt mit diesem Flug; ohne Landung gesperrt", async () => {
    const landung = vi.fn();
    const r = zeigen(landung);
    const knopf = await screen.findByRole("button", { name: "Landung ansehen" });
    await waitFor(() => expect(knopf.hasAttribute("disabled")).toBe(false));
    fireEvent.click(knopf);
    expect(landung).toHaveBeenCalledWith("DLH1156");
    r.unmount();
    h.landung = null;
    zeigen(landung);
    await waitFor(() => expect(screen.getByRole("button", { name: "Landung ansehen" }).getAttribute("title")).toBeTruthy());
    expect(screen.getByRole("button", { name: "Landung ansehen" }).hasAttribute("disabled")).toBe(true);
  });

  it("Protokoll bleibt da", async () => {
    zeigen();
    fireEvent.click(reiter(/^Protokoll$/));
    expect(document.body.textContent).toContain("Phase: Pushback");
    expect(reiter(/^Protokoll$/).getAttribute("aria-selected")).toBe("true");
  });
});
