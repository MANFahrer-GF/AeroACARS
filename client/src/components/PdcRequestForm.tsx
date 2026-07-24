// v1.3.0 (#Hoppie-PDC-CPDLC) — PDC (Pre-Departure Clearance) request form.
//
// Seven fields, per the format verified against quassbutreally/EasyCPDLC's
// source (GPL-3.0, RequestForm.cs:588) — there is no dedicated PDC wire
// type in the Hoppie protocol, it's sent as a plain telex:
//   REQUEST PREDEP CLEARANCE {callsign} {type} TO {dest} AT {dep} STAND {stand} ATIS {atis}
//
// Prefilled from the active flight (via hoppie_get_flight_context) where
// possible; stand + ATIS letter are always manual (not derivable from the
// flight context).

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";

interface FlightContext {
  callsign: string | null;
  aircraft_type: string | null;
  dep_icao: string | null;
  dest_icao: string | null;
}

interface PdcRequestArgs {
  recipient: string;
  callsign: string;
  aircraft_type: string;
  dep_icao: string;
  dest_icao: string;
  stand: string;
  atis_letter: string;
}

interface PdcSendResult {
  sent_text: string;
  sent_at: string;
}

interface Props {
  /** Whether the Hoppie connection is actually up — submit is disabled
   *  otherwise, with a hint pointing at Settings. */
  connected: boolean;
  onSent: () => void;
}

export function PdcRequestForm({ connected, onSent }: Props) {
  const { t } = useTranslation();
  const [fields, setFields] = useState<PdcRequestArgs>({
    recipient: "",
    callsign: "",
    aircraft_type: "",
    dep_icao: "",
    dest_icao: "",
    stand: "",
    atis_letter: "",
  });
  const [hasFlightContext, setHasFlightContext] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSent, setLastSent] = useState<PdcSendResult | null>(null);

  useEffect(() => {
    void invoke<FlightContext>("hoppie_get_flight_context").then((ctx) => {
      if (!ctx.callsign && !ctx.dep_icao) return;
      setHasFlightContext(true);
      setFields((prev) => ({
        ...prev,
        recipient: ctx.dep_icao ?? prev.recipient,
        callsign: ctx.callsign ?? prev.callsign,
        aircraft_type: ctx.aircraft_type ?? prev.aircraft_type,
        dep_icao: ctx.dep_icao ?? prev.dep_icao,
        dest_icao: ctx.dest_icao ?? prev.dest_icao,
      }));
    });
  }, []);

  const set = (key: keyof PdcRequestArgs) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setFields((prev) => ({ ...prev, [key]: e.target.value }));
  };

  const canSubmit =
    connected &&
    !busy &&
    fields.recipient.trim() !== "" &&
    fields.callsign.trim() !== "" &&
    fields.aircraft_type.trim() !== "" &&
    fields.dep_icao.trim() !== "" &&
    fields.dest_icao.trim() !== "" &&
    fields.stand.trim() !== "" &&
    fields.atis_letter.trim() !== "";

  const previewText = `REQUEST PREDEP CLEARANCE ${fields.callsign || "___"} ${
    fields.aircraft_type || "___"
  } TO ${fields.dest_icao || "___"} AT ${fields.dep_icao || "___"} STAND ${
    fields.stand || "___"
  } ATIS ${fields.atis_letter || "___"}`;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<PdcSendResult>("hoppie_send_pdc_request", {
        request: fields,
      });
      setLastSent(result);
      onSent();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="cpdlc-pdc-form">
      <h3>{t("cpdlc.pdc_form_title")}</h3>
      {!hasFlightContext && (
        <p className="settings__row-hint">{t("cpdlc.pdc_form_no_flight_hint")}</p>
      )}

      <div className="cpdlc-pdc-form__grid">
        <label className="cpdlc-pdc-form__field cpdlc-pdc-form__field--wide">
          <span>{t("cpdlc.pdc_form_recipient")}</span>
          <input type="text" value={fields.recipient} onChange={set("recipient")} disabled={busy} />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_callsign")}</span>
          <input type="text" value={fields.callsign} onChange={set("callsign")} disabled={busy} />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_aircraft_type")}</span>
          <input type="text" value={fields.aircraft_type} onChange={set("aircraft_type")} disabled={busy} />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_dep")}</span>
          <input type="text" value={fields.dep_icao} onChange={set("dep_icao")} disabled={busy} />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_dest")}</span>
          <input type="text" value={fields.dest_icao} onChange={set("dest_icao")} disabled={busy} />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_stand")}</span>
          <input
            type="text"
            value={fields.stand}
            onChange={set("stand")}
            placeholder={t("cpdlc.pdc_form_stand_placeholder")}
            disabled={busy}
          />
        </label>
        <label className="cpdlc-pdc-form__field">
          <span>{t("cpdlc.pdc_form_atis")}</span>
          <input
            type="text"
            maxLength={1}
            value={fields.atis_letter}
            onChange={set("atis_letter")}
            placeholder={t("cpdlc.pdc_form_atis_placeholder")}
            disabled={busy}
          />
        </label>
      </div>

      <div className="cpdlc-pdc-form__preview">
        <p className="settings__row-hint">{t("cpdlc.pdc_form_preview_label")}</p>
        <code>{previewText}</code>
      </div>

      {!connected && <p className="cpdlc-pdc-form__hint">{t("cpdlc.pdc_form_not_connected_hint")}</p>}
      {error && <p className="cpdlc-pdc-form__error">{error}</p>}
      {lastSent && !error && (
        <p className="cpdlc-pdc-form__success">{t("cpdlc.pdc_form_sent_hint")}</p>
      )}

      <button type="button" className="button button--primary" disabled={!canSubmit} onClick={() => void submit()}>
        {busy ? t("cpdlc.pdc_form_sending") : t("cpdlc.pdc_form_submit")}
      </button>
    </section>
  );
}
