// v1.3.5 (#Datalink-3a) — history rendering regression tests.
//
// Built with synthetic-but-wire-realistic fixtures on purpose: there was
// no live CPDLC traffic on hand to demo the redesign against (no GSG
// pilot had an active session with a real uplink to show), so these
// fixtures are the "test data" that stands in for it — one PDC clearance
// reply shaped like hoppie-protocol's own verified fixture, and one
// CPDLC WU uplink shaped like the devHazz-documented worked example
// (see hoppie-protocol/tests/fixtures/*.txt for both originals).

import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  formatIpcError: (e: unknown) => (e as { message?: string })?.message ?? String(e),
}));

import { DatalinkHistory } from "./DatalinkHistory";

beforeEach(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  localStorage.clear();
  // QS 2026-08-04: der Squawk-Merker liegt jetzt in sessionStorage (vorher
  // localStorage, siehe DatalinkHistory.tsx) — ohne dieses Leeren schleppt
  // ein Test, der den Knopf drueckt, seinen Zustand in alle folgenden.
  sessionStorage.clear();
});

const t = (k: string, opts?: Record<string, unknown>) => i18next.t(k, opts);

const PDC_SENT: ThreadEntry = {
  kind: "telex",
  direction: "sent",
  text: "REQUEST PREDEP CLEARANCE BTI4TK A320 TO EDDM AT EDDK STAND B34 ATIS K",
  at: "2026-08-03T13:58:04.000Z",
  min: null,
  mrn: null,
  response: null,
  element_id: null,
  closed: null,
  deferred: null,
};

// Shaped like hoppie-protocol/tests/fixtures/pdc_request_reply.txt, with
// the extra RWY/CTOT/NEXT-FREQ fields the new grid also parses.
const PDC_REPLY: ThreadEntry = {
  kind: "telex",
  direction: "received",
  text: "CLD BTI4TK CLRD TO EDDM OFF 14L VIA DOMUX2N SQUAWK 4231 INITIAL CLIMB 5000FT NEXT FREQ 121.150 ATIS K QNH 1011 CTOT 1436 SET SQUAWK BEFORE PUSH CONTACT EDDK_GND FOR PUSH",
  at: "2026-08-03T13:58:41.000Z",
  min: null,
  mrn: null,
  response: null,
  element_id: null,
  closed: null,
  deferred: null,
};

// Shaped like hoppie-protocol/tests/fixtures/cpdlc_direct_to_sequence.txt
// (devHazz's worked example): an unsolicited WU uplink still open.
const CPDLC_UPLINK_OPEN: ThreadEntry = {
  kind: "cpdlc",
  direction: "received",
  text: "PROCEED DIRECT TO UDROS",
  at: "2026-08-03T14:05:00.000Z",
  min: 7,
  mrn: null,
  response: "WU",
  element_id: "UM_DIRECT",
  closed: false,
  deferred: false,
};

function renderHistory(messages: ThreadEntry[]) {
  return render(
    <DatalinkHistory callsign="BTI4TK" cpdlcStation="EDGG" pdcRecipient="EDDK_DEL" messages={messages} onChanged={() => {}} />,
  );
}

describe("DatalinkHistory — uplink parsing", () => {
  it("parses all eight fields from a realistic PDC clearance reply into the value grid", () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    expect(screen.getByText("4231")).toBeInTheDocument(); // SQUAWK
    expect(screen.getByText("DOMUX2N")).toBeInTheDocument(); // SID
    expect(screen.getByText("5000")).toBeInTheDocument(); // INITIAL CLIMB
    expect(screen.getByText("121.150")).toBeInTheDocument(); // DEP FREQ
    expect(screen.getByText("14L")).toBeInTheDocument(); // RWY
    expect(screen.getByText("14:36z")).toBeInTheDocument(); // CTOT, formatted
    expect(screen.getByText("1011")).toBeInTheDocument(); // QNH
    expect(screen.getByText("K")).toBeInTheDocument(); // ATIS
  });

  it("shows every unrecognized stretch as station conditions, never dropped", () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    const conditions = document.querySelector(".datalink-uplink__conditions p");
    expect(conditions?.textContent).toContain("SET SQUAWK BEFORE PUSH CONTACT EDDK_GND FOR PUSH");
    expect(conditions?.textContent).toContain("CLD BTI4TK CLRD TO EDDM");
  });

  it("keeps the full original telex visible and expanded by default", () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    expect(
      screen.getByText((_, el) => el?.className === "datalink-uplink__original-text" && el.textContent === PDC_REPLY.text),
    ).toBeInTheDocument();
  });

  it("never truncates the sent telex either", () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    expect(screen.getByText(PDC_SENT.text)).toBeInTheDocument();
  });

  it("falls back to raw text with no grid when nothing is recognized", () => {
    const unparsed: ThreadEntry = { ...CPDLC_UPLINK_OPEN, text: "STANDBY FOR FURTHER INSTRUCTIONS", response: "R" };
    renderHistory([unparsed]);
    expect(screen.getByText(t("cpdlc.uplink_title_unparsed"))).toBeInTheDocument();
    expect(screen.getByText("STANDBY FOR FURTHER INSTRUCTIONS")).toBeInTheDocument();
    expect(screen.queryByText(t("cpdlc.field_squawk"))).not.toBeInTheDocument();
  });
});

describe("DatalinkHistory — answer row", () => {
  it("only the latest unanswered uplink gets WILCO/STANDBY/UNABLE", () => {
    renderHistory([PDC_SENT, PDC_REPLY, CPDLC_UPLINK_OPEN]);
    // CPDLC_UPLINK_OPEN is the newest — it gets the row; the PDC reply
    // (now superseded) must not also show one.
    const wilco = screen.getAllByText(t("cpdlc.response_wilco"));
    expect(wilco).toHaveLength(1);
  });

  it("sends a squawk-take-over request tied to the parsed value", async () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    const button = screen.getByRole("button", { name: t("cpdlc.squawk_take", { squawk: "4231" }) });
    await userEvent.click(button);
    expect(await screen.findByRole("button", { name: t("cpdlc.squawk_taken", { squawk: "4231" }) })).toBeDisabled();
  });

  it("replaces the answer row with a status line once a reply exists", () => {
    const reply: ThreadEntry = {
      kind: "cpdlc",
      direction: "sent",
      text: "WILCO",
      at: "2026-08-03T14:05:20.000Z",
      min: null,
      mrn: 7,
      response: null,
      element_id: "DM0",
      closed: null,
      deferred: null,
    };
    const closedUplink: ThreadEntry = { ...CPDLC_UPLINK_OPEN, closed: true };
    renderHistory([closedUplink, reply]);
    expect(screen.queryByRole("button", { name: t("cpdlc.response_wilco") })).not.toBeInTheDocument();
    expect(screen.getByText(t("cpdlc.answered_status", { answer: "WILCO", time: "14:05:20z" }))).toBeInTheDocument();
  });

  // QS 2026-08-04: Telex hat keine MIN/MRN-Verkettung, deshalb galt frueher
  // schlicht "das naechste gesendete Telex ist die Antwort". Der Verfasser-
  // Bereich laesst aber jederzeit eine ganz neue PDC-Anfrage los, ohne den
  // Verlauf anzusehen — dann stand unter einer noch unbestaetigten Freigabe
  // "Beantwortet: REQUEST PREDEP CLEARANCE ...", also der Text einer voellig
  // unabhaengigen Nachricht als angebliche Bestaetigung.
  it("does not count a NEW clearance request as the answer to an earlier uplink", () => {
    const secondRequest: ThreadEntry = {
      ...PDC_SENT,
      text: "REQUEST PREDEP CLEARANCE BTI4TK A320 TO EDDF AT EDDK STAND B34 ATIS L",
      at: "2026-08-03T14:10:00.000Z",
    };
    renderHistory([PDC_SENT, PDC_REPLY, secondRequest]);
    // Kein "Beantwortet:"-Status, der die neue Anfrage als Antwort ausgibt.
    expect(
      screen.queryByText(new RegExp(t("cpdlc.answered_status", { answer: "", time: "" }).split("{")[0].trim())),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/REQUEST PREDEP CLEARANCE BTI4TK A320 TO EDDF/)).toBeInTheDocument();
  });

  it("still counts a genuine acknowledgement telex as the answer", () => {
    const wilco: ThreadEntry = {
      ...PDC_SENT,
      text: "WILCO",
      at: "2026-08-03T14:10:00.000Z",
    };
    renderHistory([PDC_SENT, PDC_REPLY, wilco]);
    expect(
      screen.getByText(t("cpdlc.answered_status", { answer: "WILCO", time: "14:10:00z" })),
    ).toBeInTheDocument();
  });

  // QS 2026-08-04: der Merker lag in localStorage — also dauerhaft ueber
  // Fluege und Programmstarts hinweg, ohne je geloescht zu werden. Squawks
  // sind vierstellig und werden laufend wiederverwendet: ein frueher einmal
  // "uebernommener" Code zeigte bei der naechsten Freigabe mit demselben
  // Code sofort "uebernommen", obwohl der Transponder nie angefasst wurde.
  it("does not carry a squawk-taken memo over from a previous session", () => {
    localStorage.setItem("aeroacars.transponder.squawk_memo", "4231");
    renderHistory([PDC_SENT, PDC_REPLY]);
    const btn = screen.getByRole("button", { name: t("cpdlc.squawk_take", { squawk: "4231" }) });
    expect(btn).toBeEnabled();
  });
});

describe("DatalinkHistory — filters and footer", () => {
  it("filters PDC and CPDLC traffic independently", async () => {
    renderHistory([PDC_SENT, PDC_REPLY, CPDLC_UPLINK_OPEN]);
    await userEvent.click(screen.getByRole("button", { name: t("cpdlc.filter_pdc") }));
    expect(screen.queryByText("PROCEED DIRECT TO UDROS")).not.toBeInTheDocument();
    expect(screen.getByText(PDC_SENT.text)).toBeInTheDocument();
  });

  it("counts the current filter's messages in the footer", () => {
    renderHistory([PDC_SENT, PDC_REPLY]);
    expect(screen.getByText(t("cpdlc.history_footer_count", { count: 2 }))).toBeInTheDocument();
  });

  it("shows an empty-state instead of a bare blank log", () => {
    renderHistory([]);
    expect(screen.getByText(t("cpdlc.history_empty"))).toBeInTheDocument();
  });
});

describe("DatalinkHistory — new-uplink fade-in (README §6)", () => {
  // The animation itself is CSS-only (jsdom doesn't run it); what matters
  // here is that the class lands on the right element and nowhere else —
  // not on history already on screen at mount, not on a later re-render
  // of that same entry, only on a genuinely new arrival.
  it("does not mark pre-existing history as fresh on initial mount", () => {
    renderHistory([PDC_SENT, CPDLC_UPLINK_OPEN]);
    const article = screen.getByLabelText(new RegExp(t("cpdlc.thread_received")));
    expect(article).not.toHaveClass("datalink-uplink--fresh");
  });

  it("marks only a newly arrived uplink as fresh, and only once", () => {
    const { rerender } = renderHistory([PDC_SENT]);

    const rerenderWith = (messages: ThreadEntry[]) =>
      rerender(
        <DatalinkHistory
          callsign="BTI4TK"
          cpdlcStation="EDGG"
          pdcRecipient="EDDK_DEL"
          messages={messages}
          onChanged={() => {}}
        />,
      );

    rerenderWith([PDC_SENT, CPDLC_UPLINK_OPEN]);
    const article = screen.getByLabelText(new RegExp(t("cpdlc.thread_received")));
    expect(article).toHaveClass("datalink-uplink--fresh");

    // A later re-render of the SAME entries (e.g. a poll tick that
    // returns identical data) must not re-flag it — it already faded in.
    rerenderWith([PDC_SENT, CPDLC_UPLINK_OPEN]);
    const articleAgain = screen.getByLabelText(new RegExp(t("cpdlc.thread_received")));
    expect(articleAgain).not.toHaveClass("datalink-uplink--fresh");
  });
});
