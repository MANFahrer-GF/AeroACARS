// Sende-Dialog (Entwurf Senden.dc.html): auf die Fensterhoehe begrenzt, Inhalt rollt,
// Fusszeile bleibt. Vorschau der Nachricht, Text, drei Geo-Stufen mit der Vorgabe
// „verdeckt“. Hinweise nur, wenn etwas nicht geht (Thomas 07.11.).
import { useEffect, useState } from "react";
import { Button, Modal } from "../ui";
import { GRENZEN, TABELLE } from "./logik/grenzen";
import { reichtKontingent } from "./logik/kontingent";
import { simulatorName, streckeText } from "./logik/metaFormat";
import { sendeMeta, vorschauBauen, nurKopf } from "./logik/nachrichtVorschau";
import { schonAufDiscord } from "./logik/sendezustand";
import type { Eintrag, GeoStufe, Kontingent } from "./logik/typen";
import { useKachelTexte } from "./Kacheln";
import { Symbol } from "./Symbole";
import { useBildUrl } from "./useBildUrl";

function Vorschaubild({ e }: { e: Eintrag }) {
  const url = useBildUrl(e.vorschau_pfad ?? e.pfad);
  return (
    <div className="bt-nachricht__bild">
      {url && <img src={url} alt="" />}
    </div>
  );
}

function SendenInhalt({
  offen, bilder, pilotName, startText, geoStufe, kontingent, serverAus, onGeo, onSenden, onZu,
}: {
  offen: boolean;
  bilder: Eintrag[];
  pilotName: string | null;
  startText: string;
  geoStufe: GeoStufe;
  kontingent: Kontingent | null;
  serverAus: boolean;
  onGeo: (s: GeoStufe) => void;
  onSenden: (text: string, stufe: GeoStufe) => void;
  onZu: () => void;
}) {
  const { t, sprache, phase } = useKachelTexte();
  const [text, setText] = useState(startText);
  const [spoilerOffen, setSpoilerOffen] = useState(false);
  useEffect(() => { if (offen) { setText(startText); setSpoilerOffen(false); } }, [offen, startText]);
  const metas = bilder.map((b) => b.sidecar!.meta!);
  // Der Kopf der Vorschau zeigt nur, was die Stufe wirklich mitschickt.
  const m0 = metas[0] && nurKopf(geoStufe) ? sendeMeta(metas[0], geoStufe, "") : metas[0];
  const v = vorschauBauen(pilotName, bilder.map((b) => sendeMeta(b.sidecar!.meta!, geoStufe, phase(b))), geoStufe, text, GRENZEN.nachricht_max_zeichen);
  const schonDa = bilder.filter((b) => schonAufDiscord(b.sidecar?.sendungen)).length;
  const zuViele = bilder.length > GRENZEN.max_je_sendung;
  const reicht = reichtKontingent(bilder.length, kontingent);
  const initialen = (pilotName ?? "").split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]?.toUpperCase()).join("");
  const keinOrt = metas.every((m) => m.breite === null || m.laenge === null);
  const titel = bilder.length === 1 ? t("bilder.senden.titel_eins") : t("bilder.senden.titel_n", { count: bilder.length });
  const stufen: Array<[GeoStufe, string]> = TABELLE.geoStufen.map((s) => [s, t(`bilder.senden.geo.${s === "aus" ? "aus" : s}`)]);
  const datum = !nurKopf(geoStufe) && m0?.zeit_utc ? new Date(m0.zeit_utc).toLocaleDateString(sprache, { timeZone: "UTC", day: "2-digit", month: "2-digit", year: "numeric" }) : "";
  const sim = !nurKopf(geoStufe) && m0?.simulator ? simulatorName(m0.simulator) : "";
  return (
    <Modal
      open={offen}
      onClose={onZu}
      size="lg"
      closeLabel={t("bilder.senden.zu")}
      title={<>{titel} <span style={{ fontSize: "var(--fs-300)", color: "var(--text-dim)", fontWeight: 400, marginLeft: 8 }}>{t("bilder.senden.kanal")}</span></>}
      footer={
        <div className="bt-senden__fuss">
          {(zuViele || !reicht) && (
            <span className="bt-hinweis" data-warn="an">
              {zuViele ? t("bilder.senden.zu_viele", { max: GRENZEN.max_je_sendung }) : t("bilder.senden.kontingent_zu_wenig", { rest: kontingent?.rest ?? 0 })}
            </span>
          )}
          <span style={{ marginLeft: "auto" }} />
          <Button variant="quiet" onClick={onZu}>{t("bilder.senden.abbrechen")}</Button>
          <Button variant="primary" iconLeft={Symbol.senden} disabled={bilder.length === 0 || zuViele || !reicht || serverAus} onClick={() => onSenden(text.trim(), geoStufe)}>
            {t("bilder.senden.jetzt")}
          </Button>
        </div>
      }
    >
      <div className="bt-senden">
        {serverAus && <div className="bt-notiz bt-notiz--fehler" role="status">{t("bilder.senden.server_offline")}</div>}
        {schonDa > 0 && (
          <div className="bt-notiz" role="status">
            {bilder.length === 1 ? t("bilder.senden.schon_gesendet_eins") : t("bilder.senden.schon_gesendet_n", { count: schonDa, gesamt: bilder.length })}
          </div>
        )}
        <div className="bt-senden__vorschau">
          <span className="bt-label">{t("bilder.senden.vorschau.titel")}</span>
          <div className="bt-nachricht">
            <div className="bt-nachricht__kopf">
              <span className="bt-nachricht__avatar" aria-hidden="true">{initialen || "?"}</span>
              <strong>{pilotName ?? "Pilot"}</strong>
              {m0?.rufzeichen && <><span className="bt-band__dim">{"·"}</span><span className="bt-acars">{m0.rufzeichen}</span></>}
              {streckeText(m0) && <span>{streckeText(m0)}</span>}
              <span className="bt-band__dim">{[m0?.flugzeug_titel ?? m0?.flugzeug_icao, sim, datum].filter(Boolean).join(" · ")}</span>
            </div>
            {v.zeilen.map((z, i) => (
              <span key={i} className="bt-geozeile">
                {z.nummer ? `${z.nummer}: ` : ""}{z.info}{z.info && z.ort ? " · " : ""}
                {z.ort && !z.verdeckt && z.ort}
                {z.ort && z.verdeckt && (
                  <button type="button" className="bt-spoiler" data-offen={spoilerOffen ? "an" : undefined} aria-expanded={spoilerOffen} onClick={() => setSpoilerOffen((o) => !o)}>
                    {!spoilerOffen && Symbol.auge}
                    {spoilerOffen ? z.ort : t("bilder.senden.vorschau.verdeckt")}
                  </button>
                )}
              </span>
            ))}
            {v.text && <p className="bt-nachricht__text">{v.text}</p>}
            <div className="bt-nachricht__bilder">{bilder.map((b) => <Vorschaubild key={b.id} e={b} />)}</div>
            {keinOrt && geoStufe !== "aus" && !nurKopf(geoStufe) && <span className="bt-hinweis">{t("bilder.senden.geo.ohne_position")}</span>}
          </div>
        </div>

        <div className="bt-abschnitt">
          <label htmlFor="bt-sendetext" className="bt-label">{bilder.length === 1 ? t("bilder.senden.text_label_eins") : t("bilder.senden.text_label_n", { count: bilder.length })}</label>
          <textarea id="bt-sendetext" className="bt-feld" value={text} maxLength={GRENZEN.text_max_zeichen} placeholder={t("bilder.senden.text.platzhalter")} onChange={(ev) => setText(ev.target.value)} />
          <span className="bt-zaehler">{t("bilder.senden.text.zaehler", { n: text.length, max: GRENZEN.text_max_zeichen })}</span>
          {v.gekuerzt && <span className="bt-hinweis" data-warn="an">{t("bilder.senden.vorschau.gekuerzt")}</span>}
        </div>

        <fieldset className="bt-pillen">
          <legend className="bt-label">{t("bilder.senden.geo.titel")}</legend>
          {stufen.map(([s, name]) => (
            <label key={s} className="bt-pille" data-an={geoStufe === s ? "an" : undefined}>
              <input type="radio" name="bt-geo" checked={geoStufe === s} onChange={() => onGeo(s)} />
              {name}
            </label>
          ))}
          <span className="bt-hinweis" data-geo-erklaerung={geoStufe}>{t(`bilder.senden.geo.${geoStufe}_text`)}</span>
        </fieldset>
      </div>
    </Modal>
  );
}

/** Der Dialog entsteht erst, wenn er offen ist: ohne Auswahl gibt es nichts zu zeigen. */
export function SendenDialog(p: Parameters<typeof SendenInhalt>[0]) {
  return p.offen && p.bilder.length > 0 ? <SendenInhalt {...p} /> : null;
}
