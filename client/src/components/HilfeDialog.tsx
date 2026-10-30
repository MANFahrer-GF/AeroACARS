// Hilfe anfordern (AeroACARS 3.0, P-9.20 bis P-9.22, N-06).
//
// Der Pilot schreibt Betreff und Text; der Client lädt zuerst die Diagnose
// auf live (Kennung `D-…`) und legt dann das Ticket im GSG-HelpDesk an
// (Kategorie AeroACARS). Was mit der Diagnose mitgeht, steht VOR dem Senden
// im Dialog (P-9.22). Scheitert das Ticket, bleibt die Kennung sichtbar, damit
// der Pilot sie weitergeben kann. Netz und Schlüssel bleiben in Rust
// (`hilfe_anfordern`), die Oberfläche bekommt nur das Ergebnis.
//
// Bilder (Screenshots): Datei wählen, Einfügen (Strg/⌘+V) oder Ziehen; sie
// werden hier verkleinert und neu gezeichnet (`lib/hilfeBilder.ts`, dort die
// Grenzen) und gehen als Anhang ans Ticket — nicht in die Diagnose. Über die
// LAN-Brücke (Tablet) nicht: deren Befehlsweg nimmt höchstens 2 MiB.
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri } from "../lib/ipc";
import {
  BILD_MAX_ANZAHL,
  BILD_TYPEN,
  auswahlPruefen,
  bildVorbereiten,
  dateienAus,
  type BildFehler,
  type HilfeBildVorbereitet,
} from "../lib/hilfeBilder";
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
  ticket: { id: unknown; ticket_number: string | null; url: string | null; anhaenge?: number | null } | null;
  ticket_fehler: string | null;
  /** Mitgeschickte Bilder (fehlt bei einem Programm ohne Bild-Unterstützung). */
  bilder_gesendet?: number;
}

const BETREFF_MAX = 200;
const TEXT_MAX = 20000;

/** F1 ohne Zusatztaste und ohne Tastenwiederholung öffnet „Hilfe anfordern“ (App.tsx). */
export function istHilfeTaste(e: Pick<KeyboardEvent, "key" | "repeat" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey">): boolean {
  return e.key === "F1" && !e.repeat && !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey;
}

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
  const [bilder, setBilder] = useState<HilfeBildVorbereitet[]>([]);
  const [bereitet, setBereitet] = useState(0);
  const [bildFehler, setBildFehler] = useState<BildFehler[]>([]);
  const [ziehen, setZiehen] = useState(false);
  const dateiWahl = useRef<HTMLInputElement>(null);
  // Zählt laufende Vorbereitungen mit, damit zwei schnelle Einfügungen
  // zusammen nicht über die Höchstzahl kommen.
  const belegt = useRef(0);

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

  const bilderHinzu = async (dateien: File[]) => {
    if (!isTauri || sendet || dateien.length === 0) return;
    const { angenommen, fehler: f } = auswahlPruefen(belegt.current, dateien);
    belegt.current += angenommen.length;
    setBereitet((n) => n + angenommen.length);
    const neueFehler = new Set<BildFehler>(f);
    const fertig = await Promise.all(
      angenommen.map(async (i) => {
        try {
          return await bildVorbereiten(dateien[i], dateien[i].name || "bild");
        } catch (e) {
          neueFehler.add(e === "gross" ? "gross" : "lesen");
          belegt.current -= 1;
          return null;
        }
      }),
    );
    setBereitet((n) => n - angenommen.length);
    setBilder((alt) => [...alt, ...fertig.filter((b): b is HilfeBildVorbereitet => b !== null)]);
    setBildFehler([...neueFehler]);
  };

  const bildWeg = (i: number) => {
    belegt.current -= 1;
    setBildFehler([]);
    setBilder((alt) => alt.filter((_, j) => j !== i));
  };

  const beimEinfuegen = (e: ClipboardEvent) => {
    const dateien = dateienAus(e.clipboardData);
    if (dateien.length === 0) return; // Text normal einfügen
    e.preventDefault();
    void bilderHinzu(dateien);
  };

  const beimAblegen = (e: DragEvent) => {
    e.preventDefault();
    setZiehen(false);
    void bilderHinzu(dateienAus(e.dataTransfer));
  };

  const senden = async () => {
    setFehler(null);
    setSendet(true);
    try {
      // Ohne Bilder genau die bisherigen Argumente (älteres Programm).
      const args = bilder.length > 0 ? { betreff, text, bilder: bilder.map((b) => ({ daten: b.daten })) } : { betreff, text };
      setErgebnis(await invoke<HilfeErgebnis>("hilfe_anfordern", args));
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
    !sendet && bereitet === 0 && betreff.trim().length > 0 && text.trim().length > 0 && betreff.length <= BETREFF_MAX && text.length <= TEXT_MAX;

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="hilfe-titel" onClick={onClose}>
      <div
        className="modal modal--wide hilfe"
        onClick={(e) => e.stopPropagation()}
        onPaste={ergebnis ? undefined : beimEinfuegen}
      >
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
            <section
              className={`hilfe__bilder${ziehen ? " hilfe__bilder--ziehen" : ""}`}
              data-testid="hilfe-bilder"
              aria-labelledby="hilfe-bilder-titel"
              onDragOver={(e) => {
                if (!isTauri) return;
                e.preventDefault();
                setZiehen(true);
              }}
              onDragLeave={() => setZiehen(false)}
              onDrop={isTauri ? beimAblegen : undefined}
            >
              <div className="hilfe__bilder-kopf">
                <h3 id="hilfe-bilder-titel">{t("hilfe.bilder.titel")}</h3>
                {isTauri && (
                  <span className="hilfe__bilder-zahl">
                    {t("hilfe.bilder.zahl", { anzahl: bilder.length, max: BILD_MAX_ANZAHL })}
                  </span>
                )}
              </div>
              {isTauri ? (
                <>
                  <p className="modal__hint">{t("hilfe.bilder.anleitung")}</p>
                  {bilder.length > 0 && (
                    <ul className="hilfe__bildliste">
                      {bilder.map((b, i) => (
                        <li key={i} className="hilfe__bild">
                          <img src={b.vorschau} alt={t("hilfe.bilder.vorschau_alt", { nr: i + 1 })} />
                          <button
                            type="button"
                            className="hilfe__bild-weg"
                            onClick={() => bildWeg(i)}
                            disabled={sendet}
                            aria-label={t("hilfe.bilder.entfernen", { nr: i + 1 })}
                            title={t("hilfe.bilder.entfernen", { nr: i + 1 })}
                          >
                            ×
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                  {bereitet > 0 && <p className="modal__hint">{t("hilfe.bilder.bereitet")}</p>}
                  {bildFehler.map((f) => (
                    <p key={f} className="modal__error" role="alert">
                      {t(`hilfe.bilder.fehler.${f}`, { max: BILD_MAX_ANZAHL })}
                    </p>
                  ))}
                  <div className="hilfe__bilder-aktion">
                    <button
                      type="button"
                      className="button"
                      onClick={() => dateiWahl.current?.click()}
                      disabled={sendet || bilder.length + bereitet >= BILD_MAX_ANZAHL}
                    >
                      {t("hilfe.bilder.waehlen")}
                    </button>
                    <input
                      ref={dateiWahl}
                      type="file"
                      accept={BILD_TYPEN.join(",")}
                      multiple
                      hidden
                      data-testid="hilfe-bilder-datei"
                      onChange={(e) => {
                        const dateien = e.target.files ? Array.from(e.target.files) : [];
                        e.target.value = "";
                        void bilderHinzu(dateien);
                      }}
                    />
                  </div>
                  <p className="hilfe__bilder-datenschutz">{t("hilfe.bilder.datenschutz")}</p>
                </>
              ) : (
                <p className="modal__hint">{t("hilfe.bilder.nur_am_pc")}</p>
              )}
            </section>
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
                <dt>{t("hilfe.feld.bilder")}</dt>
                <dd>
                  {bilder.length > 0 ? t("hilfe.bilder.mitgeht", { count: bilder.length }) : t("hilfe.bilder.keine")}
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
            {(ergebnis.bilder_gesendet ?? 0) > 0 && (
              <p
                className={ergebnis.ticket && ergebnis.ticket.anhaenge === ergebnis.bilder_gesendet ? "modal__hint" : "modal__error"}
                data-testid="hilfe-bilder-ergebnis"
              >
                {!ergebnis.ticket
                  ? t("hilfe.bilder.nicht_angekommen")
                  : ergebnis.ticket.anhaenge === ergebnis.bilder_gesendet
                    ? t("hilfe.bilder.angekommen", { count: ergebnis.bilder_gesendet })
                    : t("hilfe.bilder.teilweise", {
                        angekommen: ergebnis.ticket.anhaenge ?? 0,
                        gesendet: ergebnis.bilder_gesendet,
                      })}
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
              {!ergebnis.ticket && (
                // Betreff, Text und Bilder stehen noch im Formular.
                <button type="button" className="button" onClick={() => setErgebnis(null)}>
                  {t("hilfe.zurueck")}
                </button>
              )}
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
