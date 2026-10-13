import { describe, expect, it } from "vitest";
import {
  cockpitSchaetzung,
  dauern,
  hatPlan,
  kopf,
  minutenText,
  uhrzeit,
  zeitachse,
  zeitZeilen,
  type FlugZeiten,
} from "./flugzeiten";

// Beispielflug des Entwurfs: GSG 1709 EDDF → EDDM am 13.10.2026.
const T = Date.UTC(2026, 9, 13, 0, 0, 0) / 1000;
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

describe("Zeiten gegen Flugplan", () => {
  it("rechnet die Abweichungen des Entwurfs nach", () => {
    expect(zeitZeilen(BEISPIEL).map((z) => z.abw_min)).toEqual([11, 13, 7, 7]);
    expect(dauern(BEISPIEL)).toEqual({ block_plan: 70, block_ist: 66, flug_plan: 47, flug_ist: 41 });
    expect(kopf(BEISPIEL)).toEqual({ schritt: "on_block", abw_min: 7, block_diff_min: -4 });
  });

  it("schreibt UTC-Uhrzeit und Minuten mit Vorzeichen", () => {
    expect(uhrzeit(hm(14, 30))).toBe("14:30z");
    expect(minutenText(7)).toBe("+7 min");
    expect(minutenText(-4)).toBe("−4 min");
    expect(minutenText(0)).toBe("±0 min");
  });

  it("ohne Plan gibt es keinen Kopfsatz", () => {
    const ohne: FlugZeiten = { off_block: hm(14, 41), on_block: hm(15, 47) };
    expect(hatPlan(ohne)).toBe(false);
    expect(kopf(ohne)).toBeNull();
  });

  it("nimmt den spätesten Schritt mit Plan und Ist, wenn On-Block fehlt", () => {
    const k = kopf({ ...BEISPIEL, on_block: null });
    expect(k?.schritt).toBe("aufsetzen");
    expect(k?.block_diff_min).toBeNull();
  });

  it("legt Plan und Ist auf eine gemeinsame Achse", () => {
    const a = zeitachse(BEISPIEL)!;
    expect(a.plan.map((b) => b.art)).toEqual(["rollen", "flug", "rollen"]);
    expect(a.ist).toHaveLength(3);
    // Alles innerhalb der Achse, Ist beginnt nach dem Plan.
    for (const b of [...a.plan, ...a.ist]) {
      expect(b.von).toBeGreaterThanOrEqual(0);
      expect(b.bis).toBeLessThanOrEqual(1);
    }
    expect(a.ist[0].von).toBeGreaterThan(a.plan[0].von);
    expect(a.striche.length).toBeGreaterThanOrEqual(2);
    expect(a.striche.length).toBeLessThanOrEqual(6);
  });

  describe("Schätzung im Cockpit", () => {
    it("in der Luft: Planzeit ab dem Ort plus geplante Rollzeit", () => {
      const z = { ...BEISPIEL, aufsetzen: null, on_block: null };
      const s = cockpitSchaetzung(z, hm(15, 41))!;
      expect(s.quelle).toBe("route");
      expect(uhrzeit(s.erwartet)).toBe("15:49z");
      expect(s.gegen_plan_min).toBe(9);
      // Beim Abheben +13, jetzt +9 → 4 min aufgeholt.
      expect(s.aufgeholt_min).toBe(4);
    });

    it("vor dem Abheben: Off-Block plus geplante Blockzeit", () => {
      const z: FlugZeiten = { ...BEISPIEL, abheben: null, aufsetzen: null, on_block: null };
      const s = cockpitSchaetzung(z, null)!;
      expect(s.quelle).toBe("off_block");
      expect(s.gegen_plan_min).toBe(11);
      expect(s.aufgeholt_min).toBeNull();
    });

    it("nach dem Abstellen gilt das echte On-Block", () => {
      expect(cockpitSchaetzung(BEISPIEL, hm(15, 10))?.gegen_plan_min).toBe(7);
    });

    it("ohne geplantes On-Block keine Schätzung", () => {
      expect(cockpitSchaetzung({ off_block: hm(14, 41) }, null)).toBeNull();
    });
  });
});
