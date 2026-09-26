// Bordbuch — Einstellungen. Jeder Pilot stellt seine Liste selbst zusammen;
// Voreinstellung sind die Umfrage-Favoriten. Die Einstellungen liegen lokal
// und auf dem Server und kommen nach einer Neuinstallation zurück.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import { schalterAn, type Einstellungen, type Klasse, type Schalter } from "../../lib/bordbuch";
import "./bordbuch.css";

const KLASSEN: Klasse[] = ["airliner", "business_jet", "ga"];

/** Gruppen wie in der Konzeptseite. */
const GRUPPEN: Array<{ key: string; schalter: Schalter[] }> = [
  { key: "lichter", schalter: ["beacon", "strobes", "nav", "transponder", "landelicht"] },
  { key: "boden", schalter: ["rolltempo", "parkbremse", "apu"] },
  { key: "anflug", schalter: ["autobrake", "spoiler", "anschnallzeichen", "klappen"] },
];

export function BordbuchEinstellungen() {
  const { t } = useTranslation();
  const [e, setE] = useState<Einstellungen | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [neuSchluessel, setNeuSchluessel] = useState("");
  const [neuKlasse, setNeuKlasse] = useState<Klasse>("business_jet");

  useEffect(() => {
    void invoke<Einstellungen>("bordbuch_einstellungen_holen")
      .then(setE)
      .catch((err) => setFehler(String(err)));
  }, []);

  const speichern = async (neu: Einstellungen) => {
    setE(neu);
    try {
      const gespeichert = await invoke<Einstellungen>("bordbuch_einstellungen_setzen", { einstellungen: neu });
      setE(gespeichert);
      setFehler(null);
    } catch (err) {
      setFehler(String(err));
    }
  };

  if (!e) {
    return <div className="settings__section">{fehler ?? "…"}</div>;
  }

  const setzeSchalter = (s: Schalter, an: boolean) =>
    void speichern({ ...e, regeln: { ...e.regeln, [s]: an } });

  const zahl = (feld: "rolltempo_kt" | "rolltempo_ga_kt", roh: string) => {
    const v = Number(roh.replace(",", "."));
    if (Number.isFinite(v) && v >= 5 && v <= 60) void speichern({ ...e, [feld]: v });
  };

  return (
    <>
      <div className="settings__section">
        <h3>{t("bordbuch.einst.titel")}</h3>
        <p className="settings__row-hint">{t("bordbuch.einst.erklaerung")}</p>
        <label className="settings__checkbox">
          <input
            type="checkbox"
            checked={e.hinweise_im_flug}
            onChange={(ev) => void speichern({ ...e, hinweise_im_flug: ev.target.checked })}
          />
          <span>
            <strong>{t("bordbuch.einst.hinweise")}</strong>
            <span className="settings__row-hint">{t("bordbuch.einst.hinweise_hint")}</span>
          </span>
        </label>
        <label className="settings__checkbox">
          <input type="checkbox" checked={e.ga_an} onChange={(ev) => void speichern({ ...e, ga_an: ev.target.checked })} />
          <span>
            <strong>{t("bordbuch.einst.ga")}</strong>
            <span className="settings__row-hint">{t("bordbuch.einst.ga_hint")}</span>
          </span>
        </label>
        <label className="settings__checkbox">
          <input type="checkbox" checked={e.vfr_an} onChange={(ev) => void speichern({ ...e, vfr_an: ev.target.checked })} />
          <span>
            <strong>{t("bordbuch.einst.vfr")}</strong>
            <span className="settings__row-hint">{t("bordbuch.einst.vfr_hint")}</span>
          </span>
        </label>
      </div>

      {GRUPPEN.map((g) => (
        <div key={g.key} className="settings__section">
          <h3>{t(`bordbuch.einst.gruppe.${g.key}`)}</h3>
          {g.schalter.map((s) => (
            <label key={s} className="settings__checkbox">
              <input type="checkbox" checked={schalterAn(e, s)} onChange={(ev) => setzeSchalter(s, ev.target.checked)} />
              <span>
                <strong>{t(`bordbuch.einst.regel.${s}`)}</strong>
                <span className="settings__row-hint">{t(`bordbuch.einst.regel_hint.${s}`)}</span>
              </span>
            </label>
          ))}
          {g.key === "boden" && schalterAn(e, "rolltempo") && (
            <div className="bb-einst-zahlen">
              <label>
                {t("bordbuch.einst.rolltempo_kt")}
                <input
                  type="number"
                  min={5}
                  max={60}
                  step={1}
                  defaultValue={e.rolltempo_kt}
                  onBlur={(ev) => zahl("rolltempo_kt", ev.target.value)}
                />
                kt
              </label>
              <label>
                {t("bordbuch.einst.rolltempo_ga_kt")}
                <input
                  type="number"
                  min={5}
                  max={60}
                  step={1}
                  defaultValue={e.rolltempo_ga_kt}
                  onBlur={(ev) => zahl("rolltempo_ga_kt", ev.target.value)}
                />
                kt
              </label>
            </div>
          )}
        </div>
      ))}

      <div className="settings__section">
        <h3>{t("bordbuch.einst.klassen")}</h3>
        <p className="settings__row-hint">{t("bordbuch.einst.klassen_hint")}</p>
        {Object.entries(e.klassen_override).length > 0 && (
          <ul className="bb-einst-klassen">
            {Object.entries(e.klassen_override).map(([k, v]) => (
              <li key={k}>
                <code>{k}</code> {t(`bordbuch.klasse.${v}`)}
                <button
                  type="button"
                  className="bb-knopf bb-knopf--leise"
                  onClick={() => {
                    const rest = { ...e.klassen_override };
                    delete rest[k];
                    void speichern({ ...e, klassen_override: rest });
                  }}
                >
                  {t("bordbuch.einst.entfernen")}
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="bb-einst-neu">
          <input
            type="text"
            value={neuSchluessel}
            maxLength={12}
            placeholder={t("bordbuch.einst.schluessel_platzhalter")}
            onChange={(ev) => setNeuSchluessel(ev.target.value.toUpperCase())}
          />
          <select value={neuKlasse} onChange={(ev) => setNeuKlasse(ev.target.value as Klasse)}>
            {KLASSEN.map((k) => (
              <option key={k} value={k}>
                {t(`bordbuch.klasse.${k}`)}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="bb-knopf"
            disabled={neuSchluessel.trim().length < 2}
            onClick={() => {
              void speichern({
                ...e,
                klassen_override: { ...e.klassen_override, [neuSchluessel.trim()]: neuKlasse },
              });
              setNeuSchluessel("");
            }}
          >
            {t("bordbuch.einst.hinzufuegen")}
          </button>
        </div>
      </div>

      <div className="settings__section">
        <p className="settings__row-hint">{t("bordbuch.einst.nicht_dabei")}</p>
        <p className="settings__row-hint">{t("bordbuch.einst.nicht_messbar")}</p>
        {fehler && <p className="settings__row-hint bb-fehler">{fehler}</p>}
      </div>
    </>
  );
}

