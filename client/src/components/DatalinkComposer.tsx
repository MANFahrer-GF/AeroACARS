// v1.3.5 (#Datalink-3a) — composer, left column (README §3).
//
// PDC and CPDLC are one connection with two message kinds, not two tabs:
// switching the segmented control at the top only swaps the field grid
// below it and the primary action's label — history, the status row, and
// the rest of the layout never move (README §0, decision 3).
//
// Both modes share one preview/answer shape: a `Token[]` (static text,
// filled value, or blank placeholder) drives both the coloured preview
// and the "n of m open" counter, so PDC's fixed six fields and CPDLC's
// GOLD element placeholders render through the exact same code instead of
// two parallel implementations that could drift apart.

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";
import { useStationOnline, type StationStatus } from "../hooks/useStationOnline";
import { StationBadge } from "./StationBadge";
import type { DatalinkMode } from "./CpdlcPanel";

interface FlightContext {
  callsign: string | null;
  aircraft_type: string | null;
  dep_icao: string | null;
  dest_icao: string | null;
}

interface ElementSpecDto {
  id: string;
  template: string;
  placeholders: string[];
  response: string;
}

interface PdcFields {
  dep_icao: string;
  dest_icao: string;
  aircraft_type: string;
  stand: string;
  atis_letter: string;
}

type Token = { text: string; kind: "static" | "value" | "blank" };

/** Splits a "@1 @2 ..." template and pairs each slot with its value —
 *  shared by the PDC telex template and every GOLD element template. */
function tokenize(template: string, values: string[]): Token[] {
  return template.split(/(@\d+)/g).map((part): Token => {
    const m = /^@(\d+)$/.exec(part);
    if (!m) return { text: part, kind: "static" };
    const v = values[Number(m[1]) - 1]?.trim();
    return v ? { text: v, kind: "value" } : { text: "___", kind: "blank" };
  });
}

const PDC_TEMPLATE = "REQUEST PREDEP CLEARANCE @1 @2 TO @3 AT @4 STAND @5 ATIS @6";

const CPDLC_MENU: { key: string; ids: string[] }[] = [
  { key: "request", ids: ["DM9", "DM10", "DM22", "DM25", "DM15", "DM26", "DM27"] },
  { key: "expect", ids: ["DM49", "DM51", "DM52", "DM53"] },
  { key: "report", ids: ["DM32", "DM33", "DM34", "DM38", "DM48"] },
  { key: "emergency", ids: ["DM56", "DM55", "DM57", "DM61", "DM59", "DM60", "DM58"] },
];
const FREE_TEXT_ID = "DM67";

interface Props {
  online: boolean;
  mode: DatalinkMode;
  setMode: (m: DatalinkMode) => void;
  /** Resolved ACARS callsign (override, else the active flight's). Read
   *  only — the CALLSIGN cell in the PDC grid mirrors it but does not
   *  independently edit it; see the note at that field below. */
  callsign: string | null;
  loggedOn: boolean;
  onChanged: () => void;
  /** Mirrors the current PDC recipient up to the panel, which passes it
   *  on to the history — the wire has no per-message "sent to" field
   *  (ThreadEntryDto carries no station), so the history labels PDC
   *  traffic with whatever station is currently in this field. An
   *  approximation, same one the pre-rebuild PdcView already made. */
  onRecipientChange: (recipient: string) => void;

  // --- CPDLC logon (README §2d originally, now lives here — see the
  // block below for why). State ownership stays in CpdlcPanel (it
  // already computes `loggedOn`/`online` for other consumers, and
  // DatalinkHistory needs the station too); this component only renders
  // it and reports intent back up. ---
  stationInput: string;
  onStationInputChange: (value: string) => void;
  stationEditing: boolean;
  onStationEditingChange: (value: boolean) => void;
  logonBusy: boolean;
  logonWord: "ok" | "pending" | "none";
  logonValue: string;
  hasLogonOrPending: boolean;
  showStationInput: boolean;
  cpdlcStationStatus: StationStatus | null;
  alreadyOn: boolean;
  cooldownRunning: boolean;
  cooldownLeft: number;
  onSendLogon: () => void;
  onSendLogoff: () => void;
}

export function DatalinkComposer({
  online,
  mode,
  setMode,
  callsign,
  loggedOn,
  onChanged,
  onRecipientChange,
  stationInput,
  onStationInputChange,
  stationEditing,
  onStationEditingChange,
  logonBusy,
  logonWord,
  logonValue,
  hasLogonOrPending,
  showStationInput,
  cpdlcStationStatus,
  alreadyOn,
  cooldownRunning,
  cooldownLeft,
  onSendLogon,
  onSendLogoff,
}: Props) {
  const { t } = useTranslation();

  // --- PDC state ---
  const [recipient, setRecipientState] = useState("");
  const setRecipient = (value: string) => {
    setRecipientState(value);
    onRecipientChange(value);
  };
  const pdcStationStatus = useStationOnline(recipient, online);
  const [fields, setFields] = useState<PdcFields>({
    dep_icao: "",
    dest_icao: "",
    aircraft_type: "",
    stand: "",
    atis_letter: "",
  });

  useEffect(() => {
    void invoke<FlightContext>("hoppie_get_flight_context").then((ctx) => {
      // Prefill only what's still blank — a booking is a starting point,
      // never a re-assertion over something the pilot already typed
      // (README §3.3: "Vorbelegung ist niemals Festlegung").
      setRecipientState((prev) => {
        const next = prev || ctx.dep_icao || "";
        onRecipientChange(next);
        return next;
      });
      setFields((prev) => ({
        dep_icao: prev.dep_icao || ctx.dep_icao || "",
        dest_icao: prev.dest_icao || ctx.dest_icao || "",
        aircraft_type: prev.aircraft_type || ctx.aircraft_type || "",
        stand: prev.stand,
        atis_letter: prev.atis_letter,
      }));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setField = (key: keyof PdcFields) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setFields((prev) => ({ ...prev, [key]: e.target.value.toUpperCase() }));

  // --- CPDLC state ---
  const [elements, setElements] = useState<ElementSpecDto[]>([]);
  const [group, setGroup] = useState("request");
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [values, setValues] = useState<string[]>([]);

  useEffect(() => {
    void invoke<ElementSpecDto[]>("hoppie_list_elements").then(setElements);
  }, []);

  const byId = useMemo(() => new Map(elements.map((e) => [e.id, e])), [elements]);
  const shown = useMemo(() => {
    const q = search.trim().toUpperCase();
    if (q !== "") {
      return elements
        .filter((e) => e.template.toUpperCase().includes(q) || e.id.toUpperCase() === q)
        .slice(0, 30);
    }
    if (group === "freetext") return [];
    const ids = CPDLC_MENU.find((m) => m.key === group)?.ids ?? [];
    return ids.map((id) => byId.get(id)).filter((e): e is ElementSpecDto => Boolean(e));
  }, [elements, byId, group, search]);
  const selected = selectedId ? (byId.get(selectedId) ?? null) : null;

  const pick = (e: ElementSpecDto) => {
    setSelectedId(e.id);
    setValues(new Array(e.placeholders.length).fill(""));
  };

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // --- Shared preview/counter, built the same way for both modes ---
  const tokens: Token[] =
    mode === "pdc"
      ? tokenize(PDC_TEMPLATE, [
          callsign ?? "",
          fields.aircraft_type,
          fields.dest_icao,
          fields.dep_icao,
          fields.stand,
          fields.atis_letter,
        ])
      : selected
        ? tokenize(selected.template, values)
        : [];

  // PDC counts the station field too (it gates sending just as much as
  // the six grid fields, even though it never appears in the telex
  // BODY) — matches the "1 von 7 offen" reading in datalink-dark.png.
  const blankCount =
    tokens.filter((tk) => tk.kind === "blank").length + (mode === "pdc" && recipient.trim() === "" ? 1 : 0);
  const totalCount = (mode === "pdc" ? 7 : selected ? selected.placeholders.length : 0);

  const canSubmit =
    mode === "pdc"
      ? online && !busy && recipient.trim() !== "" && blankCount === 0
      : online && loggedOn && !busy && selected !== null && blankCount === 0;

  const submitPdc = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_pdc_request", {
        request: {
          recipient: recipient.trim().toUpperCase(),
          aircraft_type: fields.aircraft_type.trim().toUpperCase(),
          dep_icao: fields.dep_icao.trim().toUpperCase(),
          dest_icao: fields.dest_icao.trim().toUpperCase(),
          stand: fields.stand.trim().toUpperCase(),
          atis_letter: fields.atis_letter.trim().toUpperCase(),
        },
      });
      onChanged();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const submitCpdlc = async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_cpdlc_element", {
        elementId: selected.id,
        values: values.map((v) => v.trim()),
        mrn: null,
      });
      setSelectedId("");
      setValues([]);
      setSearch("");
      onChanged();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = () => void (mode === "pdc" ? submitPdc() : submitCpdlc());

  // Next unfilled field in the PDC grid gets the accent border — exactly
  // one at a time, so the pilot sees where to go next without an error
  // message (README §3.3).
  const PDC_FIELD_ORDER: (keyof PdcFields)[] = [
    "dep_icao",
    "dest_icao",
    "aircraft_type",
    "stand",
    "atis_letter",
  ];
  const nextEmptyPdcField = PDC_FIELD_ORDER.find((k) => fields[k].trim() === "") ?? null;
  const nextEmptyCpdlcIndex = selected ? values.findIndex((v) => v.trim() === "") : -1;

  const pdcField = (key: keyof PdcFields, labelKey: string, opts?: { maxLength?: number; placeholderKey?: string }) => (
    <label
      className={`datalink-field ${nextEmptyPdcField === key ? "datalink-field--focus" : ""}`}
    >
      <span>{t(labelKey)}</span>
      <input
        type="text"
        value={fields[key]}
        onChange={setField(key)}
        maxLength={opts?.maxLength}
        placeholder={opts?.placeholderKey ? t(opts.placeholderKey) : undefined}
        disabled={busy}
      />
    </label>
  );

  return (
    <div className="datalink-composer">
      <div className="datalink-composer__mode" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={mode === "pdc"}
          className={`datalink-mode ${mode === "pdc" ? "datalink-mode--active" : ""}`}
          onClick={() => setMode("pdc")}
        >
          {t("cpdlc.mode_pdc")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === "cpdlc"}
          className={`datalink-mode ${mode === "cpdlc" ? "datalink-mode--active" : ""}`}
          onClick={() => setMode("cpdlc")}
        >
          {t("cpdlc.mode_cpdlc")}
        </button>
      </div>

      {mode === "pdc" && (
        <div className="datalink-station">
          <div className="datalink-station__head">
            <label className="datalink-station__head-label" htmlFor="datalink-pdc-station-input">
              {t("cpdlc.station_label")}
              <StationBadge status={pdcStationStatus} />
            </label>
            <span className="datalink-station__hint">{t("cpdlc.station_hint")}</span>
          </div>
          <div className="datalink-station__box">
            <input
              id="datalink-pdc-station-input"
              type="text"
              value={recipient}
              onChange={(e) => setRecipient(e.target.value.toUpperCase().slice(0, 12))}
              disabled={busy}
            />
            <span className="datalink-station__edit" aria-hidden="true">
              {t("cpdlc.station_edit_hint")}
            </span>
          </div>
        </div>
      )}

      {mode === "pdc" ? (
        <div className="datalink-fieldgrid">
          {pdcField("dep_icao", "cpdlc.field_dep")}
          {pdcField("dest_icao", "cpdlc.field_dest")}
          {pdcField("aircraft_type", "cpdlc.field_type")}
          {pdcField("stand", "cpdlc.field_stand", { placeholderKey: "cpdlc.field_stand_placeholder" })}
          {pdcField("atis_letter", "cpdlc.field_atis", { maxLength: 1, placeholderKey: "cpdlc.field_atis_placeholder" })}
          {/* Deliberately NOT wired to `fields` / the outgoing request: the
              backend always sends PDC under the ACARS connection's own
              callsign (mod.rs: "A second, separately-typed callsign meant
              PDC went out under a different identity than CPDLC") — a
              real fix for a real field bug. This cell mirrors that value
              for the same at-a-glance layout the mockup shows, but a
              second, independently-editable callsign field would
              reintroduce exactly the bug that comment describes. */}
          <label className="datalink-field">
            <span>{t("cpdlc.field_callsign")}</span>
            <input type="text" value={callsign ?? ""} disabled readOnly title={t("cpdlc.field_callsign_hint")} />
          </label>
        </div>
      ) : (
        <div className="datalink-cpdlc-picker">
          {/* Used to live in the shared 66px status row above (README §2d
              originally) — dimmed-and-tooltipped there in PDC mode. That
              fixed-height row fighting for space against connection state
              AND the callsign block was the source of three separate real
              bugs in one afternoon (an input-width CSS collision, a poll
              timer pushed off-canvas, a hyphen in "CPDLC-LOGON" wrapping
              the label and having the overflow rules clip the rest) —
              every one of them a symptom of the same thing: cramming
              variable-width, mode-only-sometimes-relevant content into a
              row that also has to hold things that are ALWAYS relevant.
              CPDLC logon is only ever relevant in CPDLC mode anyway (it
              was dimmed out entirely in PDC mode before), so it belongs
              with the rest of the CPDLC-only UI, with a full composer
              column's width to breathe in instead of one fixed-height
              shared line. */}
          <div className="datalink-logon">
            <div className="datalink-logon__head">
              <label className="datalink-logon__head-label" htmlFor="datalink-cpdlc-logon-input">
                {t("cpdlc.logon_label")}
                {showStationInput && <StationBadge status={cpdlcStationStatus} />}
              </label>
              <span className={`datalink-status-word datalink-status-word--${logonWord}`}>
                {logonWord === "ok"
                  ? t("cpdlc.logon_ok")
                  : logonWord === "pending"
                    ? t("cpdlc.logon_running")
                    : t("cpdlc.logon_none")}
              </span>
            </div>
            <div className="datalink-logon__row">
              {showStationInput ? (
                <input
                  id="datalink-cpdlc-logon-input"
                  autoFocus={stationEditing}
                  type="text"
                  className="datalink-logon__input"
                  value={stationInput}
                  onChange={(e) => onStationInputChange(e.target.value.toUpperCase())}
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && hasLogonOrPending) onStationEditingChange(false);
                    if (e.key === "Enter" && !hasLogonOrPending) onSendLogon();
                  }}
                  placeholder={t("cpdlc.center_placeholder")}
                  disabled={logonBusy}
                />
              ) : (
                <span className="datalink-logon__value">{logonValue || "—"}</span>
              )}
              <div className="datalink-logon__actions">
                {hasLogonOrPending ? (
                  <>
                    <button
                      type="button"
                      className="button button--secondary"
                      disabled={logonBusy}
                      onClick={() => onStationEditingChange(!stationEditing)}
                    >
                      {t("cpdlc.logon_switch")}
                    </button>
                    <button
                      type="button"
                      className="button button--secondary"
                      disabled={!online || logonBusy}
                      onClick={onSendLogoff}
                    >
                      {t("cpdlc.logoff")}
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="button button--secondary"
                    disabled={!online || logonBusy || stationInput.trim() === "" || alreadyOn || cooldownRunning}
                    title={cooldownRunning ? t("cpdlc.logon_cooldown", { seconds: cooldownLeft }) : undefined}
                    onClick={onSendLogon}
                  >
                    {t("cpdlc.logon_send")}
                  </button>
                )}
              </div>
            </div>
          </div>
          <div className="datalink-composer__groups" role="tablist">
            {CPDLC_MENU.map((m) => (
              <button
                key={m.key}
                type="button"
                role="tab"
                aria-selected={group === m.key && search === ""}
                className={`cpdlc-chip ${group === m.key && search === "" ? "cpdlc-chip--active" : ""}`}
                onClick={() => {
                  setGroup(m.key);
                  setSearch("");
                  setSelectedId("");
                }}
              >
                {t(`cpdlc.group_${m.key}`)}
              </button>
            ))}
            <button
              type="button"
              role="tab"
              aria-selected={group === "freetext" && search === ""}
              className={`cpdlc-chip ${group === "freetext" && search === "" ? "cpdlc-chip--active" : ""}`}
              onClick={() => {
                setGroup("freetext");
                setSearch("");
                const spec = byId.get(FREE_TEXT_ID);
                if (spec) pick(spec);
              }}
            >
              {t("cpdlc.group_freetext")}
            </button>
          </div>
          <input
            type="text"
            className="datalink-cpdlc-picker__search"
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setSelectedId("");
            }}
            placeholder={t("cpdlc.composer_search_all")}
          />
          {shown.length > 0 && (
            <ul className="datalink-cpdlc-picker__options">
              {shown.map((e) => (
                <li key={e.id}>
                  <button
                    type="button"
                    className={`cpdlc-composer__option ${selectedId === e.id ? "cpdlc-composer__option--active" : ""}`}
                    onClick={() => pick(e)}
                  >
                    <span className="cpdlc-composer__option-text">{e.template.replace(/@\d+/g, "___")}</span>
                    <span className="cpdlc-composer__option-id">{e.id}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {search !== "" && shown.length === 0 && <p className="cpdlc-log__empty">{t("cpdlc.composer_no_match")}</p>}
          {selected && (
            <div className="datalink-cpdlc-picker__detail">
              {selected.placeholders.map((kind, i) => (
                <label
                  key={i}
                  className={`datalink-field ${nextEmptyCpdlcIndex === i ? "datalink-field--focus" : ""}`}
                >
                  <span>{t(`cpdlc.placeholder_${kind}`, { defaultValue: kind })}</span>
                  <input
                    type="text"
                    value={values[i] ?? ""}
                    onChange={(e) => {
                      const next = [...values];
                      next[i] = e.target.value.toUpperCase();
                      setValues(next);
                    }}
                    disabled={busy}
                  />
                </label>
              ))}
            </div>
          )}
        </div>
      )}

      {(mode === "pdc" || selected) && (
        <div className="datalink-preview">
          <div className="datalink-preview__head">
            <span>{t("cpdlc.preview_label")}</span>
            <span>{t("cpdlc.preview_open", { open: blankCount, total: totalCount })}</span>
          </div>
          <p className="datalink-preview__text">
            {tokens.map((tk, i) => (
              <span
                key={i}
                className={
                  tk.kind === "value"
                    ? "datalink-preview__value"
                    : tk.kind === "blank"
                      ? "datalink-preview__blank"
                      : undefined
                }
              >
                {tk.text}
              </span>
            ))}
          </p>
        </div>
      )}

      {error && <p className="cpdlc-panel__error">{error}</p>}

      <div className="datalink-primary">
        <button type="button" className="datalink-primary__button" disabled={!canSubmit} aria-disabled={!canSubmit} onClick={submit}>
          {busy ? t("cpdlc.pdc_form_sending") : mode === "pdc" ? t("cpdlc.mode_pdc") : t("cpdlc.mode_cpdlc")}
        </button>
        <p className="datalink-primary__help">
          {mode === "pdc" ? t("cpdlc.helper_pdc") : loggedOn ? t("cpdlc.helper_cpdlc") : t("cpdlc.helper_cpdlc_no_logon")}
        </p>
      </div>
      <div className="datalink-composer__spacer" />
    </div>
  );
}
