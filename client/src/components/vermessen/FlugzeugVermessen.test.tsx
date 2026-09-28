import { describe, it, expect, beforeAll, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../locales/de/common.json";
import { SCHRITTE } from "./schritte";

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
  liste: [] as Array<{ sim: string; icao: string; titel: string; zuletzt: number; anzahl: number }>,
  ruheLoesen: null as null | (() => void),
}));

vi.mock("../../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.aufrufe.push({ cmd, args });
    switch (cmd) {
      case "sim_status":
        return Promise.resolve({ kind: h.kind, snapshot: { on_ground: h.amBoden, aircraft_title: "Boeing 777-300ER", aircraft_icao: "B77W" } });
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
        return Promise.resolve({ kandidaten: args?.uebersprungen ? 0 : 4, beispiele: [] });
      case "vermessung_scan_namen":
        return Promise.resolve(h.scanNamen);
      case "vermessung_liste":
        return Promise.resolve(h.liste);
      case "vermessung_senden":
        return Promise.resolve({ id: "abc" });
      default:
        return Promise.resolve(undefined);
    }
  },
}));

import { FlugzeugVermessen, schonVermessen } from "./FlugzeugVermessen";

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
});

describe("Flugzeug vermessen", () => {
  it("Startseite: schon vermessenes Flugzeug wird erkannt, Liste zeigt den Bestand", async () => {
    h.liste = [
      { sim: "xplane", icao: "B77W", titel: "Boeing 777-300ER", zuletzt: Date.UTC(2026, 8, 28), anzahl: 2 },
      { sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: Date.UTC(2026, 8, 28), anzahl: 1 },
    ];
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(/Dieses Flugzeug ist schon vermessen/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Trotzdem messen" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Messung starten" })).toBeNull();
    expect(screen.getByText("Schon vermessen (2)")).toBeTruthy();
    expect(screen.getByText("A380-800 RR Basic")).toBeTruthy();
    expect(screen.getByText(/B77W · X-Plane · .* · 2 Messungen/)).toBeTruthy();
  });

  it("Startseite MSFS ohne Scan: Tipp, erst einen Scan zu machen", async () => {
    h.kind = "msfs2024";
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(/Tipp: Mach zuerst einen Scan/)).toBeTruthy();
    const scan = h.aufrufe.find((a) => a.cmd === "vermessung_scan_namen");
    expect(scan?.args).toEqual({ icao: "B77W", titel: "Boeing 777-300ER" });
  });

  it("Startseite MSFS mit Scan: kein Tipp, Anzahl der Namen", async () => {
    h.kind = "msfs2024";
    h.scanNamen = 235;
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(/235 Variablennamen werden mitgelesen/)).toBeTruthy();
    expect(screen.queryByText(/Tipp: Mach zuerst einen Scan/)).toBeNull();
  });

  it("Startseite X-Plane: kein Scan-Hinweis, keine Anfrage", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    expect(screen.queryByText(/Tipp: Mach zuerst einen Scan/)).toBeNull();
    expect(h.aufrufe.some((a) => a.cmd === "vermessung_scan_namen")).toBe(false);
  });

  it("Startseite: noch nicht vermessen → normaler Start, kein Hinweis", async () => {
    h.liste = [{ sim: "msfs", icao: "A388", titel: "A380-800 RR Basic", zuletzt: 1, anzahl: 1 }];
    render(<FlugzeugVermessen />);
    await screen.findByText("Schon vermessen (1)");
    expect(screen.getByRole("button", { name: "Messung starten" })).toBeTruthy();
    expect(screen.queryByText(/Dieses Flugzeug ist schon vermessen/)).toBeNull();
  });

  it("in der Luft lässt sich nicht starten", async () => {
    h.amBoden = false;
    render(<FlugzeugVermessen />);
    expect(await screen.findByText(/in der Luft/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Messung starten" }) as HTMLButtonElement).disabled).toBe(true);
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
    for (let i = 1; i < SCHRITTE.length; i++) {
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
    expect(abschluesse).toHaveLength(SCHRITTE.length);
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

  it("Klappen: nach zwei Rasten lässt sich die letzte Stellung wählen", async () => {
    render(<FlugzeugVermessen />);
    await screen.findByText(/Simulator verbunden/);
    await klick("Messung starten");
    await klick(/Ruhemessung starten/);
    await klick("Weiter");
    const bisKlappen = SCHRITTE.findIndex((s) => s.schalter === "klappen");
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
    const bisKlappen = SCHRITTE.findIndex((s) => s.schalter === "klappen");
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
