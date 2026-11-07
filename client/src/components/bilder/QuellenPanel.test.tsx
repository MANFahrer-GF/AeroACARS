import fs from "node:fs";
import path from "node:path";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { QuellenPanel } from "./QuellenPanel";
import type { BilderStore } from "./logik/store";
import type { BilderListe } from "./logik/typen";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

const liste = { quellen: [], altbestand: [], tastenkuerzel: { text: "Ctrl+Shift+S", verfuegbar: true, aktiv: true, fehler: null, yoke: null, yoke_verfuegbar: false } } as unknown as BilderListe;

describe("Tastenkuerzel festlegen", () => {
  const start = () => {
    const tastenkuerzelSetzen = vi.fn(async () => undefined);
    render(<QuellenPanel liste={liste} vorschlaege={[]} store={{ tastenkuerzelSetzen } as unknown as BilderStore} onZu={() => undefined} simulator="msfs" yokeLernt={false} />);
    return { tastenkuerzelSetzen, knopf: screen.getByRole("button", { name: "bilder.quellen.kuerzel_setzen" }) };
  };

  it("kein Tippfeld: Knopf klicken, Tasten druecken, gespeichert", () => {
    const { tastenkuerzelSetzen, knopf } = start();
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.keyDown(window, { key: "F9", code: "F9", altKey: true });
    expect(tastenkuerzelSetzen).not.toHaveBeenCalled();
    fireEvent.click(knopf);
    fireEvent.keyDown(window, { key: "Alt", code: "AltLeft", altKey: true });
    expect(tastenkuerzelSetzen).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "F9", code: "F9", altKey: true });
    expect(tastenkuerzelSetzen).toHaveBeenCalledWith("Alt+F9");
  });

  it("ohne Modifikator ein Hinweis, Esc bricht ab", () => {
    const { tastenkuerzelSetzen, knopf } = start();
    fireEvent.click(knopf);
    fireEvent.keyDown(window, { key: "s", code: "KeyS" });
    expect(screen.getByText("bilder.quellen.yoke.kuerzel_nur_taste")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
    fireEvent.keyDown(window, { key: "S", code: "KeyS", ctrlKey: true });
    expect(tastenkuerzelSetzen).not.toHaveBeenCalled();
  });
});

// jsdom rechnet kein CSS: gemessen im Browser (459 von 459 px bei 1440x720); die Regel ist hier der Beleg.
it("das Panel klappt ganz auf, die Liste darunter gibt den Platz ab", () => {
  const css = fs.readFileSync(path.join(__dirname, "bilder.css"), "utf-8");
  expect(css).toMatch(/\.bt-quellen \{ flex: none; max-height: 100%; overflow-y: auto;/);
});
