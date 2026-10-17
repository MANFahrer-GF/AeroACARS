// Kabinenansagen (Beta): Einstellungen. Gleiche Hausform wie der
// Hoppie-Abschnitt (Hauptschalter, darunter eingerückt die Optionen).
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatIpcError, listen, openExternal } from "../lib/ipc";
import {
  abspielen,
  azureSchluesselSetzen,
  einstellungenLesen,
  einstellungenSetzen,
  piperInstallieren,
  probe,
  stoppen,
  zustandLesen,
  type Einstellungen,
  type Rolle,
  type Zustand,
} from "../lib/kabinenansagen";

const EIGENE_SPRACHEN = ["de-DE", "de-AT", "de-CH", "en-GB", "en-US", "fr-FR", "it-IT", "es-ES", "nl-NL", "pt-PT", "pl-PL"];
const TAGESFORMEN = ["zufall", "sachlich", "herzlich", "charmant", "witzig", "rustikal"];

interface Fortschritt {
  schritt: string;
  bytes: number;
  gesamt: number;
}

export function KabinenSettingsPanel({ onAnChange }: { onAnChange?: (an: boolean) => void }) {
  const { t } = useTranslation();
  const [e, setE] = useState<Einstellungen | null>(null);
  const [z, setZ] = useState<Zustand | null>(null);
  const [schluessel, setSchluessel] = useState("");
  const [busy, setBusy] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [laden, setLaden] = useState<Fortschritt | null>(null);

  useEffect(() => {
    void einstellungenLesen().then(setE);
    void zustandLesen().then(setZ).catch(() => setZ(null));
    const ab = listen<Fortschritt>("kabine-piper-fortschritt", (ev) => setLaden(ev.payload));
    return () => {
      void ab.then((f) => f());
    };
  }, []);

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

  const schluesselSpeichern = (wert: string | null) =>
    mitBusy(async () => {
      const da = await azureSchluesselSetzen(wert);
      setSchluessel("");
      setZ((alt) => (alt ? { ...alt, azure_schluessel: da } : alt));
    });

  const piperLaden = () =>
    mitBusy(async () => {
      const fertig = await piperInstallieren();
      setLaden(null);
      setZ((alt) => (alt ? { ...alt, piper_installiert: fertig } : alt));
    });

  const hoeren = (rolle: Rolle, sprache: string) =>
    mitBusy(async () => {
      abspielen(await probe(rolle, sprache));
    });

  const anbieterWahl = (feld: "anbieter_kapitaen" | "anbieter_kabine" | "anbieter_kommando", label: string) => (
    <label className="settings__field">
      <span className="settings__field-label">{label}</span>
      <select value={e[feld]} onChange={(ev) => void aendern({ [feld]: ev.target.value } as Partial<Einstellungen>)}>
        <option value="piper">{t("kabine.einstellungen.anbieter_piper")}</option>
        <option value="azure">{t("kabine.einstellungen.anbieter_azure")}</option>
      </select>
    </label>
  );

  const betaText =
    z?.beta === true
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
              <option value="ziel">{t("kabine.einstellungen.sprache_ziel")}</option>
              <option value="eigene">{t("kabine.einstellungen.sprache_eigene")}</option>
              <option value="keine">{t("kabine.einstellungen.sprache_keine")}</option>
            </select>
            <span className="settings__row-hint">{t("kabine.einstellungen.sprache_hint")}</span>
          </label>
          {e.zweite_sprache === "eigene" && (
            <label className="settings__field">
              <span className="settings__field-label">{t("kabine.einstellungen.eigene_sprache")}</span>
              <select value={e.eigene_sprache ?? ""} onChange={(ev) => void aendern({ eigene_sprache: ev.target.value || null })}>
                <option value="">—</option>
                {EIGENE_SPRACHEN.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
          )}

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.stimmen")}</span>
            <span className="settings__row-hint">{t("kabine.einstellungen.stimmen_hint")}</span>
          </div>
          {anbieterWahl("anbieter_kapitaen", t("kabine.rolle.kapitaen"))}
          {anbieterWahl("anbieter_kabine", t("kabine.rolle.kabine"))}
          {anbieterWahl("anbieter_kommando", t("kabine.rolle.kommando"))}

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.piper_titel")}</span>
            {!z?.piper_moeglich ? (
              <p className="settings__row-hint">{t("kabine.einstellungen.piper_nur_windows")}</p>
            ) : z.piper_installiert ? (
              <p className="settings__row-hint">{t("kabine.einstellungen.piper_installiert")}</p>
            ) : (
              <>
                <p className="settings__row-hint">{t("kabine.einstellungen.piper_hint")}</p>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <button type="button" className="button button--primary" disabled={busy} onClick={() => void piperLaden()}>
                    {t("kabine.einstellungen.piper_laden")}
                  </button>
                  {laden && (
                    <span className="settings__row-hint">
                      {laden.schritt}: {Math.round(laden.bytes / 1e6)} {laden.gesamt > 0 ? `/ ${Math.round(laden.gesamt / 1e6)} ` : ""}MB
                    </span>
                  )}
                </div>
              </>
            )}
          </div>

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.azure_titel")}</span>
            <p className="settings__row-hint">
              {z?.azure_schluessel ? t("kabine.einstellungen.azure_gespeichert") : t("kabine.einstellungen.azure_hint")}
            </p>
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <input
                style={{ flex: "1 1 220px", minWidth: 0 }}
                type="password"
                value={schluessel}
                onChange={(ev) => setSchluessel(ev.target.value)}
                placeholder={t("kabine.einstellungen.azure_platzhalter")}
                disabled={busy}
                autoComplete="off"
              />
              <button
                type="button"
                className="button button--primary"
                disabled={busy || schluessel.trim() === ""}
                onClick={() => void schluesselSpeichern(schluessel)}
              >
                {t("kabine.einstellungen.speichern")}
              </button>
              {z?.azure_schluessel && (
                <button
                  type="button"
                  className="button button--ghost"
                  style={{ whiteSpace: "nowrap" }}
                  disabled={busy}
                  onClick={() => void schluesselSpeichern(null)}
                >
                  {t("kabine.einstellungen.loeschen")}
                </button>
              )}
            </div>
          </div>
          <label className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.azure_region")}</span>
            <input
              type="text"
              value={e.azure_region}
              onChange={(ev) => setE({ ...e, azure_region: ev.target.value })}
              onBlur={() => void aendern({ azure_region: e.azure_region })}
            />
          </label>

          <label className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.tagesform_kapitaen")}</span>
            <select value={e.tagesform_kapitaen} onChange={(ev) => void aendern({ tagesform_kapitaen: ev.target.value })}>
              {TAGESFORMEN.map((f) => (
                <option key={f} value={f}>
                  {t(`kabine.tagesform.${f}`)}
                </option>
              ))}
            </select>
          </label>
          <label className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.tagesform_kabine")}</span>
            <select value={e.tagesform_kabine} onChange={(ev) => void aendern({ tagesform_kabine: ev.target.value })}>
              {TAGESFORMEN.map((f) => (
                <option key={f} value={f}>
                  {t(`kabine.tagesform.${f}`)}
                </option>
              ))}
            </select>
          </label>

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
            <span className="settings__field-label">
              {t("kabine.einstellungen.lautstaerke")} · {Math.round(e.lautstaerke * 100)} %
            </span>
            <input
              type="range"
              min={0}
              max={100}
              value={Math.round(e.lautstaerke * 100)}
              onChange={(ev) => setE({ ...e, lautstaerke: Number(ev.target.value) / 100 })}
              onMouseUp={() => void aendern({ lautstaerke: e.lautstaerke })}
              onKeyUp={() => void aendern({ lautstaerke: e.lautstaerke })}
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

          <div className="settings__field">
            <span className="settings__field-label">{t("kabine.einstellungen.lizenzen")}</span>
            <p className="settings__row-hint">{t("kabine.einstellungen.lizenz_text")}</p>
            {z?.piper_quelle && (
              <button type="button" className="button button--ghost" onClick={() => void openExternal(z.piper_quelle)}>
                {t("kabine.einstellungen.quelle")}
              </button>
            )}
          </div>
        </div>
      )}

      {fehler && <p className="cpdlc-panel__error">{fehler}</p>}
    </div>
  );
}
