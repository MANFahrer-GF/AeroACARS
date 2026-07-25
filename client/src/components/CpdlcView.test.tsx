// v1.3.0 (#Hoppie-PDC-CPDLC) — CPDLC logon + reply regression tests.
//
// Covers the defects found in the first cut:
//   1. you could fire LOGON at the centre you were already logged on to
//   2. reply keys had to be reachable without digging through the
//      element catalog
// Plus the DCDU rules the research pinned down: STANDBY only in the W/U
// set, and answering is two-step so a stray click can't put WILCO on the
// wire.

import { describe, it, expect, beforeAll, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => String(e),
}));

import { CpdlcView } from "./CpdlcView";

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
  invokeMock.mockResolvedValue([]);
});

const t = (k: string, o?: Record<string, unknown>) => i18next.t(k, o ?? {});

const uplink = (over: Partial<ThreadEntry> = {}): ThreadEntry => ({
  kind: "cpdlc",
  direction: "received",
  text: "CLIMB TO AND MAINTAIN FL240",
  at: "2026-07-25T10:00:00Z",
  min: 12,
  mrn: null,
  response: "WU",
  element_id: null,
  closed: false,
  deferred: false,
  ...over,
});

describe("CpdlcView logon", () => {
  it("refuses a second logon to the centre already logged on to", async () => {
    render(
      <CpdlcView
        online
        loggedOn
        station="EDGG"
        logonSent={false}
        messages={[]}
        onChanged={() => {}}
      />,
    );

    const logon = screen.getByRole("button", { name: t("cpdlc.logon_send") });
    expect(
      logon,
      "logging on again to the same centre is a no-op that confuses ATC",
    ).toBeDisabled();
  });

  it("allows logon once a different centre is entered", async () => {
    render(
      <CpdlcView
        online
        loggedOn
        station="EDGG"
        logonSent={false}
        messages={[]}
        onChanged={() => {}}
      />,
    );

    const field = screen.getByLabelText(t("cpdlc.center_label"));
    await userEvent.clear(field);
    await userEvent.type(field, "EDUU");

    const logon = screen.getByRole("button", { name: t("cpdlc.logon_send") });
    expect(logon).toBeEnabled();

    await userEvent.click(logon);
    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_logon_request");
    expect(call![1]).toMatchObject({ station: "EDUU" });
  });

  it("does not offer logon while ACARS reception is off", () => {
    render(
      <CpdlcView
        online={false}
        loggedOn={false}
        station={null}
        logonSent={false}
        messages={[]}
        onChanged={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: t("cpdlc.logon_send") })).toBeDisabled();
  });
});

describe("CpdlcView replies", () => {
  const renderWith = (m: ThreadEntry) =>
    render(
      <CpdlcView
        online
        loggedOn
        station="EDGG"
        logonSent={false}
        messages={[m]}
        onChanged={() => {}}
      />,
    );

  it("puts WILCO/UNABLE/STANDBY straight on a W/U uplink", () => {
    renderWith(uplink());
    expect(screen.getByRole("button", { name: t("cpdlc.response_wilco") })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("cpdlc.response_unable") })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("cpdlc.response_standby") })).toBeInTheDocument();
  });

  it("offers STANDBY only for W/U, never for AFFIRM/NEGATIVE", () => {
    renderWith(uplink({ response: "AN", text: "CONFIRM SQUAWK" }));
    expect(screen.getByRole("button", { name: t("cpdlc.response_affirm") })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("cpdlc.response_negative") })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: t("cpdlc.response_standby") }),
      "STANDBY is not a legal answer to an affirm/negative uplink",
    ).not.toBeInTheDocument();
  });

  it("requires a second confirm before a reply goes on the wire", async () => {
    renderWith(uplink());

    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_wilco") }));
    expect(
      invokeMock.mock.calls.filter((c) => c[0] === "hoppie_send_cpdlc_element"),
      "arming an answer must not transmit it",
    ).toHaveLength(0);

    expect(
      screen.getByText(t("cpdlc.reply_confirm", { answer: t("cpdlc.response_wilco") })),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.reply_send") }));
    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_cpdlc_element");
    expect(call![1]).toMatchObject({ elementId: "DM0", mrn: 12 });
  });

  it("can back out of an armed reply without sending", async () => {
    renderWith(uplink());
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.response_unable") }));
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.reply_cancel") }));

    expect(invokeMock.mock.calls.filter((c) => c[0] === "hoppie_send_cpdlc_element")).toHaveLength(0);
    expect(screen.getByRole("button", { name: t("cpdlc.response_wilco") })).toBeInTheDocument();
  });

  // Datalink vocabulary is not translated: the key must name the element
  // it actually sends, so the pilot can see WILCO going on the wire.
  it("labels the keys with the wire command, not a translation", () => {
    renderWith(uplink({ text: "CLRD TO EDDM VIA DCT" }));
    expect(screen.getByRole("button", { name: "WILCO" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "UNABLE" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "STANDBY" })).toBeInTheDocument();
  });

  // STANDBY may be sent once. A second deferral of the same clearance
  // tells the controller nothing and clutters their screen.
  it("drops the STANDBY key once the instruction was already deferred", () => {
    renderWith(uplink({ deferred: true }));
    expect(screen.getByRole("button", { name: t("cpdlc.response_wilco") })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("cpdlc.response_unable") })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: t("cpdlc.response_standby") }),
      "deferring twice is not a thing",
    ).not.toBeInTheDocument();
  });

  it("shows no reply keys on an already-answered uplink", () => {
    renderWith(uplink({ closed: true }));
    expect(screen.queryByRole("button", { name: t("cpdlc.response_wilco") })).not.toBeInTheDocument();
  });

  // A logon accept expects no response (GOLD code NE). Offering ROGER
  // there tempts the pilot into sending protocol noise at ATC.
  it.each([
    ["NE", "LOGON ACCEPTED"],
    ["N", "ATIS EDDF C CURRENT"],
  ])("offers no reply keys for a %s uplink", (response, text) => {
    renderWith(uplink({ response, text, closed: false, min: 9000 }));
    for (const key of [
      "cpdlc.response_roger",
      "cpdlc.response_wilco",
      "cpdlc.response_standby",
      "cpdlc.response_affirm",
    ]) {
      expect(
        screen.queryByRole("button", { name: t(key) }),
        `"${t(key)}" must not be offered for a ${response} uplink`,
      ).not.toBeInTheDocument();
    }
  });

  it("does not flag a no-response uplink as awaiting a reply", () => {
    renderWith(uplink({ response: "NE", text: "LOGON ACCEPTED", closed: false }));
    expect(screen.queryByText(t("cpdlc.awaiting_reply"))).not.toBeInTheDocument();
  });

  // The live exchange must never be pushed out of view by history, but
  // nothing may be silently dropped either.
  it("folds settled older traffic to one line and keeps the recent tail open", () => {
    const many: ThreadEntry[] = Array.from({ length: 8 }, (_, i) =>
      uplink({
        min: i + 1,
        at: `2026-07-25T10:0${i}:00Z`,
        text: `OLD MESSAGE ${i}`,
        response: "NE",
        closed: true,
      }),
    );
    render(
      <CpdlcView
        online
        loggedOn
        station="EDDB"
        logonSent={false}
        logonTimedOut={false}
        messages={many}
        onChanged={() => {}}
      />,
    );

    // Every message is still present — folding is not dropping.
    for (let i = 0; i < 8; i++) {
      expect(screen.getByText(new RegExp(`OLD MESSAGE ${i}`))).toBeInTheDocument();
    }
    const collapsed = document.querySelectorAll(".cpdlc-msg--collapsed");
    expect(collapsed.length, "everything but the recent tail folds").toBe(5);
  });

  it("never folds a message that still needs an answer, however old", () => {
    const messages: ThreadEntry[] = [
      uplink({ min: 1, text: "CLIMB TO AND MAINTAIN FL240", response: "WU", closed: false }),
      ...Array.from({ length: 6 }, (_, i) =>
        uplink({ min: i + 2, text: `LATER ${i}`, response: "NE", closed: true }),
      ),
    ];
    render(
      <CpdlcView
        online
        loggedOn
        station="EDDB"
        logonSent={false}
        logonTimedOut={false}
        messages={messages}
        onChanged={() => {}}
      />,
    );
    // Its reply keys must still be right there.
    expect(screen.getByRole("button", { name: t("cpdlc.response_wilco") })).toBeInTheDocument();
  });

  // Our own request carries AnyRequired and stays open until ATC answers.
  // It must read as "waiting on them", never as "you must reply".
  it("marks an unanswered own request as awaiting ATC, not as a pilot task", () => {
    renderWith(
      uplink({
        direction: "sent",
        text: "REQUEST DIRECT TO UDROS",
        response: "Y",
        closed: false,
        min: 3,
      }),
    );
    expect(screen.getByText(t("cpdlc.awaiting_atc"))).toBeInTheDocument();
    expect(screen.queryByText(t("cpdlc.awaiting_reply"))).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: t("cpdlc.composer_send") }),
      "our own outstanding request must not sprout reply keys",
    ).not.toBeInTheDocument();
  });
});
