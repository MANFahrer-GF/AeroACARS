import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";

vi.mock("./ipc", () => ({ invoke: vi.fn(() => Promise.resolve(true)), isTauri: false }));

import { benoetigteHoehe, useFensterAnInhalt } from "./useFensterAnInhalt";
import { invoke } from "./ipc";

describe("benoetigteHoehe", () => {
  it("nimmt den Überstand der scrollenden Seite", () => {
    expect(benoetigteHoehe(770, { scrollHeight: 1030, clientHeight: 770 }, { scrollHeight: 770 })).toBe(1030);
  });

  it("nimmt den Überstand des Dokuments, wenn das Dokument scrollt", () => {
    expect(benoetigteHoehe(770, { scrollHeight: 1100, clientHeight: 1100 }, { scrollHeight: 1100 })).toBe(1100);
  });

  it("verlangt nie weniger als das Fenster schon hat", () => {
    expect(benoetigteHoehe(770, { scrollHeight: 500, clientHeight: 770 }, { scrollHeight: 600 })).toBe(770);
    expect(benoetigteHoehe(770, null, { scrollHeight: 0 })).toBe(770);
  });
});

describe("useFensterAnInhalt", () => {
  it("fasst außerhalb der Desktop-App (LAN-Tablet) kein Fenster an", () => {
    document.body.innerHTML = '<main class="app" style="height:5000px"></main>';
    renderHook(() => useFensterAnInhalt(true, "X"));
    expect(invoke).not.toHaveBeenCalled();
  });
});
