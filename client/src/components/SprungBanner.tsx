import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import type { ActiveFlightInfo, FlightEndOutcome } from "../types";
import { resolveFlightIdent } from "../lib/callsign";

interface Props {
  activeFlight: ActiveFlightInfo;
  /** Ein PIREP wurde eingereicht — Elternteil zeigt das Erfolgs-Banner
   *  und räumt den Flug weg (gleiche Zusage wie beim Divert-Banner). */
  onFiledSuccess: (outcome: FlightEndOutcome) => void;
  /** Der Flug wurde verworfen — Elternteil räumt ihn weg. */
  onDiscarded: () => void;
}

/**
 * Banner, wenn beim Wiederaufnehmen ein physikalisch unmöglicher Sprung
 * erkannt wurde: Der Simulator wurde neu geladen, der Flug ging so nicht
 * weiter.
 *
 * Anlass MSC1588 (22.09.2026, EDDN → HECA): Der Simulator verschwand
 * 49 NM vor Kairo. Nach dem Neuladen stand das Flugzeug am Zielflughafen,
 * die Phasen liefen im Stand bis „Arrived" durch, und die App gab den
 * PIREP **von selbst** ab — mit voller Strecke und ohne je gemessene
 * Landung. Seitdem gibt die App in diesem Fall nicht mehr selbst ab
 * (`unmoeglicher_sprung`), sondern fragt hier den Piloten:
 *
 *   1. Flug verwerfen (empfohlen)
 *   2. Trotzdem einreichen — landet dann in der Prüfliste der VA
 *
 * Bewusst KEIN dritter Weg: Die fehlenden Meilen lassen sich nicht
 * nachträglich fliegen, und eine „halbe" Abgabe gibt es in phpVMS nicht.
 *
 * Lücke mit Absicht: Steht gleichzeitig ein Divert an, hat dessen Banner
 * Vorrang und dieses hier bleibt weg. Der Divert-Weg reicht den PIREP dann
 * selbst ein — die Sprung-Notiz hängt trotzdem dran, weil sie in
 * `flight_end` aus `stats.resume_discontinuity` kommt und nicht aus diesem
 * Banner; nur die freiwillige Begründung kann der Pilot dort nicht
 * eintippen (Cloud-QS 23.09.2026).
 */
export function SprungBanner({ activeFlight, onFiledSuccess, onDiscarded }: Props) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Freiwillig — sie landet in den PIREP-Notizen, damit die VA beim
   *  Prüfen weiß, was passiert ist (Thomas, 23.09.2026). */
  const [begruendung, setBegruendung] = useState("");

  if (!activeFlight.unmoeglicher_sprung) return null;
  // Erst entscheiden lassen, wenn die Entscheidung ansteht — und nicht,
  // während der Resume-Hinweis noch offen ist.
  if (activeFlight.was_just_resumed) return null;
  if (activeFlight.phase !== "arrived") return null;
  // Ein Divert will zuerst beantwortet werden; zwei Banner mit
  // widersprüchlichen Tasten helfen niemandem.
  if (activeFlight.divert_hint) return null;

  const verwerfen = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("flight_cancel", { force: true });
      onDiscarded();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const trotzdemEinreichen = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("flight_end", { sprungBegruendung: begruendung.trim() || null });
      onFiledSuccess({
        kind: "filed",
        callsign: activeFlight.airline_icao
          ? `${activeFlight.airline_icao} ${resolveFlightIdent(activeFlight.flight_number, activeFlight.callsign)}`
          : resolveFlightIdent(activeFlight.flight_number, activeFlight.callsign),
        dpt: activeFlight.dpt_airport,
        arr: activeFlight.arr_airport,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="divert-banner" role="alert" aria-live="polite" data-testid="sprung-banner">
      <header className="divert-banner__header">
        <span className="divert-banner__icon" aria-hidden="true">
          ⚠
        </span>
        <h2 className="divert-banner__title">{t("sprung.title")}</h2>
      </header>
      <p className="divert-banner__body">{t("sprung.body")}</p>
      <label className="sprung-banner__feld">
        <span>{t("sprung.begruendung_label")}</span>
        <textarea
          className="sprung-banner__eingabe"
          rows={2}
          maxLength={500}
          value={begruendung}
          disabled={busy}
          placeholder={t("sprung.begruendung_platzhalter")}
          onChange={(e) => setBegruendung(e.target.value)}
        />
      </label>
      {error && <p className="divert-banner__error">{error}</p>}
      <div className="divert-banner__actions">
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => void verwerfen()}
        >
          {busy ? t("sprung.laeuft") : t("sprung.verwerfen")}
        </button>
        <button
          type="button"
          className="button button--ghost"
          disabled={busy}
          onClick={() => void trotzdemEinreichen()}
        >
          {busy ? t("sprung.laeuft") : t("sprung.trotzdem")}
        </button>
      </div>
    </section>
  );
}
