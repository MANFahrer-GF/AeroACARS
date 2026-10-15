// Briefing-Zeiten (v2.0.20): SimBrief ist die Planung des Piloten und hat
// Vorrang vor phpVMS (Thomas 15.10.2026).
import { describe, expect, it } from "vitest";
import { briefingZeiten } from "./BidsList";

const T = Date.UTC(2026, 9, 15) / 1000;
const hm = (h: number, m: number) => T + h * 3600 + m * 60;
const ms = (h: number, m: number) => hm(h, m) * 1000;
const OFP = { sched_out_epoch: hm(17, 5), sched_in_epoch: hm(19, 58) };

describe("briefingZeiten", () => {
  it("SimBrief vor phpVMS", () => {
    const z = briefingZeiten({ dpt_time: "16:20", arr_time: "19:10" }, OFP, ms(16, 40));
    expect(z.dptTimeDisplay).toBe("17:05");
    expect(z.arrTimeDisplay).toBe("19:58");
    expect(z.dptCountdown).toMatchObject({ minutes: 25, overdue: false });
    expect(z.ofpOffBlockVorbei).toBe(false);
  });

  it("ohne OFP (oder OFP für einen anderen Flug) bleibt phpVMS", () => {
    const z = briefingZeiten({ dpt_time: "16:20", arr_time: "19:10" }, null, ms(16, 0));
    expect(z.dptTimeDisplay).toBe("16:20");
    expect(z.arrTimeDisplay).toBe("19:10");
    expect(z.ofpOffBlockVorbei).toBe(false);
  });

  it("Off-Block des OFP mehr als 15 min vorbei: Hinweis, OFP neu zu erstellen", () => {
    expect(briefingZeiten({}, OFP, ms(17, 20)).ofpOffBlockVorbei).toBe(false);
    expect(briefingZeiten({}, OFP, ms(17, 21)).ofpOffBlockVorbei).toBe(true);
  });

  it("ein überfälliges phpVMS-STD allein löst den Hinweis nicht aus", () => {
    expect(briefingZeiten({ dpt_time: "14:00" }, null, ms(17, 0)).ofpOffBlockVorbei).toBe(false);
  });
});
