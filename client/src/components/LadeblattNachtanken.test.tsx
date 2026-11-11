// 3.0.1 (DLH 41 UUDD–EDDK, 10.11.2026): In der Luft kamen 20 762 kg dazu.
// `block_fuel_kg` trägt sie mit (22 146 + 20 762 = 42 908), gegen den Plan
// steht im Ladeblatt aber der Tank vom Anlassen.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { LadeblattAbschnitt, blockVomAnlassen } from "./LadeblattAbschnitt";

const zahl = (s: string) => s.replace(/[.,\s\u00a0\u202f]/g, "");

describe("Ladeblatt nach Nachtanken in der Luft", () => {
  const dlh41 = {
    ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord),
    sprit: null,
    block_fuel_kg: 42908,
    planned_block_fuel_kg: 22100,
    nachgetankt_luft_kg: 20762,
  } as LandingRecord;

  it("Block ist = Tank vom Anlassen, die Menge steht in ihrer Zeile", () => {
    expect(blockVomAnlassen(dlh41)).toBe(22146);
    const text = zahl(render(<LadeblattAbschnitt record={dlh41} />).container.textContent ?? "");
    expect(text).toContain("22146");
    expect(text).toContain("20762");
    expect(text).not.toContain("42908");
  });

  it("ohne Nachtanken und im Altbestand bleibt der Block, wie er ist", () => {
    expect(blockVomAnlassen({ block_fuel_kg: 9733, nachgetankt_luft_kg: null })).toBe(9733);
    expect(blockVomAnlassen({ block_fuel_kg: 9733 })).toBe(9733);
    expect(blockVomAnlassen({ block_fuel_kg: null, nachgetankt_luft_kg: 500 })).toBeNull();
  });
});
