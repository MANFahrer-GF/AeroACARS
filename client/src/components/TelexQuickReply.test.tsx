// v1.3.0 (#Hoppie-PDC-CPDLC) — PDC acknowledgement rules.
//
// Pinned to what the CONTROLLER side actually does, not to what looks
// realistic:
//   - vSMR (VATSIM UK plugin, SMRPlugin.cpp:176) marks a clearance
//     acknowledged on the substrings WILCO / ROGER / RGR alone. It reads
//     no items, so a squawk readback communicates nothing.
//   - The same file tests its REQUEST branch FIRST (:168), matching
//     CLR / REQ / PDC / PREDEP / REQUEST — a reply containing any of
//     those is misread as a new clearance request.
//   - FAA/L3Harris pilot handbook p.4: the reply is "ACCEPT/WILCO,
//     ROGER, or REJECT/UNABLE", and free text to ATC is prohibited.
// An earlier version prefilled "WILCO SQUAWK 2200". That was invented.

import { describe, it, expect, beforeAll, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => String(e),
}));

import { TelexQuickReply } from "./CpdlcQuickReply";

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

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

const t = (k: string) => i18next.t(k);
const CLEARANCE = "DLH4TK CLRD TO EDDM VIA DCT SQUAWK 2200 INITIAL CLB FL050";

describe("PDC acknowledgement", () => {
  it("prefills a bare WILCO, without echoing the squawk", async () => {
    render(<TelexQuickReply recipient="EDDP" clearance={CLEARANCE} onReplied={() => {}} />);

    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_readback") }));

    const field = screen.getByRole("textbox") as HTMLInputElement;
    expect(field.value).toBe("WILCO");
    expect(field.value).not.toMatch(/SQUAWK|2200/);
  });

  it("sends exactly WILCO", async () => {
    render(<TelexQuickReply recipient="EDDP" clearance={CLEARANCE} onReplied={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_readback") }));
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.reply_send") }));

    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_telex");
    expect(call![1]).toMatchObject({ text: "WILCO", recipient: "EDDP" });
  });

  // The dangerous case: these tokens flip the controller's client back
  // into "aircraft is requesting clearance".
  it.each(["WILCO CLR TO EDDM", "REQUEST AGAIN", "PDC RECEIVED", "WILCO PREDEP OK"])(
    "refuses to send %s because ATC would read it as a new request",
    async (typed) => {
      render(<TelexQuickReply recipient="EDDP" clearance={CLEARANCE} onReplied={() => {}} />);
      await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_readback") }));

      const field = screen.getByRole("textbox");
      await userEvent.clear(field);
      await userEvent.type(field, typed);
      await userEvent.click(screen.getByRole("button", { name: t("cpdlc.reply_send") }));

      expect(
        invokeMock.mock.calls.filter((c) => c[0] === "hoppie_send_telex"),
        "must not reach the network",
      ).toHaveLength(0);
      expect(screen.getByText(/neue Freigabeanfrage/)).toBeInTheDocument();
    },
  );

  // A METAR or a controller's remark is not a clearance — there is
  // nothing to WILCO, so no keys may appear.
  it.each([
    "EDDP 251350Z 24008KT 9999 FEW035 18/12 Q1015",
    "CALL ME ON 121.905",
  ])("offers no acknowledgement keys for a non-clearance telex: %s", (text) => {
    const { container } = render(
      <TelexQuickReply recipient="EDDP" clearance={text} onReplied={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("still allows UNABLE", async () => {
    render(<TelexQuickReply recipient="EDDP" clearance={CLEARANCE} onReplied={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_unable") }));

    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_telex");
    expect(call![1]).toMatchObject({ text: "UNABLE" });
  });
});
