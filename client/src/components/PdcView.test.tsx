// v1.3.0 (#Hoppie-PDC-CPDLC) — PDC form regression tests.
//
// These exist because the first cut of this panel shipped three defects
// straight to the pilot, all of which a test would have caught:
//   1. the request form was hidden behind a button, so the fields
//      weren't even visible until you clicked "request clearance"
//   2. nothing told you WHICH field was still missing
//   3. an incomplete request looked submittable
// If someone re-hides the form or loosens the completeness check, these
// go red.

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

import { PdcView } from "./PdcView";

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
  // No active flight — nothing is prefilled, so every field starts empty.
  invokeMock.mockResolvedValue({
    callsign: null,
    aircraft_type: null,
    dep_icao: null,
    dest_icao: null,
  });
});

const t = (k: string) => i18next.t(k);

describe("PdcView", () => {
  it("shows the request fields immediately, without clicking anything first", async () => {
    render(<PdcView online callsign="DLH4TK" messages={[]} onChanged={() => {}} />);

    // All seven PDC fields must be reachable on arrival.
    for (const key of [
      "cpdlc.pdc_station_label",
      "cpdlc.pdc_form_aircraft_type",
      "cpdlc.pdc_form_dep",
      "cpdlc.pdc_form_dest",
      "cpdlc.pdc_form_stand",
      "cpdlc.pdc_form_atis",
    ]) {
      expect(
        await screen.findByLabelText(t(key)),
        `field "${t(key)}" must be visible without opening anything`,
      ).toBeInTheDocument();
    }
  });

  it("refuses to submit while any field is empty and names what is missing", async () => {
    render(<PdcView online callsign="DLH4TK" messages={[]} onChanged={() => {}} />);

    const submit = await screen.findByRole("button", { name: t("cpdlc.pdc_form_submit") });
    expect(submit).toBeDisabled();

    await userEvent.click(submit);
    expect(
      invokeMock.mock.calls.filter((c) => c[0] === "hoppie_send_pdc_request"),
      "an incomplete PDC request must never reach the backend",
    ).toHaveLength(0);

    // The pilot has to be told which fields are still open, not just
    // handed a dead button.
    expect(screen.getByTestId("pdc-missing")).toBeInTheDocument();
  });

  it("enables submit once every field is filled and sends uppercased values", async () => {
    render(<PdcView online callsign="DLH4TK" messages={[]} onChanged={() => {}} />);

    const fill = async (labelKey: string, value: string) => {
      const input = await screen.findByLabelText(t(labelKey));
      await userEvent.clear(input);
      await userEvent.type(input, value);
    };

    await fill("cpdlc.pdc_station_label", "eddf");
    await fill("cpdlc.pdc_form_aircraft_type", "b738");
    await fill("cpdlc.pdc_form_dep", "eddf");
    await fill("cpdlc.pdc_form_dest", "eddm");
    await fill("cpdlc.pdc_form_stand", "a12");
    await fill("cpdlc.pdc_form_atis", "c");

    const submit = screen.getByRole("button", { name: t("cpdlc.pdc_form_submit") });
    expect(submit).toBeEnabled();
    expect(screen.queryByTestId("pdc-missing")).not.toBeInTheDocument();

    invokeMock.mockResolvedValueOnce(undefined);
    await userEvent.click(submit);

    const call = invokeMock.mock.calls.find((c) => c[0] === "hoppie_send_pdc_request");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({
      request: {
        recipient: "EDDF",
        aircraft_type: "B738",
        dep_icao: "EDDF",
        dest_icao: "EDDM",
        stand: "A12",
        atis_letter: "C",
      },
    });
  });

  // The callsign is not a form field: PDC and CPDLC must go out under
  // the same identity, or a controller looking the aircraft up by one of
  // them never finds it.
  it("has no callsign field of its own", async () => {
    render(<PdcView online callsign="DLH4TK" messages={[]} onChanged={() => {}} />);
    await screen.findByLabelText(t("cpdlc.pdc_station_label"));
    expect(screen.queryByLabelText(t("cpdlc.pdc_form_callsign"))).not.toBeInTheDocument();
  });

  it("shows the ACARS callsign in the preview instead", async () => {
    render(<PdcView online callsign="DLH4TK" messages={[]} onChanged={() => {}} />);
    expect(await screen.findByText(/REQUEST PREDEP CLEARANCE DLH4TK/)).toBeInTheDocument();
  });

  it("keeps submit disabled while ACARS reception is off", async () => {
    render(<PdcView online={false} callsign="DLH4TK" messages={[]} onChanged={() => {}} />);
    const submit = await screen.findByRole("button", { name: t("cpdlc.pdc_form_submit") });
    expect(submit).toBeDisabled();
  });
});
