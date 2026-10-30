// v3 (30.10.2026, Thomas): Auf den ersten Blick sehen, welche Paket-
// Generation und welche Fassung der Landebewertung gilt — auf „Über“ UND
// „Hilfe anfordern“. Quelle ist allein der Kern (`paket_info`, ui-befehle-4);
// dazu „neues Paket wartet“ mit „Jetzt neu starten“ (nur ohne offenen Flug)
// und „Auf neues Paket prüfen“ (`paket_jetzt_pruefen`).
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, isTauri } from "../lib/ipc";
import "./paketUebersicht.css";

interface Pruefung {
  zeit: string;
  ergebnis: "neu" | "kein_neueres" | "fehler" | "flug_offen" | "laeuft";
  generation: number | null;
  fehler: string | null;
}

export interface PaketInfo {
  herkunft: "paket" | "eingebaut" | "pin";
  id: string;
  generation: number;
  bewertung_fassung: number;
  flug_pin: { generation: number } | null;
  naechster_start: { generation: number; bewertung_fassung: number } | null;
  letzte_pruefung: Pruefung | null;
  flug_offen?: boolean;
}

/** Die Kopfzeile in Klartext (auch für Tests und andere Seiten). */
export function paketZeile(t: (k: string, o?: Record<string, unknown>) => string, i: PaketInfo): string {
  return i.generation === 0
    ? t("paket.zeile_eingebaut", { f: i.bewertung_fassung })
    : t("paket.zeile_generation", { n: i.generation, f: i.bewertung_fassung });
}

export function PaketUebersicht({ sichtbar = true }: { sichtbar?: boolean }) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<PaketInfo | null>(null);
  const [pruefe, setPruefe] = useState(false);
  const [antwort, setAntwort] = useState<Pruefung | null>(null);
  const [startFehler, setStartFehler] = useState<string | null>(null);
  const [startet, setStartet] = useState(false);

  const laden = useCallback(async () => {
    try {
      setInfo(await invoke<PaketInfo>("paket_info"));
    } catch {
      /* älterer Kern oder LAN-Tablet: Zeile bleibt weg */
    }
  }, []);

  useEffect(() => {
    if (sichtbar) void laden();
  }, [sichtbar, laden]);

  const pruefen = async () => {
    setPruefe(true);
    try {
      setAntwort(await invoke<Pruefung>("paket_jetzt_pruefen"));
      await laden();
    } catch (e) {
      setAntwort({ zeit: "", ergebnis: "fehler", generation: null, fehler: String((e as { message?: string })?.message ?? e) });
    } finally {
      setPruefe(false);
    }
  };

  const neuStarten = async () => {
    if (startet) return;
    setStartFehler(null);
    setStartet(true);
    try {
      await invoke("paket_neu_starten");
    } catch (e) {
      const code = (e as { code?: string })?.code;
      setStartFehler(code === "flug_offen" || code === "zu_frueh" || code === "kein_neues_paket" ? code : "allgemein");
      setStartet(false);
      void laden();
    }
  };

  if (!info) return null;
  const wartet = info.naechster_start;
  const letzteFehler = !antwort && info.letzte_pruefung?.ergebnis === "fehler" ? info.letzte_pruefung.fehler : null;

  return (
    <div className="paket-uebersicht" data-testid="paket-uebersicht">
      <p className="paket-uebersicht__zeile" data-testid="paket-zeile">
        {paketZeile(t, info)}
      </p>
      {info.flug_pin && <p className="paket-uebersicht__hinweis">{t("paket.pin", { n: info.flug_pin.generation })}</p>}
      {wartet && (
        <div className="paket-uebersicht__neu" data-testid="paket-neu" role="status">
          <span>{t("paket.neu_wartet", { n: wartet.generation })}</span>
          {/* Neustart nur am Sim-PC (die LAN-Brücke sperrt ihn). */}
          {!isTauri ? null : info.flug_offen ? (
            <span className="paket-uebersicht__hinweis">{t("paket.neu_starten_nach_flug")}</span>
          ) : (
            <button type="button" className="button button--primary" disabled={startet} onClick={() => void neuStarten()}>
              {t("paket.neu_starten")}
            </button>
          )}
        </div>
      )}
      {startFehler && (
        <p className="paket-uebersicht__fehler" role="alert">
          {startFehler === "flug_offen"
            ? t("paket.neu_starten_nach_flug")
            : startFehler === "zu_frueh"
              ? t("paket.neustart_zu_frueh")
              : t("paket.neustart_fehler")}
        </p>
      )}
      <div className="paket-uebersicht__pruefen">
        <button type="button" className="button button--ghost" disabled={pruefe} onClick={() => void pruefen()}>
          {pruefe ? t("paket.pruefe") : t("paket.pruefen")}
        </button>
        {antwort && (
          <span
            className={antwort.ergebnis === "fehler" ? "paket-uebersicht__fehler" : "paket-uebersicht__hinweis"}
            data-testid="paket-antwort"
            role="status"
          >
            {antwort.ergebnis === "neu"
              ? t("paket.ergebnis_neu", { n: antwort.generation })
              : antwort.ergebnis === "fehler"
                ? t("paket.ergebnis_fehler", { fehler: antwort.fehler ?? "" })
                : antwort.ergebnis === "flug_offen"
                  ? t("paket.ergebnis_flug_offen")
                  : antwort.ergebnis === "laeuft"
                    ? t("paket.ergebnis_laeuft")
                    : t("paket.ergebnis_kein_neueres")}
          </span>
        )}
        {letzteFehler && (
          <span className="paket-uebersicht__fehler" data-testid="paket-letzte-fehler">
            {t("paket.letzte_fehler", { fehler: letzteFehler })}
          </span>
        )}
      </div>
    </div>
  );
}
