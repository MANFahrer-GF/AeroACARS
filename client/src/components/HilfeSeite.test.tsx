// Hilfe anfordern (P-9.22, N-06), eigene Seite: Der Pilot sieht vor dem Senden, was
// mitgeht; scheitert das Ticket (HelpDesk noch nicht veröffentlicht), bleibt
// die Diagnose-Kennung zum Weitergeben sichtbar.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import HILFE_RS from "../../src-tauri/src/hilfe.rs?raw";

const invokeMock = vi.fn();
let amSimPc = true;
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  get isTauri() {
    return amSimPc;
  },
}));
// jsdom hat kein Canvas: Verkleinern ersetzt, die Grenzlogik bleibt echt.
const vorbereitenMock = vi.fn();
vi.mock("../lib/hilfeBilder", async (orig) => ({
  ...(await orig<typeof import("../lib/hilfeBilder")>()),
  bildVorbereiten: (...a: unknown[]) => vorbereitenMock(...a),
}));

import { HilfeSeite, istHilfeTaste, type HilfeErgebnis, type HilfeVorschau } from "./HilfeSeite";

const VORSCHAU: HilfeVorschau = {
  meta: {
    client_version: "3.0.0-beta.2",
    sim: "msfs2024",
    sim_version: "1.4.7",
    flugzeug_profil: "Fenix A320",
    addon: "FenixA320 Lufthansa D-AIUA",
    paket_generation: "12-0123456789ab",
    letzte_flugdatei: "GSG1234.jsonl",
  },
  dateien: [{ name: "aeroacars.2026-10-30.log", bytes: 204800 }],
  log_gesperrt: false,
};

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

afterEach(() => {
  cleanup();
  invokeMock.mockReset();
  vorbereitenMock.mockReset();
  amSimPc = true;
});

function bildDatei(name = "screen.png", type = "image/png") {
  return new File([new Uint8Array([1, 2, 3])], name, { type });
}

/** Vorbereitet = Base64 des Dateinamens, damit der Versand prüfbar ist. */
function vorbereitenWieEcht() {
  vorbereitenMock.mockImplementation(async (_d: Blob, name: string) => ({
    daten: btoa(name),
    vorschau: `data:image/jpeg;base64,${btoa(name)}`,
    bytes: 3,
    name,
  }));
}

async function waehlen(...dateien: File[]) {
  fireEvent.change(screen.getByTestId("hilfe-bilder-datei"), { target: { files: dateien } });
  await flush();
  await flush();
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function oeffnen(ergebnis: HilfeErgebnis | { reject: unknown }) {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "hilfe_vorschau") return Promise.resolve(VORSCHAU);
    if (cmd === "hilfe_anfordern") {
      return "reject" in ergebnis ? Promise.reject(ergebnis.reject) : Promise.resolve(ergebnis);
    }
    return Promise.reject(new Error(cmd));
  });
  render(<HilfeSeite />);
}

async function ausfuellenUndSenden() {
  fireEvent.change(screen.getByLabelText("Betreff"), { target: { value: "Landung fehlt" } });
  fireEvent.change(screen.getByLabelText("Was ist passiert?"), { target: { value: "Nach dem Aufsetzen keine Note." } });
  fireEvent.click(screen.getByRole("button", { name: "Senden" }));
  await flush();
}

describe("HilfeSeite", () => {
  it("zeigt vor dem Senden, was mit der Diagnose mitgeht", async () => {
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: null, ticket_fehler: null });
    await flush();
    const mitgeht = screen.getByTestId("hilfe-mitgeht").textContent ?? "";
    for (const teil of ["3.0.0-beta.2", "msfs2024 · 1.4.7", "Fenix A320", "12-0123456789ab", "GSG1234.jsonl", "aeroacars.2026-10-30.log (200 KB)"]) {
      expect(mitgeht).toContain(teil);
    }
    expect(screen.getByText("AeroACARS")).toBeInTheDocument();
    // ohne Betreff und Text nichts senden
    expect(screen.getByRole("button", { name: "Senden" })).toBeDisabled();
  });

  it("sagt, wenn das Log wegen RUST_LOG nicht mitgeht", async () => {
    invokeMock.mockResolvedValueOnce({ ...VORSCHAU, log_gesperrt: true });
    render(<HilfeSeite />);
    await flush();
    expect(screen.getByTestId("hilfe-mitgeht").textContent).toContain("RUST_LOG");
  });

  it("Ticket angelegt: Nummer und Kennung", async () => {
    oeffnen({
      kennung: "D-0123456789ab",
      link: "https://live.kant.ovh/admin/diagnose.html?k=D-0123456789ab",
      diagnose_fehler: null,
      ticket: { id: 7, ticket_number: "HD-0007", url: "https://german-sky-group.eu/helpdesk/tickets/7" },
      ticket_fehler: null,
    });
    await flush();
    await ausfuellenUndSenden();
    expect(invokeMock).toHaveBeenCalledWith("hilfe_anfordern", { betreff: "Landung fehlt", text: "Nach dem Aufsetzen keine Note." });
    expect(screen.getByText("Ticket angelegt")).toBeInTheDocument();
    expect(screen.getByTestId("hilfe-ergebnis").textContent).toContain("HD-0007");
    expect(screen.getByTestId("hilfe-kennung").textContent).toBe("Diagnose-Kennung: D-0123456789ab");
    expect(screen.getByRole("button", { name: "Ticket öffnen" })).toBeInTheDocument();
    // Neue Anfrage: leeres Formular
    fireEvent.click(screen.getByRole("button", { name: "Neue Anfrage" }));
    expect((screen.getByLabelText("Betreff") as HTMLInputElement).value).toBe("");
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Hilfe anfordern");
  });

  it("HelpDesk noch nicht veröffentlicht: verständliche Meldung, Kennung zum Weitergeben", async () => {
    oeffnen({ kennung: "D-0123456789ab", link: null, diagnose_fehler: null, ticket: null, ticket_fehler: "ticket_helpdesk_fehlt" });
    await flush();
    await ausfuellenUndSenden();
    expect(screen.getByText("Ticket nicht angelegt")).toBeInTheDocument();
    expect(screen.getByRole("alert").textContent).toContain("noch nicht freigeschaltet");
    expect(screen.getByTestId("hilfe-kennung").textContent).toContain("D-0123456789ab");
    expect(screen.getByTestId("hilfe-ergebnis").textContent).toContain("Gib diese Kennung beim Support an");
  });

  it("Kategorie fehlt (503) wird benannt", async () => {
    oeffnen({ kennung: "D-0123456789ab", link: null, diagnose_fehler: null, ticket: null, ticket_fehler: "ticket_kategorie_fehlt" });
    await flush();
    await ausfuellenUndSenden();
    expect(screen.getByRole("alert").textContent).toContain("Kategorie „AeroACARS“");
  });

  it("Ticket ohne Diagnose: der Grund steht dabei", async () => {
    oeffnen({
      kennung: null,
      link: null,
      diagnose_fehler: "diagnose_zu_viele",
      ticket: { id: 8, ticket_number: "HD-0008", url: null },
      ticket_fehler: null,
    });
    await flush();
    await ausfuellenUndSenden();
    expect(screen.getByTestId("hilfe-ergebnis").textContent).toContain("ohne Diagnose");
    expect(screen.getByTestId("hilfe-ergebnis").textContent).toContain("10 Diagnosen");
    expect(screen.queryByRole("button", { name: "Ticket öffnen" })).toBeNull();
  });

  it("vom Kern abgelehnte Eingabe bleibt im Formular", async () => {
    oeffnen({ reject: { code: "hilfe_betreff", message: "x" } });
    await flush();
    await ausfuellenUndSenden();
    expect(screen.getByRole("alert").textContent).toContain("Betreff");
    expect(screen.getByLabelText("Betreff")).toBeInTheDocument();
  });

  it("Bild wählen: Vorschau, Zähler, Datenschutz-Hinweis, geht als Anhang mit", async () => {
    vorbereitenWieEcht();
    oeffnen({
      kennung: "D-0123456789ab",
      link: null,
      diagnose_fehler: null,
      ticket: { id: 9, ticket_number: "HD-0009", url: null, anhaenge: 2 },
      ticket_fehler: null,
      bilder_gesendet: 2,
    });
    await flush();
    expect(screen.getByTestId("hilfe-bilder").textContent).toContain("persönliche Daten");
    expect(screen.getByText("0 von 5")).toBeInTheDocument();
    await waehlen(bildDatei("a.png"), bildDatei("b.jpg", "image/jpeg"));
    expect(screen.getByText("2 von 5")).toBeInTheDocument();
    expect(screen.getByAltText("Bild 1").getAttribute("src")).toBe(`data:image/jpeg;base64,${btoa("a.png")}`);
    expect(screen.getByTestId("hilfe-mitgeht").textContent).toContain("2 Bilder, als Anhang am Ticket");
    await ausfuellenUndSenden();
    expect(invokeMock).toHaveBeenCalledWith("hilfe_anfordern", {
      betreff: "Landung fehlt",
      text: "Nach dem Aufsetzen keine Note.",
      bilder: [{ daten: btoa("a.png") }, { daten: btoa("b.jpg") }],
    });
    expect(screen.getByTestId("hilfe-bilder-ergebnis").textContent).toBe("2 Bilder sind am Ticket angehängt.");
  });

  it("Bild entfernen: geht nicht mit", async () => {
    vorbereitenWieEcht();
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: { id: 1, ticket_number: "HD-1", url: null }, ticket_fehler: null });
    await flush();
    await waehlen(bildDatei("a.png"), bildDatei("b.png"));
    fireEvent.click(screen.getByRole("button", { name: "Bild 1 entfernen" }));
    expect(screen.queryByAltText("Bild 2")).toBeNull();
    expect(screen.getByText("1 von 5")).toBeInTheDocument();
    // der Platz ist wieder frei: noch vier gehen, das fünfte nicht
    await waehlen(...Array.from({ length: 5 }, (_, i) => bildDatei(`${i}.png`)));
    expect(screen.getByText("5 von 5")).toBeInTheDocument();
    for (let n = 5; n >= 2; n--) fireEvent.click(screen.getByRole("button", { name: `Bild ${n} entfernen` }));
    expect(screen.getByAltText("Bild 1").getAttribute("src")).toContain(btoa("b.png"));
    fireEvent.click(screen.getByRole("button", { name: "Bild 1 entfernen" }));
    await ausfuellenUndSenden();
    // ohne Bilder genau die bisherigen Argumente
    expect(invokeMock).toHaveBeenCalledWith("hilfe_anfordern", { betreff: "Landung fehlt", text: "Nach dem Aufsetzen keine Note." });
  });

  it("Einfügen aus der Zwischenablage und Ablegen per Ziehen", async () => {
    vorbereitenWieEcht();
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: null, ticket_fehler: null });
    await flush();
    const zwischenablage = { files: [bildDatei("eingefuegt.png")], items: [] };
    fireEvent.paste(screen.getByLabelText("Was ist passiert?"), { clipboardData: zwischenablage });
    await flush();
    await flush();
    expect(screen.getByAltText("Bild 1")).toBeInTheDocument();
    // auch ohne Fokus in einem Feld (Einfügen irgendwo auf der Seite)
    fireEvent.paste(document.body, { clipboardData: { files: [bildDatei("body.png")], items: [] } });
    await flush();
    await flush();
    expect(screen.getByAltText("Bild 2").getAttribute("src")).toContain(btoa("body.png"));
    fireEvent.drop(screen.getByTestId("hilfe-bilder"), { dataTransfer: { files: [bildDatei("gezogen.webp", "image/webp")], items: [] } });
    await flush();
    await flush();
    expect(screen.getByAltText("Bild 3").getAttribute("src")).toContain(btoa("gezogen.webp"));
    // Text einfügen bleibt Text (kein Bild, kein preventDefault)
    const ereignis = fireEvent.paste(screen.getByLabelText("Was ist passiert?"), {
      clipboardData: { files: [], items: [{ kind: "string", getAsFile: () => null }] },
    });
    expect(ereignis).toBe(true);
    expect(screen.queryByAltText("Bild 4")).toBeNull();
  });

  it("Grenzen: falscher Typ und mehr als fünf Bilder werden benannt", async () => {
    vorbereitenWieEcht();
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: null, ticket_fehler: null });
    await flush();
    await waehlen(bildDatei("x.gif", "image/gif"));
    expect(screen.getByRole("alert").textContent).toBe("Nur PNG, JPEG oder WebP lassen sich anhängen.");
    expect(vorbereitenMock).not.toHaveBeenCalled();
    await waehlen(...Array.from({ length: 6 }, (_, i) => bildDatei(`${i}.png`)));
    expect(screen.getByText("5 von 5")).toBeInTheDocument();
    expect(screen.getByRole("alert").textContent).toBe("Höchstens 5 Bilder je Anfrage.");
    expect(screen.getByRole("button", { name: "Bild auswählen …" })).toBeDisabled();
  });

  it("unlesbares Bild: Meldung, kein Eintrag, Platz wird wieder frei", async () => {
    vorbereitenMock.mockRejectedValue("lesen");
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: null, ticket_fehler: null });
    await flush();
    await waehlen(bildDatei("kaputt.png"));
    expect(screen.getByRole("alert").textContent).toBe("Das Bild ließ sich nicht lesen.");
    expect(screen.getByText("0 von 5")).toBeInTheDocument();
    vorbereitenWieEcht();
    await waehlen(...Array.from({ length: 5 }, (_, i) => bildDatei(`${i}.png`)));
    expect(screen.getByText("5 von 5")).toBeInTheDocument();
  });

  it("Ticket gescheitert: sagt, dass die Bilder NICHT angekommen sind", async () => {
    vorbereitenWieEcht();
    oeffnen({ kennung: "D-0123456789ab", link: null, diagnose_fehler: null, ticket: null, ticket_fehler: "ticket_helpdesk_fehlt", bilder_gesendet: 1 });
    await flush();
    await waehlen(bildDatei());
    await ausfuellenUndSenden();
    expect(screen.getByTestId("hilfe-bilder-ergebnis").textContent).toContain("nicht angekommen");
    // Zurück: Betreff, Text und Bild stehen noch da, erneut senden geht
    fireEvent.click(screen.getByRole("button", { name: "Zurück zur Anfrage" }));
    expect((screen.getByLabelText("Betreff") as HTMLInputElement).value).toBe("Landung fehlt");
    expect(screen.getByAltText("Bild 1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Senden" })).toBeEnabled();
  });

  it("Ticket angelegt: kein Zurück (sonst doppeltes Ticket)", async () => {
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: { id: 1, ticket_number: "HD-1", url: null }, ticket_fehler: null });
    await flush();
    await ausfuellenUndSenden();
    expect(screen.queryByRole("button", { name: "Zurück zur Anfrage" })).toBeNull();
  });

  it("HelpDesk ohne Bild-Unterstützung (anhaenge fehlt): ehrlich „0 von 1“", async () => {
    vorbereitenWieEcht();
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: { id: 2, ticket_number: "HD-2", url: null }, ticket_fehler: null, bilder_gesendet: 1 });
    await flush();
    await waehlen(bildDatei());
    await ausfuellenUndSenden();
    expect(screen.getByTestId("hilfe-bilder-ergebnis").textContent).toContain("Nur 0 von 1 Bildern");
  });

  it("Tablet (LAN-Brücke): kein Anhängen, Hinweis auf den Sim-PC", async () => {
    amSimPc = false;
    oeffnen({ kennung: null, link: null, diagnose_fehler: null, ticket: null, ticket_fehler: null });
    await flush();
    expect(screen.getByTestId("hilfe-bilder").textContent).toContain("nur am Sim-PC");
    expect(screen.queryByRole("button", { name: "Bild auswählen …" })).toBeNull();
  });

  it("F1 ohne Zusatztaste öffnet die Hilfe, sonst nicht", () => {
    const k = (over: Partial<KeyboardEvent>) =>
      istHilfeTaste({ key: "F1", repeat: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...over });
    expect(k({})).toBe(true);
    expect(k({ key: "F2" })).toBe(false);
    expect(k({ repeat: true })).toBe(false);
    expect(k({ ctrlKey: true })).toBe(false);
    expect(k({ metaKey: true })).toBe(false);
  });

  it("jeder Fehlerschlüssel aus Rust hat einen Text", () => {
    // Aus dem Quelltext (ohne Testmodul), damit ein neuer Code nicht still roh erscheint.
    const prod = HILFE_RS.split("#[cfg(test)]")[0];
    const codes = [...new Set([...prod.matchAll(/"((?:diagnose|ticket|hilfe)_[a-z_]+)"/g)].map((m) => m[1]))];
    // `ticket_number` ist ein Feld der HelpDesk-Antwort, kein Fehler.
    codes.splice(codes.indexOf("ticket_number"), 1);
    codes.push("not_logged_in");
    expect(codes.length).toBeGreaterThan(15);
    const fehler = (deCommon as unknown as { hilfe: { fehler: Record<string, string> } }).hilfe.fehler;
    expect(codes.filter((c) => typeof fehler[c] !== "string")).toEqual([]);
  });
});
