// Erster Start (Entwurf Leer.dc.html): Einschalten mit Datenschutz-Hinweis und die
// Anleitung, wie Bilder hierher kommen. Dieselbe Karte erscheint als Dialog, wenn
// jemand ohne Einschalten senden will.
import { Fragment, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui";
import { Symbol } from "./Symbole";

const MARKE = "\u0001";

/** Text mit {{taste}}: an der Stelle stehen die Tasten als Tastenkappen. */
function MitTasten({ schluessel, tasten }: { schluessel: string; tasten: string[] }) {
  const { t } = useTranslation();
  const teile = t(schluessel, { taste: MARKE }).split(MARKE);
  return (
    <span>
      {teile[0]}
      {teile.length > 1 && tasten.map((k, i) => (
        <Fragment key={i}>{i > 0 && " + "}<span className="bt-taste">{k}</span></Fragment>
      ))}
      {teile.slice(1).join("")}
    </span>
  );
}

export function EinschaltenKarte({ onEinschalten, onSpaeter }: { onEinschalten: () => void; onSpaeter?: () => void }) {
  const { t } = useTranslation();
  const punkt = (k: string): ReactNode => <div className="bt-ok">{Symbol.haken}<span>{t(`bilder.erststart.karte1.${k}`)}</span></div>;
  return (
    <section className="bt-karte-box" aria-labelledby="bt-h-einschalten">
      <div className="bt-erst__kopf">
        <span className="bt-erst__symbol" aria-hidden="true">{Symbol.kamera}</span>
        <h2 id="bt-h-einschalten">{t("bilder.erststart.karte1.titel")}</h2>
      </div>
      <p>{t("bilder.erststart.karte1.text")}</p>
      <div className="bt-erst__punkte">{punkt("punkt1")}{punkt("punkt2")}{punkt("punkt3")}</div>
      <div className="bt-erst__knoepfe">
        <Button variant="primary" onClick={onEinschalten}>{t("bilder.erststart.karte1.einschalten")}</Button>
        {onSpaeter && <Button variant="quiet" onClick={onSpaeter}>{t("bilder.erststart.karte1.spaeter")}</Button>}
      </div>
    </section>
  );
}

export function WegeKarte({ xplaneTaste, msfsTaste, altbestand, onAlt, arbeitet }: { xplaneTaste: string | null; msfsTaste: string | null; altbestand: number; onAlt: () => void; arbeitet: boolean }) {
  const { t } = useTranslation();
  const schritt = (n: number, inhalt: ReactNode) => <div className="bt-schritt"><span className="bt-schritt__n">{n}</span>{inhalt}</div>;
  const tasten = (z: string | null) => (z ?? "").split("+").map((x) => x.trim()).filter(Boolean);
  return (
    <section className="bt-karte-box" aria-labelledby="bt-h-wie">
      <h2 id="bt-h-wie">{t("bilder.erststart.karte2.titel")}</h2>
      <div className="bt-erst__spalten">
        <div className="bt-erst__spalte">
          <span className="bt-label">{t("bilder.erststart.karte2.xplane")}</span>
          {schritt(1, xplaneTaste ? <MitTasten schluessel="bilder.erststart.karte2.xplane1" tasten={tasten(xplaneTaste)} /> : <span>{t("bilder.erststart.karte2.xplane1_neutral")}</span>)}
          {schritt(2, <span>{t("bilder.erststart.karte2.xplane2")}</span>)}
          {schritt(3, <span>{t("bilder.erststart.karte2.xplane3")}</span>)}
        </div>
        <div className="bt-erst__spalte">
          <span className="bt-label">{t("bilder.erststart.karte2.msfs")}</span>
          {schritt(1, msfsTaste ? <MitTasten schluessel="bilder.erststart.karte2.msfs1" tasten={tasten(msfsTaste)} /> : <span>{t("bilder.erststart.karte2.msfs1_neutral")}</span>)}
          {schritt(2, <span>{t("bilder.erststart.karte2.msfs2")}</span>)}
          {schritt(3, <span>{t("bilder.erststart.karte2.msfs3")}</span>)}
        </div>
      </div>
      <div className="bt-hinweis">{t("bilder.erststart.karte2.eigene_werkzeuge")}</div>
      <div className="bt-ziehen">{Symbol.hochladen}<span>{t("bilder.erststart.karte2.ziehen")}</span></div>
      {altbestand > 0 && (
        <div className="bt-alt">
          <div className="bt-alt__text">
            <span>{t("bilder.altbestand.titel", { n: altbestand })}</span>
            <span>{t("bilder.altbestand.text")}</span>
          </div>
          <Button onClick={onAlt} disabled={arbeitet}>{arbeitet ? t("bilder.altbestand.laeuft") : t("bilder.altbestand.uebernehmen")}</Button>
        </div>
      )}
    </section>
  );
}
