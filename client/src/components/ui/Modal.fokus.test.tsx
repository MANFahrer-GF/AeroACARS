import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Modal } from "./index";

describe("Modal: Fokus", () => {
  it("bleibt im Textfeld, wenn der Aufrufer neu zeichnet und ein neues onClose gibt", () => {
    const zeichne = (onClose: () => void) => <Modal open onClose={onClose} title="T"><button>erst</button><textarea aria-label="text" /></Modal>;
    const { rerender } = render(zeichne(() => undefined));
    const feld = screen.getByLabelText("text");
    feld.focus();
    expect(document.activeElement).toBe(feld);
    const zuletzt = vi.fn();
    rerender(zeichne(zuletzt));
    expect(document.activeElement).toBe(feld);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(zuletzt).toHaveBeenCalledOnce();
  });
});
