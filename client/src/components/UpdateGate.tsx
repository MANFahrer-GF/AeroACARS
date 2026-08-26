import { useTranslation } from "react-i18next";
import type { FlightPhase } from "../types";
import type { UseUpdateCheckerResult } from "../hooks/useUpdateChecker";

/**
 * Pflicht-Riegel: erst aktualisieren, dann fliegen.
 *
 * # Warum
 *
 * Die bisherige Eskalation (Knopf → Pulsieren → Banner) ist höflich und
 * lässt sich vollständig ignorieren. Genau das passierte: Piloten flogen
 * wochenlang auf alten Ständen, und Befunde aus dem Betrieb kamen von
 * Versionen, deren Fehler längst behoben waren. Bei einem Werkzeug, das
 * Flüge bewertet und einreicht, ist ein alter Stand nicht nur unbequem —
 * er erzeugt Daten, die niemand mehr zuordnen kann.
 *
 * Deshalb ab v1.7.6: Liegt beim Start eine neuere Version vor, geht es
 * erst nach dem Update weiter.
 *
 * # Wann er NICHT sperrt — die vier Ausnahmen
 *
 * Ein Riegel, der im falschen Moment zufällt, ist schlimmer als gar
 * keiner. Jede dieser Ausnahmen hat einen Grund, den ein Pilot sonst
 * ausbaden müsste:
 *
 *   1. **Kein Netz, Server weg, Prüfung gescheitert.** Dann gibt es kein
 *      `update`-Objekt, und der Riegel bleibt aus. Ein Ausfall bei
 *      GitHub darf niemanden am Fliegen hindern.
 *   2. **Update erst mitten in der Sitzung gefunden.** `pflichtUpdate`
 *      ist nur wahr, wenn die Version schon beim Start vorlag — siehe
 *      `STARTFENSTER_MS` im Hook. Der Vier-Stunden-Turnus sperrt nie.
 *   3. **Ein Flug läuft.** In jeder aktiven Phase bleibt der Riegel aus.
 *      Er greift beim Start, und beim Start liegt höchstens ein
 *      wiederaufnehmbarer Flug vor — den darf er nicht kosten.
 *   4. **Die Installation ist gescheitert.** Dann erscheint ein Ausweg.
 *      Ohne ihn wäre der Client für jemanden, dessen Updater nicht
 *      durchkommt (Rechteproblem, Virenscanner, gesperrtes Netz),
 *      dauerhaft unbenutzbar — und zwar ohne Weg zurück.
 *
 * # Kein „Später"
 *
 * Bewusst nicht. Ein Aufschub, der sich anklicken lässt, ist wieder das
 * Banner — und das gibt es schon. Der Ausweg entsteht ausschließlich aus
 * einem echten Fehlschlag, nicht aus Unlust.
 */

/**
 * Phasen, in denen ein Flug läuft.
 *
 * ⚠ Bewusst dieselbe Liste wie in `UpdateBanner.tsx`. Sie hier
 * herauszuziehen wäre sauberer, würde aber zwei Komponenten koppeln,
 * die verschiedene Fragen stellen — das Banner „darf ich stören?", der
 * Riegel „darf ich sperren?". Wer eine Phase ergänzt, muss beide
 * anfassen; der Test `UpdateGate.test.tsx` hält das fest.
 */
const AKTIVE_FLUGPHASEN: ReadonlySet<FlightPhase> = new Set([
  "pushback",
  "taxi_out",
  "takeoff_roll",
  "takeoff",
  "climb",
  "cruise",
  "holding",
  "descent",
  "approach",
  "final",
  "landing",
  "taxi_in",
  "blocks_on",
] as FlightPhase[]);

interface Props {
  checker: UseUpdateCheckerResult;
  /** Phase des laufenden Flugs, sonst null. */
  activePhase: FlightPhase | null;
  /**
   * Notausstieg für den Betrieb: Setzt jemand
   * `localStorage.aeroacars.update.gate_off = "1"`, bleibt der Riegel
   * aus. Gedacht für den Fall, dass er sich im Feld als untragbar
   * erweist — dann braucht es keinen neuen Client, um ihn abzustellen.
   */
  ausgeschaltet?: boolean;
}

function riegelAbgeschaltet(): boolean {
  try {
    return localStorage.getItem("aeroacars.update.gate_off") === "1";
  } catch {
    return false;
  }
}

export function UpdateGate({ checker, activePhase, ausgeschaltet }: Props) {
  const { t } = useTranslation();
  const {
    update,
    pflichtUpdate,
    installing,
    progress,
    installAndRelaunch,
    installationGescheitert,
  } = checker;

  if (ausgeschaltet ?? riegelAbgeschaltet()) return null;
  // Ausnahmen 1 und 2.
  if (!update || !pflichtUpdate) return null;
  // Ausnahme 3.
  if (activePhase != null && AKTIVE_FLUGPHASEN.has(activePhase)) return null;

  return (
    <div className="update-gate" role="alertdialog" aria-modal="true">
      <div className="update-gate__card">
        <div className="update-gate__icon" aria-hidden="true">
          ⬇
        </div>
        <h2 className="update-gate__title">
          {t("update.gate_title", { version: update.version })}
        </h2>
        <p className="update-gate__lead">{t("update.gate_lead")}</p>

        {progress && <p className="update-gate__progress">{progress}</p>}

        <button
          type="button"
          className="update-gate__install"
          onClick={() => void installAndRelaunch()}
          disabled={installing}
        >
          {installing ? t("update.gate_installing") : t("update.gate_install")}
        </button>

        {/* Ausnahme 4 — erst nach einem echten Fehlschlag. */}
        {installationGescheitert && (
          <div className="update-gate__fallback">
            <p>{t("update.gate_failed")}</p>
            <button
              type="button"
              className="update-gate__continue"
              onClick={() => {
                try {
                  localStorage.setItem("aeroacars.update.gate_off", "1");
                } catch {
                  /* Ohne Speicher kommt der Riegel beim nächsten Start
                     wieder — unschön, aber diese Sitzung läuft. */
                }
                // Neu laden, damit der Riegel verschwindet.
                window.location.reload();
              }}
            >
              {t("update.gate_continue")}
            </button>
          </div>
        )}

        <p className="update-gate__hint">{t("update.gate_hint")}</p>
      </div>
    </div>
  );
}
