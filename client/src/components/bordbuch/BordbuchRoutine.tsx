// Bordbuch — Variante C: „Deine Routine" im Logbuch. Serien und kleine
// Meilensteine über die letzten Flüge, nur für den Piloten sichtbar —
// keine Rangliste, kein Vergleich (Konzept 26.09.2026).

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../../lib/ipc";
import {
  meilensteine,
  routine,
  schalterAn,
  SCHALTER,
  type Einstellungen,
  type Eintrag,
  type Schalter,
  type Zelle,
} from "../../lib/bordbuch";
import "./bordbuch.css";

const ANZAHL = 12;

export function BordbuchRoutine() {
  const [eintraege, setEintraege] = useState<Eintrag[] | null>(null);
  const [einst, setEinst] = useState<Einstellungen | null>(null);

  useEffect(() => {
    let aktiv = true;
    void Promise.all([
      invoke<Eintrag[]>("bordbuch_liste"),
      invoke<Einstellungen>("bordbuch_einstellungen_holen"),
    ])
      .then(([l, e]) => {
        if (!aktiv) return;
        setEintraege(l);
        setEinst(e);
      })
      .catch(() => {
        if (aktiv) setEintraege([]);
      });
    return () => {
      aktiv = false;
    };
  }, []);

  const eingeschaltet = useMemo(
    () => (einst ? SCHALTER.filter((s) => schalterAn(einst, s)) : []),
    [einst],
  );
  return <BordbuchRoutineAnzeige eintraege={eintraege} eingeschaltet={eingeschaltet} />;
}

/** Reine Anzeige — ohne Laden, damit Vorschau und Tests sie direkt nutzen. */
export function BordbuchRoutineAnzeige({
  eintraege,
  eingeschaltet,
}: {
  eintraege: Eintrag[] | null;
  eingeschaltet: Schalter[];
}) {
  const { t } = useTranslation();
  const zeilen = useMemo(
    () => (eintraege ? routine(eintraege, eingeschaltet, ANZAHL) : []),
    [eintraege, eingeschaltet],
  );
  const ms = useMemo(() => meilensteine(zeilen), [zeilen]);
  const spalten = zeilen[0]?.zellen.length ?? 0;

  if (!eintraege || zeilen.length === 0) {
    return (
      <div className="bb-routine">
        <div className="bb-routine-kopf">
          <span className="bb-routine-titel">{t("bordbuch.routine.titel_kurz")}</span>
          <span className="bb-routine-privat">{t("bordbuch.routine.privat")}</span>
        </div>
        <div className="bb-leer">{eintraege ? t("bordbuch.routine.leer") : "…"}</div>
      </div>
    );
  }

  return (
    <div className="bb-routine">
      <div className="bb-routine-kopf">
        <span className="bb-routine-titel">{t("bordbuch.routine.titel", { count: spalten })}</span>
        <span className="bb-routine-privat">{t("bordbuch.routine.privat")}</span>
      </div>
      <div className="bb-routine-raster" style={{ gridTemplateColumns: `minmax(90px, 140px) repeat(${spalten}, minmax(14px, 1fr)) minmax(70px, auto)` }}>
        {zeilen.map((z) => (
          <div key={z.schalter} className="bb-routine-zeile" role="row">
            <span className="bb-routine-name">{t(`bordbuch.schalter.${z.schalter}`)}</span>
            {z.zellen.map((c, i) => (
              <span key={i} className={`bb-zelle bb-zelle--${c}`} title={t(`bordbuch.zelle.${c}`)} aria-label={t(`bordbuch.zelle.${c}`)} />
            ))}
            <span className="bb-routine-serie">
              {z.serie >= 2 ? t("bordbuch.routine.in_folge", { count: z.serie }) : ""}
            </span>
          </div>
        ))}
      </div>
      <div className="bb-routine-legende">
        {(["ok", "atc", "ohne", "nm"] as Zelle[]).map((c) => (
          <span key={c} className="bb-leg-routine">
            <span className={`bb-zelle bb-zelle--${c}`} aria-hidden /> {t(`bordbuch.zelle.${c}`)}
          </span>
        ))}
      </div>
      {ms.length > 0 && (
        <ul className="bb-meilensteine">
          {ms.map((m) => (
            <li key={`${m.art}-${m.schalter}`}>
              {m.art === "rekord"
                ? t("bordbuch.routine.rekord", { was: t(`bordbuch.schalter.${m.schalter}`), count: m.serie })
                : t("bordbuch.routine.zieht_an", { was: t(`bordbuch.schalter.${m.schalter}`), von5: m.von5 })}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
