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
    render(<ApproachStabilityCard sampleCount={90} gate={qaf434} />);
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

  it("Urteil kommt aus der Prüfliste, nicht aus den alten Feldern", () => {
    // Alte Felder würden UNSTABLE ergeben — die Prüfliste sagt STABLE.
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
});
