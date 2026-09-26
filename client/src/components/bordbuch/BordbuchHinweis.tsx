// Bordbuch — leiser Hinweis im Flug. Nur an Schlüsselstellen, als Frage
// („Strobes?"), solange es noch Sinn hat, verschwindet nach wenigen
// Sekunden und ist abschaltbar (Einstellungen → Bordbuch). Dazu ein kurzer
// grüner Haken, wenn ein Pflichtpunkt erledigt ist.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import { sichtbar, type LiveAnsicht, type Regel } from "../../lib/bordbuch";
import { Haken } from "./Symbole";
import "./bordbuch.css";

const TAKT_MS = 2000;
const HAKEN_MS = 4000;

export function BordbuchHinweis({ aktiv }: { aktiv: boolean }) {
  const { t } = useTranslation();
  const [frage, setFrage] = useState<Regel | null>(null);
  const [haken, setHaken] = useState<Regel | null>(null);
  const erledigtRef = useRef<Set<Regel> | null>(null);
  const hakenBisRef = useRef(0);

  useEffect(() => {
    if (!aktiv) {
      setFrage(null);
      erledigtRef.current = null;
      return;
    }
    let lebt = true;
    const abfragen = async () => {
      let lv: LiveAnsicht | null = null;
      try {
        lv = await invoke<LiveAnsicht | null>("bordbuch_live");
      } catch {
        return;
      }
      if (!lebt) return;
      if (!lv || lv.aus_grund) {
        setFrage(null);
        return;
      }
      const hinweiseAn = lv.hinweise_im_flug;
      setFrage(lv.hinweis?.regel ?? null);
      const jetztErledigt = new Set(
        lv.punkte
          .filter((p) => p.art === "pflicht" && sichtbar(p, lv!.eingeschaltet) && (p.status === "erledigt"))
          .map((p) => p.regel),
      );
      const vorher = erledigtRef.current;
      erledigtRef.current = jetztErledigt;
      if (vorher && hinweiseAn) {
        const neu = [...jetztErledigt].find((r) => !vorher.has(r));
        if (neu) {
          setHaken(neu);
          hakenBisRef.current = Date.now() + HAKEN_MS;
        }
      }
      if (hakenBisRef.current && Date.now() > hakenBisRef.current) {
        setHaken(null);
        hakenBisRef.current = 0;
      }
    };
    void abfragen();
    const id = window.setInterval(() => void abfragen(), TAKT_MS);
    return () => {
      lebt = false;
      window.clearInterval(id);
    };
  }, [aktiv]);

  if (!frage && !haken) return null;
  return (
    <div className="bb-hinweis-leiste" role="status" aria-live="polite">
      {frage ? (
        <span className="bb-hinweis-frage">
          <span className="bb-hinweis-marke">{t("bordbuch.titel")}</span>
          {t(`bordbuch.frage.${frage}`)}
        </span>
      ) : haken ? (
        <span className="bb-hinweis-haken">
          <Haken /> {t(`bordbuch.kurz.${haken}`)}
        </span>
      ) : null}
    </div>
  );
}
