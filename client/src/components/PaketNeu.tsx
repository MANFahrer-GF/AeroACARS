// „Neu in Generation N“ nach dem Neustart (30.10.2026, Thomas: die Piloten
// sehen nicht, was in einer Generation neu ist). Der Text steht in den
// Paket-Texten (`paket.neu_text`, DE/EN/IT) und kommt mit der Generation —
// ohne neuen Client. Gezeigt wird er einmal: „Verstanden“ merkt sich den
// (deutschen) Text, nicht die Generation. Eine Generation ohne neuen Text zeigt darum nichts
// Altes erneut, und ein leerer Text zeigt gar nichts. Im Flug kein Band.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { PaketInfo } from "./PaketUebersicht";

export const GESEHEN_KEY = "aeroacars.paket.neu_gesehen";

function gesehenLesen(): string | null {
  try {
    return localStorage.getItem(GESEHEN_KEY);
  } catch {
    return null;
  }
}

export function PaketNeu({ imFlug }: { imFlug: boolean }) {
  const { t, i18n } = useTranslation();
  const text = t("paket.neu_text", { defaultValue: "" }).trim();
  // Gemerkt wird der deutsche Text: sprachunabhängig, sonst käme dieselbe
  // Neuigkeit nach einem Sprachwechsel noch einmal (Prüfer 30.10.).
  const kennung = i18n.getFixedT("de")("paket.neu_text", { defaultValue: "" }).trim() || text;
  const [gesehen, setGesehen] = useState(gesehenLesen);
  const [generation, setGeneration] = useState<number | null>(null);

  useEffect(() => {
    void invoke<PaketInfo>("paket_info")
      .then((i) => setGeneration(i.generation))
      .catch(() => undefined);
  }, []);

  if (imFlug || !text || kennung === gesehen) return null;

  const verstanden = () => {
    try {
      localStorage.setItem(GESEHEN_KEY, kennung);
    } catch {
      /* ohne Speicher: nur für diese Sitzung weg */
    }
    setGesehen(kennung);
  };

  return (
    <div className="update-banner" role="status" aria-live="polite">
      <div className="update-banner__icon" aria-hidden="true">
        ✦
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">
          {generation ? t("paket.neu_titel", { n: generation }) : t("paket.neu_titel_ohne")}
        </div>
        <div className="update-banner__subtitle">{text}</div>
      </div>
      <div className="update-banner__actions">
        <button type="button" className="button button--primary" onClick={verstanden}>
          {t("paket.neu_verstanden")}
        </button>
      </div>
    </div>
  );
}
