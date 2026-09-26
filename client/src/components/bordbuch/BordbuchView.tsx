// Bordbuch (26.09.2026) — eigener Reiter neben „Landung".
//
// Oben der laufende Flug (Haken erscheinen, sobald erledigt), darunter die
// vergangenen Flüge. Ein Flug zeigt Variante A (Checkliste) oder B
// (Flugprofil). Kein Rot, keine Punkte, kein Einfluss auf die Bewertung —
// die Seite bestätigt, was geklappt hat.

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import {
  bilanz,
  type Eintrag,
  type LiveAnsicht,
  type Regel,
} from "../../lib/bordbuch";
import { BordbuchCheckliste } from "./BordbuchCheckliste";
import { BordbuchProfil } from "./BordbuchProfil";
import "./bordbuch.css";

type Variante = "checkliste" | "profil";
const VARIANTE_KEY = "aeroacars.bordbuch.variante";

function gespeicherteVariante(): Variante {
  try {
    return localStorage.getItem(VARIANTE_KEY) === "profil" ? "profil" : "checkliste";
  } catch {
    return "checkliste";
  }
}

export function BordbuchView() {
  const { t, i18n } = useTranslation();
  const [liste, setListe] = useState<Eintrag[]>([]);
  const [live, setLive] = useState<LiveAnsicht | null>(null);
  const [gewaehlt, setGewaehlt] = useState<Eintrag | null>(null);
  const [laedt, setLaedt] = useState(true);
  const [variante, setVariante] = useState<Variante>(gespeicherteVariante);

  const waehleVariante = (v: Variante) => {
    setVariante(v);
    try {
      localStorage.setItem(VARIANTE_KEY, v);
    } catch {
      /* egal */
    }
  };

  const neuLaden = useCallback(async () => {
    try {
      const [l, lv] = await Promise.all([
        invoke<Eintrag[]>("bordbuch_liste"),
        invoke<LiveAnsicht | null>("bordbuch_live"),
      ]);
      setListe(l);
      setLive(lv);
    } catch (e) {
      console.warn("bordbuch laden", e);
    } finally {
      setLaedt(false);
    }
  }, []);

  useEffect(() => {
    void neuLaden();
    const id = window.setInterval(() => void neuLaden(), 4000);
    let aktiv = true;
    let weg: (() => void) | undefined;
    void listen("bordbuch_geaendert", () => void neuLaden())
      .then((u) => {
        if (aktiv) weg = u;
        else u();
      })
      .catch(() => undefined);
    return () => {
      aktiv = false;
      window.clearInterval(id);
      weg?.();
    };
  }, [neuLaden]);

  const oeffnen = async (pirepId: string) => {
    try {
      const e = await invoke<Eintrag | null>("bordbuch_eintrag", { pirepId });
      if (e) setGewaehlt(e);
    } catch (err) {
      console.warn("bordbuch_eintrag", err);
    }
  };

  const markieren = async (regel: Regel, nachAtc: boolean) => {
    if (!gewaehlt) return;
    const neu = await invoke<Eintrag | null>("bordbuch_markieren", {
      pirepId: gewaehlt.pirep_id,
      regel,
      nachAtc,
    });
    if (neu) setGewaehlt(neu);
    void neuLaden();
  };

  const datum = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { day: "2-digit", month: "2-digit", year: "2-digit" }),
    [i18n.language],
  );

  if (gewaehlt) {
    const b = bilanz(gewaehlt.punkte, gewaehlt.eingeschaltet);
    return (
      <section className="bb-seite">
        <button type="button" className="bb-zurueck" onClick={() => setGewaehlt(null)}>
          {t("bordbuch.zurueck")}
        </button>
        <header className="bb-kopf">
          <div>
            <h2>
              {gewaehlt.flug.callsign ?? t("bordbuch.flug")}{" "}
              <span className="bb-route">
                {gewaehlt.flug.dep ?? "?"}–{gewaehlt.flug.arr ?? "?"}
              </span>
            </h2>
            <div className="bb-unter">
              {datum.format(new Date(gewaehlt.erstellt_at))}
              {gewaehlt.flug.muster ? ` · ${gewaehlt.flug.muster}` : ""}
              {` · ${t(`bordbuch.klasse.${gewaehlt.klasse}`)}`}
              {` · ${gewaehlt.regelwerk.toUpperCase()}`}
              {gewaehlt.nacht_start ? ` · ${t("bordbuch.nachts")}` : ""}
            </div>
          </div>
          {!gewaehlt.aus_grund && <Bilanz ok={b.ok} von={b.von} />}
        </header>
        {gewaehlt.aus_grund ? (
          <AusHinweis grund={gewaehlt.aus_grund} />
        ) : (
          <>
            <VariantenWahl variante={variante} onWahl={waehleVariante} />
            {variante === "checkliste" ? (
              <BordbuchCheckliste
                punkte={gewaehlt.punkte}
                eingeschaltet={gewaehlt.eingeschaltet}
                flugzeug={gewaehlt.flug.titel ?? gewaehlt.flug.muster ?? null}
                onMarkieren={markieren}
              />
            ) : (
              <BordbuchProfil
                punkte={gewaehlt.punkte}
                eingeschaltet={gewaehlt.eingeschaltet}
                profil={gewaehlt.profil}
                dep={gewaehlt.flug.dep}
                arr={gewaehlt.flug.arr}
              />
            )}
            <p className="bb-fuss">{t("bordbuch.fuss_privat")}</p>
          </>
        )}
      </section>
    );
  }

  return (
    <section className="bb-seite">
      <header className="bb-kopf">
        <div>
          <h2>{t("bordbuch.titel")}</h2>
          <div className="bb-unter">{t("bordbuch.untertitel")}</div>
        </div>
      </header>

      {live && (
        <div className="bb-live">
          <div className="bb-live-kopf">
            <span className="bb-live-titel">{t("bordbuch.laufender_flug")}</span>
            {!live.aus_grund && (() => {
              const b = bilanz(live.punkte, live.eingeschaltet);
              return b.von > 0 ? <Bilanz ok={b.ok} von={b.von} klein /> : null;
            })()}
          </div>
          {live.aus_grund ? (
            <AusHinweis grund={live.aus_grund} />
          ) : (
            <>
              <VariantenWahl variante={variante} onWahl={waehleVariante} />
              {variante === "checkliste" ? (
                <BordbuchCheckliste punkte={live.punkte} eingeschaltet={live.eingeschaltet} flugzeug={null} />
              ) : (
                <BordbuchProfil punkte={live.punkte} eingeschaltet={live.eingeschaltet} profil={live.profil} />
              )}
            </>
          )}
        </div>
      )}

      {!laedt && liste.length === 0 && !live && <div className="bb-leer">{t("bordbuch.leer")}</div>}

      {liste.length > 0 && (
        <ul className="bb-liste">
          {liste.map((e) => {
            const b = bilanz(e.punkte, e.eingeschaltet);
            return (
              <li key={e.pirep_id}>
                <button type="button" className="bb-listen-zeile" onClick={() => void oeffnen(e.pirep_id)}>
                  <span className="bb-l-datum">{datum.format(new Date(e.erstellt_at))}</span>
                  <span className="bb-l-flug">
                    {e.flug.callsign ?? "—"} <span className="bb-route">{e.flug.dep ?? "?"}–{e.flug.arr ?? "?"}</span>
                  </span>
                  <span className="bb-l-muster">{e.flug.muster ?? ""}</span>
                  <span className="bb-l-bilanz">
                    {e.aus_grund
                      ? t(`bordbuch.aus_kurz.${e.aus_grund}`)
                      : b.von > 0
                        ? t("bordbuch.bilanz_kurz", { ok: b.ok, von: b.von })
                        : "—"}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function Bilanz({ ok, von, klein }: { ok: number; von: number; klein?: boolean }) {
  const { t } = useTranslation();
  const alles = von > 0 && ok === von;
  return (
    <div className={`bb-bilanz${klein ? " bb-bilanz--klein" : ""}${alles ? " ist-alles" : ""}`}>
      <span className="bb-bilanz-zahl">{ok}</span>
      <span className="bb-bilanz-text">
        {t("bordbuch.bilanz", { ok, von })}
        {!klein && <span className="bb-bilanz-satz">{alles ? t("bordbuch.satz_alles") : t("bordbuch.satz_teil")}</span>}
      </span>
    </div>
  );
}

function VariantenWahl({ variante, onWahl }: { variante: Variante; onWahl: (v: Variante) => void }) {
  const { t } = useTranslation();
  return (
    <div className="bb-varianten" role="tablist">
      {(["checkliste", "profil"] as Variante[]).map((v) => (
        <button
          key={v}
          type="button"
          role="tab"
          aria-selected={variante === v}
          className={`bb-variante${variante === v ? " ist-aktiv" : ""}`}
          onClick={() => onWahl(v)}
        >
          {t(`bordbuch.variante.${v}`)}
        </button>
      ))}
    </div>
  );
}

function AusHinweis({ grund }: { grund: string }) {
  const { t } = useTranslation();
  return <div className="bb-aus-hinweis">{t(`bordbuch.aus.${grund}`, t("bordbuch.aus.ga"))}</div>;
}
