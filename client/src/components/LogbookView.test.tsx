import { describe, it, expect, beforeAll } from "vitest";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";
import { fmtNm } from "./LogbookView";

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

describe("fmtNm", () => {
  it("does not round a real distance down to zero", () => {
    expect(fmtNm(300)).toBe("300");
    expect(fmtNm(999)).toBe("999");
  });

  it("switches to thousands only once that reads better", () => {
    expect(fmtNm(1500)).toBe("1.5k");
    expect(fmtNm(42000)).toBe("42k");
  });

  it("shows a dash when there is nothing to show", () => {
    expect(fmtNm(undefined)).toBe("—");
  });
});
