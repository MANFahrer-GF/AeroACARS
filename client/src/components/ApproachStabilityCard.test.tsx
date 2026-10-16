// Score-Version 19 (05.10.2026): Die Anflug-Karte zeigt das Stable Gate aus
// der Prüfliste des Datensatzes — Urteil und Grund, ohne nachzurechnen.
// Anlass GSG1709: „Stabiler Anflug" neben „teilweise stabil", nirgends ein Warum.
import { describe, it, expect, beforeAll } from "vitest";
import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { ApproachStabilityCard } from "./ApproachStabilityCard";
import type { GatePunkt } from "../lib/stableGate";

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

const gut = (key: string, wert: number, grenze: number): GatePunkt => ({
  key,
  stufe: "gut",
  wert,
  gut_unter: grenze,
});

/** QAF434: im Schnitt 1,69 Dots neben dem Gleitpfad, sonst ruhig. */
const qaf434: GatePunkt[] = [
  { key: "gleitpfad", stufe: "mittel", wert: 1.69, gut_unter: 1 },
  gut("fahrt", 0.97, 5),
  gut("querneigung", 1.51, 3),
  gut("ruck", 21.4, 100),
  { key: "sinken", stufe: "gut" },
  { key: "konfiguration", stufe: "gut" },
];

describe("ApproachStabilityCard mit Prüfliste (Score-Version 19)", () => {
  it("QAF434: teilweise stabil, und der Grund steht da", () => {
    render(<ApproachStabilityCard sampleCount={90} gate={qaf434} marke="anflug_partial" />);
    expect(screen.getByText("⚠ PARTIAL")).toBeTruthy();
    const gruende = screen.getByTestId("gate-gruende").textContent ?? "";
    expect(gruende).toContain("Gleitpfad: im Schnitt 1,69 Dots daneben (gut unter 1)");
    // Nur die eine auffällige Prüfung wird begründet.
    expect(gruende).not.toContain("Fahrt");
    // Sechs Kacheln, die alten Sinkraten-Abweichungen sind weg.
    expect(screen.getByText("Gleitpfad Ø")).toBeTruthy();
    expect(screen.queryByText(/V\/S vs/i)).toBeNull();
  });

  it("GSG1709: stabil, keine Begründungsliste", () => {
    const gsg1709 = qaf434.map((p) =>
      p.key === "gleitpfad" ? { ...p, stufe: "gut" as const, wert: 0.24 } : p,
    );
    render(<ApproachStabilityCard sampleCount={110} gate={gsg1709} />);
    expect(screen.getByText("✓ STABLE GATE")).toBeTruthy();
    expect(screen.queryByTestId("gate-gruende")).toBeNull();
  });

  it("Urteil kommt aus der Marke der Note, nicht aus den alten Feldern", () => {
    // Alte Felder würden UNSTABLE ergeben — die Note sagt STABLE (keine Marke).
    render(
      <ApproachStabilityCard
        sampleCount={90}
        vsDeviationFpm={300}
        maxVsDeviationBelow500Fpm={500}
        excessiveSink={true}
        gate={qaf434.map((p) => ({ ...p, stufe: "gut" as const }))}
      />,
    );
    expect(screen.getByText("✓ STABLE GATE")).toBeTruthy();
  });

  it("die Karte zählt nicht selbst — UNSTABLE nur, wenn die Note es sagt", () => {
    // Eine mittlere Prüfung, aber die eingefrorene Marke sagt „instabil":
    // die Karte folgt der Note (eine Zählregel, nur in Rust).
    render(<ApproachStabilityCard sampleCount={90} gate={qaf434} marke="anflug_unstable" />);
    expect(screen.getByText(/UNSTABLE/)).toBeTruthy();
  });
});

/** Score-Version 24: Wind im Anflug (JBU1500 MDPC→MUVR, Joel, 14.10.2026 —
 *  Werte aus dem Client-Protokoll, Gate nach der neuen Sinkregel). */
describe("ApproachStabilityCard: Wind im Anflug (Score-Version 24)", () => {
  const jbu1500Gate: GatePunkt[] = [
    gut("gleitpfad", 0.89, 1),
    gut("fahrt", 3.04, 5),
    gut("querneigung", 1.96, 3),
    gut("ruck", 55, 100),
    { key: "sinken", stufe: "gut" },
    { key: "konfiguration", stufe: "gut" },
  ];
  const jbu1500Wind = {
    wind_proben: 110,
    gegenwind_mittel_kt: 13.5,
    gegenwind_min_kt: 3.9,
    gegenwind_max_kt: 22.3,
    seitenwind_mittel_kt: -3.1,
    seitenwind_max_kt: 10.0,
    schwankung_kt: 3.8,
    gs_mittel_kt: 146.2,
    soll_sinkrate_fpm: -776,
    sinkgrenze_fpm: -1016,
    sinklauf: { dauer_s: 9.9, von_ft: 565, bis_ft: 351, spitze_fpm: -1442 },
  };

  it("JBU1500: Gegenwind, Seitenwind, Böigkeit und Soll-Sinkrate stehen da", () => {
    render(<ApproachStabilityCard sampleCount={110} gate={jbu1500Gate} wind={jbu1500Wind} />);
    const w = screen.getByTestId("anflug-wind").textContent ?? "";
    expect(w).toContain("zählt nicht in die Note");
    expect(w).toContain("Gegenwind im Mittel");
    expect(w).toContain("14 kt");
    expect(w).toContain("4 – 22 kt");
    expect(w).toContain("Seitenwind höchstens10 kt");
    expect(w).toContain("meist von links");
    expect(w).toContain("± 4 kt");
    expect(w).toContain("Soll-Sinkrate bei 146 kt über Grund780 fpm");
    expect(w).toContain("Grenze 1020 fpm");
    // Unter 10 s: kein Sinklauf-Satz, die Sinkrate war ok.
    expect(screen.queryByTestId("gate-gruende")).toBeNull();
  });

  it("Sinken schlecht: der Grund nennt Dauer, Höhen und Spitze statt des allgemeinen Satzes", () => {
    const gate = jbu1500Gate.map((p) =>
      p.key === "sinken" ? { ...p, stufe: "schlecht" as const } : p,
    );
    const wind = {
      ...jbu1500Wind,
      sinkgrenze_fpm: -1000,
      sinklauf: { dauer_s: 10.6, von_ft: 549, bis_ft: 340, spitze_fpm: -1442 },
    };
    render(<ApproachStabilityCard sampleCount={110} gate={gate} marke="anflug_partial" wind={wind} />);
    const g = screen.getByTestId("gate-gruende").textContent ?? "";
    expect(g).toContain(
      "Sinkrate 10,6 s am Stück stärker als 1000 fpm (549 → 340 ft über der Schwelle, bis 1440 fpm)",
    );
    expect(g).not.toContain("über 10 Sekunden am Stück zu hoch");
  });

  it("ältere Landung ohne Windmessung: Hinweis statt Nullen", () => {
    render(<ApproachStabilityCard sampleCount={110} gate={jbu1500Gate} />);
    const w = screen.getByTestId("anflug-wind").textContent ?? "";
    expect(w).toContain("noch nicht aufgezeichnet");
    expect(w).not.toContain("0 kt");
  });

  it("Simulator ohne Wind: keine Windkacheln, Soll-Sinkrate bleibt", () => {
    const wind = {
      wind_proben: 0,
      gs_mittel_kt: 146.2,
      soll_sinkrate_fpm: -776,
      sinkgrenze_fpm: -1016,
    };
    render(<ApproachStabilityCard sampleCount={110} gate={jbu1500Gate} wind={wind} />);
    const w = screen.getByTestId("anflug-wind").textContent ?? "";
    expect(w).toContain("keinen Wind geliefert");
    expect(w).not.toContain("Gegenwind");
    expect(w).toContain("780 fpm");
  });
});
