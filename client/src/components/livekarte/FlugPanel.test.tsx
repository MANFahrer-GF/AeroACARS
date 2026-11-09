// Das Fluginfo-Panel der Live-Karte: Werte, Fortschritt, Muster, einklappbar.
//
// Abnahmeliste Live-Karte, Zeile B11 (docs/spec/livekarte-umbau-abnahme.md): Phase, Hoehe, GS, Rest,
// ETA, Fortschritt, Muster/Registrierung; einklappbar. Das Panel ist eine reine Komponente und wird
// hier ohne Karte gerendert. Die Phasenfarbe kommt aus dem echten `phaseColor`.

import { describe, it, expect, beforeAll, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";
import { FlugPanel } from "./FlugPanel";
import { phaseColor } from "../../lib/phaseColors";

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
beforeEach(() => localStorage.clear());
afterEach(() => cleanup());

const flug = {
  pirep_id: "P1", airline_icao: "DLH", flight_number: "400", dpt_airport: "EDDF", arr_airport: "KJFK",
  phase: "cruise", distance_nm: 300, aircraft_name: "Airbus A350-900", aircraft_icao: "A359",
  planned_registration: "D-AIXA",
};
const werte = { hoehe: "FL380", gs: "487 kts", rest: "100 nm", eta: "21:42z", restNm: 100 as number | null | undefined };
const ereignisse = <div data-testid="ereignisse">Liste</div>;
const panel = (f: Record<string, unknown> = {}, w: Partial<typeof werte> = {}) => (
  <FlugPanel flug={{ ...flug, ...f } as never} phase="Reiseflug" werte={{ ...werte, ...w }} ereignisse={ereignisse} />
);

/** Der Wert neben einer Beschriftung, so wie ihn der Pilot liest. */
function wertZu(beschriftung: string): string | null | undefined {
  const label = [...document.querySelectorAll(".aa-livemap-flightcard__label")].find((e) => e.textContent === beschriftung);
  return label?.parentElement?.querySelector(".aa-livemap-flightcard__value")?.textContent;
}
const fuellung = () => (document.querySelector(".aa-livemap-flightcard__bar-fill") as HTMLElement).style.width;

describe("Fluginfo-Panel (B11)", () => {
  it("zeigt Phase, Hoehe, GS, Rest und ETA mit ihren Beschriftungen", () => {
    render(panel());
    expect(wertZu("PHASE")).toBe("Reiseflug");
    expect(wertZu("HÖHE")).toBe("FL380");
    expect(wertZu("GS")).toBe("487 kts");
    expect(wertZu("REST")).toBe("100 nm");
    expect(wertZu("ETA")).toBe("21:42z");
  });

  it("faerbt die Phase in der Farbe ihrer Phase", () => {
    render(panel({ phase: "descent" }));
    const phase = [...document.querySelectorAll(".aa-livemap-flightcard__phase-row .aa-livemap-flightcard__value")][0] as HTMLElement;
    expect(phaseColor("descent")).toBe("#3b82f6");
    expect(phase.style.color).toBe("rgb(59, 130, 246)");
  });

  it("nennt Abflug- und Zielplatz, Rubrik und LIVE-Marke", () => {
    render(panel());
    expect([...document.querySelectorAll(".aa-livemap-flightcard__airport")].map((e) => e.textContent)).toEqual(["EDDF", "KJFK"]);
    expect(screen.getByText("EIGENER FLUG")).toBeTruthy();
    expect(screen.getByText("LIVE")).toBeTruthy();
  });

  it("fuellt den Fortschrittsbalken mit dem Anteil des geflogenen Wegs an der ganzen Strecke", () => {
    // `distance_nm` ist die bereits GEFLOGENE Strecke (Rust: stats.distance_nm), `restNm` die noch zu fliegende.
    const { rerender } = render(panel({ distance_nm: 300 }, { restNm: 100 }));
    expect(fuellung()).toBe("75%");
    rerender(panel({ distance_nm: 100 }, { restNm: 300 }));
    expect(fuellung()).toBe("25%");
    rerender(panel({ distance_nm: 250 }, { restNm: 250 }));
    expect(fuellung()).toBe("50%");
  });

  it("steht am Anfang auf 0 % und am Ziel auf 100 %, ohne Rest-Angabe auf 0 %", () => {
    const { rerender } = render(panel({ distance_nm: 0 }, { restNm: 400 }));
    expect(fuellung()).toBe("0%");
    rerender(panel({ distance_nm: 400 }, { restNm: 0 }));
    expect(fuellung()).toBe("100%");
    rerender(panel({ distance_nm: 400 }, { restNm: null }));
    expect(fuellung()).toBe("0%");
    rerender(panel({ distance_nm: 400 }, { restNm: undefined }));
    expect(fuellung()).toBe("0%");
  });

  it("zeigt Muster und Registrierung aus dem geplanten Flugzeug", () => {
    render(panel());
    expect(document.querySelector(".aa-livemap-flightcard__foot")?.textContent).toBe("Airbus A350-900 · D-AIXA");
  });

  it("faellt ohne Namen auf den ICAO-Typ zurueck, zeigt nur die Registrierung allein und sonst keine Fusszeile", () => {
    const { rerender } = render(panel({ aircraft_name: "" }));
    expect(document.querySelector(".aa-livemap-flightcard__foot")?.textContent).toBe("A359 · D-AIXA");
    rerender(panel({ aircraft_name: "", aircraft_icao: "", planned_registration: "D-AIXA" }));
    expect(document.querySelector(".aa-livemap-flightcard__foot")?.textContent).toBe("D-AIXA");
    rerender(panel({ aircraft_name: "", aircraft_icao: "", planned_registration: "" }));
    expect(document.querySelector(".aa-livemap-flightcard__foot")).toBeNull();
  });

  it("reicht die Ereignisliste unter dem Panel durch, solange es offen ist", () => {
    render(panel());
    expect(screen.getByTestId("ereignisse")).toBeTruthy();
  });

  it("klappt ein: Werte, Fortschritt, Muster und Ereignisse verschwinden, Strecke bleibt", () => {
    render(panel());
    const klappe = screen.getByRole("button", { name: "Panel einklappen" });
    expect(klappe.getAttribute("aria-expanded")).toBe("true");
    expect(klappe.getAttribute("title")).toBe("Panel einklappen");

    fireEvent.click(klappe);
    const auf = screen.getByRole("button", { name: "Panel aufklappen" });
    expect(auf.getAttribute("aria-expanded")).toBe("false");
    expect(document.querySelector(".aa-livemap-flightcard__grid")).toBeNull();
    expect(document.querySelector(".aa-livemap-flightcard__bar")).toBeNull();
    expect(document.querySelector(".aa-livemap-flightcard__foot")).toBeNull();
    expect(screen.queryByTestId("ereignisse")).toBeNull();
    expect([...document.querySelectorAll(".aa-livemap-flightcard__airport")].map((e) => e.textContent)).toEqual(["EDDF", "KJFK"]);

    fireEvent.click(auf);
    expect(wertZu("HÖHE")).toBe("FL380");
    expect(screen.getByTestId("ereignisse")).toBeTruthy();
  });

  it("merkt sich das Einklappen ueber das Schliessen und Oeffnen der Karte", () => {
    const erste = render(panel());
    fireEvent.click(screen.getByRole("button", { name: "Panel einklappen" }));
    expect(localStorage.getItem("aaLivemapPanelZu")).toBe("1");
    erste.unmount();
    render(panel());
    expect(screen.getByRole("button", { name: "Panel aufklappen" })).toBeTruthy();
    expect(document.querySelector(".aa-livemap-flightcard__grid")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Panel aufklappen" }));
    expect(localStorage.getItem("aaLivemapPanelZu")).toBe("0");
  });
});
