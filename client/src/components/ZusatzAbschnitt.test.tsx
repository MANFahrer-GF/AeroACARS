// Zusatzfelder (V9, `zusatz-1`): was der Pilot sieht. Der Abschnitt kennt
// keinen Schluessel und zeigt nur Text.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { ZusatzAbschnitt, zusatzWertText } from "./ZusatzAbschnitt";

describe("Zusatzfelder — Anzeige", () => {
  it("ohne Zusatz, leer oder kein Objekt: kein Abschnitt", () => {
    for (const z of [undefined, null, {}, [1, 2] as unknown as Record<string, unknown>]) {
      const { container } = render(<ZusatzAbschnitt record={{ zusatz: z }} />);
      expect(container.innerHTML).toBe("");
    }
  });

  it("zeigt jeden Schluessel mit seinem Wert; Verschachteltes als JSON-Text", () => {
    const { container } = render(
      <ZusatzAbschnitt record={{ zusatz: { tch_extra_ft: 52.5, ok: true, notiz: "gut", leer: null, tief: { a: [1, 2] } } }} />,
    );
    const zeilen = [...container.querySelectorAll(".rohdaten__zeile")].map((z) => z.textContent ?? "");
    expect(zeilen).toHaveLength(5);
    expect(zeilen[0]).toBe("tch_extra_ft52.5");
    expect(zeilen[1]).toBe("oktrue");
    expect(zeilen[2]).toBe("notizgut");
    expect(zeilen[3]).toBe("leernull");
    expect(zeilen[4]).toContain('"a": [');
    expect(container.querySelector("summary")?.textContent).toBe("Zusatzfelder");
  });

  it("Text aus dem Modul wird nie als HTML eingesetzt", () => {
    const { container } = render(
      <ZusatzAbschnitt record={{ zusatz: { "<img src=x onerror=alert(1)>": "<script>alert(1)</script>" } }} />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(container.textContent).toContain("<script>alert(1)</script>");
  });

  it("zusatzWertText: einfache Werte unveraendert", () => {
    expect([1.5, "x", false, null].map(zusatzWertText)).toEqual(["1.5", "x", "false", "null"]);
  });
});
