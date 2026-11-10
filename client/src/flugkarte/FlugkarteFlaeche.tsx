// Flugkarte als Fläche: lädt die Flugakte und setzt den Baustein in ein Element.
// Eine Quelle für das Vollbild-Fenster (Landung, Logbuch) und den Reiter „Karte“ im Logbuch.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import { BildGross } from "../components/bilder/BildGross";
import { bilderStore } from "../components/bilder/bilderStore";
import { bildZeitIso } from "../components/bilder/Kacheln";
import { anzeigbar, flugSchluessel } from "../components/bilder/logik/gruppen";
import { useMarkenVorschau } from "../components/bilder/useMarkenVorschau";
import type { Eintrag } from "../components/bilder/logik/typen";
import { flugkarteAnzeigen, type FlugkarteOptionen, type FlugkarteSteuerung } from "./flugkarte";
import type { Flugakte } from "./flugakteTypen";
import stilUrl from "./stil-dunkel.json?url";
import "./fenster.css";

export type OhneAkte = { art: "fehlt" } | { art: "fehler"; text: string };

interface Props {
  pirepId: string;
  /** Wohin das grosse Bild gehaengt wird (im Fenster: das Fenster selbst) */
  bildZiel?: HTMLElement | null;
  zusatz?: Pick<FlugkarteOptionen, "zusatzEreignisse" | "zusatzKategorien">;
  onBereit?: (api: FlugkarteSteuerung) => void;
  /** Eingebettet: statt einer Meldung dem Aufrufer sagen, dass es keine Karte gibt (er zeigt die Linie) */
  onOhneAkte?: (grund: OhneAkte) => void;
}

export default function FlugkarteFlaeche({ pirepId, bildZiel, zusatz, onBereit, onOhneAkte }: Props) {
  const feld = useRef<HTMLDivElement>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [laedt, setLaedt] = useState(true);
  const [gross, setGross] = useState<{ id: string; bilder: Eintrag[] } | null>(null);
  const { t, i18n } = useTranslation();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "en";
  const vorschauVon = useMarkenVorschau();
  // Nur der Stand beim Aufbau zaehlt; ein Neuaufbau je Haken wuerde die Wiedergabe zuruecksetzen
  const aussen = useRef({ zusatz, onBereit, onOhneAkte });
  aussen.current = { zusatz, onBereit, onOhneAkte };

  useEffect(() => {
    let weg: (() => void) | null = null, abgebrochen = false;
    setFehler(null); setLaedt(true);
    // laden() faengt Fehler selbst; die Karte wartet nicht auf die Liste, die Punkte kommen nach
    const listeDa = bilderStore.laden();
    const melden = (grund: OhneAkte, text: string) => {
      if (aussen.current.onOhneAkte) aussen.current.onOhneAkte(grund);
      else setFehler(text);
    };
    invoke<string | null>("flugakte_laden", { pirepId })
      .then((text) => {
        if (abgebrochen || !feld.current) return;
        if (text == null) { melden({ art: "fehlt" }, t("flugkarte.fenster.kein_protokoll")); return; }
        const akte = JSON.parse(text) as Flugakte;
        if (aussen.current.onOhneAkte && !akte.track?.length) { aussen.current.onOhneAkte({ art: "fehlt" }); return; }
        const start = akte.kopf?.start ? new Date(akte.kopf.start) : null;
        let flugBilder: Eintrag[] = [];
        weg = flugkarteAnzeigen(feld.current, akte, {
          stilUrl,
          // Texte aus common.json (flugkarte.*), dieselben wie in der Webapp; fehlt einer, bleibt der deutsche aus der Akte
          t: (k, w) => (i18n.exists(k) ? i18n.t(k, w) : null),
          sprache,
          datum: start ? start.toLocaleDateString(sprache, { timeZone: "UTC" }) : undefined,
          bilder: listeDa.then(() => {
            flugBilder = (bilderStore.lesen().liste?.eintraege ?? [])
              .filter((e) => anzeigbar(e) && flugSchluessel(e) === pirepId)
              .sort((a, b) => (bildZeitIso(a) ?? "").localeCompare(bildZeitIso(b) ?? ""));
            return flugBilder
              .filter((e) => typeof e.sidecar?.meta?.breite === "number" && typeof e.sidecar?.meta?.laenge === "number")
              .map((e) => {
                const zeit = e.sidecar!.meta!.zeit_utc?.slice(11, 16);
                return { id: e.id, lat: e.sidecar!.meta!.breite!, lon: e.sidecar!.meta!.laenge!, hoeheFt: e.sidecar!.meta!.hoehe_ft, name: t("bilder.karte.bild_marke", { name: zeit ? `${zeit}Z` : e.id }) };
              });
          }),
          beiBild: (id) => setGross({ id, bilder: flugBilder }),
          bildVorschau: (id) => vorschauVon(flugBilder.find((e) => e.id === id)),
          // nur im Entwicklungsmodus: Karte fuer Browser-Tests greifbar (wie in der Webapp)
          beiKarte: import.meta.env.DEV ? (karte) => { (window as unknown as { __flugkarte: unknown }).__flugkarte = karte; } : undefined,
          ...aussen.current.zusatz,
          beiBereit: (api) => aussen.current.onBereit?.(api),
        });
      })
      .catch((e: unknown) => {
        if (abgebrochen) return;
        const f = e as { code?: string; message?: string };
        const text = f?.code === "flugakte_kein_zugang"
          ? t("flugkarte.fenster.kein_zugang")
          : t("flugkarte.fenster.fehler", { fehler: f?.message ?? String(e) });
        melden({ art: "fehler", text }, text);
      })
      .finally(() => { if (!abgebrochen) setLaedt(false); });
    return () => { abgebrochen = true; weg?.(); };
  }, [pirepId, sprache]); // neue Sprache: neu aufbauen (der Baustein holt seine Texte einmal)

  return (
    <>
      {laedt && <div className="flugkarte-hinweis">{t("flugkarte.fenster.laedt")}</div>}
      {fehler && <div className="flugkarte-hinweis">{fehler}</div>}
      <div className="flugkarte-inhalt" ref={feld} />
      {gross && <BildGross bilder={gross.bilder} startId={gross.id} onZu={() => setGross(null)} ziel={bildZiel} />}
    </>
  );
}
