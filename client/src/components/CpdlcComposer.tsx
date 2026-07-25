// v1.3.0 (#Hoppie-PDC-CPDLC) — CPDLC downlink composer.
//
// A pilot picks from a short menu, the way a real MCDU ATC page works:
// REQUEST / WHEN CAN WE EXPECT / REPORT / EMERGENCY / free text. The
// full ~287-element GOLD catalog stays reachable behind a search box,
// but it is not the default surface — presenting all of it at once was
// the reason this panel was unusable.
//
// Menu ids are checked against elements_data.rs. DM6/DM18/DM23/DM24 are
// deliberately absent: they all render as a bare "REQUEST @1" and are
// indistinguishable in a menu, so they only appear in the search.

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";

interface ElementSpecDto {
  id: string;
  template: string;
  placeholders: string[];
  response: string;
}

const MENU: { key: string; ids: string[] }[] = [
  { key: "request", ids: ["DM9", "DM10", "DM22", "DM25", "DM15", "DM26", "DM27"] },
  { key: "expect", ids: ["DM49", "DM51", "DM52", "DM53"] },
  { key: "report", ids: ["DM32", "DM33", "DM34", "DM38", "DM48"] },
  { key: "emergency", ids: ["DM56", "DM55", "DM57", "DM61", "DM59", "DM60", "DM58"] },
];

const FREE_TEXT_ID = "DM67";

interface Props {
  connected: boolean;
  onSent: () => void;
  onClose: () => void;
}

export function CpdlcComposer({ connected, onSent, onClose }: Props) {
  const { t } = useTranslation();
  const [elements, setElements] = useState<ElementSpecDto[]>([]);
  const [group, setGroup] = useState<string>("request");
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [values, setValues] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<ElementSpecDto[]>("hoppie_list_elements").then(setElements);
  }, []);

  const byId = useMemo(() => new Map(elements.map((e) => [e.id, e])), [elements]);

  const shown = useMemo(() => {
    const q = search.trim().toUpperCase();
    if (q !== "") {
      return elements
        .filter((e) => e.template.toUpperCase().includes(q) || e.id.toUpperCase() === q)
        .slice(0, 40);
    }
    if (group === "freetext") return [];
    const ids = MENU.find((m) => m.key === group)?.ids ?? [];
    return ids.map((id) => byId.get(id)).filter((e): e is ElementSpecDto => Boolean(e));
  }, [elements, byId, group, search]);

  const selected = selectedId ? (byId.get(selectedId) ?? null) : null;

  const pick = (e: ElementSpecDto) => {
    setSelectedId(e.id);
    setValues(new Array(e.placeholders.length).fill(""));
  };

  const preview = selected
    ? selected.placeholders.reduce(
        (text, _k, i) => text.replace(`@${i + 1}`, values[i]?.trim() || "___"),
        selected.template,
      )
    : "";

  const ready = selected !== null && values.every((v) => v.trim() !== "");

  const send = async () => {
    if (!selected || !ready || busy) return;
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
      onSent();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  const freeTextSpec = byId.get(FREE_TEXT_ID);

  return (
    <section className="cpdlc-composer">
      <header className="cpdlc-composer__head">
        <div className="cpdlc-composer__groups" role="tablist">
          {MENU.map((m) => (
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
              if (freeTextSpec) pick(freeTextSpec);
            }}
          >
            {t("cpdlc.group_freetext")}
          </button>
        </div>
        <button type="button" className="cpdlc-composer__close" onClick={onClose} aria-label={t("cpdlc.form_cancel")}>
          ✕
        </button>
      </header>

      <input
        type="text"
        className="cpdlc-composer__search"
        value={search}
        onChange={(e) => {
          setSearch(e.target.value);
          setSelectedId("");
        }}
        placeholder={t("cpdlc.composer_search_all")}
      />

      {shown.length > 0 && (
        <ul className="cpdlc-composer__options">
          {shown.map((e) => (
            <li key={e.id}>
              <button
                type="button"
                className={`cpdlc-composer__option ${selectedId === e.id ? "cpdlc-composer__option--active" : ""}`}
                onClick={() => pick(e)}
              >
                <span className="cpdlc-composer__option-text">
                  {e.template.replace(/@\d+/g, "___")}
                </span>
                <span className="cpdlc-composer__option-id">{e.id}</span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {search !== "" && shown.length === 0 && (
        <p className="cpdlc-log__empty">{t("cpdlc.composer_no_match")}</p>
      )}

      {selected && (
        <div className="cpdlc-composer__detail">
          {selected.placeholders.map((kind, i) => (
            <label key={i} className="cpdlc-field">
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
          <code className="cpdlc-form__preview">{preview}</code>
          <div className="cpdlc-form__actions">
            <button
              type="button"
              className="button button--primary"
              disabled={!connected || !ready || busy}
              onClick={() => void send()}
            >
              {busy ? t("cpdlc.pdc_form_sending") : t("cpdlc.composer_send")}
            </button>
          </div>
        </div>
      )}

      {error && <p className="cpdlc-panel__error">{error}</p>}
    </section>
  );
}
