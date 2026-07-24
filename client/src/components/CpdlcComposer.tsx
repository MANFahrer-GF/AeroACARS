// v1.3.0 (#Hoppie-PDC-CPDLC) — CPDLC downlink composer.
//
// Structured mode renders one typed input per placeholder, generated
// directly from `hoppie_list_elements` (the full ~287-row GOLD downlink
// catalog) — the payoff of the data-driven element table: this UI is
// generated, not hand-built per element. Free-text stays available as the
// always-present escape hatch (real traffic is never textbook-perfect).

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, formatIpcError } from "../lib/ipc";

interface ElementSpecDto {
  id: string;
  template: string;
  placeholders: string[];
  response: string;
}

interface Props {
  connected: boolean;
  onSent: () => void;
}

export function CpdlcComposer({ connected, onSent }: Props) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<"structured" | "freetext">("freetext");
  const [elements, setElements] = useState<ElementSpecDto[]>([]);
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [values, setValues] = useState<string[]>([]);
  const [freeText, setFreeText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<ElementSpecDto[]>("hoppie_list_elements").then(setElements);
  }, []);

  const filtered = useMemo(() => {
    const q = search.trim().toUpperCase();
    if (q === "") return elements.slice(0, 30);
    return elements.filter((e) => e.template.toUpperCase().includes(q) || e.id.toUpperCase() === q).slice(0, 30);
  }, [elements, search]);

  const selected = elements.find((e) => e.id === selectedId) ?? null;

  const pick = (e: ElementSpecDto) => {
    setSelectedId(e.id);
    setValues(new Array(e.placeholders.length).fill(""));
  };

  const previewText = selected
    ? selected.placeholders.reduce(
        (text, _kind, i) => text.replace(`@${i + 1}`, values[i]?.trim() || `[${i + 1}]`),
        selected.template,
      )
    : "";

  const sendStructured = async () => {
    if (!selected || busy) return;
    if (values.some((v) => v.trim() === "")) return;
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

  const sendFreeText = async () => {
    if (freeText.trim() === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("hoppie_send_free_text", { text: freeText.trim(), mrn: null });
      setFreeText("");
      onSent();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="cpdlc-composer">
      <h3>{t("cpdlc.composer_title")}</h3>
      <div className="cpdlc-composer__mode-switch">
        <button
          type="button"
          className={`button ${mode === "freetext" ? "button--primary" : ""}`}
          onClick={() => setMode("freetext")}
        >
          {t("cpdlc.composer_free_text")}
        </button>
        <button
          type="button"
          className={`button ${mode === "structured" ? "button--primary" : ""}`}
          onClick={() => setMode("structured")}
        >
          {t("cpdlc.composer_structured")}
        </button>
      </div>

      {mode === "freetext" && (
        <div className="cpdlc-composer__freetext">
          <textarea
            rows={2}
            value={freeText}
            onChange={(e) => setFreeText(e.target.value)}
            placeholder={t("cpdlc.composer_free_text_placeholder")}
            disabled={!connected || busy}
          />
          <button
            type="button"
            className="button button--primary"
            disabled={!connected || busy || freeText.trim() === ""}
            onClick={() => void sendFreeText()}
          >
            {t("cpdlc.composer_send")}
          </button>
        </div>
      )}

      {mode === "structured" && (
        <div className="cpdlc-composer__structured">
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("cpdlc.composer_search_placeholder")}
            disabled={!connected}
          />
          <ul className="cpdlc-composer__element-list">
            {filtered.map((e) => (
              <li key={e.id}>
                <button
                  type="button"
                  className={`cpdlc-composer__element-option ${selectedId === e.id ? "cpdlc-composer__element-option--active" : ""}`}
                  onClick={() => pick(e)}
                  disabled={!connected}
                >
                  <span className="cpdlc-composer__element-id">{e.id}</span>
                  <span>{e.template.replace(/@\d+/g, "…")}</span>
                </button>
              </li>
            ))}
          </ul>

          {selected && (
            <div className="cpdlc-composer__fields">
              {selected.placeholders.map((kind, i) => (
                <label key={i} className="cpdlc-composer__field">
                  <span>{kind}</span>
                  <input
                    type="text"
                    value={values[i] ?? ""}
                    onChange={(e) => {
                      const next = [...values];
                      next[i] = e.target.value;
                      setValues(next);
                    }}
                    disabled={busy}
                  />
                </label>
              ))}
              <p className="cpdlc-composer__preview">{previewText}</p>
              <button
                type="button"
                className="button button--primary"
                disabled={!connected || busy || values.some((v) => v.trim() === "")}
                onClick={() => void sendStructured()}
              >
                {t("cpdlc.composer_send")}
              </button>
            </div>
          )}
        </div>
      )}

      {error && <p className="cpdlc-panel__error">{error}</p>}
    </section>
  );
}
