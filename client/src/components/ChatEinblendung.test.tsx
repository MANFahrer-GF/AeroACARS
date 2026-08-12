// Die Einblendung bei einem eingehenden Zuruf.
//
// Feldbefund Thomas (12.08.2026): „bekommen die Piloten bei
// Direktnachrichten keine visuelle Benachrichtigung, wenn sie nicht im
// Chat sind? Ton kommt, aber wenn man nicht weiß, was das für ein Ton ist,
// naja." Vorher gab es nur den Ton und ein Zählerplättchen an der
// Seitenleiste — im Vollbild also ein Geräusch ohne Absender.
//
// Zwei Eigenschaften sind hier sicherheitsrelevant und nicht Kosmetik:
//   1. In den stillen Phasen (Start, Endanflug, Landung) erscheint nichts.
//      Im Endanflug hat kein Zuruf das Recht, die Aufmerksamkeit zu holen.
//   2. Derselbe Zuruf blendet nicht zweimal ein.

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_k: string, arg?: unknown) =>
      typeof arg === "string" ? arg : (arg as { defaultValue?: string })?.defaultValue ?? _k,
  }),
}));

import { ChatEinblendung, type EingehenderZuruf } from "./ChatEinblendung";

const ZURUF: EingehenderZuruf = {
  id: 42,
  text: "Sitzt du schon im Flieger?",
  von_pilot_id: "2",
  an_pilot_id: "1",
  callsign: "EZY 5077",
  anzeigename: "Michel D",
};

afterEach(() => cleanup());

describe("Zuruf-Einblendung", () => {
  it("zeigt Absender und Text, nicht bloß „neue Nachricht“", () => {
    render(<ChatEinblendung zuruf={ZURUF} phase="CRUISE" onOeffnen={() => {}} />);
    expect(screen.getByText(/Michel D/)).toBeTruthy();
    expect(screen.getByText("Sitzt du schon im Flieger?")).toBeTruthy();
  });

  it("kennzeichnet eine Direktnachricht als solche", () => {
    render(<ChatEinblendung zuruf={ZURUF} phase="CRUISE" onOeffnen={() => {}} />);
    expect(screen.getByText(/Direkt an dich/)).toBeTruthy();
  });

  it("kennzeichnet die Flugleitung", () => {
    render(
      <ChatEinblendung
        zuruf={{ ...ZURUF, von_pilot_id: "__ops", anzeigename: "Thomas" }}
        phase="CRUISE" onOeffnen={() => {}}
      />,
    );
    expect(screen.getByText(/Flugleitung/)).toBeTruthy();
  });

  it("bleibt im Endanflug und in der Landung weg", () => {
    for (const phase of ["FINAL", "LANDING", "TAKEOFF", "TAKEOFF_ROLL"]) {
      const { container } = render(
        <ChatEinblendung zuruf={ZURUF} phase={phase} onOeffnen={() => {}} />,
      );
      expect(container.textContent, `in ${phase} wurde eingeblendet`).toBe("");
      cleanup();
    }
  });

  it("verschwindet von selbst", () => {
    vi.useFakeTimers();
    render(<ChatEinblendung zuruf={ZURUF} phase="CRUISE" onOeffnen={() => {}} />);
    expect(screen.getByText("Sitzt du schon im Flieger?")).toBeTruthy();
    act(() => { vi.advanceTimersByTime(8000); });
    expect(screen.queryByText("Sitzt du schon im Flieger?")).toBeNull();
    vi.useRealTimers();
  });

  it("führt in den Chat, wenn man sie anklickt", () => {
    const geoeffnet = vi.fn();
    render(<ChatEinblendung zuruf={ZURUF} phase="CRUISE" onOeffnen={geoeffnet} />);
    fireEvent.click(screen.getByRole("button", { name: "Öffnen" }));
    expect(geoeffnet).toHaveBeenCalled();
    expect(screen.queryByText("Sitzt du schon im Flieger?")).toBeNull();
  });

  it("blendet denselben Zuruf nicht zweimal ein", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <ChatEinblendung zuruf={ZURUF} phase="CRUISE" onOeffnen={() => {}} />,
    );
    act(() => { vi.advanceTimersByTime(8000); });
    expect(screen.queryByText("Sitzt du schon im Flieger?")).toBeNull();
    // Dieselbe Nachricht erneut — etwa nach einem Neuaufbau der Ansicht.
    rerender(<ChatEinblendung zuruf={{ ...ZURUF }} phase="CRUISE" onOeffnen={() => {}} />);
    expect(screen.queryByText("Sitzt du schon im Flieger?")).toBeNull();
    vi.useRealTimers();
  });
});
