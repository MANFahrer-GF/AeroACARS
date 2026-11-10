import { describe, it, expect, beforeAll, afterEach } from "vitest";
import { render, cleanup, screen, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import { LogbuchProtokoll } from "./LogbuchProtokoll";
import { finanzen, gruppieren } from "./protokollDaten";
import type { Detail, PirepBuchung, PirepFeld } from "./logbuchForm";

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
afterEach(cleanup);

const feld = (slug: string, name: string, value: string): PirepFeld => ({ slug, name, value });
const b = (memo: string, ein: number, aus: number): PirepBuchung => ({ memo, credit_c: ein, debit_c: aus });

// Echte Werte von AIB446 (PIREP-Seite), Summen dort: 412.887,76 / 366.743,16.
const AIB446: PirepBuchung[] = [
  b("Cargo × 37.900 t", 40557140, 2027857), b("Economy × 10", 688461, 61961), b("Baggage Fees (10)", 29500, 2655),
  b("Beverages (5)", 3400, 306), b("Seat Upgrades (2)", 3000, 270), b("WiFi (2)", 3000, 270), b("Premium Meals (2)", 2700, 243), b("Duty Free (1)", 1575, 142),
  b("Fuel (72.873 kg)", 0, 21377353), b("Aircraft Cost (Leased)", 0, 10924150), b("Maintenance (Flight Hours)", 0, 520838), b("Landing Fee PHNL", 0, 396750),
  b("ATC / Air Traffic Control", 0, 316250), b("Overflight Fees", 0, 241317), b("Fuel Service Fee", 0, 218619), b("Fuel Tax (International 1.00%)", 0, 215960),
  b("De-Icing", 0, 120000), b("Pilot Pay Payout", 0, 112875), b("Maintenance (Cycle)", 0, 102000), b("Oceanic Crossing Fee", 0, 12500),
  b("Catering (10 PAX)", 0, 12000), b("Dispatch/Operations", 0, 5000), b("Crew Scheduling", 0, 2500), b("Cabin Service/Supplies", 0, 2500),
];

describe("gruppieren", () => {
  it("ordnet nach Thema und innerhalb in fester Reihenfolge, egal wie die Felder ankommen", () => {
    const g = gruppieren([feld("landing-weight", "Landing Weight", "339744 kg"), feld("takeoff-time", "Takeoff Time", "14:31"), feld("blocks-off-time", "Blocks Off Time", "14:19"), feld("aircraft-type", "Aircraft Type", "A388")]);
    expect(g.map((x) => x.gruppe)).toEqual(["zeiten", "gewichte"]);
    expect(g[0].zeilen.map((z) => z.slug)).toEqual(["blocks-off-time", "takeoff-time"]);
    expect(g[1].zeilen.map((z) => z.slug)).toEqual(["aircraft-type", "landing-weight"]);
  });

  it("legt Unbekanntes in Weitere und lässt nur leere Platzhalter weg", () => {
    const g = gruppieren([feld("modul-xy", "Modul XY", "42"), feld("taxi-in-time", "Taxi In Time", "  "), feld("source", "Source", "AeroACARS")]);
    expect(g.map((x) => x.gruppe)).toEqual(["technik", "weitere"]);
    expect(g[1].zeilen[0].name).toBe("Modul XY");
  });

  it("kommt mit fehlenden Feldern zurecht", () => {
    expect(gruppieren(undefined)).toEqual([]);
  });
});

describe("finanzen", () => {
  it("rechnet AIB446 wie die PIREP-Seite", () => {
    const f = finanzen(AIB446);
    expect(f.summeEin).toBe(41288776);
    expect(f.summeAus).toBe(36674316);
    expect(f.ergebnis).toBe(4614460);
    expect(f.einnahmen).toHaveLength(8);
    expect(f.kosten).toHaveLength(16);
  });

  it("sortiert nach Betrag und lässt Nullzeilen weg", () => {
    const f = finanzen([b("klein", 0, 100), b("null", 0, 0), b("gross", 0, 900), b("e1", 10, 0), b("e2", 20, 5)]);
    expect(f.kosten.map((x) => x.memo)).toEqual(["gross", "klein"]);
    expect(f.einnahmen.map((x) => x.memo)).toEqual(["e2", "e1"]);
  });
});

const basis: Detail = { id: "p1", log: [{ t: 0, message: "Phase: Boarding" }, { t: 60000, message: "<b>Touchdown</b>" }] };

describe("LogbuchProtokoll", () => {
  it("zeigt Felder, Finanzen und Log eines vollständigen Flugs", () => {
    render(<LogbuchProtokoll detail={{
      ...basis, currency: "EUR", units: { weight: "kg" }, transactions: AIB446,
      fares: [{ code: "Y", name: "Economy", count: 10, type: 0 }, { code: "CGO", name: "Cargo", count: 37900, type: 1 }, { code: "J", name: "Business", count: 0, type: 0 }],
      fields: [feld("block-fuel", "Block Fuel", "88574 kg"), feld("neu-von-modul", "Neu von Modul", "wert")],
    }} />);
    const finanzenTeil = within(screen.getByRole("region", { name: "Finanzen" }));
    expect(finanzenTeil.getByText("Gewinn")).toBeTruthy();
    expect(finanzenTeil.getByText(/46\.144,60/)).toBeTruthy();
    expect(finanzenTeil.getByText(/412\.887,76/)).toBeTruthy();
    expect(finanzenTeil.getByText(/366\.743,16/)).toBeTruthy();
    expect(finanzenTeil.getByText("10 Pax")).toBeTruthy();
    expect(finanzenTeil.getByText("37.900 kg")).toBeTruthy();
    expect(document.body.textContent).not.toContain("Business");
    const felderTeil = within(screen.getByRole("region", { name: "PIREP-Felder" }));
    expect(felderTeil.getByText("2 Felder, nach Themen geordnet")).toBeTruthy();
    expect(felderTeil.getByText("Sprit")).toBeTruthy();
    expect(felderTeil.getByText("Weitere")).toBeTruthy();
    expect(felderTeil.getByText("wert")).toBeTruthy();
    expect(document.querySelector(".aa-lb-prot")?.classList.contains("aa-lb-prot--zwei")).toBe(true);
  });

  it("nennt einen Verlust so und färbt ihn rot", () => {
    render(<LogbuchProtokoll detail={{ ...basis, currency: "EUR", transactions: [b("Fuel", 0, 12345)] }} />);
    expect(screen.getByText("Verlust")).toBeTruthy();
    expect(document.querySelector(".aa-lb-prot-ergebnis")?.classList.contains("ist-minus")).toBe(true);
  });

  it("alter PIREP: ohne Felder und Finanzen bleibt das Log, einspaltig", () => {
    render(<LogbuchProtokoll detail={basis} />);
    expect(document.body.textContent).toContain("Phase: Boarding");
    expect(screen.queryByRole("region", { name: "Finanzen" })).toBeNull();
    expect(screen.queryByRole("region", { name: "PIREP-Felder" })).toBeNull();
    expect(document.querySelector(".aa-lb-prot--zwei")).toBeNull();
  });

  it("Log bleibt escaped", () => {
    render(<LogbuchProtokoll detail={basis} />);
    expect(document.querySelector(".aa-lb-m b")).toBeNull();
    expect(document.body.textContent).toContain("<b>Touchdown</b>");
  });

  it("gar nichts da: sagt es, statt leer zu bleiben", () => {
    render(<LogbuchProtokoll detail={{ id: "p1" }} />);
    expect(screen.getByText("Für diesen Flug liegen keine Protokolldaten vor.")).toBeTruthy();
  });
});
