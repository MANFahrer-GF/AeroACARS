// Kacheln der Liste (Entwurf Main.dc.html): Vorschau, Leiste mit Zeit, Hoehe und Phase,
// Auswahl-Checkbox oben rechts GETRENNT vom Oeffnen, Marke oben links fuer den
// Sendestand, Textzeile mit Grund und „Erneut versuchen“ unter der Kachel.
import { useTranslation } from "react-i18next";
import { kurzZeit } from "./logik/metaFormat";
import { fehlerCodeNormal, type KachelStand } from "./logik/sendezustand";
import type { Eintrag } from "./logik/typen";
import { Symbol } from "./Symbole";
import { useBildUrl } from "./useBildUrl";

export function bildZeitIso(e: Eintrag): string | null {
  return e.sidecar?.meta?.zeit_utc ?? (e.dateizeit_ms ? new Date(e.dateizeit_ms).toISOString() : null);
}

export function useKachelTexte() {
  const { t, i18n } = useTranslation();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "de";
  const phase = (e: Eintrag): string => {
    const m = e.sidecar?.meta;
    if (m?.phase) return t(`bilder.detail.phase.${m.phase}`, { defaultValue: m.phase });
    return m?.pirep_id ? "" : t("bilder.kachel.ohne_flug");
  };
  const hoehe = (e: Eintrag): string => {
    const h = e.sidecar?.meta?.hoehe_ft;
    return typeof h === "number" ? `${Math.round(h).toLocaleString(sprache)} ft` : t("bilder.kachel.keine_daten");
  };
  const grund = (stand: KachelStand): string => {
    const code = fehlerCodeNormal(stand.code);
    let g = t(`bilder.fehler.${code}`);
    if (stand.retryAfter) g += ` (${t("bilder.fehler.wieder_in", { minuten: Math.max(1, Math.ceil(stand.retryAfter / 60)) })})`;
    return g;
  };
  return { t, sprache, phase, hoehe, grund };
}

export function Kachel({
  e, stand, gewaehlt, offen, onWahl, onOeffnen, onErneut,
}: {
  e: Eintrag;
  stand: KachelStand | undefined;
  gewaehlt: boolean;
  offen: boolean;
  onWahl: () => void;
  onOeffnen: () => void;
  onErneut: () => void;
}) {
  const { t, phase, hoehe, grund } = useKachelTexte();
  const url = useBildUrl(e.vorschau_pfad ?? e.pfad);
  const zeit = kurzZeit(bildZeitIso(e));
  const ph = phase(e);
  const gesendetFrueher = (e.sidecar?.sendungen ?? []).some((s) => s.status === "gesendet");
  const phaseSend = stand?.phase ?? (gesendetFrueher ? "gesendet" : undefined);
  const zusatz = phaseSend === "gesendet" ? t("bilder.kachel.gesendet") : phaseSend === "sendet" || phaseSend === "angenommen" ? t("bilder.kachel.sendet") : phaseSend === "fehlgeschlagen" ? t("bilder.kachel.fehlgeschlagen") : "";
  const hoeheText = hoehe(e);
  return (
    <div className="bt-kachelwrap">
      <div className="bt-kachel" data-gewaehlt={gewaehlt ? "an" : undefined} data-offen={offen ? "an" : undefined}>
        {url ? <img src={url} alt="" /> : <div className="bt-kachel__laedt">{t("bilder.kachel.laedt")}</div>}
        <button
          type="button"
          className="bt-kachel__oeffnen"
          aria-pressed={offen}
          aria-label={`${t("bilder.kachel.oeffnen_mit", { zeit, phase: ph, hoehe: hoeheText })}${zusatz ? ", " + zusatz : ""}`}
          onClick={onOeffnen}
        >
          {zeit}
        </button>
        <span className="bt-kachel__leiste">
          <span className="bt-kachel__zeit">{zeit}</span>
          <span className="bt-kachel__hoehe">{hoeheText}</span>
          {ph && <span className="bt-kachel__phase">{ph}</span>}
        </span>
        {e.sidecar?.favorit && <span className="bt-kachel__stern" title={t("bilder.kachel.favorit")}>{Symbol.stern}</span>}
        <input
          type="checkbox"
          className="bt-kachel__wahl"
          checked={gewaehlt}
          onChange={onWahl}
          aria-label={t("bilder.kachel.auswaehlen_mit", { zeit, phase: ph })}
        />
        {gewaehlt && <span className="bt-kachel__haken" aria-hidden="true">{Symbol.haken}</span>}
        {phaseSend === "gesendet" && <span className="bt-marke bt-marke--gesendet" title={t("bilder.kachel.gesendet")} aria-hidden="true">{Symbol.senden}</span>}
        {(phaseSend === "sendet" || phaseSend === "angenommen") && <span className="bt-marke bt-marke--sendet" title={t("bilder.kachel.sendet")} aria-hidden="true">{Symbol.dreh}</span>}
        {phaseSend === "fehlgeschlagen" && <span className="bt-marke bt-marke--fehler" title={t("bilder.kachel.fehlgeschlagen")} aria-hidden="true">{Symbol.ausruf}</span>}
      </div>
      {stand?.phase === "fehlgeschlagen" && (
        <span className="bt-notiz bt-notiz--fehler">
          {t("bilder.status.fehlgeschlagen", { grund: grund(stand) })}{" "}
          <button type="button" className="bt-notiz__link" onClick={onErneut}>{t("bilder.status.erneut")}</button>
        </span>
      )}
      {(stand?.phase === "sendet") && <span className="bt-notiz bt-notiz--info">{t("bilder.status.sendet")}</span>}
      {(stand?.phase === "angenommen") && <span className="bt-notiz bt-notiz--info">{t("bilder.status.angenommen")}</span>}
    </div>
  );
}

export function SerienKachel({ bilder, onOeffnen }: { bilder: Eintrag[]; onOeffnen: () => void }) {
  const { t } = useKachelTexte();
  const url = useBildUrl(bilder[0].vorschau_pfad ?? bilder[0].pfad);
  return (
    <div className="bt-kachelwrap">
      <button type="button" className="bt-serie__stapel" onClick={onOeffnen} aria-label={`${t("bilder.kachel.serie", { n: bilder.length })}, ${t("bilder.kachel.serie_oeffnen")}`}>
        {url && <img src={url} alt="" />}
        <span className="bt-serie__zahl">{bilder.length}</span>
      </button>
    </div>
  );
}
