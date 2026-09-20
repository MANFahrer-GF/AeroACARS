// VDGS-Band — die Abflugfolge im Cockpit, ohne Fensterwechsel.
//
// ENTWURF (20.09.2026): rein darstellend, noch ohne Anbindung. Die Daten
// kommen spaeter aus `api.viffsys.com/ifps/callsign?callsign=...` (lesend,
// ohne Anmeldung); gesetzt wird weiterhin im VDGS-Fenster, s.
// VatsimCdmView.tsx. Deshalb steht hier kein Abruf, sondern nur die Form —
// damit die Anzeige am echten Korpus beurteilt werden kann.
//
// Zwei Fassungen, bewusst nebeneinander:
//   * `<VdgsPlatte>` — sieht aus wie das Geraet am Gate (dunkle Platte,
//     B612 Mono, Bernstein/Gruen). Die Zahl, auf die es ankommt (TSAT),
//     ist die groesste im Bild.
//   * `<VdgsKarte>` — dieselben Werte als gewoehnliche `.card`, fuer das
//     Cockpit-Raster neben Massen/Luftdaten/Trip.

import { useTranslation } from "react-i18next";
import "./vdgs.css";

/** Was die viffsys-Antwort an Feldern hergibt, auf das reduziert, was das
 *  Band zeigt. Feldnamen wie in der API, damit der spaetere Abruf 1:1
 *  hineinreicht. */
export interface CdmStand {
  callsign: string;
  departure: string;
  /** Aus dem VATSIM-Flugplan. */
  eobt: string;
  /** Vom Piloten gesetzt/bestaetigt. */
  tobt: string;
  /** Vom CDM zugeteilt — die Freigabezeit fuer Anlassen. */
  tsat: string;
  /** Aus der Flussregelung; leer, wenn keine Regulierung greift. */
  ctot: string;
  /** Rollzeit in Minuten. */
  taxi: number | null;
  /** COMPLY | FLS-NRA | SUSPENDED | ... */
  cdmSts: string;
  /** Grund der Regulierung, falls vorhanden. */
  regulierung?: string;
  /** Minuten bis TSAT; negativ = TSAT liegt zurueck. */
  bisTsatMin: number | null;
}

type Ampel = "ok" | "warten" | "achtung" | "ruhe";

/** Farbe folgt dem echten VDGS: gruen heisst „anlassen", bernstein
 *  „vorbereiten", rot „raus aus der Folge". Unbekannte Status bleiben
 *  absichtlich neutral, statt zu raten. */
function ampel(s: CdmStand): Ampel {
  const st = s.cdmSts.toUpperCase();
  if (st.includes("SUSPEND") || st.includes("NRA")) return "achtung";
  if (!s.tobt) return "warten";
  if (s.bisTsatMin !== null && s.bisTsatMin <= 5) return "ok";
  return "warten";
}

function zeit(v: string): string {
  return v && v.length === 4 ? `${v.slice(0, 2)}:${v.slice(2)}` : "--:--";
}

export function VdgsPlatte({ stand }: { stand: CdmStand | null }) {
  const { t } = useTranslation();

  // Kein Eintrag im CDM-System ist der Normalfall auf den meisten
  // Plaetzen — dann meldet sich das Band leise ab, statt leer zu leuchten.
  if (!stand) {
    return (
      <div className="vdgs vdgs--aus">
        <span className="vdgs__aus-text">
          {t("vdgs.kein_eintrag", "Kein CDM-Eintrag für diesen Flug")}
        </span>
      </div>
    );
  }

  const zustand = ampel(stand);
  const rest = stand.bisTsatMin;

  return (
    <div className={`vdgs vdgs--${zustand}`}>
      <div className="vdgs__kopf">
        <span className="vdgs__rufzeichen">{stand.callsign}</span>
        <span className="vdgs__platz">{stand.departure}</span>
        <span className="vdgs__quelle">VDGS</span>
      </div>

      <div className="vdgs__haupt">
        <div className="vdgs__gross">
          <span className="vdgs__gross-label">TSAT</span>
          <span className="vdgs__gross-wert">{zeit(stand.tsat)}</span>
        </div>
        {rest !== null && (
          <div className="vdgs__rest">
            {rest >= 0
              ? t("vdgs.in_min", "in {{n}} min", { n: rest })
              : t("vdgs.vor_min", "vor {{n}} min", { n: Math.abs(rest) })}
          </div>
        )}
      </div>

      <div className="vdgs__reihe">
        <Feld label="EOBT" wert={zeit(stand.eobt)} />
        <Feld label="TOBT" wert={zeit(stand.tobt)} stark={!!stand.tobt} />
        <Feld label="CTOT" wert={zeit(stand.ctot)} />
        <Feld
          label="TAXI"
          wert={stand.taxi !== null ? `${stand.taxi}′` : "--"}
        />
      </div>

      <div className="vdgs__fuss">
        <span className={`vdgs__status vdgs__status--${zustand}`}>
          {stand.cdmSts || "—"}
        </span>
        {stand.regulierung && (
          <span className="vdgs__reg">{stand.regulierung}</span>
        )}
      </div>
    </div>
  );
}

function Feld({
  label,
  wert,
  stark,
}: {
  label: string;
  wert: string;
  stark?: boolean;
}) {
  return (
    <div className={`vdgs__feld${stark ? " vdgs__feld--stark" : ""}`}>
      <span className="vdgs__feld-label">{label}</span>
      <span className="vdgs__feld-wert">{wert}</span>
    </div>
  );
}

/** Dieselben Werte in der gewoehnlichen Hausform — fuer das Cockpit-Raster. */
export function VdgsKarte({ stand }: { stand: CdmStand | null }) {
  const { t } = useTranslation();
  if (!stand) return null;
  const zustand = ampel(stand);

  return (
    <div className="card">
      <div className="card__head">
        <span className="card__title">{t("vdgs.titel", "VATSIM CDM")}</span>
        <span className={`vdgs-chip vdgs-chip--${zustand}`}>{stand.cdmSts}</span>
      </div>
      <div className="card__body">
        <Zeile label="EOBT" wert={zeit(stand.eobt)} />
        <Zeile label="TOBT" wert={zeit(stand.tobt)} />
        <Zeile label="TSAT" wert={zeit(stand.tsat)} summe />
        <Zeile label="CTOT" wert={zeit(stand.ctot)} />
        <Zeile
          label={t("vdgs.taxi", "Rollzeit")}
          wert={stand.taxi !== null ? `${stand.taxi} min` : "—"}
        />
      </div>
    </div>
  );
}

function Zeile({
  label,
  wert,
  summe,
}: {
  label: string;
  wert: string;
  summe?: boolean;
}) {
  return (
    <div className={`row${summe ? " row--sum" : ""}`}>
      <span className="row__label">{label}</span>
      <span className="row__value">{wert}</span>
    </div>
  );
}
