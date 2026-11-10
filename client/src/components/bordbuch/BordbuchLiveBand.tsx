// Laufender Flug oben im Logbuch: wer, wohin, wie weit das Bordbuch ist.
// „Live ansehen“ klappt Checkliste und Flugprofil des laufenden Flugs auf.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke, listen } from "../../lib/ipc";
import { bilanz, type LiveAnsicht } from "../../lib/bordbuch";
import { AusHinweis, BordbuchLiveInhalt } from "./BordbuchFlug";

export interface LaufenderFlug {
  kennung: string;
  dep: string;
  arr: string;
  /** Flugphase, schon übersetzt */
  phase?: string;
  flugzeug?: string;
}

export function BordbuchLiveBand({ flug }: { flug: LaufenderFlug | null }) {
  const { t } = useTranslation();
  const [live, setLive] = useState<LiveAnsicht | null>(null);
  const [offen, setOffen] = useState(false);

  const laden = useCallback(async () => {
    try {
      setLive(await invoke<LiveAnsicht | null>("bordbuch_live"));
    } catch (e) {
      console.warn("bordbuch_live", e);
    }
  }, []);

  useEffect(() => {
    void laden();
    const id = window.setInterval(() => void laden(), 4000);
    let aktiv = true;
    let weg: (() => void) | undefined;
    void listen("bordbuch_geaendert", () => void laden())
      .then((u) => {
        if (aktiv) weg = u;
        else u();
      })
      .catch(() => undefined);
    return () => {
      aktiv = false;
      window.clearInterval(id);
      weg?.();
    };
  }, [laden]);

  if (!flug && !live) return null;
  const b = live && !live.aus_grund ? bilanz(live.punkte, live.eingeschaltet) : null;
  return (
    <section className="aa-lb-live" aria-label={t("bordbuch.laufender_flug")}>
      <div className="aa-lb-live-zeile">
        <span className="aa-lb-live-titel">{t("bordbuch.laufender_flug")}</span>
        {flug && (
          <>
            <strong>{flug.kennung}</strong>
            <span className="aa-lb-rt">{flug.dep}<span className="aa-lb-arr">→</span>{flug.arr}</span>
            <span className="aa-lb-muted">{[flug.phase, flug.flugzeug].filter(Boolean).join(" · ")}</span>
          </>
        )}
        {b && b.von > 0 && <span className="aa-lb-live-stand">{t("logbook_view.live_stand", { ok: b.ok, von: b.von })}</span>}
        {live && !live.aus_grund && (
          <button type="button" className="aa-lb-btn aa-lb-btn--voll" aria-expanded={offen} onClick={() => setOffen((o) => !o)}>
            {offen ? t("logbook_view.live_zu") : t("logbook_view.live_auf")}
          </button>
        )}
      </div>
      {live?.aus_grund && <AusHinweis grund={live.aus_grund} />}
      {offen && live && !live.aus_grund && <BordbuchLiveInhalt live={live} />}
    </section>
  );
}
