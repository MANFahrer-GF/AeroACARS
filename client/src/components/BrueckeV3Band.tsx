import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { FlightPhase } from "../types";
import { formatIpcError, invoke, isTauri } from "../lib/ipc";
import { ACTIVE_FLIGHT_PHASES } from "./UpdateBanner";

/**
 * v2.1.25 — Brücke zu AeroACARS 3 (Thomas 02.11.2026): Viele Piloten blieben
 * auf 2.x, weil der 2.x-Updater von 3 nichts weiß. Der Hook fragt den
 * 3.x-Kanal ab (`bruecke_v3_pruefen`); Laden, Installer starten und Beenden
 * macht der Kern (`bruecke_v3.rs`).
 *
 * v2.1.26 (Thomas 02.11.2026: „kann das einfach wegklicken und steigt dann
 * trotzdem nicht um“): Das Band hat kein „Später“ mehr und bleibt stehen,
 * bis umgestiegen ist; zusätzlich steht der Knopf `BrueckeV3Knopf` fest in
 * der Seitenleiste. Beide teilen sich den Zustand aus `useBrueckeV3`.
 *
 * Das Band nicht im Flug (dieselben Phasen wie das Update-Band), beides
 * nicht im LAN-Browser (der Kern leitet die Befehle dort ohnehin nicht
 * weiter). Solange ein Flug offen ist oder Flüge noch auf das Senden warten,
 * sperrt der Kern den Umstieg (`sperre`). Die Sperre wird bei jedem
 * Phasenwechsel und ohne Flug jede Minute neu abgefragt — sonst bliebe der
 * Knopf nach dem Flug bis zum Neustart gesperrt.
 */

interface BrueckeV3Info {
  version: string;
  sperre: "flug" | "warteschlange" | null;
  /** 3 liegt schon auf diesem Rechner (nur Windows): öffnen statt laden. */
  installiert?: boolean;
  /** 3 ist installiert und schon einmal gestartet: Öffnen trägt also. */
  lief_schon?: boolean;
}

export interface BrueckeV3 {
  info: BrueckeV3Info | null;
  laeuft: boolean;
  fehler: string | null;
  umsteigen: () => void;
  /** Die erste Antwort kam im Startfenster und ohne Sperre. Trägt `BrueckeV3Riegel`. */
  pflichtBeimStart: boolean;
  /** Versuch aus einem früheren Start, siehe `VERSUCH_KEY`. */
  frueherVersuch: string | null;
}

/** Neu abfragen (nur ohne Flug): jede Minute bei Sperre, alle zehn Minuten ohne Antwort. */
const TAKT_GESPERRT_MS = 60 * 1000;
const TAKT_OHNE_ANTWORT_MS = 10 * 60 * 1000;

/**
 * Wie beim `UpdateGate` sperrt der Riegel nur am Anfang, nie mitten in der
 * Sitzung. Zwölf statt zehn Minuten, damit die Wiederholung nach einem Start
 * ohne Netz (`TAKT_OHNE_ANTWORT_MS`) noch hineinfällt.
 */
const STARTFENSTER_MS = 12 * 60 * 1000;

/**
 * Letzter Umstiegsversuch dieses Rechners: `laden:<version>` oder
 * `oeffnen:<version>`. Windows beendet 2.x direkt nach dem Start des
 * Installers, ein abgebrochener Installer meldet deshalb nie einen Fehler.
 * Erkennbar wird er erst beim nächsten Start: 3 fehlt, obwohl geladen wurde.
 */
const VERSUCH_KEY = "aeroacars.bruecke_v3.versuch";

function lesen(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/**
 * Lässt sich nichts speichern, kann der Riegel weder einen Fehlschlag über
 * den Neustart retten noch einen Ausweg merken. Dann sperrt er nicht, das
 * Band bleibt: Ein Riegel ohne Ausgang ist schlimmer als keiner.
 */
function speicherGeht(): boolean {
  try {
    localStorage.setItem(`${VERSUCH_KEY}.probe`, "1");
    localStorage.removeItem(`${VERSUCH_KEY}.probe`);
    return true;
  } catch {
    return false;
  }
}

function versuchKennung(info: BrueckeV3Info): string {
  return `${info.installiert ? "oeffnen" : "laden"}:${info.version}`;
}

export function useBrueckeV3(activePhase: FlightPhase | null): BrueckeV3 {
  const [info, setInfo] = useState<BrueckeV3Info | null>(null);
  const [laeuft, setLaeuft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [abfrage, setAbfrage] = useState(0);
  const [pflichtBeimStart, setPflichtBeimStart] = useState(false);
  // Nur beim Start gelesen: Der Klick dieser Sitzung ist noch kein Fehlschlag.
  const [frueherVersuch] = useState(() => lesen(VERSUCH_KEY));
  const startZeit = useRef(Date.now());
  const ersteAntwort = useRef(false);

  useEffect(() => {
    if (!isTauri) return;
    let aktiv = true;
    invoke<BrueckeV3Info | null>("bruecke_v3_pruefen")
      .then((v) => {
        // Eine leere Antwort (Netzfehler) wirft eine schon bekannte Version nicht weg.
        if (!aktiv || !v) return;
        if (!ersteAntwort.current) {
          ersteAntwort.current = true;
          if (v.sperre == null && Date.now() - startZeit.current < STARTFENSTER_MS) setPflichtBeimStart(true);
        }
        setInfo(v);
      })
      .catch(() => undefined);
    return () => {
      aktiv = false;
    };
  }, [activePhase, abfrage]);

  // Takt nur ohne Flug: Im Flug ist das Band weg und der Kern sperrt ohnehin;
  // das Flugende fängt die Abfrage beim Phasenwechsel ab.
  const gesperrt = info?.sperre != null;
  useEffect(() => {
    if (!isTauri || activePhase != null || (info && !gesperrt)) return;
    const id = setInterval(() => setAbfrage((n) => n + 1), info ? TAKT_GESPERRT_MS : TAKT_OHNE_ANTWORT_MS);
    return () => clearInterval(id);
  }, [info, gesperrt, activePhase]);

  const umsteigen = useCallback(() => {
    setLaeuft(true);
    setFehler(null);
    if (info) {
      try {
        localStorage.setItem(VERSUCH_KEY, versuchKennung(info));
      } catch {
        /* Ohne Speicher fehlt nur der Ausweg beim nächsten Start. */
      }
    }
    invoke("bruecke_v3_installieren").catch((e) => {
      // Der Prozess lebt noch, der Fehlschlag ist also gemeldet: Die Marke
      // gilt nur für den stummen Abbruch, sonst gäbe der nächste Start einen
      // Ausweg, obwohl dort gar nichts schiefging.
      try {
        localStorage.removeItem(VERSUCH_KEY);
      } catch {
        /* Dann bleibt höchstens ein Ausweg beim nächsten Start. */
      }
      const text = formatIpcError(e);
      const grund = /^gesperrt:(flug|warteschlange)$/.exec(text)?.[1];
      if (grund) setInfo((alt) => (alt ? { ...alt, sperre: grund as BrueckeV3Info["sperre"] } : alt));
      else setFehler(text);
      setLaeuft(false);
    });
  }, [info]);

  return { info, laeuft, fehler, umsteigen, pflichtBeimStart, frueherVersuch };
}

/** Derselbe Betriebsschalter wie beim `UpdateGate`, von Hand gesetzt. */
const NOTAUS_KEY = "aeroacars.update.gate_off";
/** Ausweg nach einem Fehlschlag, an die Version von 3 gebunden wie beim `UpdateGate`. */
const AUSWEG_KEY = "aeroacars.bruecke_v3.skip_version";

/**
 * v2.1.27 — Pflicht-Umstieg (Thomas 10.11.2026): Beim Start ist die
 * Oberfläche gesperrt, bis auf AeroACARS 3 umgestiegen wurde. Kein „Später“.
 *
 * Er sperrt nicht ohne Antwort aus dem Kanal (kein Netz, kein stabiles 3.x),
 * nicht bei offenem Flug oder wartenden PIREPs (`sperre`), nicht in einer
 * Flugphase und nicht, solange die Wiederaufnahme ungeklärt ist. Bei `sperre`
 * bleibt das Band und erklärt; gesperrt wird dann erst beim nächsten Start,
 * damit der Riegel nicht direkt nach dem Abschicken eines PIREPs zufällt.
 */
export function BrueckeV3Riegel({
  bruecke,
  activePhase,
  wiederaufnahmeStehtAus,
}: {
  bruecke: BrueckeV3;
  activePhase: FlightPhase | null;
  wiederaufnahmeStehtAus?: boolean;
}) {
  const { t } = useTranslation();
  const { info, laeuft, fehler, umsteigen, pflichtBeimStart, frueherVersuch } = bruecke;
  const knopfRef = useRef<HTMLButtonElement>(null);
  const [weiterMit2, setWeiterMit2] = useState(false);

  const sichtbar =
    pflichtBeimStart &&
    !weiterMit2 &&
    speicherGeht() &&
    info != null &&
    info.sperre == null &&
    lesen(NOTAUS_KEY) !== "1" &&
    lesen(AUSWEG_KEY) !== info.version &&
    !(activePhase != null && ACTIVE_FLIGHT_PHASES.has(activePhase)) &&
    wiederaufnahmeStehtAus === false;

  useEffect(() => {
    if (sichtbar) knopfRef.current?.focus();
  }, [sichtbar]);

  if (!sichtbar || !info) return null;
  // Nach einem gelungenen Öffnen gibt es keinen Ausweg, sonst wäre er ein „Später“.
  const ausweg = fehler != null || (frueherVersuch === versuchKennung(info) && !info.lief_schon);

  return (
    <div className="update-gate" role="alertdialog" aria-modal="true">
      <div className="update-gate__card">
        <div className="update-gate__icon" aria-hidden="true">
          ⬆
        </div>
        <h2 className="update-gate__title">{t("bruecke_v3.riegel_titel", { version: info.version })}</h2>
        <p className="update-gate__lead">{t(info.installiert ? "bruecke_v3.riegel_text_installiert" : "bruecke_v3.riegel_text")}</p>
        {laeuft && <p className="update-gate__progress">{t("bruecke_v3.laeuft")}</p>}
        {fehler && <p className="update-gate__progress">{t("bruecke_v3.fehler", { fehler })}</p>}
        <button ref={knopfRef} type="button" className="update-gate__install" onClick={umsteigen} disabled={laeuft}>
          {laeuft ? "…" : t(info.installiert ? "bruecke_v3.oeffnen" : "bruecke_v3.umsteigen")}
        </button>
        {ausweg && (
          <div className="update-gate__fallback">
            <p>{t("bruecke_v3.riegel_fehlgeschlagen")}</p>
            <button
              type="button"
              className="update-gate__continue"
              onClick={() => {
                try {
                  localStorage.setItem(AUSWEG_KEY, info.version);
                } catch {
                  /* Ohne Speicher kommt der Riegel beim nächsten Start wieder. */
                }
                setWeiterMit2(true);
              }}
            >
              {t("bruecke_v3.riegel_weiter")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function BrueckeV3Band({ bruecke, activePhase }: { bruecke: BrueckeV3; activePhase: FlightPhase | null }) {
  const { t } = useTranslation();
  const { info, laeuft, fehler, umsteigen } = bruecke;

  if (!info) return null;
  if (activePhase != null && ACTIVE_FLIGHT_PHASES.has(activePhase)) return null;

  return (
    <div className="update-banner" role="status" aria-live="polite">
      <div className="update-banner__icon" aria-hidden="true">
        ⬆
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">{t("bruecke_v3.titel", { version: info.version })}</div>
        <div className="update-banner__subtitle">{t("bruecke_v3.text")}</div>
        {info.sperre && <div className="update-banner__progress">{t(`bruecke_v3.sperre_${info.sperre}`)}</div>}
        {laeuft && <div className="update-banner__progress">{t("bruecke_v3.laeuft")}</div>}
        {fehler && <div className="update-banner__progress">{t("bruecke_v3.fehler", { fehler })}</div>}
      </div>
      <div className="update-banner__actions">
        <button type="button" className="button button--primary" onClick={umsteigen} disabled={laeuft || info.sperre != null}>
          {laeuft ? "…" : t(info.installiert ? "bruecke_v3.oeffnen" : "bruecke_v3.umsteigen")}
        </button>
      </div>
    </div>
  );
}

/** Fester Knopf in der Seitenleiste, auch wenn das Band nicht zu sehen ist. */
export function BrueckeV3Knopf({ bruecke }: { bruecke: BrueckeV3 }) {
  const { t } = useTranslation();
  const { info, laeuft, umsteigen } = bruecke;
  if (!info) return null;
  const titel = info.sperre ? t(`bruecke_v3.sperre_${info.sperre}`) : t("bruecke_v3.knopf_titel", { version: info.version });
  return (
    <button
      type="button"
      className="update-button update-button--escalated"
      onClick={umsteigen}
      disabled={laeuft || info.sperre != null}
      title={titel}
    >
      <span className="update-button__icon" aria-hidden="true">
        ⬆
      </span>
      <span className="update-button__label">{laeuft ? t("bruecke_v3.laeuft") : t("bruecke_v3.knopf")}</span>
    </button>
  );
}
