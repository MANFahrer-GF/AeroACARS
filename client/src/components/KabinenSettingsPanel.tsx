// Kabinenansagen (Beta): Einstellungen. Gleiche Hausform wie der
// Hoppie-Abschnitt (Hauptschalter, darunter eingerückt die Optionen).
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatIpcError } from "../lib/ipc";
import {
  abspielen,
  ausgabegeraete,
  einstellungenLesen,
  einstellungenSetzen,
  freigeben,
  lautstaerkeSetzen,
  probe,
  speicherLesen,
  stoppen,
  zustandLesen,
  type Einstellungen,
  type Geraet,
  type Rolle,
  type Speicher,
  type Zustand,
} from "../lib/kabinenansagen";


export function KabinenSettingsPanel({ onAnChange }: { onAnChange?: (an: boolean) => void }) {
  const { t } = useTranslation();
  const [e, setE] = useState<Einstellungen | null>(null);
  const [z, setZ] = useState<Zustand | null>(null);
  const [busy, setBusy] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [geraete, setGeraete] = useState<Geraet[] | null>(null);
  const [speicher, setSpeicher] = useState<Speicher | null>(null);

  useEffect(() => {
    void einstellungenLesen().then(setE);
    void zustandLesen().then(setZ).catch(() => setZ(null));
    void speicherLesen().then(setSpeicher).catch(() => setSpeicher(null));
  }, []);

  // Geräteliste beim Öffnen und beim Aufklappen der Auswahl neu lesen —
  // ein Headset wird oft erst nach dem Start eingesteckt.
  const geraeteLesen = () => void ausgabegeraete().then(setGeraete).catch(() => setGeraete([]));
  useEffect(geraeteLesen, []);

  if (!e) {
    return (
      <div className="settings__section">
        <h3>{t("kabine.einstellungen.titel")}</h3>
      </div>
    );
  }

  const aendern = async (patch: Partial<Einstellungen>) => {
    setFehler(null);
    const neu = { ...e, ...patch };
    setE(neu);
    try {
      if (patch.an === false) stoppen();
      if (patch.an === true) freigeben();
      const gespeichert = await einstellungenSetzen(neu);
      setE(gespeichert);
      if (patch.an !== undefined) onAnChange?.(gespeichert.an);
    } catch (err) {
      setFehler(formatIpcError(err));
    }
  };

  const mitBusy = async (f: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setFehler(null);
    try {
      await f();
    } catch (err) {
      setFehler(formatIpcError(err));
    } finally {
      setBusy(false);
    }
  };

  const hoeren = (rolle: Rolle, sprache: string) =>
    mitBusy(async () => {
      abspielen(await probe(rolle, sprache));
    });

  const betaText =
    z?.beta === true && z.kontingent_erschoepft
      ? t("kabine.einstellungen.beta_kontingent")
      : z?.beta === true
      ? t("kabine.einstellungen.beta_ja")
      : z?.beta === false
        ? t("kabine.einstellungen.beta_nein")
        : t("kabine.einstellungen.beta_unbekannt");

  return (
    <div className="settings__section">
      <h3>{t("kabine.einstellungen.titel")}</h3>

      <label className="settings__checkbox">
        <input type="checkbox" checked={e.an} onChange={(ev) => void aendern({ an: ev.target.checked })} />
        <span>
          <strong>{t("kabine.einstellungen.an_label")}</strong>
          <span className="settings__row-hint">{t("kabine.einstellungen.an_hint")}</span>
          <span className="settings__row-hint" data-testid="kabine-beta-status">{betaText}</span>
        </span>
      </label>

      {e.an && (
        <div style={{ margin: "4px 0 12px 28px", display: "flex", flexDirection: "column", gap: 12 }}>
          <label className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.zweite_sprache")}</span>
            <select
              value={e.zweite_sprache}
              onChange={(ev) => void aendern({ zweite_sprache: ev.target.value as Einstellungen["zweite_sprache"] })}
            >
              <option value="airline">{t("kabine.einstellungen.sprache_airline")}</option>
              <option value="airline_de">{t("kabine.einstellungen.sprache_airline_de")}</option>
            </select>
            <span className="settings__row-hint">{t("kabine.einstellungen.sprache_hint")}</span>
          </label>
          <p className="settings__row-hint" style={{ margin: 0 }}>{t("kabine.einstellungen.stimmen_hint")}</p>

          <label className="settings__checkbox">
            <input type="checkbox" checked={e.echter_name} onChange={(ev) => void aendern({ echter_name: ev.target.checked })} />
            <span>
              <strong>{t("kabine.einstellungen.echter_name")}</strong>
              <span className="settings__row-hint">{t("kabine.einstellungen.echter_name_hint")}</span>
            </span>
          </label>
          <label className="settings__checkbox">
            <input type="checkbox" checked={e.kommandos} onChange={(ev) => void aendern({ kommandos: ev.target.checked })} />
            <span>
              <strong>{t("kabine.einstellungen.kommandos")}</strong>
              <span className="settings__row-hint">{t("kabine.einstellungen.doppelt_hint")}</span>
            </span>
          </label>
          <label className="settings__checkbox">
            <input type="checkbox" checked={e.chimes} onChange={(ev) => void aendern({ chimes: ev.target.checked })} />
            <span>
              <strong>{t("kabine.einstellungen.chimes")}</strong>
            </span>
          </label>

          <label className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.ausgabe")}</span>
            <select
              value={e.ausgabe ?? ""}
              onFocus={geraeteLesen}
              onChange={(ev) => void aendern({ ausgabe: ev.target.value || null })}
            >
              <option value="">{t("kabine.einstellungen.ausgabe_standard")}</option>
              {(geraete ?? []).map((g) => (
                <option key={g.id} value={g.id}>
                  {g.name}
                </option>
              ))}
              {e.ausgabe && geraete && !geraete.some((g) => g.id === e.ausgabe) && (
                <option value={e.ausgabe}>{t("kabine.einstellungen.ausgabe_fehlt")}</option>
              )}
            </select>
            <span className="settings__row-hint">{t("kabine.einstellungen.ausgabe_hint")}</span>
          </label>

          <label className="settings__field">
            <span className="settings__field-label">
              {t("kabine.einstellungen.lautstaerke")} · {Math.round(e.lautstaerke * 100)} %
            </span>
            <input
              type="range"
              min={0}
              max={100}
              value={Math.round(e.lautstaerke * 100)}
              onChange={(ev) => {
                // Eigenes Kommando: wirkt sofort und dreht nichts anderes zurück
                // (derselbe Wert wie der Regler oben auf der Seite „Kabine“).
                const v = Number(ev.target.value) / 100;
                setE({ ...e, lautstaerke: v });
                void lautstaerkeSetzen(v, false);
              }}
              onMouseUp={(ev) => void lautstaerkeSetzen(Number(ev.currentTarget.value) / 100, true)}
              onTouchEnd={(ev) => void lautstaerkeSetzen(Number(ev.currentTarget.value) / 100, true)}
              onKeyUp={(ev) => void lautstaerkeSetzen(Number(ev.currentTarget.value) / 100, true)}
            />
          </label>

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.probe")}</span>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              {(
                [
                  ["kapitaen", "de-DE"],
                  ["kabine", "de-DE"],
                  ["kapitaen", "en-GB"],
                  ["kabine", "en-GB"],
                ] as [Rolle, string][]
              ).map(([rolle, sprache]) => (
                <button key={rolle + sprache} type="button" className="button button--ghost" disabled={busy} onClick={() => void hoeren(rolle, sprache)}>
                  {t(`kabine.rolle.${rolle}`)} · {sprache}
                </button>
              ))}
            </div>
          </div>

          <div className="settings__field" data-testid="kabine-speicher">
            <span className="settings__field-label">{t("kabine.einstellungen.speicher")}</span>
            <p className="settings__row-hint">
              {t("kabine.einstellungen.speicher_text", {
                mb: speicher ? (speicher.bytes / 1_000_000).toLocaleString(undefined, { maximumFractionDigits: 1 }) : "–",
              })}
            </p>
          </div>

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.lizenzen")}</span>
            <p className="settings__row-hint">{t("kabine.einstellungen.lizenz_text")}</p>
          </div>
        </div>
      )}

      {fehler && <p className="cpdlc-panel__error">{fehler}</p>}
    </div>
  );
}
