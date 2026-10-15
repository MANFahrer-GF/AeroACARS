// Fahrplan-Band im Cockpit (15.10.2026) — was der Pilot sieht, am Bild geprüft.
import { afterEach, describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import type { FlugZeiten } from "../lib/flugzeiten";
import type { ActiveFlightInfo } from "../types";
import { FahrplanBand, bandFortschritt, bandStationen, kurzMinuten, lokalzeit } from "./FahrplanBand";
import { InfoStrip } from "./InfoStrip";

const T = Date.UTC(2026, 9, 15) / 1000;
const hm = (h: number, m: number) => T + h * 3600 + m * 60;
const PLAN: FlugZeiten = {
  plan_off_block: hm(17, 5),
  plan_abheben: hm(17, 20),
  plan_aufsetzen: hm(19, 50),
  plan_on_block: hm(19, 58),
};

afterEach(() => vi.useRealTimers());

function band(flugzeiten: FlugZeiten, jetzt: number, landung_erwartet?: number) {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(jetzt * 1000);
  const info = { flugzeiten, landung_erwartet } as unknown as ActiveFlightInfo;
  return render(<FahrplanBand info={info} />);
}

describe("Fahrplan-Band", () => {
  it("ohne SimBrief-Plan kein Band", () => {
    const { container } = band({ off_block: hm(17, 12) }, hm(18, 0));
    expect(container.innerHTML).toBe("");
  });

  it("Reiseflug 22 min hinter Plan: Ist gefüllt, Rest erwartet, Abweichung in Hinweisfarbe", () => {
    const z = { ...PLAN, off_block: hm(17, 21), abheben: hm(17, 44) };
    const { getByTestId, container } = band(z, hm(18, 40), hm(20, 12));
    expect(getByTestId("band-off_block").textContent).toContain("17:21z +16");
    expect(getByTestId("band-abheben").textContent).toContain("17:44z +24");
    expect(getByTestId("band-aufsetzen").textContent).toContain("erw. 20:12z +22");
    expect(getByTestId("band-on_block").textContent).toContain("erw. 20:20z +22");
    // Erreichte Stationen sind gefüllt, die anderen nicht.
    expect(container.querySelectorAll(".fahrplan-band__punkt--erreicht")).toHaveLength(2);
    // Alle vier Abweichungen liegen zwischen 15 und 60 min → Hinweisfarbe.
    expect(container.querySelectorAll(".fahrplan-band__delta--spaet")).toHaveLength(4);
    // Das Flugzeug fliegt im Abschnitt Abheben → Aufsetzen.
    expect(container.querySelector(".fahrplan-band__flugzeug")).not.toBeNull();
  });

  it("pünktlich (bis 15 min) ohne Farbe, deutlich zu früh in Infofarbe", () => {
    const puenktlich = band({ ...PLAN, off_block: hm(17, 12), abheben: hm(17, 31) }, hm(18, 40), hm(19, 53));
    expect(puenktlich.container.querySelector(".fahrplan-band__delta--spaet, .fahrplan-band__delta--frueh")).toBeNull();
    puenktlich.unmount();
    const frueh = band({ ...PLAN, off_block: hm(17, 4), abheben: hm(17, 14) }, hm(18, 40), hm(19, 28));
    expect(frueh.getByTestId("band-on_block").textContent).toContain("erw. 19:36z −22");
    expect(frueh.container.querySelectorAll(".fahrplan-band__delta--frueh").length).toBeGreaterThan(0);
  });

  it("am Gate vor dem Plan: nur Planzeiten, kein Flugzeug", () => {
    const { container, getByTestId } = band(PLAN, hm(16, 53));
    expect(getByTestId("band-off_block").textContent).toBe("Off-BlockPlan 17:05z ");
    expect(container.querySelector(".fahrplan-band__flugzeug")).toBeNull();
  });

  it("am Gate mit überfälligem Off-Block: die Verspätung steht schon am Off-Block", () => {
    const { getByTestId, container } = band(PLAN, hm(17, 25));
    expect(getByTestId("band-off_block").textContent).toContain("erw. 17:25z +20");
    expect(container.querySelector(".fahrplan-band__delta--spaet")).not.toBeNull();
  });

  it("ein OFP von gestern: kein Band", () => {
    const gestern: FlugZeiten = {
      plan_off_block: PLAN.plan_off_block! - 86400,
      plan_abheben: PLAN.plan_abheben! - 86400,
      plan_aufsetzen: PLAN.plan_aufsetzen! - 86400,
      plan_on_block: PLAN.plan_on_block! - 86400,
    };
    expect(band(gestern, hm(16, 50)).container.innerHTML).toBe("");
  });
});

describe("Fahrplan-Band — Rechnung", () => {
  it("Fortschritt: vor dem Abrollen keiner, im Reiseflug zwischen Abheben und Aufsetzen", () => {
    expect(bandFortschritt(bandStationen(PLAN, null, hm(16, 50)), hm(16, 50))).toBeNull();
    const z = { ...PLAN, off_block: hm(17, 12), abheben: hm(17, 31) };
    const p = bandFortschritt(bandStationen(z, hm(19, 53), hm(18, 40)), hm(18, 40))!;
    expect(p).toBeGreaterThan(1 / 3);
    expect(p).toBeLessThan(2 / 3);
    const angekommen = { ...z, aufsetzen: hm(19, 53), on_block: hm(20, 0) };
    expect(bandFortschritt(bandStationen(angekommen, null, hm(20, 5)), hm(20, 5))).toBe(1);
  });

  it("kurze Minuten mit echtem Minuszeichen", () => {
    expect(kurzMinuten(16)).toBe("+16");
    expect(kurzMinuten(-4)).toBe("−4");
    expect(kurzMinuten(0)).toBe("±0");
  });
});

describe("Ortszeit des Piloten (15.10.2026)", () => {
  it("lokalzeit rechnet in die Zeitzone des Rechners um", () => {
    expect(lokalzeit(hm(18, 40), "Europe/Berlin")).toBe("20:40");
    expect(lokalzeit(hm(18, 40), "America/New_York")).toBe("14:40");
    expect(lokalzeit(null)).toBe("—");
  });

  it("das Band zeigt jetzt in UTC und Ortszeit", () => {
    const { getByTestId } = band({ ...PLAN, off_block: hm(17, 12) }, hm(18, 40));
    const text = getByTestId("band-uhr").textContent ?? "";
    expect(text).toContain("Jetzt 18:40z");
    expect(text).toContain(`Lokal ${lokalzeit(hm(18, 40))}`);
  });

  it("die Trip-Uhr nennt die Hauptzeit auch in Ortszeit — mit Plan, nicht ohne", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(hm(18, 40) * 1000);
    const mit = { flugzeiten: { ...PLAN, off_block: hm(17, 12), abheben: hm(17, 31) }, landung_erwartet: hm(19, 53), touch_and_go_count: 0, go_around_count: 0, distance_nm: 0 } as unknown as ActiveFlightInfo;
    const a = render(<InfoStrip info={mit} snapshot={null} elapsedMinutes={0} />);
    expect(a.getByTestId("trip-uhr-lokal").textContent).toBe(`Lokal ${lokalzeit(hm(20, 1))}`);
    a.unmount();
    const ohne = { flugzeiten: null, touch_and_go_count: 0, go_around_count: 0, distance_nm: 0 } as unknown as ActiveFlightInfo;
    expect(render(<InfoStrip info={ohne} snapshot={null} elapsedMinutes={0} />).queryByTestId("trip-uhr-lokal")).toBeNull();
  });
});
