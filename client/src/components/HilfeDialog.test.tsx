// Hilfe anfordern (P-9.22, N-06): Der Pilot sieht vor dem Senden, was
// mitgeht; scheitert das Ticket (HelpDesk noch nicht veröffentlicht), bleibt
// die Diagnose-Kennung zum Weitergeben sichtbar.
import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import HILFE_RS from "../../src-tauri/src/hilfe.rs?raw";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { HilfeDialog, type HilfeErgebnis, type HilfeVorschau } from "./HilfeDialog";

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
});

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
  render(<HilfeDialog onClose={() => {}} />);
}

async function ausfuellenUndSenden() {
  fireEvent.change(screen.getByLabelText("Betreff"), { target: { value: "Landung fehlt" } });
  fireEvent.change(screen.getByLabelText("Was ist passiert?"), { target: { value: "Nach dem Aufsetzen keine Note." } });
  fireEvent.click(screen.getByRole("button", { name: "Senden" }));
  await flush();
}

describe("HilfeDialog", () => {
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
    render(<HilfeDialog onClose={() => {}} />);
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
