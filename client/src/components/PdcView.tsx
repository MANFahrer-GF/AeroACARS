// v1.3.0 (#Hoppie-PDC-CPDLC) — PDC section.
//
// PDC has no logon. The request is a plain telex to whichever station
// issues the clearance (Delivery), and the clearance comes back the same
// way. That's the whole protocol — deliberately kept apart from the
// CPDLC section, which does have a session and a facility to log on to.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { formatDatalinkText } from "../lib/datalink";
import type { ThreadEntry } from "../hooks/useCpdlcMessages";
import { TelexQuickReply } from "./CpdlcQuickReply";
import { useMessageLog } from "../hooks/useMessageLog";
import { useStationOnline } from "../hooks/useStationOnline";
import { StationBadge } from "./StationBadge";

interface FlightContext {
  callsign: string | null;
  aircraft_type: string | null;
  dep_icao: string | null;
  dest_icao: string | null;
}

interface PdcRequestArgs {
  recipient: string;
  aircraft_type: string;
  dep_icao: string;
  dest_icao: string;
  stand: string;
  atis_letter: string;
}

interface Props {
  online: boolean;
  /** The ACARS callsign from the connection bar. PDC uses the same one,
   *  so it prefills from here when there's no active flight to take it
   *  from — otherwise the pilot types the same callsign twice. */
  callsign: string | null;
  messages: ThreadEntry[];
  onChanged: () => void;
}

export function PdcView({ online, callsign, messages, onChanged }: Props) {
  const { t } = useTranslation();
  const [fields, setFields] = useState<PdcRequestArgs>({
    recipient: "",
    aircraft_type: "",
    dep_icao: "",
    dest_icao: "",
    stand: "",
    atis_letter: "",
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<FlightContext>("hoppie_get_flight_context").then((ctx) => {
      setFields((prev) => ({
        ...prev,
        recipient: ctx.dep_icao ?? prev.recipient,
        aircraft_type: ctx.aircraft_type ?? prev.aircraft_type,
        dep_icao: ctx.dep_icao ?? prev.dep_icao,
        dest_icao: ctx.dest_icao ?? prev.dest_icao,
      }));
    });
  }, [callsign]);

  // Normalize on input, not via CSS. `text-transform: uppercase` only
  // restyles the glyphs — the stored value stayed lowercase, so the
  // preview showed lowercase while the wire got uppercase. Datalink is
  // uppercase everywhere, so the value itself is the right place.
  const set = (key: keyof PdcRequestArgs) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setFields((prev) => ({ ...prev, [key]: e.target.value.toUpperCase() }));

  // Naming the empty fields beats a dead button the pilot has to poke at
  // to discover what's wrong.
  const FIELD_LABELS: [keyof PdcRequestArgs, string][] = [
    ["recipient", "cpdlc.pdc_station_label"],
    ["aircraft_type", "cpdlc.pdc_form_aircraft_type"],
    ["dep_icao", "cpdlc.pdc_form_dep"],
    ["dest_icao", "cpdlc.pdc_form_dest"],
    ["stand", "cpdlc.pdc_form_stand"],
    ["atis_letter", "cpdlc.pdc_form_atis"],
  ];
  const missing = FIELD_LABELS.filter(([k]) => fields[k].trim() === "").map(([, l]) => t(l));
  const canSubmit = online && !busy && missing.length === 0;

  const preview = `REQUEST PREDEP CLEARANCE ${callsign || "___"} ${
    fields.aircraft_type || "___"
  } TO ${fields.dest_icao || "___"} AT ${fields.dep_icao || "___"} STAND ${
    fields.stand || "___"
  } ATIS ${fields.atis_letter || "___"}`;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    const normalized = Object.fromEntries(
      Object.entries(fields).map(([k, v]) => [k, v.trim().toUpperCase()]),
    ) as PdcRequestArgs;
    try {
      await invoke("hoppie_send_pdc_request", { request: normalized });
      onChanged();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const telex = messages.filter((m) => m.kind === "telex");
  // Same reading behaviour as the CPDLC log: newest in view, settled
  // history folded above it.
  const { logRef, onScroll, toggle, isCollapsed } = useMessageLog(telex.length);
  const stationStatus = useStationOnline(fields.recipient, online);

  return (
    <div className="cpdlc-section">
      <div className="cpdlc-section__side">
      <p className="cpdlc-section__explain">{t("cpdlc.pdc_explain")}</p>

      <form
        className="cpdlc-form"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
          <div className="cpdlc-form__grid">
            <label className="cpdlc-field">
              <span>{t("cpdlc.pdc_station_label")}</span>
              <input
                type="text"
                value={fields.recipient}
                onChange={set("recipient")}
                placeholder={t("cpdlc.pdc_station_placeholder")}
                disabled={busy}
              />
            </label>
            <label className="cpdlc-field">
              <span>{t("cpdlc.pdc_form_aircraft_type")}</span>
              <input
                type="text"
                value={fields.aircraft_type}
                onChange={set("aircraft_type")}
                disabled={busy}
              />
            </label>
            {/* Own full-width grid row instead of living inside the DELIVERY
                field (v1.2.3 fix 2026-07-31, #Hoppie-PDC-CPDLC): nested in
                the field, the badge only added height to DELIVERY, not to
                FLUGZEUGTYP next to it — their inputs no longer lined up
                (field feedback: "DELIVERY wandert, nicht mehr in einer
                Reihe"). Spanning the whole grid means it can never make one
                cell taller than its row partner; DELIVERY/FLUGZEUGTYP stay
                exactly the same height as every other field pair. */}
            <div className="cpdlc-field__badge-row">
              <StationBadge status={stationStatus} />
            </div>
            <label className="cpdlc-field">
              <span>{t("cpdlc.pdc_form_dep")}</span>
              <input type="text" value={fields.dep_icao} onChange={set("dep_icao")} disabled={busy} />
            </label>
            <label className="cpdlc-field">
              <span>{t("cpdlc.pdc_form_dest")}</span>
              <input type="text" value={fields.dest_icao} onChange={set("dest_icao")} disabled={busy} />
            </label>
            <label className="cpdlc-field">
              <span>{t("cpdlc.pdc_form_stand")}</span>
              <input
                type="text"
                value={fields.stand}
                onChange={set("stand")}
                placeholder={t("cpdlc.pdc_form_stand_placeholder")}
                disabled={busy}
              />
            </label>
            <label className="cpdlc-field">
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
          <code className="cpdlc-form__preview">{preview}</code>
          {missing.length > 0 && (
            <p className="cpdlc-form__missing" data-testid="pdc-missing">
              {t("cpdlc.pdc_missing", { fields: missing.join(", ") })}
            </p>
          )}
          {error && <p className="cpdlc-panel__error">{error}</p>}
          <div className="cpdlc-form__actions">
            <button type="submit" className="button button--primary" disabled={!canSubmit}>
              {busy ? t("cpdlc.pdc_form_sending") : t("cpdlc.pdc_form_submit")}
            </button>
          </div>
      </form>
      </div>

      <ul className="cpdlc-log" ref={logRef} onScroll={onScroll}>
        {telex.length === 0 && <li className="cpdlc-log__empty">{t("cpdlc.pdc_log_empty")}</li>}
        {telex.map((m, i) => {
          const key = `${m.at}-${i}`;
          // An inbound clearance stays open until it's acknowledged —
          // that's the one thing that must never fold away.
          const live = m.direction === "received" && i >= telex.length - 1;
          const collapsed = isCollapsed(i, key, live);
          return (
            <li
              key={key}
              className={`cpdlc-msg cpdlc-msg--${m.direction === "sent" ? "down" : "up"} ${
                collapsed ? "cpdlc-msg--collapsed" : ""
              }`}
              onClick={collapsed ? () => toggle(key) : undefined}
              role={collapsed ? "button" : undefined}
              tabIndex={collapsed ? 0 : undefined}
              onKeyDown={
                collapsed
                  ? (e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        toggle(key);
                      }
                    }
                  : undefined
              }
              title={collapsed ? t("cpdlc.expand_hint") : undefined}
            >
              <p className="cpdlc-msg__meta">
                <span className="cpdlc-msg__who">
                  {m.direction === "sent"
                    ? t("cpdlc.thread_sent")
                    : fields.recipient || t("cpdlc.thread_received")}
                </span>
                <span className="cpdlc-msg__time">{new Date(m.at).toLocaleTimeString()}</span>
              </p>
              <p className="cpdlc-msg__text">
                {collapsed ? m.text.replace(/@/g, " ") : formatDatalinkText(m.text)}
              </p>
              {m.direction === "received" && !collapsed && (
                <TelexQuickReply
                  recipient={fields.recipient || null}
                  clearance={m.text}
                  onReplied={onChanged}
                />
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
