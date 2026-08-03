// v1.3.0 (#Hoppie-PDC-CPDLC) — DCDU-style response keys.
//
// Every uplink needing an answer carries its answer keys. A pilot never
// opens the element catalog to say WILCO.
//
// Modeled on the real Airbus DCDU (FlyByWire A32NX mirrors it 1:1):
//   W/U -> UNABLE | STANDBY | WILCO
//   A/N -> NEGATIVE | AFFIRM        (no STANDBY — it is not offered here)
//   R   -> ROGER
// STANDBY exists only in the W/U set, and answering STANDBY does NOT
// close the message: it defers, so the instruction stays on the list
// until a real answer goes out (enforced in hoppie-protocol's thread.rs).
//
// Key labels are the datalink terms themselves — WILCO, UNABLE, ROGER —
// in every language. They are the vocabulary of the medium, not words to
// translate, and a button reading "Bestätigen" while WILCO goes on the
// wire hides what is actually being sent.
//
// Two-step send is also DCDU behaviour: pressing a key arms the answer,
// and a second press on SEND transmits it. These are real clearances —
// a single stray click must not put WILCO on the wire.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";

/** Substrings that make a controller's client read a telex as a NEW
 *  clearance request rather than an acknowledgement. An acknowledgement
 *  must never contain them. */
const REQUEST_TOKENS = ["CLR", "REQ", "PDC", "PREDEP", "REQUEST"];

/** GOLD downlink ids for the standard answers (elements_data.rs). */
const WILCO = "DM0";
const UNABLE = "DM1";
const STANDBY = "DM2";
const ROGER = "DM3";
const AFFIRM = "DM4";
const NEGATIVE = "DM5";

interface Armed {
  id: string;
  label: string;
}

interface Props {
  /** MIN of the uplink being answered — becomes the reply's MRN. */
  min: number | null;
  /** Already deferred once with STANDBY. The key disappears then: the
   *  instruction is still open, but deferring it again tells the
   *  controller nothing they don't already know. FlyByWire does the
   *  same (WilcoUnableButtons.tsx re-offers WILCO/UNABLE without STBY
   *  once a DM2 response exists). */
  deferred: boolean;
  /** GOLD response code of the uplink: WU | AN | R | Y | N | NE. */
  response: string | null;
  onReplied: () => void;
  /** Rendered right-aligned in the same row as the answer keys — the
   *  history's "Squawk übernehmen" button (README §4.3.e). Only shown
   *  next to the key row itself, not while a key is armed for confirm. */
  trailing?: React.ReactNode;
}

export function CpdlcQuickReply({ min, response, deferred, onReplied, trailing }: Props) {
  const { t } = useTranslation();
  const [armed, setArmed] = useState<Armed | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [freeText, setFreeText] = useState("");

  const confirm = async () => {
    if (!armed || busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_cpdlc_element", {
        elementId: armed.id,
        values: [],
        mrn: min,
      });
      setArmed(null);
      onReplied();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const sendFreeText = async () => {
    if (busy || freeText.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_free_text", { text: freeText.trim(), mrn: min });
      setFreeText("");
      onReplied();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const key = (id: string, label: string, variant?: "affirm" | "deny") => (
    <button
      type="button"
      className={`cpdlc-key ${variant ? `cpdlc-key--${variant}` : ""}`}
      disabled={busy}
      onClick={() => setArmed({ id, label })}
    >
      {label}
    </button>
  );

  if (armed) {
    return (
      <div className="cpdlc-reply">
        <p className="cpdlc-reply__confirm">
          {t("cpdlc.reply_confirm", { answer: armed.label })}
        </p>
        <div className="cpdlc-reply__keys">
          <button type="button" className="cpdlc-key" disabled={busy} onClick={() => setArmed(null)}>
            {t("cpdlc.reply_cancel")}
          </button>
          <button
            type="button"
            className="cpdlc-key cpdlc-key--affirm"
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? t("cpdlc.pdc_form_sending") : t("cpdlc.reply_send")}
          </button>
        </div>
        {error && <p className="cpdlc-panel__error">{error}</p>}
      </div>
    );
  }

  return (
    <div className="cpdlc-reply">
      <div className="cpdlc-reply__keys">
        {response === "WU" && (
          <>
            {/* Order: UNABLE, STANDBY, WILCO — matches the real Airbus
                DCDU / FlyByWire A32NX line-button order this component is
                modeled on (see the file header). The Datalink-3a spec
                (README §7) states the opposite reading order
                (WILCO/STANDBY/UNABLE) without citing hardware; real-DCDU
                fidelity wins here since this is cockpit response
                terminology, not a call to make on spec text alone. */}
            {key(UNABLE, t("cpdlc.response_unable"), "deny")}
            {!deferred && key(STANDBY, t("cpdlc.response_standby"))}
            {key(WILCO, t("cpdlc.response_wilco"), "affirm")}
          </>
        )}
        {response === "AN" && (
          <>
            {key(AFFIRM, t("cpdlc.response_affirm"), "affirm")}
            {key(NEGATIVE, t("cpdlc.response_negative"), "deny")}
          </>
        )}
        {response === "R" && (
          <>
            {key(ROGER, t("cpdlc.response_roger"), "affirm")}
            {/* The real DCDU offers only ROGER here, but a pilot who
                cannot comply must have a way to say so — the FAA
                handbook allows REJECT/UNABLE for any message, and FSM
                offers it too. Without it the only escape is free text,
                which the request-token guard may then refuse. */}
            {key(UNABLE, t("cpdlc.response_unable"), "deny")}
          </>
        )}
        {/* N / NE deliberately render nothing: an uplink that expects no
            response (LOGON ACCEPTED, advisories) must not be answered.
            Sending ROGER there is protocol noise, not politeness. */}
        {trailing && <span className="cpdlc-reply__trailing">{trailing}</span>}
      </div>

      {response === "Y" && (
        <div className="cpdlc-reply__freetext">
          <input
            type="text"
            value={freeText}
            onChange={(e) => setFreeText(e.target.value.toUpperCase())}
            placeholder={t("cpdlc.response_free_text_placeholder")}
            disabled={busy}
          />
          <button
            type="button"
            className="cpdlc-key cpdlc-key--affirm"
            disabled={busy || freeText.trim() === ""}
            onClick={() => void sendFreeText()}
          >
            {t("cpdlc.composer_send")}
          </button>
        </div>
      )}

      {error && <p className="cpdlc-panel__error">{error}</p>}
    </div>
  );
}

/** Telex has no MIN/MRN threading, so a PDC readback goes back as plain
 *  telex to the station that sent it — not as a CPDLC element. The
 *  readback text is prefilled so the pilot only has to confirm it. */
export function TelexQuickReply({
  recipient,
  clearance,
  onReplied,
  trailing,
}: {
  recipient: string | null;
  clearance: string;
  onReplied: () => void;
  /** Same trailing slot as CpdlcQuickReply — see its prop doc. */
  trailing?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [readback, setReadback] = useState("");

  // Only an actual clearance gets acknowledgement keys. Detected the way
  // EasyCPDLC does (MainForm.cs:803), which matches what vSMR emits
  // ("CLR TO ..."). Any other telex — a METAR, a controller's remark —
  // has nothing to WILCO, so offering it would be noise.
  const isClearance = /\bCLR?D? TO\b|\bCLEARED TO\b/i.test(clearance);

  const startReadback = () => {
    // A bare WILCO — nothing else. Verified against the controller side:
    // vSMR (the VATSIM UK plugin) matches the reply on the substrings
    // WILCO/ROGER/RGR only and reads no items, the FAA/L3Harris handbook
    // requires just "ACCEPT/WILCO, ROGER, or REJECT/UNABLE" (and
    // prohibits free text to ATC), and avoiding readbacks is the stated
    // purpose of DCL. Echoing the squawk was invented convention.
    setReadback("WILCO");
    setOpen(true);
  };

  const send = async (text: string) => {
    if (busy || text.trim() === "") return;
    const upper = text.trim().toUpperCase();
    // The controller side classifies an inbound telex by substring, and
    // it tests the REQUEST branch first (vSMR SMRPlugin.cpp:168). A reply
    // containing any of these reads as a brand-new clearance request and
    // re-flashes the controller's request indicator — so refuse to send
    // one instead of quietly disrupting their workflow.
    const offending = REQUEST_TOKENS.filter((tok) => upper.includes(tok));
    if (offending.length > 0) {
      setError(t("cpdlc.readback_request_token", { tokens: offending.join(", ") }));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_telex", { text: upper, recipient });
      setOpen(false);
      onReplied();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  // Nothing to acknowledge on a plain telex (METAR, a remark) — offering
  // WILCO there would send a meaningless message to the controller.
  if (!isClearance) return null;

  return (
    <div className="cpdlc-reply">
      {open ? (
        <>
          <div className="cpdlc-reply__freetext">
            <input
              type="text"
              value={readback}
              onChange={(e) => setReadback(e.target.value.toUpperCase())}
              disabled={busy}
            />
          </div>
          <div className="cpdlc-reply__keys">
            <button type="button" className="cpdlc-key" disabled={busy} onClick={() => setOpen(false)}>
              {t("cpdlc.reply_cancel")}
            </button>
            <button
              type="button"
              className="cpdlc-key cpdlc-key--affirm"
              disabled={busy || readback.trim() === ""}
              onClick={() => void send(readback)}
            >
              {busy ? t("cpdlc.pdc_form_sending") : t("cpdlc.reply_send")}
            </button>
          </div>
        </>
      ) : (
        <div className="cpdlc-reply__keys">
          <button
            type="button"
            className="cpdlc-key cpdlc-key--affirm"
            disabled={busy}
            onClick={startReadback}
          >
            {t("cpdlc.response_readback")}
          </button>
          <button
            type="button"
            className="cpdlc-key cpdlc-key--deny"
            disabled={busy}
            onClick={() => void send("UNABLE")}
          >
            {t("cpdlc.response_unable")}
          </button>
          {trailing && <span className="cpdlc-reply__trailing">{trailing}</span>}
        </div>
      )}
      {error && <p className="cpdlc-panel__error">{error}</p>}
    </div>
  );
}
