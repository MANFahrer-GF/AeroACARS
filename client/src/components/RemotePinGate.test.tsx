// v0.19.x FIX: a 429 (rate-limited) response used to show the same "network
// error, are you on the same WiFi?" message as a real transport failure —
// the pilot kept retrying, extending their own lockout.

import { describe, it, expect, beforeAll, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import { HttpStatusError } from "../lib/ipc";

const authenticateWithPinMock = vi.fn();
vi.mock("../lib/ipc", async () => {
  const actual = await vi.importActual<typeof import("../lib/ipc")>("../lib/ipc");
  return {
    ...actual,
    authenticateWithPin: (...args: unknown[]) => authenticateWithPinMock(...args),
  };
});

import { RemotePinGate } from "./RemotePinGate";

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

async function submitPin(user: ReturnType<typeof userEvent.setup>, pin: string) {
  const input = screen.getByLabelText("PIN");
  await user.type(input, pin);
  await user.click(screen.getByRole("button", { name: /verbinden/i }));
}

describe("RemotePinGate error messaging", () => {
  it("shows the rate-limited message (not the network one) on HTTP 429", async () => {
    const user = userEvent.setup();
    authenticateWithPinMock.mockRejectedValueOnce(
      new HttpStatusError(429, "auth failed: HTTP 429"),
    );
    render(<RemotePinGate onAuthenticated={vi.fn()} />);

    await submitPin(user, "123456");

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/warte kurz/i),
    );
    expect(screen.queryByText(/selben WLAN/i)).not.toBeInTheDocument();
  });

  it("still shows the network message for a genuine transport failure", async () => {
    const user = userEvent.setup();
    authenticateWithPinMock.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    render(<RemotePinGate onAuthenticated={vi.fn()} />);

    await submitPin(user, "123456");

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/selben WLAN/i),
    );
  });

  it("still shows the bad-PIN message when the PIN is simply wrong", async () => {
    const user = userEvent.setup();
    authenticateWithPinMock.mockResolvedValueOnce(null);
    render(<RemotePinGate onAuthenticated={vi.fn()} />);

    await submitPin(user, "000000");

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/falsche pin/i),
    );
  });
});
