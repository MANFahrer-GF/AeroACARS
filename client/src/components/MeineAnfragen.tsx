// „Meine Anfragen“ im Hilfe-Reiter (30.10.2026, Thomas: nach einer neuen
// Anfrage war die alte weg, ohne Betreff und Status nicht zuzuordnen).
// Liste der eigenen HelpDesk-Tickets: Nummer, Betreff, Status, letzte
// Aktivität, neue Antwort; ein Klick öffnet das Ticket auf der Website.
// Geladen über `hilfe_tickets` (Kern, Schlüssel bleibt dort), neu beim
// Sichtbarwerden und nach jedem Senden (`neuLaden` ändert sich).
// Kurz gehalten (Thomas 30.10.: bei zehn Tickets musste man zum Formular
// scrollen): nur die ersten KURZ stehen da, was auf dich wartet zuerst,
// sonst jüngste Aktivität; der Rest hinter „Alle N Anfragen anzeigen“.
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";

export interface TicketEintrag {
  ticket_number: string;
  betreff: string;
  status: { label: string; offen: boolean; geschlossen: boolean; geloest: boolean; wartet_auf_pilot: boolean } | null;
  letzte_aktivitaet: string | null;
  neue_antwort: boolean;
  url: string | null;
}

function fehlerCode(e: unknown): string {
  if (e && typeof e === "object" && "code" in e && typeof (e as { code: unknown }).code === "string") {
    return (e as { code: string }).code;
  }
  return "unbekannt";
}

/** Farbton der Status-Pille: geschlossen/gelöst gedämpft, wartet auf dich hervorgehoben. */
function ton(s: TicketEintrag["status"]): string {
  if (!s) return "neutral";
  if (s.wartet_auf_pilot) return "achtung";
  if (s.geschlossen || s.geloest) return "neutral";
  return "offen";
}

/** So viele Anfragen stehen ohne Aufklappen da. */
export const KURZ = 3;

const wartet = (a: TicketEintrag) => a.neue_antwort || !!a.status?.wartet_auf_pilot;
const zeit = (a: TicketEintrag) => {
  const ms = a.letzte_aktivitaet ? Date.parse(a.letzte_aktivitaet) : NaN;
  return Number.isNaN(ms) ? 0 : ms;
};

/** Was auf den Piloten wartet zuerst, dann jüngste Aktivität zuerst. */
export function sortiert(liste: TicketEintrag[]): TicketEintrag[] {
  return [...liste].sort((x, y) => Number(wartet(y)) - Number(wartet(x)) || zeit(y) - zeit(x));
}

export function MeineAnfragen({
  sichtbar,
  neuLaden,
  oeffnen,
}: {
  sichtbar: boolean;
  /** Ändert sich nach jedem Senden → Liste neu holen. */
  neuLaden: unknown;
  oeffnen: (url: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [liste, setListe] = useState<TicketEintrag[] | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [laedt, setLaedt] = useState(false);
  const [zaehler, setZaehler] = useState(0);
  const [alle, setAlle] = useState(false);
  const reihe = useMemo(() => (liste ? sortiert(liste) : []), [liste]);
  const gezeigt = alle ? reihe : reihe.slice(0, KURZ);

  const laden = useCallback(() => setZaehler((z) => z + 1), []);

  useEffect(() => {
    if (!sichtbar) return;
    let weg = false;
    setLaedt(true);
    invoke<TicketEintrag[]>("hilfe_tickets")
      .then((l) => {
        if (weg) return;
        setListe(Array.isArray(l) ? l : []);
        setFehler(null);
      })
      .catch((e) => {
        if (!weg) setFehler(fehlerCode(e));
      })
      .finally(() => {
        if (!weg) setLaedt(false);
      });
    return () => {
      weg = true;
    };
  }, [sichtbar, neuLaden, zaehler]);

  const datum = (iso: string | null) => {
    if (!iso) return "";
    const d = new Date(iso);
    return Number.isNaN(d.getTime())
      ? ""
      : d.toLocaleString(i18n.language, { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
  };

  return (
    <div className="settings__section meine-anfragen">
      <div className="meine-anfragen__kopf">
        <h3>{t("hilfe.anfragen.titel")}</h3>
        <button type="button" className="button button--ghost" onClick={laden} disabled={laedt}>
          {laedt ? t("hilfe.anfragen.laedt") : t("hilfe.anfragen.aktualisieren")}
        </button>
      </div>
      {fehler && (
        <p className="settings__row-hint" role="status">
          {t(`hilfe.fehler.${fehler}`, { defaultValue: t("hilfe.fehler.unbekannt") })}
        </p>
      )}
      {!fehler && liste !== null && liste.length === 0 && <p className="settings__row-hint">{t("hilfe.anfragen.leer")}</p>}
      {liste !== null && liste.length > 0 && (
        <ul className="meine-anfragen__liste">
          {gezeigt.map((a) => (
            <li key={a.ticket_number} className={`meine-anfragen__eintrag${a.status?.geschlossen || a.status?.geloest ? " meine-anfragen__eintrag--zu" : ""}`}>
              <button
                type="button"
                className="meine-anfragen__knopf"
                disabled={!a.url}
                onClick={() => a.url && oeffnen(a.url)}
                title={a.url ? t("hilfe.ticket_oeffnen") : undefined}
              >
                <span className="meine-anfragen__nummer">{a.ticket_number}</span>
                <span className="meine-anfragen__betreff">{a.betreff || "–"}</span>
                <span className="meine-anfragen__rechts">
                  {a.neue_antwort && <span className="meine-anfragen__neu">{t("hilfe.anfragen.neue_antwort")}</span>}
                  <span className={`meine-anfragen__status meine-anfragen__status--${ton(a.status)}`}>
                    {a.status?.label || t("hilfe.anfragen.ohne_status")}
                  </span>
                  <span className="meine-anfragen__zeit">{datum(a.letzte_aktivitaet)}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {reihe.length > KURZ && (
        <button
          type="button"
          className="button button--ghost meine-anfragen__mehr"
          aria-expanded={alle}
          onClick={() => setAlle((x) => !x)}
        >
          {alle ? t("hilfe.anfragen.weniger") : t("hilfe.anfragen.alle", { anzahl: reihe.length })}
        </button>
      )}
    </div>
  );
}
