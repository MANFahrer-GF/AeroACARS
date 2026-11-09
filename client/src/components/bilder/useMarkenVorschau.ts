// Was die Vorschau eines Bildpunkts zeigt: das kleine Bild, Flug und Strecke, Zeit, Hoehe, Phase und das Flugzeug.
import { useTranslation } from "react-i18next";
import type { MarkenVorschauDaten } from "../../flugkarte/markenVorschau";
import { useKachelTexte } from "./Kacheln";
import { datumText } from "./logik/datum";
import type { Eintrag } from "./logik/typen";
import { vorschauAdresse } from "./useBildUrl";

const fuegen = (teile: Array<string | null | undefined | false>) => teile.filter(Boolean).join(" · ");

export function useMarkenVorschau(): (e: Eintrag | undefined) => MarkenVorschauDaten | null {
  const { i18n } = useTranslation();
  const sprache = i18n.resolvedLanguage ?? i18n.language ?? "de";
  const { phase, hoehe } = useKachelTexte();
  return (e) => {
    if (!e) return null;
    const m = e.sidecar?.meta;
    const zeit = m?.zeit_utc?.slice(11, 16);
    // Das Datum gehoert dazu: Auf der Karte liegen Bilder verschiedener Fluege nebeneinander.
    const ms = m?.zeit_utc ? Date.parse(m.zeit_utc) : Number.NaN;
    return {
      url: vorschauAdresse(e),
      zeilen: [
        fuegen([m?.rufzeichen, m?.abflug && m?.ziel && `${m.abflug}–${m.ziel}`]),
        fuegen([!Number.isNaN(ms) && datumText(ms, sprache), zeit && `${zeit}Z`, typeof m?.hoehe_ft === "number" && hoehe(e), phase(e)]),
        m?.flugzeug_titel ?? m?.flugzeug_icao ?? "",
      ].filter(Boolean),
    };
  };
}
