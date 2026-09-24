import { useEffect, useRef, useState } from "react";
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
 *   2. Trotzdem einreichen — mit Vermerk und optionaler Begründung in der
 *      PIREP-Notiz.
 *
 * ⚠ „Trotzdem einreichen" schickt den Flug NICHT automatisch in die
 * Prüfung. Ob er angenommen wird, entscheidet wie bei jedem Flug das
 * Gate-Modul nach dem Urteil des Recorders — der kennt den Vermerk nicht.
 * Hier stand bis 24.09.2026 das Gegenteil, ungeprüft; OCN 712 (Joel) wurde
 * trotz dreier Sprung-Vermerke angenommen, weil seine Landung gemessen war.
 * Thomas hat entschieden, dass es dabei bleibt (keine zusätzliche Abnahme).
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
  /** Verwerfen ist unwiderruflich und kostet einen ganzen Flug — überall
   *  sonst in der App fragt ein Abbruch nach (Cloud-QS 23.09.2026). */
  const [sicher, setSicher] = useState(false);
  /** Nach dem ersten Klick tauscht React die Taste aus — der Fokus fiel
   *  dabei auf das Dokument zurück, und wer mit der Tastatur bedient,
   *  landete vor einer unwiderruflichen Aktion am Seitenanfang
   *  (Cloud-QS 23.09.2026). Deshalb gezielt auf „Doch nicht": Der sichere
   *  Weg ist vorbelegt, nicht der zerstörerische. */
  const dochNichtRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (sicher) dochNichtRef.current?.focus();
  }, [sicher]);

  // Zwei Gründe, dieselbe Frage. `unmoeglicher_sprung` bleibt als
  // Rückfallebene für einen älteren Client an der LAN-Brücke.
  //
  // Nur die BEKANNTEN Codes zählen: Ein neueres Backend (oder die
  // LAN-Brücke, die schicken kann was sie will) darf hier nicht dazu
  // führen, dass „der Zustand sprang" behauptet wird, obwohl etwas
  // anderes gemeint war (Cloud-QS 23.09.2026).
  const gemeldet = activeFlight.abgabe_sperre;
  const grund =
    gemeldet === "sprung" || gemeldet === "landung_fehlt"
      ? gemeldet
      : gemeldet
        ? "unbekannt"
        : activeFlight.unmoeglicher_sprung
          ? "sprung"
          : null;
  if (!grund) return null;
  // Ein unbekannter Grund bekommt eigene, neutrale Texte — der Sprung-Text
  // wäre hier eine erfundene Ursache (Cloud-QS 23.09.2026, dritte Runde).
  const titel =
    grund === "landung_fehlt"
      ? t("sprung.landung_title")
      : grund === "unbekannt"
        ? t("sprung.unbekannt_title")
        : t("sprung.title");
  const text =
    grund === "landung_fehlt"
      ? t("sprung.landung_body")
      : grund === "unbekannt"
        ? t("sprung.unbekannt_body")
        : t("sprung.body");
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
      // `flight_cancel` nimmt den Flug aus dem Zustand, BEVOR es phpVMS
      // fragt. Scheitert das, ist der Flug hier trotzdem weg — ein zweiter
      // Klick antwortet dann „no_active_flight". Das ist kein Fehler, das
      // ist der Erfolg von eben (Cloud-QS 23.09.2026).
      if (String(e).includes("no_active_flight")) {
        onDiscarded();
      } else {
        setError(String(e));
      }
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
    <section className="divert-banner" role="alert" data-testid="sprung-banner">
      <header className="divert-banner__header">
        <span className="divert-banner__icon" aria-hidden="true">
          ⚠
        </span>
        <h2 className="divert-banner__title">
          {titel}
        </h2>
      </header>
      <p className="divert-banner__body">
        {text}
      </p>
      <label className="sprung-banner__feld">
        <span>{t("sprung.begruendung_label")}</span>
        <textarea
          className="sprung-banner__eingabe"
          rows={2}
          maxLength={500}
          value={begruendung}
          disabled={busy}
          placeholder={
            grund === "landung_fehlt"
              ? t("sprung.landung_platzhalter")
              : grund === "unbekannt"
                ? t("sprung.unbekannt_platzhalter")
                : t("sprung.begruendung_platzhalter")
          }
          onChange={(e) => setBegruendung(e.target.value)}
        />
      </label>
      {error && <p className="divert-banner__error">{error}</p>}
      <div className="divert-banner__actions">
        {sicher ? (
          <>
            <button
              type="button"
              className="button button--primary"
              disabled={busy}
              onClick={() => void verwerfen()}
            >
              {busy ? t("sprung.laeuft") : t("sprung.verwerfen_sicher")}
            </button>
            <button
              ref={dochNichtRef}
              type="button"
              className="button button--ghost"
              disabled={busy}
              onClick={() => setSicher(false)}
            >
              {t("sprung.doch_nicht")}
            </button>
          </>
        ) : (
          <button
            type="button"
            className="button button--primary"
            disabled={busy}
            onClick={() => setSicher(true)}
          >
            {t("sprung.verwerfen")}
          </button>
        )}
        {!sicher && (
          <button
            type="button"
            className="button button--ghost"
            disabled={busy}
            onClick={() => void trotzdemEinreichen()}
          >
            {busy ? t("sprung.laeuft") : t("sprung.trotzdem")}
          </button>
        )}
      </div>
    </section>
  );
}
