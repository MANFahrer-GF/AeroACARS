// Bordbuch in der Landungsanzeige und im PDF — nur Anzeige, ohne Laden.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN). Bis
// 05.10.2026 zeigte die Webapp das Bordbuch nur in der PIREP-Karte, mit
// eigenen, fest deutschen Texten; in der Landungsanalyse fehlte es. Das Laden
// bleibt je App eigen (Client: BordbuchLandung.tsx über IPC, Webapp:
// /admin/bordbuch).

import { useTranslation } from "react-i18next";
import { bilanz, type Eintrag, type Regel } from "../../lib/bordbuch";
import { BordbuchCheckliste } from "./BordbuchCheckliste";
import "./bordbuchLandung.css";
import { InfoBadge } from "../InfoBadge";
import { useDruck } from "../../lib/druck";

/** Kopfzeile „11 von 13 Punkten erledigt" bzw. der Aus-Hinweis. */
export function Kopf({ e, druck = false }: { e: Eintrag; druck?: boolean }) {
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

/** Abschnitt in der Landungs-Detailansicht (mit ATC-Markierung) und im
 *  PDF-Bericht. Im Druck (DruckKontext) ohne Bedienung: Satz ohne
 *  „Tippe den Punkt an", keine ATC-Knöpfe, Papierfarben (`.bb-bericht`). */
export function BordbuchLandungsAbschnitt({
  eintrag,
  onMarkieren,
}: {
  eintrag: Eintrag | null;
  onMarkieren?: (regel: Regel, nachAtc: boolean) => Promise<void>;
}) {
  const { t } = useTranslation();
  const druck = useDruck();
  if (!eintrag) return null;
  return (
    <section className={druck ? "landing-section bb-landung bb-bericht" : "landing-section bb-landung"}>
      <h3>{t("bordbuch.titel")}{" "}<InfoBadge explanation={t("landing.erklaer.bordbuch")} /></h3>
      <Kopf e={eintrag} druck={druck} />
      {!eintrag.aus_grund && (
        <BordbuchCheckliste
          punkte={eintrag.punkte}
          eingeschaltet={eintrag.eingeschaltet}
          flugzeug={eintrag.flug.titel ?? eintrag.flug.muster ?? null}
          onMarkieren={druck ? undefined : onMarkieren}
          rolltempoGrenzeKt={eintrag.rolltempo_grenze_kt}
        />
      )}
    </section>
  );
}
