// v0.13.0 Slice 6 — Mid-Session-Integrity-Banner
//
// Zeigt eine Meldung, wenn der Recorder ein integrity-flag-Ereignis
// geschickt hat:
//   info     — kommt seit dem 12.08.2026 gar nicht mehr im Client an
//              (der Server funkt nur noch Kritisches ins Cockpit)
//   anomaly  — gelbe Meldung, schliessbar
//   critical — rote Meldung, ebenfalls schliessbar
//
// Hinweis: Der Kopfkommentar sagte frueher, das Banner warne den Piloten
// davor, dass sein Flugbericht "vermutlich untrusted" werde. Das war schon
// seit v0.13.4 des Servers nicht mehr wahr und ist am 12.08.2026 aus Text
// UND Absicht entfernt worden: das Banner meldet einen Datenzustand, es
// droht keine Folge an. Welche Berichte wirklich in die Pruefung gehen,
// entscheidet ausschliesslich scoreTrust.ts im Recorder (fehlende Landung,
// Sim-Absturz-Signatur).
//
// Redesign Stufe B — BUGFIX: Diese Komponente war vollständig in
// Tailwind-Syntax geschrieben (`fixed top-12`, `bg-red-900/95`,
// `rounded-lg`, `max-w-2xl` …), obwohl das Projekt kein Tailwind hat und
// nie hatte. Keine dieser Klassen existierte in App.css. Ergebnis: wenn
// mitten im Flug ein kritisches Integritätsproblem auftrat, sah der Pilot
// ein ungestyltes <div> im normalen Textfluss — nicht fixiert, nicht rot,
// ohne Rahmen. Jetzt über die Notice-Primitive, die dieselbe Wirkung mit
// echten Tokens erzielt. Texte und Daten unverändert.

import { useTranslation } from "react-i18next";
import { useIntegrityFlags } from "../hooks/useIntegrityFlags";
import { Button, Notice } from "./ui";

export function IntegrityBanner() {
  const { t } = useTranslation();
  const { state, dismiss } = useIntegrityFlags();

  if (state.sessionSeverity === "info" || state.recentFlags.length === 0) return null;
  if (state.dismissed) return null;

  const isCritical = state.sessionSeverity === "critical";

  // Feldbefund 12.08.2026: Hier stand `state.recentFlags[0]` — der ZULETZT
  // eingegangene Fall. Im roten Kasten stand deshalb ein harmloser Hinweis,
  // während der eigentliche Grund für die rote Farbe nirgends auftauchte.
  // Jetzt der schwerste Fall; die Überschrift und der Text erzählen
  // dieselbe Geschichte.
  const schlimmster = state.schwersterFlag ?? state.recentFlags[0];
  const flagType = schlimmster?.flag.type ?? "UNKNOWN";
  const flagPhase = schlimmster?.flag.phase ?? "";

  // Feldbefund Thomas (12.08.2026): "POSITION_DELTA_EXCESSIVE in Phase CLIMB"
  // stand so im Cockpit — Maschinenbezeichner, für einen Piloten kryptisch.
  // Jetzt Klartext, mit dem Rohnamen als Rückfallebene, damit ein neuer
  // Melder-Typ nicht als leerer Text erscheint, sondern wenigstens erkennbar
  // bleibt.
  const wasIstLos = t(`integrity.flag_type.${flagType}`, { defaultValue: flagType });
  const wannWar = flagPhase
    ? t(`integrity.phase_name.${flagPhase}`, { defaultValue: flagPhase })
    : "";

  const title = isCritical
    ? t("integrity.title_critical", "Data-Integrity-Problem entdeckt")
    : t("integrity.title_anomaly", "Datenanomalie");

  return (
    <Notice
      floating
      role="alert"
      aria-live="assertive"
      tone={isCritical ? "error" : "warn"}
      level={title}
      detail={
        <>
          <span>
            {wannWar
              ? t("integrity.flag_description_readable", {
                  defaultValue: "{{was}} {{wann}}",
                  was: wasIstLos,
                  wann: wannWar,
                })
              : wasIstLos}
          </span>
          {/* Der frühere Satz hier lautete "Der PIREP wird wahrscheinlich als
              'untrusted' eingestuft und für VA-Admin-Review markiert." Das
              stimmt seit v0.13.4 des Servers nicht mehr: KEIN Integritäts-
              Merkmal führt automatisch zu einer Prüfung. In die Prüfung geht
              ein Bericht nur, wenn die Landung gar nicht erfasst wurde oder
              das Aufsetzen die Signatur eines Sim-Absturzes trägt. Eine
              Warnung, die eine Folge androht, die nicht eintritt, macht
              Piloten grundlos nervös — und wer sie einmal als falsch
              erkannt hat, liest die nächste nicht mehr. */}
          {" · "}
          <span>
            {isCritical
              ? t(
                  "integrity.folge_kritisch",
                  "Der Flug wird normal gewertet. Bleibt die Aufzeichnung aber lückenhaft, fehlt am Ende womöglich die Landung — dann geht der Bericht zur Prüfung. Am besten die Verbindung zum Simulator im Blick behalten.",
                )
              : t(
                  "integrity.folge_harmlos",
                  "Kein Eingriff nötig — der Flug wird normal gewertet.",
                )}
          </span>
          {/* Gezählt wird, wie oft es AUFGETRETEN ist, nicht wie viele
              Ereignisse hereinkamen: der Server fasst Gleichlautendes
              zusammen und schickt die Anzahl mit. */}
          {state.meldungenGesamt > 1 && (
            <>
              {" · "}
              <span>
                {t("integrity.flag_count_readable", {
                  defaultValue: "{{count}}-mal in diesem Flug",
                  count: state.meldungenGesamt,
                })}
              </span>
            </>
          )}

        </>
      }
      actions={
        /* Auch der rote Kasten laesst sich jetzt schliessen. Er hatte
           keinen Schliessknopf, weil er als Vorwarnung vor einer Sperre
           gedacht war — die es nicht gibt. Eine neue kritische Meldung
           holt ihn ohnehin zurueck (siehe useIntegrityFlags). */
        (
          <Button
            variant="quiet"
            size="sm"
            onClick={dismiss}
            aria-label={t("integrity.dismiss", "Schließen")}
          >
            ✕
          </Button>
        )
      }
    />
  );
}
