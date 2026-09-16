import { describe, expect, it } from "vitest";
import { VERALTET_AB_MIN, wetterAlter } from "./wetterAlter";

const jetzt = new Date("2026-09-16T12:00:00Z");
const vor = (min: number) => new Date(jetzt.getTime() - min * 60000).toISOString();

describe("wetterAlter", () => {
  it("zeigt frisches Wetter in Minuten", () => {
    expect(wetterAlter(vor(4), jetzt)).toMatchObject({
      minuten: 4,
      veraltet: false,
      text: { key: "weather.age_minutes", werte: { min: 4 } },
    });
  });

  it("sagt bei unter einer Minute 'gerade eben'", () => {
    expect(wetterAlter(vor(0), jetzt)?.text.key).toBe("weather.age_now");
  });

  it("kennzeichnet ab einer Stunde als veraltet", () => {
    expect(wetterAlter(vor(VERALTET_AB_MIN - 1), jetzt)?.veraltet).toBe(false);
    expect(wetterAlter(vor(VERALTET_AB_MIN), jetzt)?.veraltet).toBe(true);
  });

  it("wechselt bei langen Abständen auf Stunden und Tage", () => {
    expect(wetterAlter(vor(89), jetzt)?.text).toEqual({
      key: "weather.age_minutes",
      werte: { min: 89 },
    });
    expect(wetterAlter(vor(90), jetzt)?.text).toEqual({
      key: "weather.age_hours",
      werte: { std: 1 },
    });
    // Einzahl wird vermieden: bis 47 Std. bleibt es bei Stunden.
    expect(wetterAlter(vor(60 * 30), jetzt)?.text).toEqual({
      key: "weather.age_hours",
      werte: { std: 30 },
    });
    expect(wetterAlter(vor(60 * 48), jetzt)?.text).toEqual({
      key: "weather.age_days",
      werte: { tage: 2 },
    });
  });

  it("macht aus einem Zeitstempel in der Zukunft keine negative Angabe", () => {
    const zukunft = new Date(jetzt.getTime() + 20 * 60000).toISOString();
    expect(wetterAlter(zukunft, jetzt)).toMatchObject({
      minuten: 0,
      veraltet: false,
      text: { key: "weather.age_now" },
    });
  });

  it("zeigt lieber nichts als eine erfundene Angabe", () => {
    expect(wetterAlter(null, jetzt)).toBeNull();
    expect(wetterAlter("", jetzt)).toBeNull();
    expect(wetterAlter("kein Datum", jetzt)).toBeNull();
  });
});
