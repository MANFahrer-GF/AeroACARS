// Bordbuch — Variante A: Checkliste nach Flugphasen, im Stil einer
// Papier-Checkliste („BEACON ........ ON"). Offene Punkte bleiben grau und
// unaufgeregt; ein grauer Punkt lässt sich antippen und als „nach
// ATC-Anweisung" markieren (nur nach dem Flug).

import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ABSCHNITTE,
  sichtbar,
  zulu,
  type Punkt,
  type Regel,
  type Schalter,
} from "../../lib/bordbuch";
import { Haken } from "./Symbole";

interface Props {
  punkte: Punkt[];
  eingeschaltet: Schalter[];
  flugzeug: string | null;
  /** Nur nach dem Flug: Punkt als „nach ATC" markieren oder zurück. */
  onMarkieren?: (regel: Regel, nachAtc: boolean) => Promise<void>;
  /** Grenze fürs Rolltempo dieses Flugs — ohne sie stand nur „max 22 kt"
   *  da, und Thomas las die gefahrene Spitze als erlaubtes Tempo. */
  rolltempoGrenzeKt?: number;
}

export function BordbuchCheckliste({ punkte, eingeschaltet, flugzeug, onMarkieren, rolltempoGrenzeKt }: Props) {
  const { t } = useTranslation();
  const [offen, setOffen] = useState<Regel | null>(null);
  const [laeuft, setLaeuft] = useState(false);

  const markieren = async (regel: Regel, nachAtc: boolean) => {
    if (!onMarkieren) return;
    setLaeuft(true);
    try {
      await onMarkieren(regel, nachAtc);
      setOffen(null);
    } finally {
      setLaeuft(false);
    }
  };

  return (
    <div className="bb-checkliste">
      {ABSCHNITTE.map((a) => {
        const ps = punkte.filter((p) => p.abschnitt === a && sichtbar(p, eingeschaltet));
        if (ps.length === 0) return null;
        const pflicht = ps.filter((p) => p.art === "pflicht" && p.status !== "nicht_messbar" && p.status !== "offen");
        const ok = pflicht.filter((p) => p.status === "erledigt" || p.status === "nach_atc").length;
        return (
          <section key={a} className="bb-cl-abschnitt">
            <header className="bb-cl-kopf">
              <span>{t(`bordbuch.abschnitt.${a}`)}</span>
              {pflicht.length > 0 && (
                <span className="bb-cl-zahl">
                  {ok}/{pflicht.length}
                </span>
              )}
            </header>
            <ul>
              {ps.map((p) => {
                const tippbar =
                  !!onMarkieren && (p.status === "diesmal_ohne" || p.status === "nach_atc");
                const istOffen = offen === p.regel;
                return (
                  <li key={p.regel} className={`bb-cl-punkt bb-st-${p.status}${p.art === "bestaetigung" ? " bb-bestaetigung" : ""}`}>
                    <button
                      type="button"
                      className="bb-cl-zeile"
                      disabled={!tippbar}
                      aria-expanded={tippbar ? istOffen : undefined}
                      onClick={() => setOffen(istOffen ? null : p.regel)}
                    >
                      <span className="bb-cl-marke" aria-hidden>
                        {(p.status === "erledigt" || p.status === "nach_atc") && <Haken />}
                      </span>
                      <span className="bb-cl-name">{t(`bordbuch.regel.${p.regel}`)}</span>
                      <span className="bb-cl-linie" aria-hidden />
                      <span className="bb-cl-wert">{wertText(p, t)}</span>
                    </button>
                    {(p.status === "erledigt" || p.status === "nach_atc") && (
                      <div className="bb-cl-zusatz">
                        {p.status === "nach_atc"
                          ? [t("bordbuch.status.nach_atc"), rollenText(p, rolltempoGrenzeKt, t)].filter(Boolean).join(" · ")
                          : [rollenText(p, rolltempoGrenzeKt, t), zulu(p.zeit), p.art === "bestaetigung" ? t("bordbuch.zusaetzlich") : null]
                              .filter(Boolean)
                              .join(" · ")}
                      </div>
                    )}
                    {p.status === "diesmal_ohne" && rollenText(p, rolltempoGrenzeKt, t) && (
                      <div className="bb-cl-zusatz">{rollenText(p, rolltempoGrenzeKt, t)}</div>
                    )}
                    {p.status === "nicht_messbar" && (
                      <div className="bb-cl-zusatz bb-cl-nm">
                        {p.grund === "kein_tcas_modus"
                          ? t("bordbuch.nicht_messbar.kein_tcas_modus")
                          : t("bordbuch.nicht_messbar.wert_fehlt", {
                              flugzeug: flugzeug || t("bordbuch.dieses_flugzeug"),
                              was: t(`bordbuch.was.${p.schalter}`),
                            })}
                      </div>
                    )}
                    {istOffen && tippbar && (
                      <div className="bb-cl-aktion">
                        {p.status === "diesmal_ohne" ? (
                          <button type="button" className="bb-knopf" disabled={laeuft} onClick={() => void markieren(p.regel, true)}>
                            {t("bordbuch.atc_markieren")}
                          </button>
                        ) : (
                          <button type="button" className="bb-knopf bb-knopf--leise" disabled={laeuft} onClick={() => void markieren(p.regel, false)}>
                            {t("bordbuch.atc_zuruecknehmen")}
                          </button>
                        )}
                        <span className="bb-cl-aktion-hinweis">{t("bordbuch.atc_erklaerung")}</span>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

/** Gefahrene Spitze beim Rollen (aus dem Beleg), sonst null. */
function rollenMax(p: Punkt): number | null {
  if (p.regel !== "rolltempo_abflug" && p.regel !== "rolltempo_ankunft") return null;
  const kt = p.beleg?.max_kt;
  return typeof kt === "number" && Number.isFinite(kt) ? kt : null;
}

/** „schnellstes Rollen 22 kt · Grenze 30 kt" — Spitze und Grenze nebeneinander,
 *  damit die gemessene Zahl nicht wie das erlaubte Tempo aussieht. */
function rollenText(
  p: Punkt,
  grenze: number | undefined,
  t: (k: string, o?: Record<string, unknown>) => string,
): string | null {
  const kt = rollenMax(p);
  if (kt === null || grenze === undefined) return null;
  const s = p.beleg?.laengste_ueber_grenze_s;
  if ((p.status === "diesmal_ohne" || p.status === "nach_atc") && typeof s === "number" && s > 0) {
    return t("bordbuch.rollen_ueber", { kt: Math.round(kt), s: Math.round(s), grenze: Math.round(grenze) });
  }
  return t("bordbuch.rollen_gefahren", { kt: Math.round(kt), grenze: Math.round(grenze) });
}

function wertText(p: Punkt, t: (k: string, o?: Record<string, unknown>) => string): string {
  switch (p.status) {
    case "erledigt": {
      const kt = rollenMax(p);
      if (kt !== null) return t("bordbuch.rollen_wert", { kt: Math.round(kt) });
      return p.stellung || t(`bordbuch.soll.${p.regel}`);
    }
    case "nach_atc":
      // Nicht die Soll-Stellung zeigen — der Schalter stand ja anders.
      return t("bordbuch.wert_atc");
    case "diesmal_ohne":
      return t("bordbuch.status.diesmal_ohne");
    case "nicht_messbar":
      return t("bordbuch.status.nicht_messbar");
    case "offen":
      return t("bordbuch.status.offen");
    default:
      return "";
  }
}
