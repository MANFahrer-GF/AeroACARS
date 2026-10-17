// Kabinenansagen (Beta): die Ansage-Seite. Oben, was gerade gesprochen
// wird (Sprecher, Text, Pegel live), darunter was als Nächstes kommt und
// was schon lief. Gleiche Seitenform wie das Bordbuch.
import { useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { abonnieren, aktuell, ueberblick, type Gespielt, type Ueberblick } from "../../lib/kabinenansagen";
import "./kabine.css";

const BALKEN = 48;

function uhrzeit(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Pegelanzeige: die letzten Messwerte als senkrechte Balken. */
function Pegel({ werte, aktiv }: { werte: number[]; aktiv: boolean }) {
  const reihe = Array.from({ length: BALKEN }, (_, i) => werte[werte.length - BALKEN + i] ?? 0);
  return (
    <svg className={`kabine-pegel${aktiv ? " ist-aktiv" : ""}`} viewBox={`0 0 ${BALKEN * 6} 64`} preserveAspectRatio="none" aria-hidden="true">
      {reihe.map((w, i) => {
        const h = Math.max(2, Math.round(w * 60));
        return <rect key={i} x={i * 6 + 1} y={32 - h / 2} width={4} height={h} rx={2} />;
      })}
    </svg>
  );
}

/** „Britisches Englisch“, „Schweizer Hochdeutsch“ — und bei der
 *  Standardform nur „Deutsch“ statt „Deutsch (Deutschland)“. */
function sprachName(sprache: string, sprachen: Intl.DisplayNames | null): string {
  try {
    const name = sprachen?.of(sprache) ?? sprache;
    return name.includes("(") ? (sprachen?.of(sprache.split("-")[0]) ?? name) : name;
  } catch {
    return sprache;
  }
}

export function KabinenSeite({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { t, i18n } = useTranslation();
  const live = useSyncExternalStore(abonnieren, aktuell);
  const [daten, setDaten] = useState<Ueberblick | null | undefined>(undefined);

  // Verlauf und Warteschlange kommen aus Rust; alle drei Sekunden und nach
  // jeder neuen Ansage nachlesen.
  const ansageZeit = live.ansage?.zeit;
  useEffect(() => {
    let weg = false;
    const holen = () => void ueberblick().then((d) => !weg && setDaten(d)).catch(() => !weg && setDaten(null));
    holen();
    const iv = setInterval(holen, 3000);
    return () => {
      weg = true;
      clearInterval(iv);
    };
  }, [ansageZeit]);

  let namen: Intl.DisplayNames | null = null;
  try {
    namen = new Intl.DisplayNames([i18n.language], { type: "language", languageDisplay: "dialect" });
  } catch {
    namen = null;
  }

  const a: Gespielt | null = live.ansage;
  const verlauf = [...(daten?.verlauf ?? [])].reverse();
  const wartend = daten?.wartend ?? [];

  return (
    <section className="kabine-seite">
      <header className="kabine-kopf">
        <div>
          <h2>{t("kabine.seite.titel")}</h2>
          <div className="kabine-unter">
            {daten === null || daten === undefined
              ? t("kabine.seite.kein_flug")
              : daten.beta === false
                ? t("kabine.seite.nicht_freigeschaltet")
                : daten.sprachen.length > 0
                  ? daten.sprachen.map((s) => sprachName(s, namen)).join(" · ")
                  : t("kabine.seite.warte_auf_texte")}
          </div>
        </div>
        <button type="button" className="button button--ghost" onClick={onOpenSettings}>
          {t("kabine.seite.einstellungen")}
        </button>
      </header>

      <div className={`kabine-live${a ? " ist-aktiv" : ""}`} aria-live="polite">
        <div className="kabine-live-kopf">
          <span className="kabine-sprecher">
            {a ? t(`kabine.spricht.${a.rolle}`) : t("kabine.seite.ruhig")}
          </span>
          {a && <span className="kabine-slot">{t(`kabine.slot.${a.slot}`)}</span>}
        </div>
        <Pegel werte={live.verlauf} aktiv={a !== null} />
        {a && a.rolle !== "chime" && (
          <ol className="kabine-teile">
            {a.teile.map((teil, i) => (
              <li key={teil.sprache} className={i === live.teil ? "ist-jetzt" : i < live.teil ? "ist-gesagt" : ""}>
                <span className="kabine-sprache">{sprachName(teil.sprache, namen)}</span>
                <p>{teil.text}</p>
              </li>
            ))}
          </ol>
        )}
        {!a && (
          <p className="kabine-ruhe-text">
            {wartend.length > 0
              ? t("kabine.seite.naechste", { slot: t(`kabine.slot.${wartend[0]}`) })
              : t("kabine.seite.ruhe_hinweis")}
          </p>
        )}
      </div>

      <div className="kabine-spalten">
        <div>
          <h3 className="kabine-h3">{t("kabine.seite.als_naechstes")}</h3>
          {wartend.length === 0 ? (
            <div className="kabine-leer">{t("kabine.seite.nichts_wartet")}</div>
          ) : (
            <ul className="kabine-liste">
              {wartend.map((s, i) => (
                <li key={s + i}>{t(`kabine.slot.${s}`)}</li>
              ))}
            </ul>
          )}
        </div>
        <div>
          <h3 className="kabine-h3">{t("kabine.seite.bisher")}</h3>
          {verlauf.length === 0 ? (
            <div className="kabine-leer">{t("kabine.seite.noch_nichts")}</div>
          ) : (
            <ul className="kabine-liste kabine-liste--verlauf">
              {verlauf.map((g) => (
                <li key={g.zeit + g.slot}>
                  <div className="kabine-verlauf-kopf">
                    <span className="kabine-zeit">{uhrzeit(g.zeit)}</span>
                    <strong>{t(`kabine.slot.${g.slot}`)}</strong>
                    <span className="kabine-rolle">{t(`kabine.rolle.${g.rolle}`)}</span>
                  </div>
                  {g.teile[0] && <p className="kabine-verlauf-text">{g.teile[0].text}</p>}
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}
