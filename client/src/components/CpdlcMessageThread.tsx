// v1.3.0 (#Hoppie-PDC-CPDLC) — DCDU-style message log for the CPDLC tab.
//
// Uplink (received) entries render left-aligned, downlink (sent) right-
// aligned. Response buttons are generated from the entry's `response` code
// — never hardcoded per message — and disappear once the entry is
// `closed` (a matching reply arrived/was sent). Plain telex/PDC traffic
// has no response mechanics and just renders as text.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";

interface Props {
  messages: ThreadEntry[];
  onResponded: () => void;
}

export function CpdlcMessageThread({ messages, onResponded }: Props) {
  const { t } = useTranslation();

  if (messages.length === 0) {
    return <p className="settings__row-hint">{t("cpdlc.thread_empty")}</p>;
  }

  return (
    <ul className="cpdlc-thread">
      {messages.map((m, i) => (
        <Entry key={`${m.at}-${i}`} entry={m} onResponded={onResponded} />
      ))}
    </ul>
  );
}

function Entry({ entry, onResponded }: { entry: ThreadEntry; onResponded: () => void }) {
  const { t } = useTranslation();
  const side = entry.direction === "sent" ? "sent" : "received";
  const isCpdlc = entry.kind === "cpdlc";
  const needsResponse = isCpdlc && entry.direction === "received" && entry.closed === false;

  return (
    <li className={`cpdlc-thread__entry cpdlc-thread__entry--${side} ${isCpdlc && entry.closed ? "cpdlc-thread__entry--closed" : ""}`}>
      <p className="cpdlc-thread__meta">
        {entry.direction === "received" ? t("cpdlc.thread_received") : t("cpdlc.thread_sent")}
        {isCpdlc && entry.min != null && ` · MIN ${entry.min}`}
        {isCpdlc && entry.mrn != null && ` · MRN ${entry.mrn}`}
        {" · "}
        {new Date(entry.at).toLocaleTimeString()}
        {isCpdlc && entry.response && entry.response !== "NE" && entry.response !== "N" && (
          <span className="cpdlc-thread__response-badge">{entry.response}</span>
        )}
        {isCpdlc && entry.closed && (
          <span className="cpdlc-thread__closed-mark" aria-hidden="true">
            ✓
          </span>
        )}
      </p>
      <p className="cpdlc-thread__text">{entry.text}</p>
      {needsResponse && <ResponseButtons entry={entry} onResponded={onResponded} />}
    </li>
  );
}

function ResponseButtons({ entry, onResponded }: { entry: ThreadEntry; onResponded: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [freeReply, setFreeReply] = useState("");

  const respond = async (elementId: string) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_cpdlc_element", { elementId, values: [], mrn: entry.min });
      onResponded();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const respondFreeText = async () => {
    if (busy || freeReply.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_free_text", { text: freeReply.trim(), mrn: entry.min });
      onResponded();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="cpdlc-thread__response-buttons">
      {entry.response === "WU" && (
        <>
          <button type="button" className="button button--primary" disabled={busy} onClick={() => void respond("DM0")}>
            {t("cpdlc.response_wilco")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => void respond("DM1")}>
            {t("cpdlc.response_unable")}
          </button>
        </>
      )}
      {entry.response === "AN" && (
        <>
          <button type="button" className="button button--primary" disabled={busy} onClick={() => void respond("DM4")}>
            {t("cpdlc.response_affirm")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => void respond("DM5")}>
            {t("cpdlc.response_negative")}
          </button>
        </>
      )}
      {entry.response === "R" && (
        <button type="button" className="button button--primary" disabled={busy} onClick={() => void respond("DM3")}>
          {t("cpdlc.response_roger")}
        </button>
      )}
      {entry.response === "Y" && (
        <div className="cpdlc-thread__response-freetext">
          <input
            type="text"
            value={freeReply}
            onChange={(e) => setFreeReply(e.target.value)}
            placeholder={t("cpdlc.response_free_text_placeholder")}
            disabled={busy}
          />
          <button
            type="button"
            className="button button--primary"
            disabled={busy || freeReply.trim() === ""}
            onClick={() => void respondFreeText()}
          >
            {t("cpdlc.composer_send")}
          </button>
        </div>
      )}
      {error && <p className="cpdlc-panel__error">{error}</p>}
    </div>
  );
}
