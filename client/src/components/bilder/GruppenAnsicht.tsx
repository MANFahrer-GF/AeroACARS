// Liste nach Flug gruppiert (Kopf je Flug, Kacheln, Serien aufklappbar).
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui";
import { datumArt, datumText } from "./logik/datum";
import type { Gruppe } from "./logik/gruppen";
import { bytesText, wurdeGesendet } from "./logik/gruppen";
import { streckeText } from "./logik/metaFormat";
import type { Sendestand } from "./logik/sendezustand";
import { standAusVermerk } from "./logik/sendezustand";
import type { Eintrag } from "./logik/typen";
import { Kachel, SerienKachel } from "./Kacheln";
import type { Kachel as KachelDaten } from "./logik/serien";

export function GruppenAnsicht({
  gruppen, sendestand, auswahl, offenId, aktivePirep, jetzt, onWahl, onSerieWahl, onOeffnen, onErneut, onZuordnen, herkunft,
}: {
  gruppen: Gruppe[];
  sendestand: Sendestand;
  auswahl: ReadonlySet<string>;
  offenId: string | null;
  aktivePirep: string | null;
  jetzt: number;
  onWahl: (id: string) => void;
  onSerieWahl: (ids: string[]) => void;
  onOeffnen: (e: Eintrag) => void;
  onErneut: (id: string) => void;
  onZuordnen: (g: Gruppe) => void;
  herkunft: (g: Gruppe) => string | null;
}) {
  const { t, i18n } = useTranslation();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "de";
  const [offeneSerien, setOffeneSerien] = useState<ReadonlySet<string>>(new Set());
  const umschalten = (k: string) =>
    setOffeneSerien((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });

  const einzel = (e: Eintrag) => (
    <Kachel
      key={e.id}
      e={e}
      stand={sendestand[e.id] ?? standAusVermerk([...(e.sidecar?.sendungen ?? [])].reverse()[0])}
      gewaehlt={auswahl.has(e.id)}
      offen={offenId === e.id}
      onWahl={() => onWahl(e.id)}
      onOeffnen={() => onOeffnen(e)}
      onErneut={() => onErneut(e.id)}
    />
  );

  const kachel = (k: KachelDaten) => {
    if (k.art === "bild") return einzel(k.bild);
    if (!offeneSerien.has(k.schluessel)) return <SerienKachel key={k.schluessel} bilder={k.bilder} auswahl={auswahl} onOeffnen={() => umschalten(k.schluessel)} onWahl={() => onSerieWahl(k.bilder.map((b) => b.id))} />;
    return (
      <div key={k.schluessel} className="bt-serie__offen">
        <div className="bt-serie__kopf">
          <span>{t("bilder.kachel.serie", { n: k.bilder.length })}</span>
          <Button size="sm" variant="quiet" onClick={() => umschalten(k.schluessel)}>{t("bilder.kachel.serie_zu")}</Button>
        </div>
        <div className="bt-kacheln">{k.bilder.map(einzel)}</div>
      </div>
    );
  };

  return (
    <>
      {gruppen.map((g) => {
        const ohne = g.schluessel === "";
        const d = datumArt(g.zeit, jetzt);
        const datum = datumText(g.zeit, sprache);
        const titel = ohne
          ? t("bilder.gruppe.ohne_flug", { datum })
          : d.art === "heute" ? t("bilder.gruppe.heute", { datum }) : d.art === "gestern" ? t("bilder.gruppe.gestern", { datum }) : datum;
        const gesendet = g.kacheln.flatMap((k) => (k.art === "bild" ? [k.bild] : k.bilder)).filter(wurdeGesendet).length;
        const h = ohne ? herkunft(g) : null;
        return (
          <article className="bt-gruppe" key={g.schluessel || "ohne"} aria-label={titel}>
            <div className="bt-gruppe__kopf">
              <span className="bt-gruppe__datum">{titel}</span>
              {g.rufzeichen && <span className="bt-acars" style={{ color: "var(--text)" }}>{g.rufzeichen}</span>}
              {(g.abflug || g.ziel) && <span>{streckeText(g)}</span>}
              {ohne && <span>{h ? t("bilder.gruppe.herkunft", { ordner: h }) : t("bilder.gruppe.hinzugefuegt")}</span>}
              {g.flugzeug && <span>{g.flugzeug}</span>}
              <span style={{ color: "var(--tick)" }} aria-hidden="true">·</span>
              <span>{g.anzahl === 1 ? t("bilder.gruppe.bild_eins") : t("bilder.gruppe.bilder", { count: g.anzahl })}</span>
              <span title={t("bilder.kopf.platz", { platz: bytesText(g.belegt_bytes, sprache) })}>{bytesText(g.belegt_bytes, sprache)}</span>
              {g.schluessel !== "" && g.schluessel === aktivePirep && <span className="bt-chip bt-chip--ok">{t("bilder.gruppe.laeuft")}</span>}
              {gesendet > 0 && <span className="bt-chip">{t("bilder.gruppe.gesendet_n", { count: gesendet })}</span>}
              {ohne && <button type="button" className="bt-gruppe__zuordnen" onClick={() => onZuordnen(g)}>{t("bilder.gruppe.zuordnen")}</button>}
            </div>
            <div className="bt-kacheln">{g.kacheln.map(kachel)}</div>
          </article>
        );
      })}
    </>
  );
}
