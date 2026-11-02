import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { FlightPhase } from "../types";
import { formatIpcError, invoke, isTauri } from "../lib/ipc";
import { ACTIVE_FLIGHT_PHASES } from "./UpdateBanner";

/**
 * v2.1.25 — Brücke zu AeroACARS 3 (Thomas 02.11.2026): Viele Piloten blieben
 * auf 2.x, weil der 2.x-Updater von 3 nichts weiß. Das Band fragt einmal beim
 * Start den 3.x-Kanal ab (`bruecke_v3_pruefen`) und bietet den Umstieg an;
 * Laden, Installer starten und Beenden macht der Kern (`bruecke_v3.rs`).
 *
 * Nicht im Flug (dieselben Phasen wie das Update-Band), nicht im
 * LAN-Browser (der Kern leitet die Befehle dort ohnehin nicht weiter).
 * „Später“ blendet es für 24 Stunden aus.
 */

const SPAETER_KEY = "aa-bruecke-v3-spaeter";
const SPAETER_MS = 24 * 3600 * 1000;

function spaeterAktiv(): boolean {
  try {
    const t = Number(localStorage.getItem(SPAETER_KEY));
    return Number.isFinite(t) && Date.now() - t < SPAETER_MS;
  } catch {
    return false;
  }
}

export function BrueckeV3Band({ activePhase }: { activePhase: FlightPhase | null }) {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [spaeter, setSpaeter] = useState(spaeterAktiv);
  const [laeuft, setLaeuft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    let aktiv = true;
    invoke<string | null>("bruecke_v3_pruefen")
      .then((v) => {
        if (aktiv) setVersion(v ?? null);
      })
      .catch(() => undefined);
    return () => {
      aktiv = false;
    };
  }, []);

  if (!version || spaeter) return null;
  if (activePhase != null && ACTIVE_FLIGHT_PHASES.has(activePhase)) return null;

  const umsteigen = () => {
    setLaeuft(true);
    setFehler(null);
    invoke("bruecke_v3_installieren").catch((e) => {
      setFehler(formatIpcError(e));
      setLaeuft(false);
    });
  };
  const spaeterKlick = () => {
    try {
      localStorage.setItem(SPAETER_KEY, String(Date.now()));
    } catch {
      /* ohne Speicher gilt „Später“ nur bis zum Neustart */
    }
    setSpaeter(true);
  };

  return (
    <div className="update-banner" role="status" aria-live="polite">
      <div className="update-banner__icon" aria-hidden="true">
        ⬆
      </div>
      <div className="update-banner__text">
        <div className="update-banner__title">{t("bruecke_v3.titel", { version })}</div>
        <div className="update-banner__subtitle">{t("bruecke_v3.text")}</div>
        {laeuft && <div className="update-banner__progress">{t("bruecke_v3.laeuft")}</div>}
        {fehler && <div className="update-banner__progress">{t("bruecke_v3.fehler", { fehler })}</div>}
      </div>
      <div className="update-banner__actions">
        <button type="button" className="button button--primary" onClick={umsteigen} disabled={laeuft}>
          {laeuft ? "…" : t("bruecke_v3.umsteigen")}
        </button>
        <button type="button" className="button button--ghost" onClick={spaeterKlick} disabled={laeuft}>
          {t("update.later")}
        </button>
      </div>
    </div>
  );
}
