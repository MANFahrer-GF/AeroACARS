// VATSIM CDM — die Abflugfolge, ohne aus dem Sim zu wechseln.
//
// Zeigt `vats.im/vdgs` (das A-CDM-Werkzeug von VATSIM Spain) in einem
// eigenen Fenster der App. Drei Dinge, die man beim Lesen wissen muss:
//
//   1. **Kein Rahmen, sondern ein Fenster.** Der erste Entwurf bettete die
//      Seite in diese Ansicht ein. Das geht nicht: `auth.vatsim.net` setzt
//      `X-Frame-Options: SAMEORIGIN` (gemessen am 12.08.2026, im Betrieb
//      bestätigt mit „auth.vatsim.net haben die Verbindung verweigert").
//      Eine Anmeldeseite, die sich einbetten ließe, wäre auch ein
//      Sicherheitsproblem. Ein eigenes Fenster ist dagegen normale
//      Navigation — und bleibt trotzdem in der App, kein fremder Browser.
//
//   2. **Wir übergeben dorthin nichts.** Rufzeichen und EOBT holt sich die
//      Seite selbst aus dem VATSIM-Flugplan, TSAT und CTOT rechnet deren
//      CDM-Logik. Der Pilot trägt dort höchstens seine TOBT ein.
//
//   3. **Die Anmeldung gehört ihnen.** Der Login läuft über VATSIM-Connect
//      mit deren Anwendungskennung; daran automatisieren wir nichts. Einmal
//      selbst anmelden, danach bleibt die Sitzung im Fenster bestehen.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri, openExternal } from "../lib/ipc";
import { Button } from "./ui";

const VDGS_URL = "https://vats.im/vdgs";

export function VatsimCdmView() {
  const { t } = useTranslation();
  const [offen, setOffen] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

  const standPruefen = useCallback(async () => {
    if (!isTauri) return;
    try {
      setOffen(await invoke<boolean>("vdgs_fenster_offen"));
    } catch {
      /* ohne Antwort bleibt es beim bisherigen Stand */
    }
  }, []);

  useEffect(() => {
    void standPruefen();
    // Das Fenster kann auch zugeklappt werden, ohne dass wir es merken —
    // deshalb im Takt nachsehen, damit die Beschriftung stimmt.
    const id = setInterval(() => void standPruefen(), 3000);
    return () => clearInterval(id);
  }, [standPruefen]);

  const oeffnen = useCallback(async () => {
    setFehler(null);
    if (!isTauri) {
      // Im LAN-Browser gibt es keine App-Fenster — dort der normale Weg.
      void openExternal(VDGS_URL).catch(() => {});
      return;
    }
    try {
      await invoke("vdgs_fenster_oeffnen");
      await standPruefen();
    } catch (e) {
      setFehler(
        (e as { message?: string })?.message ??
          t("cdm.fehler", "Das Fenster ließ sich nicht öffnen."),
      );
    }
  }, [standPruefen, t]);

  return (
    <div className="cdm">
      <div className="cdm__kopf">
        <h2 className="cdm__titel">{t("cdm.titel", "VATSIM CDM")}</h2>
        <span className="cdm__quelle">vats.im/vdgs</span>
        <div className="cdm__knoepfe">
          <Button size="sm" onClick={() => void oeffnen()}>
            {offen
              ? t("cdm.nach_vorn", "Fenster nach vorn holen")
              : t("cdm.oeffnen", "VDGS öffnen")}
          </Button>
        </div>
      </div>

      <div className="cdm__flaeche">
        <div className="cdm__erklaerung">
          <strong>{t("cdm.was_titel", "Deine Abflugfolge auf VATSIM.")}</strong>
          <span>
            {t(
              "cdm.was_text",
              "TOBT, TSAT und CTOT kommen von der CDM-Seite von VATSIM Spain. Sie öffnet sich in einem eigenen Fenster von AeroACARS — anmelden musst du dich einmal selbst mit deinem VATSIM-Konto, danach bleibt die Anmeldung dort bestehen.",
            )}
          </span>
          <span className="cdm__leise">
            {t(
              "cdm.warum_fenster",
              "Warum ein eigenes Fenster und nicht hier drin: VATSIM erlaubt es nicht, die Anmeldeseite in ein anderes Programm einzubetten — aus gutem Grund, sonst könnte jemand eine falsche davorlegen.",
            )}
          </span>
          <Button onClick={() => void oeffnen()}>
            {offen
              ? t("cdm.nach_vorn", "Fenster nach vorn holen")
              : t("cdm.oeffnen", "VDGS öffnen")}
          </Button>
          {fehler && <span className="cdm__fehler">{fehler}</span>}
        </div>
      </div>

      <p className="cdm__fuss">
        {t(
          "cdm.fuss",
          "Seite von VATSIM Spain. AeroACARS zeigt sie nur an, übergibt nichts dorthin und speichert nichts davon.",
        )}
      </p>
    </div>
  );
}
