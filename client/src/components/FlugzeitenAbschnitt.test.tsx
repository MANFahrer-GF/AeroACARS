// „Zeiten gegen Flugplan" (13.10.2026): was der Pilot sieht — am Bild
// geprüft, nicht an der Rechnung (die steht in lib/flugzeiten.test.ts).
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import type { LandingRecord } from "../lib/landungsDatensatz";
import type { FlugZeiten } from "../lib/flugzeiten";
import type { ActiveFlightInfo } from "../types";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { FlugzeitenAbschnitt, FlugzeitenKurz } from "./FlugzeitenAbschnitt";
import { InfoStrip } from "./InfoStrip";

const T = Date.UTC(2026, 9, 13) / 1000;
const hm = (h: number, m: number) => T + h * 3600 + m * 60;
const BEISPIEL: FlugZeiten = {
  plan_off_block: hm(14, 30),
  plan_abheben: hm(14, 45),
  plan_aufsetzen: hm(15, 32),
  plan_on_block: hm(15, 40),
  off_block: hm(14, 41),
  abheben: hm(14, 58),
  aufsetzen: hm(15, 39),
  on_block: hm(15, 47),
};
const mit = (flugzeiten: FlugZeiten | null | undefined) =>
  ({ ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), flugzeiten }) as LandingRecord;

describe("Zeiten gegen Flugplan — Anzeige", () => {
  it("A: Kopfsatz, Tabelle und Airline-Maßstab wie im Entwurf", () => {
    const text = render(<FlugzeitenAbschnitt record={mit(BEISPIEL)} />).container.textContent ?? "";
    expect(text).toContain("On-Block 7 min nach Plan");
    expect(text).toContain("Blockzeit 4 min kürzer als geplant");
    expect(text).toContain("pünktlich nach Airline-Maßstab (bis 15 min)");
    for (const s of ["14:30z", "14:41z", "+11 min", "15:47z", "1:10", "1:06", "−4 min", "−6 min"]) {
      expect(text).toContain(s);
    }
  });

  it("A: über 15 min heißt es verspätet — weiterhin ohne Farbe", () => {
    const spaet = { ...BEISPIEL, on_block: hm(16, 5) };
    const { container } = render(<FlugzeitenAbschnitt record={mit(spaet)} />);
    expect(container.textContent).toContain("verspätet nach Airline-Maßstab (über 15 min)");
    expect(container.querySelector(".farbwert")).toBeNull();
  });

  it("D: ohne SimBrief-Plan steht das ausdrücklich da", () => {
    const text =
      render(<FlugzeitenAbschnitt record={mit({ off_block: hm(14, 41), on_block: hm(15, 47) })} />).container
        .textContent ?? "";
    expect(text).toContain("Kein SimBrief-OFP zu diesem Flug geladen");
    expect(text).not.toContain("min nach Plan");
  });

  it("Altbestand ohne das Feld: kein Abschnitt", () => {
    expect(render(<FlugzeitenAbschnitt record={mit(undefined)} />).container.innerHTML).toBe("");
  });

  it("B: Kurzform für Listen", () => {
    expect(render(<FlugzeitenKurz z={BEISPIEL} />).container.textContent).toBe("On-Block+7 minBlock1:06 / 1:10");
    expect(render(<FlugzeitenKurz z={{ on_block: hm(15, 47) }} />).container.innerHTML).toBe("");
  });

  it("C: Cockpit zeigt Plan, Erwartung und Aufgeholtes", () => {
    const info = {
      distance_nm: 120,
      position_count: 10,
      touch_and_go_count: 0,
      go_around_count: 0,
      flugzeiten: { ...BEISPIEL, aufsetzen: null, on_block: null },
      landung_erwartet: hm(15, 41),
    } as unknown as ActiveFlightInfo;
    const { getByTestId } = render(<InfoStrip info={info} snapshot={null} elapsedMinutes={30} />);
    const text = getByTestId("planzeiten-cockpit").textContent ?? "";
    expect(text).toContain("On-Block Plan15:40z");
    expect(text).toContain("On-Block erwartet15:49z");
    expect(text).toContain("Gegen Plan+9 min");
    expect(text).toContain("seit Abheben 4 min aufgeholt");
  });

  it("C: ohne Plan keine Zeilen im Cockpit", () => {
    const info = {
      distance_nm: 0,
      position_count: 0,
      touch_and_go_count: 0,
      go_around_count: 0,
      flugzeiten: { off_block: hm(14, 41) },
    } as unknown as ActiveFlightInfo;
    const { queryByTestId } = render(<InfoStrip info={info} snapshot={null} elapsedMinutes={0} />);
    expect(queryByTestId("planzeiten-cockpit")).toBeNull();
  });
});
