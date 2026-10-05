import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";
import { schritteFuer } from "./schritte";

const BODEN = schritteFuer("boden");

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

const h = vi.hoisted(() => ({
  aufrufe: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
  amBoden: true,
  stellungImSchritt: 0,
  ruheSpaet: false,
  scanNamen: 0,
  kind: "xplane" as string,
  titel: "Boeing 777-300ER",
  icao: "B77W",
  /** aircraft.cfg-Pfad (MSFS) bzw. UI-Name (X-Plane) im Schnappschuss. */
  pfad: null as string | null,
  uiName: null as string | null,
  liste: [] as Array<{ sim: string; icao: string; titel: string; zuletzt: number; anzahl: number }>,
  ruheLoesen: null as null | (() => void),
  beispiele: [] as Array<{ variable: string; werte: Array<number | null>; texte?: Array<string | null> }>,
}));

vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.aufrufe.push({ cmd, args });
    switch (cmd) {
      case "sim_status":
        return Promise.resolve({
          kind: h.kind,
          snapshot: {
            on_ground: h.amBoden,
            aircraft_title: h.titel,
            aircraft_icao: h.icao,
            ...(h.uiName ? { aircraft_ui_name: h.uiName } : {}),
            ...(h.pfad ? { cockpit_rohwerte: { cfg_pfad: h.pfad } } : {}),
          },
        });
      case "vermessung_starten":
        return Promise.resolve({ sim: "xplane", flugzeug: { titel: "Boeing 777-300ER", icao: "B77W" }, anzahl_werte: 9312, l_namen: 0, sitzung: 7 });
      case "vermessung_ruhe":
        if (h.ruheSpaet) return new Promise((res) => (h.ruheLoesen = () => res({ rauschen: 214 })));
        return Promise.resolve({ rauschen: 214 });
      case "vermessung_stellung": {
        const erste = h.stellungImSchritt === 0;
        h.stellungImSchritt++;
        return Promise.resolve({ erste, mitgegangen: erste ? 0 : 3 });
      }
      case "vermessung_schritt_abschliessen":
        h.stellungImSchritt = 0;
        return Promise.resolve({ kandidaten: args?.uebersprungen ? 0 : 4, beispiele: h.beispiele });
      case "vermessung_scan_namen":
        return Promise.resolve(h.scanNamen);
      case "vermessung_liste":
        return Promise.resolve(h.liste);
      case "vermessung_profile": {
        // wie im Client: Fenix hat ein Profil, Asobo nicht
        const f = (args?.flugzeuge ?? []) as Array<{ titel: string[] }>;
        return Promise.resolve(f.map((x) => (x.titel.some((t) => t.startsWith("Fenix")) ? "FenixA320" : null)));
      }
      case "vermessung_senden":
        return Promise.resolve({ id: "abc" });
      default:
        return Promise.resolve(undefined);
    }
  },
}));

import { FlugzeugVermessen, schonVermessen, simObjektOrdner, uebersicht } from "./FlugzeugVermessen";

const klick = async (text: string | RegExp) => {
  fireEvent.click(await screen.findByRole("button", { name: text }));
};

beforeEach(() => {
  h.aufrufe = [];
  h.amBoden = true;
  h.stellungImSchritt = 0;
  h.ruheSpaet = false;
  h.ruheLoesen = null;
  h.liste = [];
  h.scanNamen = 0;
  h.kind = "xplane";
  h.titel = "Boeing 777-300ER";
  h.icao = "B77W";
  h.pfad = null;
  h.uiName = null;
  h.beispiele = [];
});

describe("uebersicht: Messungen + Scans", () => {
  const messungen = [{ sim: "msfs", teil: "boden", icao: "A388", titel: "A380-800 RR Basic", zuletzt: 5, anzahl: 2, scan_namen: 235 }];
  const scans = [
    { sim: "msfs", icao: "A388", paket: "iniBuilds A380 – L:-Namen aus AAO-Profil", titel_liste: ["A380-800 RR Basic"], scan_namen: 235, profil: null, zuletzt: 4 },
    { sim: "msfs", icao: "A388", paket: "A380X (Development)", titel_liste: ["FlyByWire A380X (A380-842)"], scan_namen: 900, profil: null, zuletzt: 3 },
    { sim: "msfs", icao: "BCS3", paket: "Synaptic A220", titel_liste: ["A220-300", "A220-300 - No Cabin"], scan_namen: 1200, profil: "in_arbeit", zuletzt: 2 },
    { sim: "xplane", icao: "B738", paket: "Boeing 737-800", titel_liste: ["Boeing 737-800"], scan_namen: null, profil: "aus_scan", zuletzt: 1 },
  ];
  const z = uebersicht(messungen, scans);
  it("Scan zur Messung landet in derselben Zeile, fremdes Add-on gleicher ICAO nicht", () => {
    expect(z).toHaveLength(4);
    expect(z[0]!.titel).toBe("A380-800 RR Basic");
    expect(z[0]!.boden?.anzahl).toBe(2);
    expect(z.find((x) => x.titel === "A380X (Development)")?.boden).toBeNull();
  });
  it("Profil: der höhere Stand gewinnt (Messung geprüft vor Scan aus_scan)", () => {
    const zz = uebersicht(
      [{ ...messungen[0]!, profil: "geprueft" }],
      [{ ...scans[0]!, profil: "aus_scan" }],
    );
    expect(zz[0]!.profil).toBe("geprueft");
  });
  it("gescannt, nie vermessen: eigene Zeile mit Profil-Stand", () => {
    const a220 = z.find((x) => x.icao === "BCS3")!;
    expect(a220.boden).toBeNull();
    expect(a220.luft).toBeNull();
    expect(a220.profil).toBe("in_arbeit");
    expect(a220.titel_liste).toContain("A220-300 - No Cabin");
    expect(z.find((x) => x.icao === "B738")!.profil).toBe("aus_scan");
  });
});

describe("uebersicht: Fenix-Familie", () => {
  const z = uebersicht(
    [{ sim: "msfs", teil: "boden", icao: "A320", titel: "FenixA320 CFM SL", zuletzt: 9, anzahl: 3, profil: "geprueft" }],
    [
      { sim: "msfs", icao: "A320", paket: "Fenix A320 – L:-Namen aus HubHop", titel_liste: ["FenixA320", "FenixA320 CFM SL"], scan_namen: 1921, profil: null, zuletzt: 8, quelle: "hersteller-doku" },
      { sim: "msfs", icao: "A319", paket: "Fenix Airbus A319 & A321", titel_liste: ["FenixA319 CFM SL HD", "FenixA321 IAE WF TC"], scan_namen: 40, profil: null, zuletzt: 7 },
      { sim: "msfs", icao: "A20N", paket: "A32NX", titel_liste: ["Airbus A320neo FlyByWire"], scan_namen: 500, profil: null, zuletzt: 6 },
    ],
  );
  it("A319, A320 und A321 stehen in einer Zeile, Messung und Titel bleiben erhalten", () => {
    const fenix = z.filter((x) => x.titel_liste.some((t) => t.startsWith("Fenix")));
    expect(fenix).toHaveLength(1);
    expect(fenix[0]!.titel).toBe("Fenix A319 / A320 / A321");
    expect(fenix[0]!.boden?.anzahl).toBe(3);
    expect(fenix[0]!.profil).toBe("geprueft");
    expect(fenix[0]!.scan_namen).toBe(1921);
    expect(fenix[0]!.icao).toBeNull();
    expect(fenix[0]!.titel_liste).toEqual(expect.arrayContaining(["FenixA320 CFM SL", "FenixA321 IAE WF TC"]));
  });
  it("andere A320 (FlyByWire) bleiben eigene Zeile", () => {
    expect(z).toHaveLength(2);
    expect(z.find((x) => x.icao === "A20N")?.titel).toBe("A32NX");
  });
});

describe("schonVermessen", () => {
  const liste = [
    { sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: 1, anzahl: 1 },
    { sim: "xplane", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: 2, anzahl: 3 },
  ];
  it("erkennt Titel und Muster, Groß-/Kleinschreibung egal", () => {
    expect(schonVermessen(liste, " a380-800 rr basic ", "a388")?.zuletzt).toBe(1);
  });
  it("gleiche ICAO, anderes Add-on → nicht vermessen", () => {
    expect(schonVermessen(liste, "FlyByWire A380X (A380-842)", "A388")).toBeNull();
  });
  it("gleicher Titel, andere ICAO → nicht vermessen", () => {
    expect(schonVermessen(liste, "A380-800 RR Basic", "B748")).toBeNull();
  });
  it("ohne Titel keine Aussage", () => {
    expect(schonVermessen(liste, "", "A388")).toBeNull();
  });

  // 29.09.2026: Abgleich über den SimObject-Ordner (MSFS) und beide Titel (X-Plane).
  const ifly = { sim: "msfs", icao: "B38M", titel: "ifly-aircraft-737max8-TUI DAMAH-189Seats", ordner: "ifly 737-max8-189seats", zuletzt: 3, anzahl: 1 };
  it("MSFS: iFly in TUI vermessen, in RYR geladen → schon vermessen (Ordner)", () => {
    const o = simObjektOrdner(String.raw`SimObjects\Airplanes\iFly 737-MAX8-189Seats\aircraft.CFG`);
    expect(o).toBe("ifly 737-max8-189seats");
    expect(schonVermessen([ifly], "ifly-aircraft-737max8-RYR EI-HGA", "B38M", "boden", o)?.zuletzt).toBe(3);
  });
  it("MSFS: gleicher Titel, anderer Ordner → nicht verwechselt", () => {
    const a350 = { sim: "msfs", icao: "A35K", titel: "A350-1000 (No Cabin)", ordner: "a350", zuletzt: 4, anzahl: 1 };
    expect(schonVermessen([a350], "A350-1000 (No Cabin)", "A35K", "boden", "anderes_a350")).toBeNull();
    // ohne Ordner des Geladenen (AircraftLoaded noch nicht da): Titel wie bisher
    expect(schonVermessen([a350], "A350-1000 (No Cabin)", "A35K", "boden", null)?.zuletzt).toBe(4);
  });
  it("MSFS: alte Messung ohne Ordner passt weiter über den Titel", () => {
    expect(schonVermessen(liste, "A380-800 RR Basic", "A388", "boden", "inibuilds-a380")?.zuletzt).toBe(1);
  });
  it("MSFS: Titel aus titel_liste (andere gemessene Lackierung) zählt", () => {
    const zeile = { ...ifly, ordner: null, titel: "x", titel_liste: ["x", "ifly RYR"] };
    expect(schonVermessen([zeile], "IFLY RYR", "B38M")?.zuletzt).toBe(3);
  });
  it("X-Plane: alte Messung (acf_descrip) und neue (UI-Name) beide erkannt", () => {
    const geladen = ["ToLiSs A320 Hi Def", "A320 with high fidelity system modelling"];
    const alt = { sim: "xplane", icao: "A20N", titel: "A320 with high fidelity system modelling", zuletzt: 5, anzahl: 3 };
    const neu = { sim: "xplane", icao: "A20N", titel: "ToLiSs A320 Hi Def", zuletzt: 6, anzahl: 1 };
    expect(schonVermessen([alt], geladen, "A20N")?.zuletzt).toBe(5);
    expect(schonVermessen([neu], geladen, "A20N")?.zuletzt).toBe(6);
  });
});

describe("simObjektOrdner (wie der Server)", () => {
  it("Preset, Groß/Klein, Slash/Backslash, Rand", () => {
    expect(simObjektOrdner(String.raw`SimObjects\Airplanes\FNX_32X\presets\fnx\FNX_320_CFM_SL\config\aircraft.CFG`)).toBe("fnx_32x");
    expect(simObjektOrdner("pkg/SimObjects/AirPlanes/ Synaptic_A220 /common/config/aircraft.cfg")).toBe("synaptic_a220");
    expect(simObjektOrdner("X/aircraft.cfg")).toBe("x");
    expect(simObjektOrdner("Aircraft/ToLissA320_V1p1p7/a320.acf")).toBeNull();
    expect(simObjektOrdner(null)).toBeNull();
  });
});

describe("Flugzeug vermessen", () => {
  it("Startseite: Tabelle aller Messungen, geladenes Flugzeug oben, Boden und Luft als Spalten", async () => {
    h.liste = [
      { sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: Date.UTC(2026, 8, 28), anzahl: 1 },
      { sim: "xplane", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: Date.UTC(2026, 8, 28), anzahl: 2 },
    ];
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    // ohne Aufklappen sichtbar, mit Spaltenköpfen Boden/Luft
    expect(screen.getByRole("columnheader", { name: "Am Boden – Schalter" })).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "In der Luft – Autopilot" })).toBeTruthy();
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(2);
    // geladene 777 steht oben, markiert; Boden gemessen (2×), Luft fehlt
    expect(zeilen[0]!.className).toContain("vm-zeile--geladen");
    expect(zeilen[0]!.textContent).toContain("Boeing 777-300ER");
    expect(zeilen[0]!.textContent).toContain("geladen");
    expect(zeilen[0]!.textContent).toMatch(/✓ .*\(2×\)/);
    expect(zeilen[0]!.textContent).toContain("fehlt noch");
    expect(zeilen[1]!.textContent).toContain("A380-800 RR Basic");
    expect(screen.getByRole("button", { name: "Trotzdem messen" })).toBeTruthy();
  });

  it("Startseite: noch nie vermessenes geladenes Flugzeug steht trotzdem oben, beides fehlt", async () => {
    h.liste = [{ sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: 1, anzahl: 1 }];
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(2);
    expect(zeilen[0]!.textContent).toContain("Boeing 777-300ER");
    expect(zeilen[0]!.querySelectorAll(".vm-zelle--fehlt").length).toBe(2);
    expect(screen.getByRole("button", { name: "Messung starten" })).toBeTruthy();
  });

  it("Startseite MSFS ohne Scan: Tipp, erst einen Scan zu machen", async () => {
    h.kind = "msfs2024";
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(/Tipp: Mach zuerst einen Scan/)).toBeTruthy();
    const scan = h.aufrufe.find((a) => a.cmd === "vermessung_scan_namen");
    expect(scan?.args).toEqual({ icao: "B77W", titel: "Boeing 777-300ER" });
  });

  it("Startseite MSFS mit Scan: kein Tipp, Spalte zeigt die Namen", async () => {
    h.kind = "msfs2024";
    h.scanNamen = 235;
    h.liste = [{ sim: "msfs", icao: "A20N", titel: "FenixA320 IAE", zuletzt: 1, anzahl: 1, scan_namen: 0 }] as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    expect(screen.getByRole("columnheader", { name: "Variablen (Scan)" })).toBeTruthy();
    await screen.findByText("✓ 235 Namen");
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen[1]!.textContent).toContain("kein Scan");
    expect(screen.queryByText(/Tipp: Mach zuerst einen Scan/)).toBeNull();
  });

  // Cloud-QS 29.09.2026 (P3): der aircraft.cfg-Pfad trifft oft erst nach dem
  // ersten Statustakt ein. Dann muss die Startseite neu fragen, sonst bleibt
  // die Anzahl ohne Ordner-Zuordnung stehen.
  it("Startseite MSFS: Pfad kommt später → Scan-Namen werden neu abgefragt", async () => {
    h.kind = "msfs2024";
    render(<FlugzeugVermessen />);
    await waitFor(() => expect(h.aufrufe.filter((a) => a.cmd === "vermessung_scan_namen").length).toBe(1));
    h.pfad = String.raw`SimObjects\Airplanes\PMDG 777-300ER\aircraft.cfg`;
    await waitFor(
      () => expect(h.aufrufe.filter((a) => a.cmd === "vermessung_scan_namen").length).toBe(2),
      { timeout: 5000 },
    );
  });

  // 30.09.2026: „geprüft“ nur mit Boden UND Luft; ist nur ein Teil geprüft,
  // zeigt die Profil-Spalte die Zwischenstufe.
  it("Profil-Spalte: nur Boden geprüft → „Boden geprüft“, beide → „geprüft“", async () => {
    h.kind = "msfs2024";
    h.liste = {
      flugzeuge: [
        { sim: "msfs", teil: "boden", icao: "A20N", titel: "Airbus A320neo FlyByWire", zuletzt: 3, anzahl: 1, profil: "aus_scan", profil_teile: ["boden"] },
        { sim: "msfs", teil: "boden", icao: "A35K", titel: "A350-1000 (No Cabin)", zuletzt: 2, anzahl: 1, profil: "geprueft", profil_teile: ["boden", "luft"] },
        { sim: "msfs", teil: "luft", icao: "A35K", titel: "A350-1000 (No Cabin)", zuletzt: 2, anzahl: 1, profil: "geprueft", profil_teile: ["boden", "luft"] },
        { sim: "msfs", teil: "boden", icao: "C172", titel: "Cessna Alt", zuletzt: 1, anzahl: 1, profil: null },
      ],
      scans: [],
    } as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    const zeile = (name: string) => [...tabelle.querySelectorAll("tbody tr")].find((r) => r.textContent?.includes(name))!;
    await waitFor(() => expect(zeile("FlyByWire").textContent).toContain("Boden geprüft"));
    expect(zeile("FlyByWire").querySelector(".vm-zelle--teil")?.getAttribute("title")).toContain("Luft fehlt noch");
    expect(zeile("A350").textContent).toContain("✓ geprüft");
    expect(zeile("A350").textContent).not.toContain("Boden geprüft");
    // Älterer Server ohne profil_teile: wie bisher, keine Zwischenstufe.
    expect(zeile("Cessna Alt").textContent).not.toContain("geprüft");
  });

  it("Startseite X-Plane: kein Scan-Hinweis, keine Anfrage", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    expect(await screen.findByText("nicht nötig")).toBeTruthy();
    expect(screen.queryByText(/Tipp: Mach zuerst einen Scan/)).toBeNull();
    expect(h.aufrufe.some((a) => a.cmd === "vermessung_scan_namen")).toBe(false);
  });

  it("Startseite: geladenes Flugzeug über einen Lackierungstitel des Scans erkannt", async () => {
    h.kind = "msfs2024";
    h.liste = {
      flugzeuge: [],
      scans: [{ sim: "msfs", icao: "B77W", paket: "PMDG 777", titel_liste: ["Boeing 777-300ER", "PMDG 777-300ER Emirates"], scan_namen: 50, profil: "geprueft", zuletzt: 1 }],
    } as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(1);
    expect(zeilen[0]!.className).toContain("vm-zeile--geladen");
    expect(zeilen[0]!.textContent).toContain("PMDG 777");
    expect(zeilen[0]!.textContent).toContain("✓ geprüft");
    expect(screen.getByRole("columnheader", { name: "Profil" })).toBeTruthy();
  });

  it("Profil-Spalte: eigenes Profil vs. nur Standard (MSFS), X-Plane ohne Aussage", async () => {
    h.liste = {
      flugzeuge: [],
      scans: [
        { sim: "msfs", icao: "A320", paket: "FenixA320", titel_liste: ["FenixA320 CFM SL"], scan_namen: 1921, profil: null, zuletzt: 2 },
        { sim: "msfs", icao: "A20N", paket: "Asobo A320", titel_liste: ["Asobo A320 Neo"], scan_namen: 0, profil: null, zuletzt: 1 },
        { sim: "xplane", icao: "B738", paket: "Boeing 737-800", titel_liste: ["Boeing 737-800"], scan_namen: null, profil: null, zuletzt: 1 },
      ],
    } as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    await screen.findByText("eigenes Profil");
    const text = (name: string) => [...tabelle.querySelectorAll("tbody tr")].find((z) => z.textContent?.includes(name))!.textContent!;
    expect(text("Fenix A319 / A320 / A321")).toContain("eigenes Profil");
    expect(text("Asobo A320")).toContain("nur Standard");
    expect(text("Boeing 737-800")).not.toContain("nur Standard");
  });

  it("MSFS: iFly in TUI vermessen, in RYR geladen → „schon vermessen“ und „geladen“", async () => {
    h.kind = "msfs2024";
    h.titel = "ifly-aircraft-737max8-RYR EI-HGA";
    h.icao = "B38M";
    h.pfad = String.raw`SimObjects\Airplanes\iFly 737-MAX8-189Seats\aircraft.CFG`;
    h.liste = {
      flugzeuge: [{ sim: "msfs", teil: "boden", icao: "B38M", titel: "ifly-aircraft-737max8-TUI DAMAH-189Seats", ordner: "ifly 737-max8-189seats", titel_liste: ["ifly-aircraft-737max8-TUI DAMAH-189Seats"], zuletzt: 1, anzahl: 1 }],
      scans: [
        { sim: "msfs", icao: "B38M", paket: "737MAX", titel_liste: ["iFly 737-MAX8 (189Seats)"], scan_namen: 1873, profil: null, zuletzt: 2, ordner: ["ifly 737-max8", "ifly 737-max8-189seats"] },
        // anderes Add-on mit ähnlichem Titel-Teil: bleibt eigene, nicht geladene Zeile
        { sim: "msfs", icao: "B38M", paket: "PMDG 737 MAX 8", titel_liste: ["ifly-aircraft-737max8-RYR EI-HGA"], scan_namen: 900, profil: null, zuletzt: 3, ordner: ["pmdg 737-8"] },
      ],
    } as never;
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(deCommon.vermessen.stand_nochmal_boden)).toBeTruthy();
    const tabelle = await screen.findByRole("table");
    const zeilen = [...tabelle.querySelectorAll("tbody tr")];
    expect(zeilen.length).toBe(2);
    const geladen = zeilen.filter((z) => z.className.includes("vm-zeile--geladen"));
    expect(geladen.length).toBe(1);
    expect(geladen[0]!.textContent).toContain("ifly-aircraft-737max8-TUI DAMAH-189Seats");
    // Zwei Zeilen: der iFly-Scan ist über den Ordner in die Messzeile
    // gewandert (sonst drei), der PMDG-Scan bleibt trotz gleichem Titel eigen.
    expect(zeilen.find((z) => z.textContent?.includes("PMDG 737 MAX 8"))!.className).not.toContain("vm-zeile--geladen");
  });

  it("X-Plane: neue Messung mit UI-Name → „schon vermessen“ und „geladen“", async () => {
    h.titel = "A320 with high fidelity system modelling";
    h.uiName = "ToLiSs A320 Hi Def";
    h.icao = "A20N";
    h.liste = {
      flugzeuge: [{ sim: "xplane", teil: "boden", icao: "A20N", titel: "ToLiSs A320 Hi Def", zuletzt: 1, anzahl: 1 }],
      scans: [{ sim: "xplane", icao: "A20N", paket: "ToLissA320_V1p2p1", titel_liste: ["ToLiSs A320 Hi Def"], scan_namen: null, profil: null, zuletzt: 2 }],
    } as never;
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(deCommon.vermessen.stand_nochmal_boden)).toBeTruthy();
    const zeilen = (await screen.findByRole("table")).querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(1);
    expect(zeilen[0]!.className).toContain("vm-zeile--geladen");
  });

  it("X-Plane: alte Messung mit acf_descrip-Titel → „schon vermessen“", async () => {
    h.titel = "A320 with high fidelity system modelling";
    h.uiName = "ToLiSs A320 Hi Def";
    h.icao = "A20N";
    h.liste = [{ sim: "xplane", icao: "A20N", titel: "A320 with high fidelity system modelling", zuletzt: 1, anzahl: 3 }];
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(deCommon.vermessen.stand_nochmal_boden)).toBeTruthy();
    const zeilen = (await screen.findByRole("table")).querySelectorAll("tbody tr");
    expect(zeilen[0]!.className).toContain("vm-zeile--geladen");
  });

  it("Fenix-Familie: ein geladener A321 markiert die gemeinsame Zeile", async () => {
    h.kind = "msfs2024";
    h.titel = "FenixA321 IAE WF TC";
    h.icao = "A321";
    h.liste = {
      flugzeuge: [{ sim: "msfs", teil: "boden", icao: "A320", titel: "FenixA320 CFM SL", zuletzt: 1, anzahl: 1 }],
      scans: [{ sim: "msfs", icao: "A319", paket: "Fenix Airbus A319 & A321", titel_liste: ["FenixA321 IAE WF TC"], scan_namen: 40, profil: null, zuletzt: 2 }],
    } as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(1);
    expect(zeilen[0]!.className).toContain("vm-zeile--geladen");
    expect(zeilen[0]!.textContent).toContain("Fenix A319 / A320 / A321");
  });

  it("Startseite: Boden und Luft desselben Flugzeugs in einer Zeile", async () => {
    h.liste = [
      { sim: "xplane", teil: "boden", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: 1, anzahl: 1 },
      { sim: "xplane", teil: "luft", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: 2, anzahl: 1 },
    ] as never;
    render(<FlugzeugVermessen />);
    const tabelle = await screen.findByRole("table");
    const zeilen = tabelle.querySelectorAll("tbody tr");
    expect(zeilen.length).toBe(1);
    expect(zeilen[0]!.querySelectorAll(".vm-zelle--ok").length).toBe(2);
    expect(tabelle.textContent).not.toContain("fehlt noch");
  });

  it("Startseite: noch nicht vermessen → normaler Start, kein Hinweis", async () => {
    h.liste = [{ sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: 1, anzahl: 1 }];
    render(<FlugzeugVermessen />);
    await screen.findByRole("table");
    expect(screen.getByRole("button", { name: "Messung starten" })).toBeTruthy();
  });

  it("in der Luft: Autopilot-Teil mit eigenen Schritten", async () => {
    h.amBoden = false;
    render(<FlugzeugVermessen />);
    expect(await screen.findByText("Autopilot im Flug vermessen")).toBeTruthy();
    expect(screen.getByText(/In der Luft · Boeing 777-300ER/)).toBeTruthy();
    // Auswahl zeigt nur die Luft-Schritte.
    expect(screen.getByText("Nur bestimmte Schalter messen (9 von 9)")).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: "Beacon (Anti-Collision)" })).toBeNull();
    await klick("Autopilot-Messung starten");
    const start = h.aufrufe.find((a) => a.cmd === "vermessung_starten");
    expect(start?.args?.teil).toBe("luft");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    expect(await screen.findByText("Autopilot (AP)")).toBeTruthy();
    expect(screen.getByText("Schalter 1 von 9")).toBeTruthy();
  });

  it("am Boden: Start mit teil=boden", async () => {
    render(<FlugzeugVermessen />);
    await klick("Messung starten");
    const start = h.aufrufe.find((a) => a.cmd === "vermessung_starten");
    expect(start?.args?.teil).toBe("boden");
  });

  it("führt durch Ruhe, Schalter und Senden", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden · Boeing 777-300ER/);
    await klick("Messung starten");
    expect(await screen.findByText(/9312 Werte verbunden/)).toBeTruthy();
    await klick(/Ruhemessung starten/);
    expect(await screen.findByText(/214 Werte ändern sich von selbst/)).toBeTruthy();
    await klick("Weiter");

    // Schalter 1: Beacon — mit Erklärung, wo er sitzt.
    expect(await screen.findByText("Beacon (Anti-Collision)")).toBeTruthy();
    expect(screen.getByText(/Overhead-Panel, Lichterreihe unten/)).toBeTruthy();
    await klick("Ja – los geht's");
    expect(screen.getByText("Stelle den Schalter auf")).toBeTruthy();
    await klick("Erledigt – steht so");
    expect(await screen.findByText("Ausgangsstellung gemerkt")).toBeTruthy();
    await klick("Erledigt – steht so");
    expect(await screen.findByText("✓ 4 Werte gehen mit diesem Schalter mit")).toBeTruthy();
    expect(screen.getByText("✓ 3 Werte haben sich bewegt")).toBeTruthy();
    await klick("Nächster Schalter");

    // Alle übrigen überspringen.
    for (let i = 1; i < BODEN.length; i++) {
      await klick("Nein, überspringen");
    }
    expect(await screen.findByText("Geschafft!")).toBeTruthy();
    expect(screen.getByText("4 Werte")).toBeTruthy();
    await klick("An GSG senden");
    expect(await screen.findByText("Danke – angekommen!")).toBeTruthy();

    const cmds = h.aufrufe.map((a) => a.cmd).filter((c) => c !== "sim_status" && c !== "vermessung_liste");
    expect(cmds[0]).toBe("vermessung_starten");
    expect(cmds).toContain("vermessung_senden");
    expect(cmds[cmds.length - 1]).toBe("vermessung_beenden");
    const abschluesse = h.aufrufe.filter((a) => a.cmd === "vermessung_schritt_abschliessen");
    expect(abschluesse).toHaveLength(BODEN.length);
    expect(abschluesse[0]!.args).toEqual({ schalter: "beacon", uebersprungen: false, sitzung: 7 });
    expect(abschluesse[1]!.args).toEqual({ schalter: "strobe", uebersprungen: true, sitzung: 7 });
    // Jeder Befehl nach dem Start nennt die Sitzung (auch das Beenden).
    const nachStart = h.aufrufe.filter((a) => a.cmd.startsWith("vermessung_") && a.cmd !== "vermessung_starten" && a.cmd !== "vermessung_liste");
    expect(nachStart.every((a) => a.args?.sitzung === 7)).toBe(true);
    // Der Start trägt eine Kennung, das Beenden nennt sie wieder.
    const start = h.aufrufe.find((a) => a.cmd === "vermessung_starten")!.args?.start;
    expect(typeof start).toBe("number");
    expect(h.aufrufe.filter((a) => a.cmd === "vermessung_beenden").every((a) => a.args?.start === start)).toBe(true);
  });

  it("nur die Autobrake nachmessen: ein Schalter, dann Übersicht", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    fireEvent.click(screen.getByRole("button", { name: "Keine" }));
    expect((screen.getByRole("button", { name: "Messung starten" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "Autobrake" }));
    expect(screen.getByText("Nur bestimmte Schalter messen (1 von 12)")).toBeTruthy();
    await klick("Messung starten");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    expect(await screen.findByText("Schalter 1 von 1")).toBeTruthy();
    expect(screen.getByText("Autobrake")).toBeTruthy();
    await klick("Ja – los geht's");
    await klick("Erledigt – steht so");
    await screen.findByText("Ausgangsstellung gemerkt");
    await klick("Erledigt – steht so");
    await screen.findByText("✓ 3 Werte haben sich bewegt");
    await klick("Das war schon die letzte Stellung");
    await klick("Zur Übersicht");
    expect(await screen.findByText("Geschafft!")).toBeTruthy();
    // Übersicht nur mit dem gemessenen Schalter, kein „hat das Flugzeug nicht“.
    expect(screen.queryByText("hat das Flugzeug nicht")).toBeNull();
    const abschluesse = h.aufrufe.filter((a) => a.cmd === "vermessung_schritt_abschliessen");
    expect(abschluesse.map((a) => a.args?.schalter)).toEqual(["autobrake"]);
  });

  it("Text-Datarefs (X-Plane) zeigen den Text statt der Kennzahl", async () => {
    h.beispiele = [
      { variable: "1-sim/output/fma/roll", werte: [123456789, 987654321], texte: ["LNAV", "HDG SEL"] },
      { variable: "sim/zahl", werte: [0, 1] },
    ];
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    fireEvent.click(screen.getByRole("button", { name: "Keine" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Autobrake" }));
    await klick("Messung starten");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    await klick("Ja – los geht's");
    await klick("Erledigt – steht so");
    await screen.findByText("Ausgangsstellung gemerkt");
    await klick("Erledigt – steht so");
    await klick("Das war schon die letzte Stellung");
    expect(await screen.findByText(/"LNAV" \/ "HDG SEL"/)).toBeTruthy();
    expect(screen.queryByText(/123456789/)).toBeNull();
    expect(screen.getByText(/sim\/zahl: 0 \/ 1/)).toBeTruthy();
  });

  it("Klappen: nach zwei Rasten lässt sich die letzte Stellung wählen", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    await klick("Messung starten");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    const bisKlappen = BODEN.findIndex((s) => s.schalter === "klappen");
    for (let i = 0; i < bisKlappen; i++) await klick("Nein, überspringen");
    expect(await screen.findByText("Klappenhebel")).toBeTruthy();
    await klick("Ja – los geht's");
    expect(screen.queryByRole("button", { name: "Das war schon die letzte Stellung" })).toBeNull();
    await klick("Erledigt – steht so");
    await screen.findByText("Ausgangsstellung gemerkt");
    await klick("Erledigt – steht so");
    await screen.findByText("✓ 3 Werte haben sich bewegt");
    await klick("Das war schon die letzte Stellung");
    await waitFor(() => expect(screen.getByText(/Werte gehen mit diesem Schalter mit/)).toBeTruthy());
    const letzter = h.aufrufe.filter((a) => a.cmd === "vermessung_schritt_abschliessen").pop();
    expect(letzter!.args).toEqual({ schalter: "klappen", uebersprungen: false, sitzung: 7 });
  });

  it("Doppelklick auf „letzte Stellung“ schließt nur einmal ab", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    await klick("Messung starten");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    const bisKlappen = BODEN.findIndex((s) => s.schalter === "klappen");
    for (let i = 0; i < bisKlappen; i++) await klick("Nein, überspringen");
    await klick("Ja – los geht's");
    await klick("Erledigt – steht so");
    await screen.findByText("Ausgangsstellung gemerkt");
    await klick("Erledigt – steht so");
    await screen.findByText("✓ 3 Werte haben sich bewegt");
    const knopf = screen.getByRole("button", { name: "Das war schon die letzte Stellung" });
    fireEvent.click(knopf);
    fireEvent.click(knopf);
    await screen.findByText(/Werte gehen mit diesem Schalter mit/);
    const klappen = h.aufrufe.filter((a) => a.cmd === "vermessung_schritt_abschliessen" && a.args?.schalter === "klappen");
    expect(klappen).toHaveLength(1);
  });

  it("nach Abbruch ändert eine späte Antwort die Ansicht nicht mehr", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    await klick("Messung starten");
    h.ruheSpaet = true;
    await klick(/Ruhemessung starten/);
    // Ruhemessung läuft noch — abbrechen, dann kommt ihre Antwort.
    vi.spyOn(window, "confirm").mockReturnValue(true);
    await klick("Messung abbrechen");
    expect(await screen.findByRole("button", { name: "Messung starten" })).toBeTruthy();
    h.ruheLoesen!();
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByText(/Werte ändern sich von selbst/)).toBeNull();
    expect(screen.getByRole("button", { name: "Messung starten" })).toBeTruthy();
    const ende = h.aufrufe.filter((a) => a.cmd === "vermessung_beenden");
    expect(ende.length).toBeGreaterThan(0);
    expect(ende.every((a) => typeof a.args?.start === "number")).toBe(true);
  });
});
