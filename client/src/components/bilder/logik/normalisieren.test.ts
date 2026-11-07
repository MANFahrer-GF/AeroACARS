import { describe, expect, it } from "vitest";
import { basisLesen } from "./normalisieren";

describe("basisLesen", () => {
  it("liest naechster_flugplatz aus dem Objekt von Rust und aus dem Text eines Vermerks", () => {
    expect(basisLesen({ naechster_flugplatz: { icao: "LSZH", abstand_m: 11482 } }).naechster_flugplatz).toBe("LSZH");
    expect(basisLesen({ naechster_flugplatz: "EDDF" }).naechster_flugplatz).toBe("EDDF");
    expect(basisLesen({ naechster_flugplatz: null }).naechster_flugplatz).toBeUndefined();
  });
});
