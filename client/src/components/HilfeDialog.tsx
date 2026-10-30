// Hilfe anfordern (AeroACARS 3.0, P-9.20 bis P-9.22, N-06).
//
// Der Pilot schreibt Betreff und Text; der Client lädt zuerst die Diagnose
// auf live (Kennung `D-…`) und legt dann das Ticket im GSG-HelpDesk an
// (Kategorie AeroACARS). Was mit der Diagnose mitgeht, steht VOR dem Senden
// im Dialog (P-9.22). Scheitert das Ticket, bleibt die Kennung sichtbar, damit
// der Pilot sie weitergeben kann. Netz und Schlüssel bleiben in Rust
// (`hilfe_anfordern`), die Oberfläche bekommt nur das Ergebnis.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import "./hilfeDialog.css";

export interface HilfeVorschau {
  meta: {
    client_version: string;
    sim: string;
    sim_version: string | null;
    flugzeug_profil: string | null;
    addon: string | null;
    paket_generation: string;
    letzte_flugdatei: string | null;
  };
  dateien: { name: string; bytes: number }[];
  log_gesperrt: boolean;
}

export interface HilfeErgebnis {
  kennung: string | null;
  link: string | null;
  diagnose_fehler: string | null;
  ticket: { id: unknown; ticket_number: string | null; url: string | null } | null;
  ticket_fehler: string | null;
}

const BETREFF_MAX = 200;
const TEXT_MAX = 20000;

function fehlerCode(e: unknown): string {
  if (e && typeof e === "object" && "code" in e && typeof (e as { code: unknown }).code === "string") {
    return (e as { code: string }).code;
  }
  return "unbekannt";
}

function kb(bytes: number): string {
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function HilfeDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [vorschau, setVorschau] = useState<HilfeVorschau | null>(null);
  const [betreff, setBetreff] = useState("");
  const [text, setText] = useState("");
  const [sendet, setSendet] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [ergebnis, setErgebnis] = useState<HilfeErgebnis | null>(null);

  useEffect(() => {
    let weg = false;
    invoke<HilfeVorschau>("hilfe_vorschau")
      .then((v) => {
        if (!weg) setVorschau(v);
      })
      .catch(() => {
        /* ohne Vorschau bleibt die Liste leer; Senden geht trotzdem */
      });
    return () => {
      weg = true;
    };
  }, []);

  const fehlerText = (code: string) =>
    t(`hilfe.fehler.${code}`, { defaultValue: t("hilfe.fehler.unbekannt") });

  const senden = async () => {
    setFehler(null);
    setSendet(true);
    try {
      setErgebnis(await invoke<HilfeErgebnis>("hilfe_anfordern", { betreff, text }));
    } catch (e) {
      setFehler(fehlerText(fehlerCode(e)));
    } finally {
      setSendet(false);
    }
  };

  const oeffnen = async (url: string) => {
    try {
      await invoke("plugin:opener|open_url", { url });
    } catch {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  };

  const m = vorschau?.meta;
  const wert = (v: string | null | undefined) => (v && v.length > 0 ? v : t("hilfe.unbekannt"));
  const kannSenden =
    !sendet && betreff.trim().length > 0 && text.trim().length > 0 && betreff.length <= BETREFF_MAX && text.length <= TEXT_MAX;

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="hilfe-titel" onClick={onClose}>
      <div className="modal modal--wide hilfe" onClick={(e) => e.stopPropagation()}>
        <h2 className="modal__title" id="hilfe-titel">
          {ergebnis ? (ergebnis.ticket ? t("hilfe.ok_titel") : t("hilfe.fehler_titel")) : t("hilfe.titel")}
        </h2>

        {!ergebnis && (
          <div className="hilfe__body">
            <p className="modal__hint">{t("hilfe.einleitung")}</p>
            <div className="hilfe__kategorie">
              <span className="field__label">{t("hilfe.kategorie")}</span>
              <span className="hilfe__kategorie-wert">AeroACARS</span>
            </div>
            <label className="field">
              <span className="field__label">{t("hilfe.betreff")}</span>
              <input
                type="text"
                value={betreff}
                maxLength={BETREFF_MAX}
                onChange={(e) => setBetreff(e.target.value.replace(/[\r\n]/g, " "))}
                disabled={sendet}
              />
            </label>
            <label className="field">
              <span className="field__label">{t("hilfe.text")}</span>
              <textarea
                className="hilfe__text"
                value={text}
                maxLength={TEXT_MAX}
                rows={4}
                placeholder={t("hilfe.text_platzhalter")}
                onChange={(e) => setText(e.target.value)}
                disabled={sendet}
              />
            </label>
            <section className="hilfe__mitgeht" data-testid="hilfe-mitgeht">
              <h3>{t("hilfe.mitgeht_titel")}</h3>
              <dl>
                <dt>{t("hilfe.feld.client_version")}</dt>
                <dd>{wert(m?.client_version)}</dd>
                <dt>{t("hilfe.feld.sim")}</dt>
                <dd>
                  {wert(m?.sim)}
                  {m?.sim_version ? ` · ${m.sim_version}` : ""}
                </dd>
                <dt>{t("hilfe.feld.flugzeug_profil")}</dt>
                <dd>{wert(m?.flugzeug_profil)}</dd>
                <dt>{t("hilfe.feld.addon")}</dt>
                <dd>{wert(m?.addon)}</dd>
                <dt>{t("hilfe.feld.paket_generation")}</dt>
                <dd>{wert(m?.paket_generation)}</dd>
                <dt>{t("hilfe.feld.letzte_flugdatei")}</dt>
                <dd>{wert(m?.letzte_flugdatei)}</dd>
                <dt>{t("hilfe.feld.logs")}</dt>
                <dd>
                  {vorschau?.log_gesperrt
                    ? t("hilfe.log_gesperrt")
                    : vorschau && vorschau.dateien.length > 0
                      ? vorschau.dateien.map((d) => `${d.name} (${kb(d.bytes)})`).join(", ")
                      : t("hilfe.keine_logs")}
                </dd>
              </dl>
              <p className="modal__hint">{t("hilfe.mitgeht_hinweis")}</p>
            </section>
            {fehler && <p className="modal__error" role="alert">{fehler}</p>}
            <div className="modal__footer">
              <button type="button" className="button" onClick={onClose} disabled={sendet}>
                {t("hilfe.abbrechen")}
              </button>
              <button type="button" className="button button--primary" onClick={() => void senden()} disabled={!kannSenden}>
                {sendet ? t("hilfe.sendet") : t("hilfe.senden")}
              </button>
            </div>
          </div>
        )}

        {ergebnis && (
          <div className="hilfe__body" data-testid="hilfe-ergebnis">
            {ergebnis.ticket ? (
              <p>{t("hilfe.ok_text", { nummer: ergebnis.ticket.ticket_number ?? "" })}</p>
            ) : (
              <p className="modal__error" role="alert">
                {fehlerText(ergebnis.ticket_fehler ?? "unbekannt")}
              </p>
            )}
            {ergebnis.kennung ? (
              <div className="hilfe__kennung">
                <strong data-testid="hilfe-kennung">{t("hilfe.kennung", { kennung: ergebnis.kennung })}</strong>
                {!ergebnis.ticket && <p className="modal__hint">{t("hilfe.kennung_weitergeben")}</p>}
              </div>
            ) : (
              ergebnis.diagnose_fehler && (
                <p className="modal__hint">
                  {t(ergebnis.ticket ? "hilfe.ohne_diagnose" : "hilfe.diagnose_fehlt", {
                    grund: fehlerText(ergebnis.diagnose_fehler),
                  })}
                </p>
              )
            )}
            <div className="modal__footer">
              {ergebnis.ticket?.url && (
                <button type="button" className="button" onClick={() => void oeffnen(ergebnis.ticket!.url!)}>
                  {t("hilfe.ticket_oeffnen")}
                </button>
              )}
              <button type="button" className="button button--primary" onClick={onClose}>
                {t("hilfe.schliessen")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
