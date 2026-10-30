// Meldung „neue Paket-Generation bereit“ im Hauptfenster (30.10.2026,
// Thomas: man sah ein neues Paket nur, wenn man selbst in Über/Hilfe
// nachschaute). Fragt alle 30 Minuten bei live nach (`paket_jetzt_pruefen`,
// der Kern holt nichts bei offenem Flug, P-3.20) und zeigt ein Band mit
// „Jetzt neu starten“, sobald ein neuer Stand wartet (`paket_info`,
// `naechster_start`). Im Flug kein Band — der Neustart ginge ohnehin nicht.
// „Später“ blendet es für genau diese Generation aus.
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { PaketInfo } from "./PaketUebersicht";

/** Abstand der Nachfrage bei live. */
export const PRUEF_ABSTAND_MS = 30 * 60 * 1000;
/** Erste Nachfrage kurz nach dem Start (der Startabruf lief schon). */
export const ERSTE_PRUEFUNG_MS = 2 * 60 * 1000;

export function PaketHinweis({ imFlug }: { imFlug: boolean }) {
  const { t } = useTranslation();
  const [wartet, setWartet] = useState<number | null>(null);
  const [spaeter, setSpaeter] = useState<number | null>(null);
  const [startet, setStartet] = useState(false);
  const [fehler, setFehler] = useState(false);

  const lesen = useCallback(async () => {
    try {
      const i = await invoke<PaketInfo>("paket_info");
      setWartet(i.naechster_start && !i.flug_offen ? i.naechster_start.generation : null);
    } catch {
      /* älterer Kern oder LAN-Tablet: kein Band */
    }
  }, []);

  useEffect(() => {
    let weg = false;
    const pruefen = async () => {
      if (weg || imFlug) return;
      try {
        await invoke("paket_jetzt_pruefen");
      } catch {
        /* ohne Netz: beim nächsten Mal */
      }
      if (!weg) await lesen();
    };
    void lesen();
    const erste = window.setTimeout(() => void pruefen(), ERSTE_PRUEFUNG_MS);
    const takt = window.setInterval(() => void pruefen(), PRUEF_ABSTAND_MS);
    return () => {
      weg = true;
      window.clearTimeout(erste);
      window.clearInterval(takt);
    };
  }, [imFlug, lesen]);

  if (imFlug || wartet === null || wartet === spaeter) return null;

  const neuStarten = async () => {
    setStartet(true);
    setFehler(false);
    try {
      await invoke("paket_neu_starten");
    } catch {
      setFehler(true);
      setStartet(false);
    }
  };

  return (
    <div className="update-banner" role="status" aria-live="polite">
      <div className="update-banner__icon" aria-hidden="true">
        ⟳
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">{t("paket.hinweis_titel", { n: wartet })}</div>
        <div className="update-banner__subtitle">{fehler ? t("paket.neustart_fehler") : t("paket.hinweis_text")}</div>
      </div>
      <div className="update-banner__actions">
        <button type="button" className="button button--primary" onClick={() => void neuStarten()} disabled={startet}>
          {startet ? "…" : t("paket.neu_starten")}
        </button>
        <button type="button" className="button button--ghost" onClick={() => setSpaeter(wartet)} disabled={startet}>
          {t("paket.hinweis_spaeter")}
        </button>
      </div>
    </div>
  );
}
