// v0.19.x FIX: the "last packet age" readout ("vor 12s" / "vor 3 min")
// was hardcoded German, shown verbatim regardless of the active locale
// even though the rest of this panel already goes through t(). Pins
// that it now follows the active language like everything else here.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import type { PmdgStatus } from "../types";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { PmdgPremiumPanel } from "./PmdgPremiumPanel";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon }, en: { common: enCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(async () => {
  cleanup();
  await i18next.changeLanguage("de");
});

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

function status(overrides: Partial<PmdgStatus> = {}): PmdgStatus {
  return {
    variant: "ng3",
    subscribed: true,
    ever_received: true,
    stale_secs: 12,
    looks_like_sdk_disabled: false,
    ...overrides,
  };
}

describe("PmdgPremiumPanel — packet-age label follows the active locale", () => {
  it("shows 'vor 12s' under de, 'ago' phrasing under en", async () => {
    invokeMock.mockResolvedValue(status({ stale_secs: 12 }));

    await i18next.changeLanguage("de");
    const de = render(<PmdgPremiumPanel simState="connected" simSnapshot={null} />);
    await flush();
    expect(de.getByText("vor 12s")).toBeInTheDocument();
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<PmdgPremiumPanel simState="connected" simSnapshot={null} />);
    await flush();
    expect(en.getByText("12s ago")).toBeInTheDocument();
    expect(en.queryByText("vor 12s")).not.toBeInTheDocument();
  });

  it("localizes the minutes phrasing too", async () => {
    invokeMock.mockResolvedValue(status({ stale_secs: 185 }));

    await i18next.changeLanguage("de");
    const de = render(<PmdgPremiumPanel simState="connected" simSnapshot={null} />);
    await flush();
    expect(de.getByText("vor 3 min")).toBeInTheDocument();
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<PmdgPremiumPanel simState="connected" simSnapshot={null} />);
    await flush();
    expect(en.getByText("3 min ago")).toBeInTheDocument();
  });

  it("shows the live indicator for a fresh packet, regardless of locale wording", async () => {
    invokeMock.mockResolvedValue(status({ stale_secs: 0 }));
    render(<PmdgPremiumPanel simState="connected" simSnapshot={null} />);
    await flush();
    expect(screen.getByText("📡 live")).toBeInTheDocument();
  });
});
