import { describe, expect, it } from "vitest";
import { kuerzelLesen, type TastenDruck } from "./kuerzel";

const d = (key: string, code: string, m: Partial<TastenDruck> = {}): TastenDruck => ({ key, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...m });

describe("kuerzelLesen", () => {
  it("baut die Schreibweise des Kerns", () => {
    expect(kuerzelLesen(d("S", "KeyS", { ctrlKey: true, shiftKey: true }))).toEqual({ kuerzel: "Ctrl+Shift+S" });
    expect(kuerzelLesen(d("F9", "F9", { altKey: true }))).toEqual({ kuerzel: "Alt+F9" });
    expect(kuerzelLesen(d("!", "Digit1", { shiftKey: true, ctrlKey: true }))).toEqual({ kuerzel: "Ctrl+Shift+1" });
  });
  it("QWERTZ: die beschriftete Taste zaehlt, nicht ihre Lage", () => {
    expect(kuerzelLesen(d("z", "KeyY", { ctrlKey: true }))).toEqual({ kuerzel: "Ctrl+Z" });
  });
  it("Modifikatoren allein warten, ohne Modifikator kommt der Hinweis, Unbekanntes wartet", () => {
    expect(kuerzelLesen(d("Control", "ControlLeft", { ctrlKey: true }))).toBeNull();
    expect(kuerzelLesen(d("s", "KeyS"))).toBe("modifikator");
    expect(kuerzelLesen(d("F25", "F25", { ctrlKey: true }))).toBeNull();
    expect(kuerzelLesen(d("ArrowUp", "ArrowUp", { ctrlKey: true }))).toBeNull();
  });
});
