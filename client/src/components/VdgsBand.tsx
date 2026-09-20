// VDGS-Band — die eigene Abflugfolge im Cockpit, ohne Fensterwechsel.
//
// Zeigt das, was am echten Flughafen auf der Anzeige am Gate steht:
// TOBT, TSAT, CTOT, Rollzeit, Bahn/Abflugstrecke und den CDM-Status.
// Die Werte kommen vom Rechenwerk hinter `vats.im/vdgs` (VATSIM Spain),
// abgerufen im Hintergrund von `vdgs_stand` (src-tauri/src/vdgs.rs).
//
// Drei Entscheidungen, die man beim Lesen kennen muss:
//
//   1. **Nur lesend.** Die TOBT setzt der Pilot weiterhin auf deren
//      Seite — dafür bleibt die Taste in der CDM-Ansicht
//      (`VatsimCdmView.tsx`). Schreiben braucht einen Schlüssel von
//      VATSIM Spain und eine Bindung an die echte Pilotenkennung.
//
//   2. **Nur wenn es etwas zu zeigen gibt.** Kein Eintrag im CDM-System
//      (der Normalfall auf den meisten Plätzen), kein laufender Flug,
//      oder der Dienst antwortet nicht → das Band erscheint gar nicht.
//      Kein leeres Gerät, keine Fehlermeldung im Cockpit.
//
//   3. **Nur vor dem Abheben.** Danach ist die Abflugfolge Geschichte;
//      das entscheidet CockpitView über `takeoff_at`.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "../lib/ipc";
import "./vdgs.css";

/** Wie `VdgsStand` in src-tauri/src/vdgs.rs. Zeiten sind `HH:MM` (UTC)
 *  oder leer — umgerechnet wird schon dort, damit hier nur angezeigt
 *  wird (siehe die beiden Zeitformate der Gegenseite). */
/** Was der Befehl liefert: das verwendete Rufzeichen UND der Stand. */
export interface VdgsAntwort {
  /** Womit gefragt wurde — auch dann gesetzt, wenn es keinen Eintrag gibt. */
  gefragt_als: string;
  stand: VdgsStand | null;
}

export interface VdgsStand {
  callsign: string;
  departure: string;
  eobt: string;
  tobt: string;
  tsat: string;
  ctot: string;
  taxi_min: number | null;
  cdm_sts: string;
  regulierung: string;
  rwy_sid: string;
}

export type Ampel = "frei" | "warten" | "achtung";

/** Abstand in Minuten von jetzt (UTC) bis zu einer `HH:MM`-Zeit.
 *
 *  Der Tageswechsel ist hier der ganze Punkt: eine TSAT um 23:58, von
 *  00:03 aus gesehen, liegt 5 Minuten ZURÜCK und nicht 1435 Minuten in
 *  der Zukunft. Alles außerhalb eines halben Tages wird deshalb auf die
 *  andere Seite gedreht. */
export function minutenBis(zeit: string, jetzt: Date = new Date()): number | null {
  const m = /^(\d{2}):(\d{2})$/.exec(zeit.trim());
  if (!m) return null;
  const std = Number(m[1]);
  const min = Number(m[2]);
  if (std > 23 || min > 59) return null;
  const jetztMin = jetzt.getUTCHours() * 60 + jetzt.getUTCMinutes();
  let diff = std * 60 + min - jetztMin;
  if (diff > 720) diff -= 1440;
  if (diff <= -720) diff += 1440;
  return diff;
}

/** Farbe wie am echten Gerät: grün heißt „anlassen", bernstein
 *  „vorbereiten", rot „raus aus der Folge". */
export function ampel(stand: VdgsStand, jetzt: Date = new Date()): Ampel {
  const st = stand.cdm_sts.toUpperCase();
  if (st.includes("SUSPEND") || st.includes("NRA")) return "achtung";
  const rest = minutenBis(stand.tsat || stand.tobt, jetzt);
  // Ohne TOBT ist nichts bestätigt — und ohne Zeitangabe wissen wir
  // schlicht nicht genug, um grün zu zeigen.
  if (!stand.tobt) return "warten";
  if (rest !== null && rest <= 5 && rest >= -10) return "frei";
  return "warten";
}

/**
 * Holt den Stand im Hintergrund, solange `aktiv`.
 *
 * Zwei Takte: alle 60 s ein Abruf (der Rust-Teil hält zusätzlich einen
 * Zwischenspeicher, damit die fremde Seite nicht öfter getroffen wird),
 * und jede Minute ein Neuzeichnen für den Countdown — sonst stünde „in
 * 4 min" eine Minute später immer noch da.
 */
export function useVdgsStand(aktiv: boolean): [VdgsAntwort | null, () => void] {
  const [stand, setStand] = useState<VdgsAntwort | null>(null);
  const [, setTakt] = useState(0);
  // Nach einer Rufzeichen-Aenderung sofort neu fragen, statt bis zum
  // naechsten Takt zu warten — sonst sieht der Pilot bis zu eine Minute
  // lang weiter „kein Eintrag" und haelt seine Korrektur fuer wirkungslos.
  const [neuLadenZaehler, setNeuLadenZaehler] = useState(0);

  useEffect(() => {
    if (!aktiv) {
      setStand(null);
      return;
    }
    let abgemeldet = false;
    const holen = () => {
      void invoke<VdgsAntwort | null>("vdgs_stand")
        .then((s) => {
          if (!abgemeldet) setStand(s ?? null);
        })
        // Ein Fehler ist hier kein Ereignis: das Band bleibt einfach weg.
        .catch(() => {
          if (!abgemeldet) setStand(null);
        });
    };
    holen();
    const id = window.setInterval(holen, 60_000);
    return () => {
      abgemeldet = true;
      window.clearInterval(id);
    };
  }, [aktiv, neuLadenZaehler]);

  useEffect(() => {
    if (!aktiv || !stand) return;
    const id = window.setInterval(() => setTakt((n) => n + 1), 30_000);
    return () => window.clearInterval(id);
  }, [aktiv, stand]);

  return [stand, () => setNeuLadenZaehler((n) => n + 1)];
}

/**
 * Das Rufzeichen — sichtbar und aenderbar.
 *
 * Es ist der Schluessel zum CDM-Eintrag. Passt es nicht, bleibt das Band
 * leer, und vorher war nicht einmal zu sehen, WOMIT gefragt wurde
 * (Thomas, 20.09.2026: „ist das nicht besser als was anzunehmen").
 * Geschrieben wird die Uebersteuerung, die auch der Funk nimmt — eine
 * Quelle, kein Auseinanderlaufen.
 */
function Rufzeichen({
  wert,
  onGesetzt,
}: {
  wert: string;
  onGesetzt: () => void;
}) {
  const { t } = useTranslation();
  const [bearbeiten, setBearbeiten] = useState(false);
  const [entwurf, setEntwurf] = useState(wert);

  if (!bearbeiten) {
    return (
      <button
        type="button"
        className="vdgs__rufzeichen vdgs__rufzeichen--knopf"
        title={t("cdm.band.rufzeichen_aendern", "Rufzeichen ändern")}
        onClick={() => {
          setEntwurf(wert);
          setBearbeiten(true);
        }}
      >
        {wert}
      </button>
    );
  }

  const speichern = () => {
    setBearbeiten(false);
    void invoke("vdgs_rufzeichen_setzen", { callsign: entwurf })
      .then(onGesetzt)
      .catch(() => {});
  };

  return (
    <input
      className="vdgs__rufzeichen vdgs__rufzeichen--feld"
      value={entwurf}
      autoFocus
      maxLength={16}
      aria-label={t("cdm.band.rufzeichen_aendern", "Rufzeichen ändern")}
      onChange={(e) => setEntwurf(e.target.value.toUpperCase())}
      onBlur={speichern}
      onKeyDown={(e) => {
        if (e.key === "Enter") speichern();
        // Abbrechen laesst den alten Wert stehen — sonst genuegte ein
        // versehentliches Escape, um das Funk-Rufzeichen zu aendern.
        if (e.key === "Escape") setBearbeiten(false);
      }}
    />
  );
}

export function VdgsPlatte({
  antwort,
  onRufzeichenGesetzt,
}: {
  antwort: VdgsAntwort | null;
  onRufzeichenGesetzt?: () => void;
}) {
  const { t } = useTranslation();
  if (!antwort) return null;
  const stand = antwort.stand;
  const neuLaden = onRufzeichenGesetzt ?? (() => {});

  // Kein Eintrag: schmale Zeile statt gar nichts. Sie sagt, womit
  // gefragt wurde, und laesst es richtigstellen.
  if (!stand) {
    return (
      <div className="vdgs vdgs--leer" data-testid="vdgs-band-leer">
        <span className="vdgs__quelle">VDGS</span>
        <Rufzeichen wert={antwort.gefragt_als} onGesetzt={neuLaden} />
        <span className="vdgs__leer-text">
          {t("cdm.band.kein_eintrag", "kein CDM-Eintrag")}
        </span>
      </div>
    );
  }

  const zustand = ampel(stand);
  // Die große Zahl ist die, auf die gewartet wird: TSAT, wo das CDM
  // sequenziert — sonst die TOBT. Plätze ohne Sequenzierung liefern
  // gar keine TSAT (EDDF, EGLL, EHAM am 20.09.2026), da stünde sonst
  // dauerhaft „--:--" als Hauptzahl.
  const hatTsat = stand.tsat !== "";
  const kopfzahl = hatTsat ? stand.tsat : stand.tobt;
  const kopflabel = hatTsat ? "TSAT" : "TOBT";
  const rest = minutenBis(kopfzahl);

  return (
    <div className={`vdgs vdgs--${zustand}`} data-testid="vdgs-band">
      <div className="vdgs__kopf">
        <Rufzeichen wert={antwort.gefragt_als} onGesetzt={neuLaden} />
        <span className="vdgs__platz">{stand.departure}</span>
        {stand.rwy_sid && <span className="vdgs__sid">{stand.rwy_sid}</span>}
        <span className="vdgs__quelle">VDGS</span>
      </div>

      <div className="vdgs__haupt">
        {/* Ohne gesetzte TOBT gibt es keine TSAT und kein Sequencing —
            der Flug steht nicht in der Folge. Das ist kein Leerwert,
            sondern eine offene Aufgabe (Thomas, 20.09.2026: „die muss
            doch gesetzt werden, das ist doch Pflicht"). Gesetzt wird sie
            nicht hier, sondern auf vats.im/vdgs, also fuehrt der Klick
            genau dorthin. */}
        {!stand.tobt ? (
          <button
            type="button"
            className="vdgs__auftrag"
            onClick={() => {
              // Auf dem Tablet gibt es kein Fenster zu oeffnen (die
              // LAN-Bruecke laesst den Befehl nicht durch). Dann bleibt
              // der Text stehen und sagt, wo es geht.
              void invoke("vdgs_fenster_oeffnen").catch(() => {});
            }}
          >
            <span className="vdgs__auftrag-wort">
              {t("cdm.band.tobt_setzen", "TOBT SETZEN")}
            </span>
            <span className="vdgs__auftrag-grund">
              {t(
                "cdm.band.tobt_setzen_grund",
                "Ohne TOBT keine TSAT — im VDGS-Fenster setzen",
              )}
            </span>
          </button>
        ) : (
          <div className="vdgs__gross">
            <span className="vdgs__gross-label">{kopflabel}</span>
            <span className="vdgs__gross-wert">{kopfzahl || "--:--"}</span>
          </div>
        )}
        {stand.tobt && rest !== null && (
          <div className="vdgs__rest">
            {rest >= 0
              ? t("cdm.band.in_min", "in {{n}} min", { n: rest })
              : t("cdm.band.vor_min", "vor {{n}} min", { n: Math.abs(rest) })}
          </div>
        )}
      </div>

      <div className="vdgs__reihe">
        <Feld label="EOBT" wert={stand.eobt} />
        <Feld label="TOBT" wert={stand.tobt} stark={hatTsat} />
        <Feld label="CTOT" wert={stand.ctot} />
        <Feld
          label="TAXI"
          wert={stand.taxi_min !== null ? `${stand.taxi_min}′` : ""}
        />
      </div>

      {(stand.cdm_sts || stand.regulierung) && (
        <div className="vdgs__fuss">
          {stand.cdm_sts && (
            <span className={`vdgs__status vdgs__status--${zustand}`}>
              {stand.cdm_sts}
            </span>
          )}
          {stand.regulierung && (
            <span className="vdgs__reg">{stand.regulierung}</span>
          )}
        </div>
      )}
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
      <span className="vdgs__feld-wert">{wert || "--:--"}</span>
    </div>
  );
}
