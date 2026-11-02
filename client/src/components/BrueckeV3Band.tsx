import { useCallback, useEffect, useState } from "react";
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
}

export interface BrueckeV3 {
  info: BrueckeV3Info | null;
  laeuft: boolean;
  fehler: string | null;
  umsteigen: () => void;
}

/** Neu abfragen (nur ohne Flug): jede Minute bei Sperre, alle zehn Minuten ohne Antwort. */
const TAKT_GESPERRT_MS = 60 * 1000;
const TAKT_OHNE_ANTWORT_MS = 10 * 60 * 1000;

export function useBrueckeV3(activePhase: FlightPhase | null): BrueckeV3 {
  const [info, setInfo] = useState<BrueckeV3Info | null>(null);
  const [laeuft, setLaeuft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [abfrage, setAbfrage] = useState(0);

  useEffect(() => {
    if (!isTauri) return;
    let aktiv = true;
    invoke<BrueckeV3Info | null>("bruecke_v3_pruefen")
      .then((v) => {
        // Eine leere Antwort (Netzfehler) wirft eine schon bekannte Version nicht weg.
        if (aktiv && v) setInfo(v);
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
    invoke("bruecke_v3_installieren").catch((e) => {
      const text = formatIpcError(e);
      const grund = /^gesperrt:(flug|warteschlange)$/.exec(text)?.[1];
      if (grund) setInfo((alt) => (alt ? { ...alt, sperre: grund as BrueckeV3Info["sperre"] } : alt));
      else setFehler(text);
      setLaeuft(false);
    });
  }, []);

  return { info, laeuft, fehler, umsteigen };
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
          {laeuft ? "…" : t("bruecke_v3.umsteigen")}
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
