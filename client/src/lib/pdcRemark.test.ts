import { describe, it, expect } from "vitest";
import { cleanPdcRemark, PDC_REMARK_MAX } from "./pdcRemark";

describe("cleanPdcRemark", () => {
  it("uppercases, drops braces and collapses whitespace", () => {
    expect(cleanPdcRemark("req {de-icing}  pad\t2")).toBe("REQ DE-ICING PAD 2");
    expect(cleanPdcRemark("a}b{c")).toBe("ABC");
  });

  it("keeps a trailing space while typing but no leading one", () => {
    expect(cleanPdcRemark("  ready ")).toBe("READY ");
  });

  it("spells out German letters and drops other non-ASCII, like the backend", () => {
    expect(cleanPdcRemark("enteisung größer")).toBe("ENTEISUNG GROESSER");
    expect(cleanPdcRemark("über → pad ✈ 2")).toBe("UEBER PAD 2");
  });

  it("caps at the backend's limit", () => {
    expect(cleanPdcRemark("x".repeat(PDC_REMARK_MAX + 10))).toHaveLength(PDC_REMARK_MAX);
    expect(PDC_REMARK_MAX).toBe(48);
  });
});
