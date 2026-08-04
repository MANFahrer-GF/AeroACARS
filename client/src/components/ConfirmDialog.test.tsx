// v0.19.x FIX — Enter must activate whichever button is focused, not
// unconditionally confirm. The bug: Cancel gets `autoFocus` as the safe
// default for destructive actions, but a global keydown handler still
// force-confirmed on Enter regardless of focus — pressing Enter to
// dismiss a "cancel flight" dialog actually confirmed it.

import { describe, it, expect, beforeAll } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { useConfirm } from "./ConfirmDialog";

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

function Harness({ onResult }: { onResult: (ok: boolean) => void }) {
  const { confirm, dialog } = useConfirm();
  return (
    <>
      {dialog}
      <button
        onClick={async () => {
          const ok = await confirm({ message: "Wirklich verwerfen?", destructive: true });
          onResult(ok);
        }}
      >
        open
      </button>
    </>
  );
}

async function openDialog(user: ReturnType<typeof userEvent.setup>, onResult: (ok: boolean) => void) {
  render(<Harness onResult={onResult} />);
  await user.click(screen.getByText("open"));
  await waitFor(() => expect(screen.getByText("Abbrechen")).toBeInTheDocument());
}

describe("ConfirmDialog Enter-key handling", () => {
  it("does NOT confirm when Cancel (the auto-focused button) has focus", async () => {
    const user = userEvent.setup();
    let result: boolean | null = null;
    await openDialog(user, (ok) => {
      result = ok;
    });

    await waitFor(() => expect(screen.getByText("Abbrechen")).toHaveFocus());
    await user.keyboard("{Enter}");

    await waitFor(() => expect(result).toBe(false));
  });

  it("confirms when the Confirm button has focus", async () => {
    const user = userEvent.setup();
    let result: boolean | null = null;
    await openDialog(user, (ok) => {
      result = ok;
    });

    // Move focus directly (rather than via user.tab()) — jsdom has no
    // layout, so the Modal's own focus-trap can't distinguish "one
    // focusable element" from "two" (its visibility check relies on
    // offsetParent, always null in jsdom) and would otherwise re-trap
    // focus on every Tab press. That's a jsdom-only artifact, not a
    // real-browser issue — irrelevant to what this test is pinning down.
    screen.getByText("Bestätigen").focus();
    await waitFor(() => expect(screen.getByText("Bestätigen")).toHaveFocus());
    await user.keyboard("{Enter}");

    await waitFor(() => expect(result).toBe(true));
  });

  it("falls back to confirm when focus is on neither dialog button", async () => {
    const user = userEvent.setup();
    let result: boolean | null = null;
    await openDialog(user, (ok) => {
      result = ok;
    });

    (document.activeElement as HTMLElement | null)?.blur();
    await user.keyboard("{Enter}");

    await waitFor(() => expect(result).toBe(true));
  });
});
