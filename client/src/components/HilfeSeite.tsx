// Hilfe anfordern (AeroACARS 3.0, P-9.20 bis P-9.22, N-06) — eigene Seite
// in der Seitenleiste (Beta-1-Rückmeldung: „zu versteckt“, „kein Bild“).
//
// Der Pilot schreibt Betreff und Text; der Client lädt zuerst die Diagnose
// auf live (Kennung `D-…`) und legt dann das Ticket im GSG-HelpDesk an
// (Kategorie AeroACARS). Was mit der Diagnose mitgeht, steht VOR dem Senden
// auf der Seite (P-9.22). Scheitert das Ticket, bleibt die Kennung sichtbar,
// damit der Pilot sie weitergeben kann. Netz und Schlüssel bleiben in Rust
// (`hilfe_anfordern`), die Oberfläche bekommt nur das Ergebnis.
//
// Bilder (Screenshots): Datei wählen, Einfügen (Strg/⌘+V irgendwo auf der
// Seite) oder Ziehen; sie werden hier verkleinert und neu gezeichnet
// (`lib/hilfeBilder.ts`, dort die Grenzen) und gehen als Anhang ans Ticket —
// nicht in die Diagnose. Über die LAN-Brücke (Tablet) nicht: deren
// Befehlsweg nimmt höchstens 2 MiB.
//
// Zusatzfelder (Flugnummer, Simulator …) beschreibt das Laufzeit-Paket in
// `tabellen/hilfe-formular.json` (Fähigkeit `hilfe-formular-1`); die Seite
// zeigt sie ohne Codeänderung und schickt sie als `zusatz` mit. Der Kern
// nimmt nur die dort genannten Namen an und reicht sie als
// `custom_fields[<name>]` ans Ticket. Fehlt die Datei, sieht die Seite aus
// wie vorher.
import { useEffect, useRef, useState, type DragEvent } from "react";
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
import { Notice } from "./ui";
import "./hilfeSeite.css";

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
/** Grenzen wie im Kern (`hilfe.rs`: ZUSATZ_MAX_ANZAHL, ZUSATZ_WERT_MAX). */
const ZUSATZ_MAX_ANZAHL = 12;
const ZUSATZ_WERT_MAX = 2000;
const ZUSATZ_NAME = /^[a-z][a-z0-9_]{0,31}$/;

type Sprachtext = Partial<Record<string, string>>;

export interface ZusatzFeld {
  name: string;
  /** `text`, `mehrzeilig`, `auswahl`; Unbekanntes wird als Textfeld gezeigt. */
  typ: string;
  pflicht: boolean;
  max: number;
  bezeichnung: Sprachtext;
  hinweis: Sprachtext;
  optionen: { wert: string; bezeichnung: Sprachtext }[];
}

function sprachtext(v: unknown): Sprachtext {
  if (!v || typeof v !== "object") return {};
  return Object.fromEntries(Object.entries(v).filter(([, x]) => typeof x === "string")) as Sprachtext;
}

/**
 * Liest `tabellen/hilfe-formular.json`. Nachsichtig: Ein Feld, das der Kern
 * nicht annähme (Name, Auswahl ohne Optionen), fällt weg statt die Seite zu
 * stören — der Selbsttest des Pakets hat es ohnehin schon abgewiesen.
 */
export function formularLesen(json: unknown): ZusatzFeld[] {
  if (!json || typeof json !== "object") return [];
  const d = json as { format?: unknown; felder?: unknown };
  if (d.format !== 1 || !Array.isArray(d.felder)) return [];
  const namen = new Set<string>();
  const felder: ZusatzFeld[] = [];
  for (const roh of d.felder) {
    if (!roh || typeof roh !== "object") continue;
    const f = roh as Record<string, unknown>;
    const name = typeof f.name === "string" ? f.name : "";
    if (!ZUSATZ_NAME.test(name) || namen.has(name)) continue;
    const optionen = Array.isArray(f.optionen)
      ? f.optionen
          .filter((o): o is Record<string, unknown> => !!o && typeof o === "object" && typeof (o as { wert?: unknown }).wert === "string")
          .map((o) => ({ wert: o.wert as string, bezeichnung: sprachtext(o.bezeichnung) }))
      : [];
    const typ = typeof f.typ === "string" ? f.typ : "text";
    if (typ === "auswahl" && optionen.length === 0) continue;
    const max = typeof f.max === "number" && f.max > 0 ? Math.min(Math.floor(f.max), ZUSATZ_WERT_MAX) : ZUSATZ_WERT_MAX;
    namen.add(name);
    felder.push({ name, typ, pflicht: f.pflicht === true, max, bezeichnung: sprachtext(f.bezeichnung), hinweis: sprachtext(f.hinweis), optionen });
    if (felder.length === ZUSATZ_MAX_ANZAHL) break;
  }
  return felder;
}

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

export async function extern(url: string) {
  try {
    await invoke("plugin:opener|open_url", { url });
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

/**
 * `sichtbar`: App hängt die Seite nach dem ersten Besuch nicht mehr aus
 * (Entwurf, Bilder und ein laufendes Senden überleben den Reiterwechsel —
 * sonst ginge die Ticketnummer verloren und der Pilot sendete doppelt);
 * ausgeblendet hört sie nicht auf Einfügen.
 */
export function HilfeSeite({ sichtbar = true }: { sichtbar?: boolean } = {}) {
  const { t, i18n } = useTranslation();
  const titel = useRef<HTMLHeadingElement>(null);
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
  const [zusatzFelder, setZusatzFelder] = useState<ZusatzFeld[]>([]);
  const [zusatz, setZusatz] = useState<Record<string, string>>({});
  // Zählt laufende Vorbereitungen mit, damit zwei schnelle Einfügungen
  // zusammen nicht über die Höchstzahl kommen.
  const belegt = useRef(0);

  // Bei jedem Sichtbarwerden neu: Die Seite bleibt eingehängt, aber was
  // mitgeht (letzte Flugdatei, Log, Profil) ändert sich während des Flugs —
  // angezeigt wird, was Rust beim Senden jetzt einsammeln würde (P-9.22).
  useEffect(() => {
    if (!sichtbar) return;
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
  }, [sichtbar]);

  // Zusatzfelder aus dem Paket (der Stand gilt den ganzen Lauf). Ohne Datei
  // (älteres Paket/Programm) bleibt die Seite wie vorher.
  useEffect(() => {
    let weg = false;
    void (async () => {
      try {
        const roh = await invoke<string>("paket_datei", { pfad: "tabellen/hilfe-formular.json" });
        const felder = formularLesen(JSON.parse(roh));
        if (!weg) setZusatzFelder(felder);
      } catch {
        /* keine Zusatzfelder */
      }
    })();
    return () => {
      weg = true;
    };
  }, []);

  const sprache = (i18n.language || "de").slice(0, 2);
  const inSprache = (v: Sprachtext, ersatz: string) => v[sprache] ?? v.de ?? v.en ?? ersatz;
  const zusatzWerte = () =>
    Object.fromEntries(
      zusatzFelder.map((f) => [f.name, (zusatz[f.name] ?? "").trim()] as const).filter(([, w]) => w !== ""),
    );

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

  // Einfügen wirkt auf der ganzen Seite (auch ohne Fokus in einem Feld),
  // solange das Formular steht. Reiner Text bleibt normales Einfügen.
  const hinzuRef = useRef(bilderHinzu);
  hinzuRef.current = bilderHinzu;
  const formularOffen = ergebnis === null;
  useEffect(() => {
    if (!formularOffen || !sichtbar) return;
    const beimEinfuegen = (e: ClipboardEvent) => {
      const dateien = dateienAus(e.clipboardData);
      if (dateien.length === 0 || !isTauri) return;
      // Text aus Word/Browser bringt oft eine Bildvorschau mit: in einem
      // Textfeld gewinnt der Text.
      const ziel = e.target as HTMLElement | null;
      if (ziel?.closest?.("input, textarea") && (e.clipboardData?.getData("text/plain") ?? "") !== "") return;
      e.preventDefault();
      void hinzuRef.current(dateien);
    };
    document.addEventListener("paste", beimEinfuegen);
    return () => document.removeEventListener("paste", beimEinfuegen);
  }, [formularOffen, sichtbar]);

  // Nach dem Senden verschwindet der fokussierte Knopf: Fokus auf die
  // Überschrift des Ergebnisses, damit Tastatur und Screenreader folgen.
  useEffect(() => {
    if (ergebnis) titel.current?.focus();
  }, [ergebnis]);

  const bildWeg = (i: number) => {
    belegt.current -= 1;
    setBildFehler([]);
    setBilder((alt) => alt.filter((_, j) => j !== i));
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
      // Ohne Bilder und Zusatzfelder genau die bisherigen Argumente (älteres Programm).
      const werte = zusatzWerte();
      const args = {
        betreff,
        text,
        ...(bilder.length > 0 ? { bilder: bilder.map((b) => ({ daten: b.daten })) } : {}),
        ...(Object.keys(werte).length > 0 ? { zusatz: werte } : {}),
      };
      setErgebnis(await invoke<HilfeErgebnis>("hilfe_anfordern", args));
    } catch (e) {
      setFehler(fehlerText(fehlerCode(e)));
    } finally {
      setSendet(false);
    }
  };

  const neueAnfrage = () => {
    setErgebnis(null);
    setBetreff("");
    setText("");
    setZusatz({});
    setBilder([]);
    setBildFehler([]);
    belegt.current = 0;
  };

  const m = vorschau?.meta;
  const wert = (v: string | null | undefined) => (v && v.length > 0 ? v : t("hilfe.unbekannt"));
  const kannSenden =
    !sendet &&
    bereitet === 0 &&
    betreff.trim().length > 0 &&
    text.trim().length > 0 &&
    betreff.length <= BETREFF_MAX &&
    text.length <= TEXT_MAX &&
    zusatzFelder.every((f) => !f.pflicht || (zusatz[f.name] ?? "").trim().length > 0);
  const voll = bilder.length + bereitet >= BILD_MAX_ANZAHL;

  return (
    <section className="settings hilfe-seite" aria-labelledby="hilfe-titel">
      <header className="settings__header">
        <h2 id="hilfe-titel" ref={titel} tabIndex={-1} className="hilfe-seite__titel">
          {ergebnis ? (ergebnis.ticket ? t("hilfe.ok_titel") : t("hilfe.fehler_titel")) : t("hilfe.titel")}
        </h2>
        {!ergebnis && <p className="settings__hint">{t("hilfe.einleitung")}</p>}
      </header>

      {!ergebnis && (
        <>
          <div className="settings__section">
            <h3>{t("hilfe.anfrage_titel")}</h3>
            <div className="hilfe-seite__kategorie">
              <span className="settings__field-label">{t("hilfe.kategorie")}</span>
              <span className="hilfe-seite__marke">AeroACARS</span>
            </div>
            <label className="settings__field hilfe-seite__feld">
              <span className="settings__field-label">{t("hilfe.betreff")}</span>
              <input
                type="text"
                value={betreff}
                maxLength={BETREFF_MAX}
                onChange={(e) => setBetreff(e.target.value.replace(/[\r\n]/g, " "))}
                disabled={sendet}
              />
            </label>
            <label className="settings__field hilfe-seite__feld">
              <span className="settings__field-label">{t("hilfe.text")}</span>
              <textarea
                className="hilfe-seite__text"
                value={text}
                maxLength={TEXT_MAX}
                rows={6}
                placeholder={t("hilfe.text_platzhalter")}
                onChange={(e) => setText(e.target.value)}
                disabled={sendet}
              />
            </label>
            {zusatzFelder.map((f) => {
              const wert = zusatz[f.name] ?? "";
              const setzen = (v: string) => setZusatz((alt) => ({ ...alt, [f.name]: v }));
              const hinweis = inSprache(f.hinweis, "") || undefined;
              return (
                <label key={f.name} className="settings__field hilfe-seite__feld" data-testid={`hilfe-zusatz-${f.name}`}>
                  <span className="settings__field-label">
                    {inSprache(f.bezeichnung, f.name)}
                    {f.pflicht && <span className="hilfe-seite__pflicht"> ({t("hilfe.pflicht")})</span>}
                  </span>
                  {f.typ === "auswahl" ? (
                    <select value={wert} onChange={(e) => setzen(e.target.value)} disabled={sendet} required={f.pflicht}>
                      <option value="">{t("hilfe.zusatz_waehlen")}</option>
                      {f.optionen.map((o) => (
                        <option key={o.wert} value={o.wert}>
                          {inSprache(o.bezeichnung, o.wert)}
                        </option>
                      ))}
                    </select>
                  ) : f.typ === "mehrzeilig" ? (
                    <textarea
                      value={wert}
                      maxLength={f.max}
                      rows={3}
                      placeholder={hinweis}
                      onChange={(e) => setzen(e.target.value)}
                      disabled={sendet}
                      required={f.pflicht}
                    />
                  ) : (
                    <input
                      type="text"
                      value={wert}
                      maxLength={f.max}
                      placeholder={hinweis}
                      onChange={(e) => setzen(e.target.value.replace(/[\r\n]/g, " "))}
                      disabled={sendet}
                      required={f.pflicht}
                    />
                  )}
                </label>
              );
            })}
          </div>

          <div
            className="settings__section"
            data-testid="hilfe-bilder"
            onDragOver={(e) => {
              if (!isTauri || !e.dataTransfer?.types?.includes("Files")) return;
              e.preventDefault();
              setZiehen(true);
            }}
            onDragLeave={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setZiehen(false);
            }}
            onDrop={isTauri ? beimAblegen : undefined}
          >
            <div className="hilfe-seite__kopf">
              <h3 id="hilfe-bilder-titel">{t("hilfe.bilder.titel")}</h3>
              {isTauri && (
                <span className="hilfe-seite__zahl">{t("hilfe.bilder.zahl", { anzahl: bilder.length, max: BILD_MAX_ANZAHL })}</span>
              )}
            </div>
            {isTauri ? (
              <>
                <div className={`hilfe-seite__ablage${ziehen ? " hilfe-seite__ablage--ziehen" : ""}`}>
                  {bilder.length > 0 && (
                    <ul className="hilfe-seite__kacheln" aria-labelledby="hilfe-bilder-titel">
                      {bilder.map((b, i) => (
                        <li key={i} className="hilfe-seite__kachel">
                          <img src={b.vorschau} alt={t("hilfe.bilder.vorschau_alt", { nr: i + 1 })} />
                          <button
                            type="button"
                            className="hilfe-seite__weg"
                            onClick={() => bildWeg(i)}
                            disabled={sendet}
                            aria-label={t("hilfe.bilder.entfernen", { nr: i + 1 })}
                            title={t("hilfe.bilder.entfernen", { nr: i + 1 })}
                          >
                            <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                              <path d="M6 6l12 12M18 6 6 18" />
                            </svg>
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="hilfe-seite__ablage-text">
                    <p>{t("hilfe.bilder.anleitung")}</p>
                    <button
                      type="button"
                      className="button"
                      onClick={() => dateiWahl.current?.click()}
                      disabled={sendet || voll}
                    >
                      {t("hilfe.bilder.waehlen")}
                    </button>
                  </div>
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
                {bereitet > 0 && <p className="settings__row-hint">{t("hilfe.bilder.bereitet")}</p>}
                {bildFehler.map((f) => (
                  <p key={f} className="hilfe-seite__fehler" role="alert">
                    {t(`hilfe.bilder.fehler.${f}`, { max: BILD_MAX_ANZAHL })}
                  </p>
                ))}
                <p className="settings__row-hint">{t("hilfe.bilder.datenschutz")}</p>
              </>
            ) : (
              <p className="settings__row-hint">{t("hilfe.bilder.nur_am_pc")}</p>
            )}
          </div>

          <div className="settings__section">
            <details className="hilfe-seite__mitgeht" data-testid="hilfe-mitgeht">
              <summary>
                <span>{t("hilfe.mitgeht_titel")}</span>
                <span className="hilfe-seite__mitgeht-kurz">
                  {wert(m?.client_version)} · {wert(m?.sim)}
                </span>
              </summary>
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
                <dd>{bilder.length > 0 ? t("hilfe.bilder.mitgeht", { count: bilder.length }) : t("hilfe.bilder.keine")}</dd>
              </dl>
              <p className="settings__row-hint">{t("hilfe.mitgeht_hinweis")}</p>
            </details>
          </div>

          <div className="hilfe-seite__aktionen">
            {fehler && (
              <p className="hilfe-seite__fehler" role="alert">
                {fehler}
              </p>
            )}
            <button type="button" className="button button--primary" onClick={() => void senden()} disabled={!kannSenden}>
              {sendet ? t("hilfe.sendet") : t("hilfe.senden")}
            </button>
          </div>
        </>
      )}

      {ergebnis && (
        <div className="hilfe-seite__ergebnis" data-testid="hilfe-ergebnis">
          {ergebnis.ticket ? (
            <Notice
              tone="success"
              title={t("hilfe.ok_text", { nummer: ergebnis.ticket.ticket_number ?? "" })}
            />
          ) : (
            <Notice tone="error" role="alert" title={fehlerText(ergebnis.ticket_fehler ?? "unbekannt")} />
          )}
          {(ergebnis.bilder_gesendet ?? 0) > 0 && (
            <p
              className={ergebnis.ticket && ergebnis.ticket.anhaenge === ergebnis.bilder_gesendet ? "settings__row-hint" : "hilfe-seite__fehler"}
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
            <div className="hilfe-seite__kennung">
              <strong data-testid="hilfe-kennung">{t("hilfe.kennung", { kennung: ergebnis.kennung })}</strong>
              {!ergebnis.ticket && <p className="settings__row-hint">{t("hilfe.kennung_weitergeben")}</p>}
            </div>
          ) : (
            ergebnis.diagnose_fehler && (
              <p className="settings__row-hint">
                {t(ergebnis.ticket ? "hilfe.ohne_diagnose" : "hilfe.diagnose_fehlt", {
                  grund: fehlerText(ergebnis.diagnose_fehler),
                })}
              </p>
            )
          )}
          <div className="hilfe-seite__aktionen">
            {!ergebnis.ticket && (
              // Betreff, Text und Bilder stehen noch im Formular.
              <button
                type="button"
                className="button"
                onClick={() => {
                  setBildFehler([]);
                  setErgebnis(null);
                }}
              >
                {t("hilfe.zurueck")}
              </button>
            )}
            {ergebnis.ticket && (
              <button type="button" className="button" onClick={neueAnfrage}>
                {t("hilfe.neu")}
              </button>
            )}
            {ergebnis.ticket?.url && (
              <button type="button" className="button button--primary" onClick={() => void extern(ergebnis.ticket!.url!)}>
                {t("hilfe.ticket_oeffnen")}
              </button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
