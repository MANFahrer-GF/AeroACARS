// Bordbuch im Landungs-Tab und im PDF-Bericht (27.09.2026, Thomas: „wo sieht
// der Pilot das nach der Landung — und kommt es aufs PDF?"). Dieselbe
// Checkliste wie im Reiter „Bordbuch", an die Landung gehängt.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { bilanz, type Eintrag, type Regel } from "../../lib/bordbuch";
import { BordbuchCheckliste } from "./BordbuchCheckliste";
import "./bordbuch.css";

/** Lädt das Bordbuch einer PIREP und hält es aktuell (ATC-Markierung,
 *  Server-Abgleich). `null` = zu diesem Flug gibt es keins (ältere Flüge). */
export function useBordbuchEintrag(pirepId: string | null | undefined) {
  const [eintrag, setEintrag] = useState<Eintrag | null>(null);
  // Zu welcher Landung der Stand gehört — `bereit` heisst: für DIESE Landung
  // ist die erste Antwort da (der PDF-Export wartet darauf).
  const [geladenFuer, setGeladenFuer] = useState<string | null>(null);
  // Jede Anfrage bekommt eine Nummer; nur die jüngste darf schreiben. Sonst
  // überschreibt eine späte Antwort (vorige Landung, oder ein Laden, das vor
  // der ATC-Markierung losging) den neueren Stand (Codex-QS).
  const aktuell = useRef(pirepId);
  aktuell.current = pirepId;
  const nummer = useRef(0);
  const laden = useCallback(async () => {
    if (!pirepId) return;
    const meine = ++nummer.current;
    let neu: Eintrag | null = null;
    try {
      const e = await invoke<Eintrag | null>("bordbuch_eintrag", { pirepId });
      // Nur echte Einträge übernehmen (Tests/ältere Brücken liefern evtl. Unsinn).
      neu = e && Array.isArray((e as Eintrag).punkte) ? e : null;
    } catch {
      neu = null;
    }
    if (meine !== nummer.current || aktuell.current !== pirepId) return;
    setEintrag(neu);
    setGeladenFuer(pirepId);
  }, [pirepId]);
  useEffect(() => {
    setEintrag(null);
    void laden();
    let aktiv = true;
    let weg: (() => void) | undefined;
    // Ein Fehler hier darf die Landungsseite nie mitreissen — das Bordbuch
    // ist Beiwerk (ohne Aktualisierung bleibt es beim geladenen Stand).
    try {
      Promise.resolve(listen("bordbuch_geaendert", () => void laden()))
        .then((u) => {
          if (typeof u !== "function") return;
          if (aktiv) weg = u;
          else u();
        })
        .catch(() => undefined);
    } catch {
      /* ohne Ereigniskanal */
    }
    return () => {
      aktiv = false;
      weg?.();
    };
  }, [laden]);
  const markieren = useCallback(
    async (regel: Regel, nachAtc: boolean) => {
      if (!pirepId) return;
      const meine = ++nummer.current;
      const neu = await invoke<Eintrag | null>("bordbuch_markieren", { pirepId, regel, nachAtc });
      if (aktuell.current !== pirepId) return;
      if (neu && meine === nummer.current) {
        setEintrag(neu);
      } else {
        // Ein Laden lief dazwischen und hat evtl. den Stand VOR der Markierung
        // gelesen. Die Markierung ist gespeichert — neu laden, das gewinnt.
        void laden();
      }
    },
    [pirepId, laden],
  );
  const bereit = !pirepId || geladenFuer === pirepId;
  return { eintrag: bereit ? eintrag : null, markieren, bereit };
}

/** Kopfzeile „11 von 13 Punkten erledigt" bzw. der Aus-Hinweis. */
function Kopf({ e, druck = false }: { e: Eintrag; druck?: boolean }) {
  const { t } = useTranslation();
  if (e.aus_grund) {
    return <p className="bb-aus-hinweis">{t(`bordbuch.aus.${e.aus_grund}`, t("bordbuch.aus.ga"))}</p>;
  }
  const b = bilanz(e.punkte, e.eingeschaltet);
  const alles = b.von > 0 && b.ok === b.von;
  return (
    <p className="bb-landung-bilanz">
      <strong>{t("bordbuch.bilanz", b)}</strong>
      {" · "}
      {alles ? t("bordbuch.satz_alles") : t(druck ? "bordbuch.satz_teil_druck" : "bordbuch.satz_teil")}
    </p>
  );
}

/** Abschnitt in der Landungs-Detailansicht (mit ATC-Markierung). */
export function BordbuchLandungsAbschnitt({
  eintrag,
  onMarkieren,
}: {
  eintrag: Eintrag | null;
  onMarkieren?: (regel: Regel, nachAtc: boolean) => Promise<void>;
}) {
  const { t } = useTranslation();
  if (!eintrag) return null;
  return (
    <section className="landing-section bb-landung">
      <h3>{t("bordbuch.titel")}</h3>
      <Kopf e={eintrag} />
      {!eintrag.aus_grund && (
        <BordbuchCheckliste
          punkte={eintrag.punkte}
          eingeschaltet={eintrag.eingeschaltet}
          flugzeug={eintrag.flug.titel ?? eintrag.flug.muster ?? null}
          onMarkieren={onMarkieren}
          rolltempoGrenzeKt={eintrag.rolltempo_grenze_kt}
        />
      )}
    </section>
  );
}

/** Fassung für den PDF-Bericht — ohne Bedienung. */
export function BordbuchBericht({ eintrag }: { eintrag: Eintrag }) {
  return (
    <div className="bb-bericht">
      <Kopf e={eintrag} druck />
      {!eintrag.aus_grund && (
        <BordbuchCheckliste
          punkte={eintrag.punkte}
          eingeschaltet={eintrag.eingeschaltet}
          flugzeug={eintrag.flug.titel ?? eintrag.flug.muster ?? null}
          rolltempoGrenzeKt={eintrag.rolltempo_grenze_kt}
        />
      )}
    </div>
  );
}
