// Bildquellen (P-11.30): Status „X-Plane gefunden, beobachtet: …“, Ordner hinzufuegen,
// pausieren, entfernen, „Ordner pruefen“ (Vorschlaege aus bild_quellen_finden), Tastenkuerzel.
// Jede Aenderung bestaetigt die Huelle (Systembefehl bild_quellen_setzen).
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui";
import type { BilderStore } from "./logik/store";
import type { BilderListe, Fundort } from "./logik/typen";

export function QuellenPanel({ liste, vorschlaege, store, onZu, simulator, yokeLernt }: { liste: BilderListe; vorschlaege: Fundort[] | null; store: BilderStore; onZu: () => void; simulator: "xplane" | "msfs" | null; yokeLernt: boolean }) {
  const { t } = useTranslation();
  const [kuerzel, setKuerzel] = useState(liste.tastenkuerzel?.text ?? "");
  useEffect(() => { setKuerzel(liste.tastenkuerzel?.text ?? ""); }, [liste.tastenkuerzel?.text]);
  const xplane = liste.quellen.filter((q) => q.art === "standard");
  const beobachtet = xplane.filter((q) => q.beobachtet && q.aktiv);
  const eingerichtet = new Set(liste.quellen.map((q) => q.pfad));
  const zustand = (q: (typeof liste.quellen)[number]) =>
    q.fehler ? t(`bilder.quellen.zustand.${q.fehler}`, { defaultValue: q.fehler })
      : !q.aktiv ? t("bilder.quellen.zustand.pausiert")
      : q.wird_geprueft ? t("bilder.quellen.zustand.wird_geprueft")
      : q.beobachtet ? t("bilder.quellen.zustand.beobachtet") : "";
  return (
    <section className="bt-quellen" aria-label={t("bilder.quellen.titel")}>
      <div className="bt-quellen__kopf">
        <h2>{t("bilder.quellen.titel")}</h2>
        <Button size="sm" onClick={() => void store.ordnerWaehlen()}>{t("bilder.quellen.hinzufuegen")}</Button>
        <Button size="sm" onClick={() => void store.quellenFinden()} title={t("bilder.quellen.pruefen_hinweis")}>{t("bilder.quellen.pruefen")}</Button>
        <Button size="sm" variant="quiet" onClick={onZu} style={{ marginLeft: "auto" }}>{t("bilder.quellen.zu")}</Button>
      </div>
      <div className="bt-hinweis">{t("bilder.quellen.bestaetigung")}</div>
      <div className="bt-hinweis">{t("bilder.quellen.eigene_werkzeuge")}</div>
      <div role="status" className="bt-notiz">
        {xplane.length > 0
          ? t(beobachtet.length > 0 ? "bilder.quellen.xplane_gefunden" : "bilder.quellen.xplane_nicht_beobachtet", { pfade: (beobachtet.length > 0 ? beobachtet : xplane).map((q) => q.pfad).join(", ") })
          : liste.quellen.length === 0 ? t("bilder.quellen.keine") : null}
      </div>
      {liste.quellen.map((q) => (
        <div className="bt-quellen__zeile" key={q.pfad}>
          <span className="bt-quellen__pfad">{q.pfad}</span>
          <div className="bt-quellen__knoepfe">
            <Button size="sm" onClick={() => void store.quelleUmschalten(q.pfad, { aktiv: !q.aktiv })}>{q.aktiv ? t("bilder.quellen.pausieren") : t("bilder.quellen.fortsetzen")}</Button>
            <Button size="sm" variant="danger" onClick={() => void store.quelleEntfernen(q.pfad)}>{t("bilder.quellen.entfernen")}</Button>
          </div>
          <span className={`bt-quellen__zusatz${q.fehler ? " bt-quellen__zusatz--fehler" : ""}`}>
            {[t(`bilder.quellen.art.${q.art ?? "hand"}`, { defaultValue: q.art ?? "" }), zustand(q)].filter(Boolean).join(" · ")}
          </span>
          {q.art !== "standard" && (
            <label className="bt-quellen__opt">
              <input type="checkbox" checked={q.original_entfernen} onChange={(ev) => void store.quelleUmschalten(q.pfad, { original_entfernen: ev.target.checked })} />
              {t("bilder.quellen.original_entfernen")}
            </label>
          )}
        </div>
      ))}
      {vorschlaege && (
        <div className="bt-abschnitt">
          <span className="bt-label">{t("bilder.quellen.vorschlaege")}</span>
          {vorschlaege.length === 0 && <span className="bt-hinweis">{t("bilder.quellen.keine_vorschlaege")}</span>}
          {vorschlaege.map((f) => (
            <div className="bt-quellen__zeile" key={f.pfad}>
              <span className="bt-quellen__pfad">{f.pfad}</span>
              <div className="bt-quellen__knoepfe">
                {eingerichtet.has(f.pfad)
                  ? <span className="bt-quellen__zusatz">{t("bilder.quellen.schon")}</span>
                  : <Button size="sm" variant="primary" disabled={!f.existiert} onClick={() => void store.quelleVorschlagen(f)}>{t("bilder.quellen.uebernehmen")}</Button>}
              </div>
              <span className="bt-quellen__zusatz">{[f.simulator, f.existiert ? null : t("bilder.quellen.fehlt")].filter(Boolean).join(" · ")}</span>
            </div>
          ))}
        </div>
      )}
      {liste.tastenkuerzel?.yoke_verfuegbar && (
        <div className="bt-quellen__kuerzel">
          <span>{t("bilder.quellen.yoke.titel")}:</span>
          <span className="bt-acars">
            {liste.tastenkuerzel.yoke
              ? t("bilder.quellen.yoke.belegt", { geraet: liste.tastenkuerzel.yoke.geraet_name || `${liste.tastenkuerzel.yoke.hersteller_id}:${liste.tastenkuerzel.yoke.produkt_id}`, knopf: liste.tastenkuerzel.yoke.knopf })
              : t("bilder.quellen.yoke.unbelegt")}
          </span>
          <Button size="sm" disabled={yokeLernt} onClick={() => void store.yokeFestlegen()}>{yokeLernt ? t("bilder.quellen.yoke.lernt") : t("bilder.quellen.yoke.festlegen")}</Button>
          {liste.tastenkuerzel.yoke && <Button size="sm" variant="danger" disabled={yokeLernt} onClick={() => void store.yokeEntfernen()}>{t("bilder.quellen.yoke.entfernen")}</Button>}
          <span role="status" className="bt-hinweis">{yokeLernt ? t("bilder.quellen.yoke.hinweis_lernen") : ""}</span>
        </div>
      )}
      {simulator === "xplane" && <div className="bt-hinweis">{t("bilder.quellen.yoke.xplane_hinweis")}</div>}
      {liste.tastenkuerzel?.verfuegbar && (
        <div className="bt-quellen__kuerzel">
          <label htmlFor="bt-kuerzel">{t("bilder.quellen.kuerzel")}</label>
          <input id="bt-kuerzel" value={kuerzel} onChange={(ev) => setKuerzel(ev.target.value)} />
          <Button size="sm" disabled={kuerzel === liste.tastenkuerzel.text || kuerzel.trim() === ""} onClick={() => void store.tastenkuerzelSetzen(kuerzel.trim())}>{t("bilder.quellen.kuerzel_setzen")}</Button>
          {liste.tastenkuerzel.fehler && <span className="bt-quellen__zusatz--fehler">{t("bilder.quellen.kuerzel_fehler", { fehler: liste.tastenkuerzel.fehler })}</span>}
        </div>
      )}
    </section>
  );
}
