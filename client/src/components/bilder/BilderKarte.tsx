// Kartenausschnitt in der Detailspalte (P-11.36): Spur des Flugs, Bildposition und
// Bildmarken desselben Flugs, Umschalter Karte/Weltkugel. Ein Klick auf die Karte
// oeffnet den Karte-Tab an dieser Stelle.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import { bildKarteAnzeigen, type Ausschnitt, type BildMarke } from "../../flugkarte/ausschnitt";
import stilUrl from "../../flugkarte/stil-dunkel.json?url";
import { bildAktionen } from "./logik/api";
import { spurAusTrack } from "./logik/datum";
import type { Eintrag } from "./logik/typen";

const spurCache = new Map<string, Array<[number, number]> | null>();

/** Spur des Flugs: im aktiven Flug aus `flight_get_track`, sonst aus `bild_kontext`. */
export function useSpur(pirepId: string | null, aktivePirep: string | null): Array<[number, number]> | null {
  const [spur, setSpur] = useState<Array<[number, number]> | null>(null);
  useEffect(() => {
    let weg = false;
    setSpur(null);
    if (!pirepId) return;
    const aktiv = pirepId === aktivePirep;
    if (!aktiv && spurCache.has(pirepId)) {
      setSpur(spurCache.get(pirepId) ?? null);
      return;
    }
    const laden = aktiv
      ? invoke("flight_get_track").then((t) => spurAusTrack(t))
      : bildAktionen.kontext({ pirepId, mitSpur: true }).then((k) => k.spur);
    laden
      .then((s) => {
        const wert = s && s.length > 1 ? s : null;
        if (!aktiv) spurCache.set(pirepId, wert);
        if (!weg) setSpur(wert);
      })
      .catch(() => { if (!weg) setSpur(null); });
    return () => { weg = true; };
  }, [pirepId, aktivePirep]);
  return spur;
}

export function markenFuerFlug(bilder: Eintrag[], aktivId: string | null, name: (e: Eintrag) => string): BildMarke[] {
  return bilder
    .filter((e) => typeof e.sidecar?.meta?.breite === "number" && typeof e.sidecar?.meta?.laenge === "number")
    .map((e) => ({ id: e.id, name: name(e), lat: e.sidecar!.meta!.breite!, lon: e.sidecar!.meta!.laenge!, aktiv: e.id === aktivId }));
}

export function BilderKarte({
  eintrag, flugBilder, aktivePirep, onOeffnen, onMarke,
}: {
  eintrag: Eintrag;
  flugBilder: Eintrag[];
  aktivePirep: string | null;
  onOeffnen: (fokus: { lat: number; lon: number; id: string; pirep: string | null }) => void;
  onMarke: (id: string) => void;
}) {
  const { t } = useTranslation();
  const feld = useRef<HTMLDivElement>(null);
  const karte = useRef<Ausschnitt | null>(null);
  const meta = eintrag.sidecar?.meta;
  const hatOrt = typeof meta?.breite === "number" && typeof meta?.laenge === "number";
  const spur = useSpur(meta?.pirep_id ?? null, aktivePirep);
  const [kugel, setKugel] = useState(false);
  const [kartenFehler, setKartenFehler] = useState(false);
  const beiMarke = useRef(onMarke);
  const beiKlick = useRef<() => void>(() => undefined);
  beiMarke.current = onMarke;
  beiKlick.current = () => {
    if (hatOrt) onOeffnen({ lat: meta!.breite!, lon: meta!.laenge!, id: eintrag.id, pirep: meta?.pirep_id ?? null });
  };

  useEffect(() => {
    if (!feld.current || !hatOrt) return;
    const k = bildKarteAnzeigen(feld.current, {
      stilUrl,
      beiMarke: (id) => beiMarke.current(id),
      beiKlick: () => beiKlick.current(),
      zoom: 8,
      beiFehler: () => setKartenFehler(true),
      beiKarte: import.meta.env.DEV ? (m) => { (window as unknown as { __bilderKarte: unknown }).__bilderKarte = m; } : undefined,
    });
    karte.current = k;
    return () => { k.aufraeumen(); karte.current = null; };
  }, [hatOrt]);

  useEffect(() => {
    if (!hatOrt || !karte.current) return;
    karte.current.aktualisieren({
      spur,
      marken: markenFuerFlug(flugBilder, eintrag.id, (e) => t("bilder.karte.bild_marke", { name: e.sidecar?.meta?.zeit_utc?.slice(11, 16) ?? e.id })),
      fokus: { lat: meta!.breite!, lon: meta!.laenge! },
      kugel,
    });
  }, [hatOrt, spur, flugBilder, eintrag.id, meta, kugel, t]);

  return (
    <div className="bt-abschnitt">
      <div className="bt-abschnitt__kopf">
        <span className="bt-label">{t("bilder.detail.karte.titel")}</span>
        {hatOrt && (
          <button type="button" className="bt-link" onClick={() => beiKlick.current()}>
            {t("bilder.detail.karte.zeigen")}
          </button>
        )}
      </div>
      <div className="bt-karte__feld" aria-label={t("bilder.karte.titel")}>
        <div className="bt-karte__map" ref={feld} />
        {!hatOrt && <div className="bt-karte__hinweis">{t("bilder.karte.ohne_position")}</div>}
        {hatOrt && kartenFehler && <div className="bt-karte__hinweis">{t("bilder.karte.nicht_verfuegbar")}</div>}
        {hatOrt && (
          <div className="bt-karte__umschalter" role="group" aria-label={t("bilder.karte.umschalter")} onClick={(e) => e.stopPropagation()}>
            <button type="button" aria-pressed={!kugel} onClick={() => setKugel(false)}>{t("bilder.karte.karte")}</button>
            <button type="button" aria-pressed={kugel} onClick={() => setKugel(true)}>{t("bilder.karte.kugel")}</button>
          </div>
        )}
      </div>
      <span className="bt-hinweis">© OpenFreeMap, OpenMapTiles, OpenStreetMap contributors</span>
      <span className="bt-hinweis">{hatOrt && !spur && meta?.pirep_id ? t("bilder.karte.keine_spur") + ". " : ""}{t("bilder.detail.karte.hinweis")}</span>
    </div>
  );
}
