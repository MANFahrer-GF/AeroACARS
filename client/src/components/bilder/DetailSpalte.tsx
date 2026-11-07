// Detailspalte (Entwurf): Bild, Zeit und Phase, Hauptaktionen OBEN (ohne Scrollen bei
// 720 px Hoehe erreichbar), Metadaten nach `tabellen/bilder.json`, Karte, Text.
import { useEffect, useState } from "react";
import { Button } from "../ui";
import { GRENZEN, TABELLE } from "./logik/grenzen";
import { langZeit, wertFormatieren } from "./logik/metaFormat";
import type { Eintrag } from "./logik/typen";
import { BilderKarte } from "./BilderKarte";
import { bildZeitIso, useKachelTexte } from "./Kacheln";
import { Symbol } from "./Symbole";
import { useBildUrl } from "./useBildUrl";

export function DetailSpalte({
  e, gewaehlt, flugBilder, aktivePirep, mitSenden, onSenden, onOrdner, onLoeschen, onText, onFavorit, onFlugAendern, onKarteOeffnen, onMarke, onZu,
}: {
  e: Eintrag | null;
  gewaehlt: boolean;
  flugBilder: Eintrag[];
  aktivePirep: string | null;
  /** Mit einer Auswahl sendet die Leiste unten; zwei Knoepfe waeren zwei Wege fuer dasselbe. */
  mitSenden: boolean;
  onSenden: () => void;
  onOrdner: () => void;
  onLoeschen: () => void;
  onText: (text: string) => void;
  onFavorit: (an: boolean) => void;
  onFlugAendern: () => void;
  onKarteOeffnen: (f: { lat: number; lon: number; id: string; pirep: string | null }) => void;
  onMarke: (id: string) => void;
  onZu: () => void;
}) {
  const { t, sprache, phase } = useKachelTexte();
  const url = useBildUrl(e?.pfad ?? null);
  const [text, setText] = useState(e?.sidecar?.text ?? "");
  useEffect(() => { setText(e?.sidecar?.text ?? ""); }, [e?.id, e?.sidecar?.text]);

  if (!e) {
    return <aside className="bt-detail" data-leer="an" aria-label={t("bilder.detail.titel")}><p className="bt-hinweis">{t("bilder.detail.leer_hinweis")}</p></aside>;
  }
  const zeit = langZeit(bildZeitIso(e));
  const ph = phase(e);
  return (
    <aside className="bt-detail" aria-label={t("bilder.detail.titel")}>
      <Button size="sm" variant="quiet" className="bt-detail__zu" onClick={onZu} aria-label={t("bilder.detail.schliessen")}>{Symbol.zu}</Button>
      <div className="bt-detail__bild">{url && <img src={url} alt="" />}</div>
      <div className="bt-detail__kopf">
        <span className="bt-acars bt-detail__zeit">{zeit}</span>
        {ph && <span className="bt-chip bt-chip--an">{ph}</span>}
        <span className="bt-detail__zusatz">{gewaehlt ? t("bilder.detail.ausgewaehlt") : t("bilder.detail.nicht_ausgewaehlt")}</span>
      </div>
      <div className="bt-detail__aktionen">
        {mitSenden && <Button variant="primary" onClick={onSenden} iconLeft={Symbol.senden}>{t("bilder.detail.aktionen.senden")}</Button>}
        <div className="bt-detail__aktionen-reihe">
          <Button variant="quiet" onClick={onOrdner} iconLeft={Symbol.ordner}>{t("bilder.detail.aktionen.ordner")}</Button>
          <Button variant="danger" onClick={onLoeschen} iconLeft={Symbol.papierkorb}>{t("bilder.detail.aktionen.loeschen")}</Button>
        </div>
      </div>
      <dl className="bt-kv">
        {TABELLE.metadaten.map((z) => {
          const w = wertFormatieren(z, e, sprache);
          return (
            <div key={z.feld} style={{ display: "contents" }}>
              <dt>{t(`bilder.detail.feld.${z.feld}`)}</dt>
              <dd data-leer={w.art === "leer" ? "an" : undefined}>
                {w.art === "leer" && z.format !== "flug" && t("bilder.detail.leer_wert")}
                {w.art === "leer" && z.format === "flug" && (
                  <>
                    {t("bilder.detail.flug.ohne")}
                    <button type="button" className="bt-link" onClick={onFlugAendern}>{t("bilder.detail.flug.aendern")}</button>
                  </>
                )}
                {w.art === "text" && (
                  <>
                    <span className={z.format === "datei" ? "bt-kv__datei" : z.format === "text" ? undefined : "bt-acars"}>{w.text}</span>
                    {w.zusatz && <> <span className="bt-kv__zusatz">{t(`bilder.detail.zusatz.${w.zusatz}`)}</span></>}
                  </>
                )}
                {w.art === "flug" && (
                  <>
                    <span className="bt-acars">{w.text.split(" ")[0]}</span> {w.text.split(" ").slice(1).join(" ")}
                    <button type="button" className="bt-link" onClick={onFlugAendern}>{t("bilder.detail.flug.aendern")}</button>
                  </>
                )}
                {w.art === "quelle" && t(`bilder.detail.quelle.${w.wert}`, { defaultValue: w.wert })}
              </dd>
            </div>
          );
        })}
      </dl>
      <BilderKarte eintrag={e} flugBilder={flugBilder} aktivePirep={aktivePirep} onOeffnen={onKarteOeffnen} onMarke={onMarke} />
      <div className="bt-abschnitt">
        <label htmlFor="bt-bildtext" className="bt-label">{t("bilder.detail.text.label")}</label>
        <textarea
          id="bt-bildtext"
          className="bt-feld"
          value={text}
          maxLength={GRENZEN.text_max_zeichen}
          placeholder={t("bilder.detail.text.platzhalter", { max: GRENZEN.text_max_zeichen })}
          onChange={(ev) => setText(ev.target.value)}
          onBlur={() => { if (text !== (e.sidecar?.text ?? "")) onText(text); }}
        />
        <span className="bt-zaehler">{t("bilder.detail.text.zaehler", { n: text.length, max: GRENZEN.text_max_zeichen })}</span>
        <label className="bt-hinweis" style={{ display: "flex", gap: 6, alignItems: "center" }}>
          <input type="checkbox" checked={e.sidecar?.favorit === true} onChange={(ev) => onFavorit(ev.target.checked)} />
          {t("bilder.detail.favorit.an")}
        </label>
      </div>
    </aside>
  );
}
